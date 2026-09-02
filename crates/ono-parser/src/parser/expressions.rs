//! Expression parsing and precedence.
//!
//! The ladder from `parse_expression` down to `parse_postfix` is the precedence table, written
//! as one function per level so the order is legible in the source rather than in a table
//! somewhere else.

use ono_core::Span;

use crate::ast::{
    BinaryExpr, BinaryOp, CallExpr, CurrentValue, Expr, FieldAccess, FieldPath, IndexExpr,
    NumberLit, ParenInner, ParenValue, RegexLit, TimestampLit, UnaryExpr, UnaryOp, UnitLit,
    Variable,
};
use crate::diagnostic::Diagnostic;
use crate::lexer::{LexMode, Token, TokenKind};

use super::literals::{current_selector, split_unit};
use super::pipelines::ends_stage;
use super::state::{MAX_DEPTH, Parser};

impl Parser<'_> {
    pub(super) fn parse_expression(&mut self) -> Expr {
        self.parse_logical_or()
    }

    pub(super) fn parse_logical_or(&mut self) -> Expr {
        let mut lhs = self.parse_logical_and();
        while self.at_keyword(LexMode::Expr, "or") {
            let op = self.bump(LexMode::Expr);
            let rhs = self.parse_logical_and();
            lhs = binary(BinaryOp::Or, op.span, lhs, rhs);
        }
        lhs
    }

    pub(super) fn parse_logical_and(&mut self) -> Expr {
        let mut lhs = self.parse_equality();
        while self.at_keyword(LexMode::Expr, "and") {
            let op = self.bump(LexMode::Expr);
            let rhs = self.parse_equality();
            lhs = binary(BinaryOp::And, op.span, lhs, rhs);
        }
        lhs
    }

    pub(super) fn parse_equality(&mut self) -> Expr {
        let mut lhs = self.parse_comparison();
        loop {
            let token = self.peek(LexMode::Expr);
            let op = match token.kind {
                TokenKind::EqEq => BinaryOp::Eq,
                TokenKind::BangEq => BinaryOp::NotEq,
                _ => break,
            };
            self.bump(LexMode::Expr);
            let rhs = self.parse_comparison();
            lhs = binary(op, token.span, lhs, rhs);
        }
        lhs
    }

    pub(super) fn parse_comparison(&mut self) -> Expr {
        let mut lhs = self.parse_membership();
        loop {
            let token = self.peek(LexMode::Expr);
            let op = match token.kind {
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::LtEq => BinaryOp::LtEq,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::GtEq => BinaryOp::GtEq,
                _ => break,
            };
            self.bump(LexMode::Expr);
            let rhs = self.parse_membership();
            lhs = binary(op, token.span, lhs, rhs);
        }
        lhs
    }

    pub(super) fn parse_membership(&mut self) -> Expr {
        let mut lhs = self.parse_additive();
        loop {
            let token = self.peek(LexMode::Expr);
            let (op, op_span) = match token.kind {
                TokenKind::Match => (BinaryOp::Match, token.span),
                TokenKind::NotMatch => (BinaryOp::NotMatch, token.span),
                TokenKind::Ident if token.text(self.source) == "in" => (BinaryOp::In, token.span),
                TokenKind::Ident if token.text(self.source) == "not" => {
                    let following = self.peek_after(LexMode::Expr, token);
                    if following.kind != TokenKind::Ident || following.text(self.source) != "in" {
                        break;
                    }
                    self.bump(LexMode::Expr);
                    (BinaryOp::NotIn, token.span.join(following.span))
                }
                _ => break,
            };
            self.bump(LexMode::Expr);
            let rhs = self.parse_additive();
            lhs = binary(op, op_span, lhs, rhs);
        }
        lhs
    }

    pub(super) fn parse_additive(&mut self) -> Expr {
        let mut lhs = self.parse_multiplicative();
        loop {
            let token = self.peek(LexMode::Expr);
            let op = match token.kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.bump(LexMode::Expr);
            let rhs = self.parse_multiplicative();
            lhs = binary(op, token.span, lhs, rhs);
        }
        lhs
    }

    pub(super) fn parse_multiplicative(&mut self) -> Expr {
        let mut lhs = self.parse_unary();
        loop {
            let token = self.peek(LexMode::Expr);
            let op = match token.kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Rem,
                _ => break,
            };
            self.bump(LexMode::Expr);
            let rhs = self.parse_unary();
            lhs = binary(op, token.span, lhs, rhs);
        }
        lhs
    }

    pub(super) fn parse_unary(&mut self) -> Expr {
        let token = self.peek(LexMode::ExprOperand);
        let op = match token.kind {
            TokenKind::Minus => UnaryOp::Neg,
            TokenKind::Ident if token.text(self.source) == "not" => UnaryOp::Not,
            _ => return self.parse_postfix(),
        };
        // A prefix operator recurses into itself, so it needs the counter the same way nesting
        // does: `- - - …` reaches no other rule, and unguarded it ran out of stack long before
        // it ran out of input.
        if self.depth >= MAX_DEPTH {
            self.report(Diagnostic::syntax(
                token.span,
                "this expression nests more deeply than the parser will follow",
            ));
            self.bump(LexMode::ExprOperand);
            return Expr::Error(Span::at(token.span.start()));
        }
        self.bump(LexMode::ExprOperand);
        self.depth += 1;
        let operand = self.parse_unary();
        self.depth -= 1;
        Expr::Unary(Box::new(UnaryExpr {
            op,
            op_span: token.span,
            span: token.span.join(operand.span()),
            operand,
        }))
    }

    pub(super) fn parse_postfix(&mut self) -> Expr {
        let mut expression = self.parse_primary();
        loop {
            let token = self.peek(LexMode::Expr);
            let adjacent = token.span.start() == expression.span().end();
            match token.kind {
                TokenKind::Dot | TokenKind::QuestionDot => {
                    self.bump(LexMode::Expr);
                    let optional = token.kind == TokenKind::QuestionDot;
                    let name = self.peek(LexMode::Expr);
                    let (field, field_span) = if name.kind == TokenKind::Ident {
                        self.bump(LexMode::Expr);
                        (name.text(self.source).to_owned(), name.span)
                    } else {
                        let description = self.describe(name);
                        self.report_unexpected(
                            name,
                            format!("expected a field name, found {description}"),
                        );
                        (String::new(), Span::at(name.span.start()))
                    };
                    expression = Expr::Field(Box::new(FieldAccess {
                        span: expression.span().join(field_span),
                        base: expression,
                        field,
                        field_span,
                        optional,
                    }));
                    if field_span.is_empty() {
                        break;
                    }
                }
                TokenKind::LBracket if adjacent => {
                    self.bump(LexMode::Expr);
                    let saved = self.no_brace;
                    self.no_brace = false;
                    // The index is one level deeper, and the counter must stay raised while it
                    // is read. `parse_primary` raises and lowers it around its own body, so by
                    // the time this loop runs it is back where it started: without this a chain
                    // of suffixes — `1[1[1[…` — recursed with the counter never rising, and the
                    // inner `parse_primary` refused nothing until the stack ran out.
                    self.depth += 1;
                    let index = self.parse_expression();
                    self.depth -= 1;
                    self.no_brace = saved;
                    let end = self
                        .close(TokenKind::RBracket, token, "`]`")
                        .unwrap_or(index.span());
                    expression = Expr::Index(Box::new(IndexExpr {
                        span: expression.span().join(end),
                        base: expression,
                        index,
                    }));
                }
                TokenKind::LParen if adjacent => {
                    // The arguments are one level deeper, for the same reason the index is.
                    self.depth += 1;
                    let (arguments, end) = self.parse_call_arguments(token);
                    self.depth -= 1;
                    expression = Expr::Call(Box::new(CallExpr {
                        span: expression.span().join(end),
                        callee: expression,
                        arguments,
                    }));
                }
                _ => break,
            }
        }
        expression
    }

    pub(super) fn parse_call_arguments(&mut self, open: Token) -> (Vec<Expr>, Span) {
        self.bump(LexMode::Expr);
        let saved = self.no_brace;
        self.no_brace = false;
        let mut arguments = Vec::new();
        let end;
        loop {
            self.skip_newlines(LexMode::ExprOperand);
            let token = self.peek(LexMode::ExprOperand);
            match token.kind {
                TokenKind::RParen => {
                    end = self.bump(LexMode::ExprOperand).span;
                    break;
                }
                TokenKind::Eof => {
                    self.report(Diagnostic::incomplete(
                        open.span,
                        "this `(` is never closed",
                    ));
                    end = token.span;
                    break;
                }
                _ => {}
            }
            let before = self.pos;
            arguments.push(self.parse_expression());
            self.skip_newlines(LexMode::Expr);
            if self.eat(LexMode::Expr, TokenKind::Comma).is_none()
                && !matches!(
                    self.peek(LexMode::Expr).kind,
                    TokenKind::RParen | TokenKind::Eof
                )
            {
                let token = self.peek(LexMode::Expr);
                let description = self.describe(token);
                self.report_unexpected(
                    token,
                    format!("expected `,` or `)` in the argument list, found {description}"),
                );
                self.bump(LexMode::Expr);
            }
            if self.pos == before {
                self.bump(LexMode::Expr);
            }
        }
        self.no_brace = saved;
        (arguments, end)
    }

    pub(super) fn parse_primary(&mut self) -> Expr {
        if self.depth >= MAX_DEPTH {
            let token = self.peek(LexMode::ExprOperand);
            self.report(Diagnostic::syntax(
                token.span,
                "this expression nests more deeply than the parser will follow",
            ));
            if !ends_stage(token.kind) {
                self.bump(LexMode::ExprOperand);
            }
            return Expr::Error(Span::at(token.span.start()));
        }
        self.depth += 1;
        let expression = self.parse_primary_inner();
        self.depth -= 1;
        expression
    }

    pub(super) fn parse_primary_inner(&mut self) -> Expr {
        let token = self.peek(LexMode::ExprOperand);
        match token.kind {
            TokenKind::Int | TokenKind::Float => {
                self.bump(LexMode::ExprOperand);
                Expr::Number(NumberLit {
                    value: self.number_value(token.text(self.source), token.span),
                    span: token.span,
                })
            }
            TokenKind::Unit => {
                self.bump(LexMode::ExprOperand);
                let text = token.text(self.source);
                let (number, unit) = split_unit(text);
                Expr::Unit(UnitLit {
                    value: self.number_value(number, token.span),
                    unit,
                    span: token.span,
                })
            }
            TokenKind::Str
            | TokenKind::RawStr
            | TokenKind::UnterminatedStr
            | TokenKind::UnterminatedRawStr => self.parse_string(token),
            TokenKind::Timestamp => {
                self.bump(LexMode::ExprOperand);
                Expr::Timestamp(TimestampLit {
                    text: token.text(self.source).to_owned(),
                    span: token.span,
                })
            }
            TokenKind::Ip => {
                self.bump(LexMode::ExprOperand);
                Expr::Ip(crate::ast::IpLit {
                    text: token.text(self.source).to_owned(),
                    span: token.span,
                })
            }
            TokenKind::Regex | TokenKind::UnterminatedRegex => {
                self.bump(LexMode::ExprOperand);
                if token.kind == TokenKind::UnterminatedRegex {
                    self.report(Diagnostic::incomplete(
                        token.span,
                        "this regex literal is never closed",
                    ));
                }
                let text = token.text(self.source);
                let body = text.strip_prefix('/').unwrap_or(text);
                let (pattern, flags) = match body.rfind('/') {
                    Some(index) if token.kind == TokenKind::Regex => {
                        (&body[..index], &body[index + 1..])
                    }
                    _ => (body, ""),
                };
                Expr::Regex(RegexLit {
                    pattern: pattern.to_owned(),
                    flags: flags.to_owned(),
                    span: token.span,
                })
            }
            TokenKind::Variable => {
                self.bump(LexMode::ExprOperand);
                let text = token.text(self.source);
                Expr::Variable(Variable {
                    name: text.strip_prefix('$').unwrap_or(text).to_owned(),
                    span: token.span,
                })
            }
            TokenKind::CurrentValue => {
                self.bump(LexMode::ExprOperand);
                Expr::CurrentValue(CurrentValue {
                    selector: current_selector(token.text(self.source)),
                    span: token.span,
                })
            }
            TokenKind::Ident => {
                self.bump(LexMode::ExprOperand);
                match token.text(self.source) {
                    "true" => Expr::Bool(true, token.span),
                    "false" => Expr::Bool(false, token.span),
                    "null" => Expr::Null(token.span),
                    name => Expr::Path(FieldPath {
                        name: name.to_owned(),
                        span: token.span,
                    }),
                }
            }
            TokenKind::LBracket => self.parse_list(),
            TokenKind::LBrace if !self.no_brace => self.parse_brace(token),
            TokenKind::LParen => {
                let paren = self.parse_paren_value();
                Expr::Paren(Box::new(paren))
            }
            _ => {
                let description = self.describe(token);
                self.report_unexpected(token, format!("expected a value, found {description}"));
                Expr::Error(Span::at(token.span.start()))
            }
        }
    }

    /// Parses `( … )`, deciding between a nested pipeline and a grouped expression.
    ///
    /// The decision is made from the first three tokens inside the parentheses, so it costs the
    /// same whatever the content is and never re-parses: a bare name (`(ls)`), a name followed
    /// by another argument (`(get process | count)`), or an operator glued to its right operand
    /// with whitespace on its left (`(ls -la)`) is a command invocation. Everything else —
    /// a literal, a variable, a call, a field access, an operator with room around it — is an
    /// expression, so `(a + b)` and `(now() - 7d)` group values.
    pub(super) fn parse_paren_value(&mut self) -> ParenValue {
        let open = self.bump(LexMode::Words);
        let saved = self.no_brace;
        self.no_brace = false;
        let inner = if self.parens_hold_expression(open) {
            let expression = self.parse_expression();
            ParenInner::Expr(expression)
        } else {
            self.skip_newlines(LexMode::Words);
            ParenInner::Pipeline(self.parse_pipeline())
        };
        self.skip_newlines(LexMode::Words);
        let end = if let Some(close) = self.eat(LexMode::Words, TokenKind::RParen) {
            close.span
        } else {
            let token = self.peek(LexMode::Words);
            if token.kind == TokenKind::Eof {
                self.report(Diagnostic::incomplete(
                    open.span,
                    "this `(` is never closed",
                ));
            } else {
                let description = self.describe(token);
                self.report_unexpected(token, format!("expected `)`, found {description}"));
            }
            token.span
        };
        self.no_brace = saved;
        let inner_span = match &inner {
            ParenInner::Expr(expression) => expression.span(),
            ParenInner::Pipeline(pipeline) => pipeline.span,
        };
        ParenValue {
            inner,
            span: open.span.join(inner_span).join(end),
        }
    }

    /// Whether `( … )` opened at `open` groups a value rather than running a command.
    pub(super) fn parens_hold_expression(&self, open: Token) -> bool {
        let first = self.peek_after(LexMode::ExprOperand, open);
        if first.kind != TokenKind::Ident {
            return !matches!(first.kind, TokenKind::RParen | TokenKind::Eof);
        }
        if matches!(first.text(self.source), "true" | "false" | "null" | "not") {
            return true;
        }
        self.operator_follows(first)
    }

    /// Whether the value token `first`, at the head of a stage, begins an expression rather than
    /// a value that seeds a pipeline: `$n + 1` and `@ * 2` do, `$hot | select …` and
    /// `$cmd -la` do not.
    pub(super) fn value_starts_expression(&self, first: Token) -> bool {
        self.operator_follows(first)
    }

    /// Whether an infix operator written as arithmetic follows `first`.
    pub(super) fn operator_follows(&self, first: Token) -> bool {
        let second = self.peek_after(LexMode::Expr, first);
        let adjacent = first.span.end() == second.span.start();
        match second.kind {
            TokenKind::Dot | TokenKind::QuestionDot => true,
            TokenKind::LParen | TokenKind::LBracket => adjacent,
            TokenKind::EqEq
            | TokenKind::BangEq
            | TokenKind::Lt
            | TokenKind::LtEq
            | TokenKind::Gt
            | TokenKind::GtEq
            | TokenKind::Match
            | TokenKind::NotMatch
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent => {
                // `ls -la` glues the operator to its right operand and leaves room on its left;
                // arithmetic a person writes does not.
                let third = self.peek_after(LexMode::ExprOperand, second);
                !(second.span.end() == third.span.start() && first.span.end() < second.span.start())
            }
            TokenKind::Ident => matches!(second.text(self.source), "and" | "or" | "in"),
            _ => false,
        }
    }
}

pub(super) fn binary(op: BinaryOp, op_span: Span, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary(Box::new(BinaryExpr {
        op,
        op_span,
        span: lhs.span().join(rhs.span()),
        lhs,
        rhs,
    }))
}
