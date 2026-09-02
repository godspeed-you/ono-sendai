//! Blocks, function declarations and control constructs.
//!
//! Everything here nests, so everything here is behind the recursion-depth guard of
//! [`MAX_DEPTH`]: adversarial input is a given for a shell (spec §35.6), and a wall of open
//! braces has to be a diagnostic rather than a stack overflow.

use ono_core::Span;

use crate::ast::{
    Block, CatchClause, Expr, FnDecl, ForStmt, IfBranch, IfStmt, MatchArm, MatchArmBody, MatchStmt,
    Param, Pattern, TryStmt, TypeRef, WhileStmt,
};
use crate::diagnostic::Diagnostic;
use crate::lexer::{LexMode, TokenKind};

use super::state::{MAX_DEPTH, Parser};

impl Parser<'_> {
    pub(super) fn parse_fn(&mut self) -> FnDecl {
        let keyword = self.bump(LexMode::Words);
        let (name, name_span) = self.expect_ident("a name for the function");
        let mut parameters = Vec::new();
        if self
            .expect(LexMode::Expr, TokenKind::LParen, "`(`")
            .is_some()
        {
            loop {
                self.skip_newlines(LexMode::ExprOperand);
                let token = self.peek(LexMode::Expr);
                match token.kind {
                    TokenKind::RParen => {
                        self.bump(LexMode::Expr);
                        break;
                    }
                    TokenKind::Eof | TokenKind::LBrace => {
                        self.report_unexpected(token, "expected `)` to close the parameter list");
                        break;
                    }
                    _ => {}
                }
                let before = self.pos;
                parameters.push(self.parse_param());
                if self.eat(LexMode::Expr, TokenKind::Comma).is_none()
                    && !matches!(
                        self.peek(LexMode::Expr).kind,
                        TokenKind::RParen | TokenKind::Eof | TokenKind::LBrace
                    )
                {
                    let token = self.peek(LexMode::Expr);
                    let description = self.describe(token);
                    self.report_unexpected(
                        token,
                        format!("expected `,` or `)` in the parameter list, found {description}"),
                    );
                    self.bump(LexMode::Expr);
                }
                if self.pos == before {
                    self.bump(LexMode::Expr);
                }
            }
        }
        let return_type = if self.eat(LexMode::Expr, TokenKind::ThinArrow).is_some() {
            Some(self.parse_type())
        } else {
            None
        };
        let body = self.parse_block_or_error();
        let span = keyword.span.join(body.span);
        FnDecl {
            name,
            name_span,
            parameters,
            return_type,
            body,
            span,
        }
    }

    pub(super) fn parse_param(&mut self) -> Param {
        let (name, name_span) = self.expect_ident("a parameter name");
        let ty = if self.eat(LexMode::Expr, TokenKind::Colon).is_some() {
            Some(self.parse_type())
        } else {
            None
        };
        let default = if self.eat(LexMode::Expr, TokenKind::Eq).is_some() {
            Some(self.parse_expression())
        } else {
            None
        };
        let end = default
            .as_ref()
            .map_or_else(|| ty.as_ref().map_or(name_span, |ty| ty.span), Expr::span);
        Param {
            name,
            name_span,
            ty,
            default,
            span: name_span.join(end),
        }
    }

    pub(super) fn parse_type(&mut self) -> TypeRef {
        let (name, span) = self.expect_ident("a type name");
        let mut arguments = Vec::new();
        let mut end = span;
        if self.eat(LexMode::Expr, TokenKind::Lt).is_some() {
            loop {
                let token = self.peek(LexMode::Expr);
                if matches!(token.kind, TokenKind::Gt | TokenKind::Eof) {
                    if let Some(close) = self.eat(LexMode::Expr, TokenKind::Gt) {
                        end = close.span;
                    } else {
                        self.report_unexpected(token, "expected `>` to close the type arguments");
                    }
                    break;
                }
                let before = self.pos;
                let argument = self.parse_type();
                end = argument.span;
                arguments.push(argument);
                if self.eat(LexMode::Expr, TokenKind::Comma).is_none()
                    && !matches!(
                        self.peek(LexMode::Expr).kind,
                        TokenKind::Gt | TokenKind::Eof
                    )
                {
                    let token = self.peek(LexMode::Expr);
                    let description = self.describe(token);
                    self.report_unexpected(
                        token,
                        format!("expected `,` or `>` in the type arguments, found {description}"),
                    );
                    self.bump(LexMode::Expr);
                }
                if self.pos == before {
                    self.bump(LexMode::Expr);
                }
            }
        }
        let optional = if let Some(mark) = self.eat(LexMode::Expr, TokenKind::Question) {
            end = mark.span;
            true
        } else {
            false
        };
        TypeRef {
            name,
            arguments,
            optional,
            span: span.join(end),
        }
    }

    pub(super) fn parse_if(&mut self) -> IfStmt {
        let keyword = self.bump(LexMode::Words);
        let mut branches = Vec::new();
        let mut else_block = None;
        let mut end;
        loop {
            let condition = self.parse_condition();
            let block = self.parse_block_or_error();
            end = block.span;
            branches.push(IfBranch {
                span: condition.span().join(block.span),
                condition,
                block,
            });
            if !self.at_keyword(LexMode::Words, "else") {
                break;
            }
            self.bump(LexMode::Words);
            if self.at_keyword(LexMode::Words, "if") {
                self.bump(LexMode::Words);
                continue;
            }
            let block = self.parse_block_or_error();
            end = block.span;
            else_block = Some(block);
            break;
        }
        IfStmt {
            branches,
            else_block,
            span: keyword.span.join(end),
        }
    }

    pub(super) fn parse_for(&mut self) -> ForStmt {
        let keyword = self.bump(LexMode::Words);
        let (binding, binding_span) = self.expect_ident("a name to bind each item to");
        if !self.at_keyword(LexMode::Expr, "in") {
            let token = self.peek(LexMode::Expr);
            let description = self.describe(token);
            self.report_unexpected(token, format!("expected `in`, found {description}"));
        } else {
            self.bump(LexMode::Expr);
        }
        let iterable = self.parse_condition();
        let body = self.parse_block_or_error();
        ForStmt {
            binding,
            binding_span,
            iterable,
            span: keyword.span.join(body.span),
            body,
        }
    }

    pub(super) fn parse_while(&mut self) -> WhileStmt {
        let keyword = self.bump(LexMode::Words);
        let condition = self.parse_condition();
        let body = self.parse_block_or_error();
        WhileStmt {
            condition,
            span: keyword.span.join(body.span),
            body,
        }
    }

    pub(super) fn parse_match(&mut self) -> MatchStmt {
        let keyword = self.bump(LexMode::Words);
        let subject = self.parse_condition();
        let mut arms = Vec::new();
        let mut end = subject.span();
        if let Some(open) = self.expect(LexMode::Expr, TokenKind::LBrace, "`{`") {
            loop {
                self.skip_separators();
                let token = self.peek(LexMode::ExprOperand);
                match token.kind {
                    TokenKind::RBrace => {
                        end = self.bump(LexMode::ExprOperand).span;
                        break;
                    }
                    TokenKind::Eof => {
                        self.report(Diagnostic::incomplete(
                            open.span,
                            "this `{` is never closed",
                        ));
                        end = token.span;
                        break;
                    }
                    _ => {}
                }
                let before = self.pos;
                arms.push(self.parse_match_arm());
                self.eat(LexMode::Expr, TokenKind::Comma);
                if self.pos == before {
                    self.bump(LexMode::Expr);
                }
            }
        }
        MatchStmt {
            subject,
            arms,
            span: keyword.span.join(end),
        }
    }

    pub(super) fn parse_match_arm(&mut self) -> MatchArm {
        let pattern = self.parse_pattern();
        if self
            .expect(LexMode::Expr, TokenKind::FatArrow, "`=>`")
            .is_none()
        {
            let span = pattern.span();
            return MatchArm {
                pattern,
                body: MatchArmBody::Expr(Expr::Error(Span::at(span.end()))),
                span,
            };
        }
        let token = self.peek(LexMode::ExprOperand);
        let body = if token.kind == TokenKind::LBrace && !self.is_record_brace(token) {
            MatchArmBody::Block(self.parse_block())
        } else {
            MatchArmBody::Expr(self.parse_expression())
        };
        let end = match &body {
            MatchArmBody::Block(block) => block.span,
            MatchArmBody::Expr(expression) => expression.span(),
        };
        MatchArm {
            span: pattern.span().join(end),
            pattern,
            body,
        }
    }

    pub(super) fn parse_pattern(&mut self) -> Pattern {
        let token = self.peek(LexMode::ExprOperand);
        match token.kind {
            TokenKind::Ident => {
                let text = token.text(self.source);
                match text {
                    "_" => {
                        self.bump(LexMode::ExprOperand);
                        Pattern::Wildcard(token.span)
                    }
                    "true" | "false" | "null" => Pattern::Literal(self.parse_expression()),
                    _ => {
                        self.bump(LexMode::ExprOperand);
                        Pattern::Binding {
                            name: text.to_owned(),
                            span: token.span,
                        }
                    }
                }
            }
            TokenKind::Int
            | TokenKind::Float
            | TokenKind::Unit
            | TokenKind::Str
            | TokenKind::RawStr
            | TokenKind::UnterminatedStr
            | TokenKind::UnterminatedRawStr
            | TokenKind::Regex
            | TokenKind::UnterminatedRegex
            | TokenKind::Minus => Pattern::Literal(self.parse_expression()),
            _ => {
                let description = self.describe(token);
                self.report_unexpected(token, format!("expected a pattern, found {description}"));
                Pattern::Error(Span::at(token.span.start()))
            }
        }
    }

    pub(super) fn parse_try(&mut self) -> TryStmt {
        let keyword = self.bump(LexMode::Words);
        let body = self.parse_block_or_error();
        let mut end = body.span;
        let catch = if self.at_keyword(LexMode::Words, "catch") {
            let catch_keyword = self.bump(LexMode::Words);
            let mut binding = None;
            let mut binding_span = None;
            if self.peek(LexMode::Expr).kind == TokenKind::Ident {
                let token = self.bump(LexMode::Expr);
                binding = Some(token.text(self.source).to_owned());
                binding_span = Some(token.span);
            }
            let handler = self.parse_block_or_error();
            end = handler.span;
            Some(CatchClause {
                binding,
                binding_span,
                span: catch_keyword.span.join(handler.span),
                body: handler,
            })
        } else {
            None
        };
        TryStmt {
            body,
            catch,
            span: keyword.span.join(end),
        }
    }

    /// Parses the expression that guards a block, where a `{` always opens the block.
    pub(super) fn parse_condition(&mut self) -> Expr {
        let saved = self.no_brace;
        self.no_brace = true;
        let expression = self.parse_expression();
        self.no_brace = saved;
        expression
    }

    pub(super) fn parse_block_or_error(&mut self) -> Block {
        let token = self.peek(LexMode::Words);
        if token.kind == TokenKind::LBrace {
            return self.parse_block();
        }
        let description = self.describe(token);
        self.report_unexpected(token, format!("expected a block, found {description}"));
        Block {
            statements: Vec::new(),
            span: Span::at(token.span.start()),
        }
    }

    /// Parses `{ statements }`, with the `{` still unconsumed.
    ///
    /// Statement nesting is depth-limited for the same reason expression nesting is: a block
    /// contains statements, a statement contains a block, and `if true { if true { … } }` nested
    /// a couple of thousand deep would otherwise overflow the stack and abort the process. This
    /// parser runs on every keystroke in the editor, so that is one pasted line away from killing
    /// a login shell.
    pub(super) fn parse_block(&mut self) -> Block {
        if self.depth >= MAX_DEPTH {
            let open = self.bump(LexMode::Words);
            self.report(Diagnostic::syntax(
                open.span,
                "this block nests more deeply than the parser will follow",
            ));
            // Skip to the matching close without descending, so the rest of the line still
            // parses and the tree is still returned.
            let end = self.skip_balanced_block();
            return Block {
                statements: Vec::new(),
                span: open.span.join(end),
            };
        }
        self.depth += 1;
        let block = self.parse_block_inner();
        self.depth -= 1;
        block
    }

    pub(super) fn parse_block_inner(&mut self) -> Block {
        let open = self.bump(LexMode::Words);
        let mut statements = Vec::new();
        let end;
        loop {
            self.skip_separators();
            let token = self.peek(LexMode::Words);
            match token.kind {
                TokenKind::RBrace => {
                    end = self.bump(LexMode::Words).span;
                    break;
                }
                TokenKind::Eof => {
                    self.report(Diagnostic::incomplete(
                        open.span,
                        "this `{` is never closed",
                    ));
                    end = token.span;
                    break;
                }
                _ => {}
            }
            let before = self.pos;
            let saved = self.no_brace;
            self.no_brace = false;
            self.parse_statement_into(&mut statements);
            self.no_brace = saved;
            if self.pos == before {
                self.bump(LexMode::Words);
            }
        }
        Block {
            statements,
            span: open.span.join(end),
        }
    }
}
