//! Pipelines, stages, redirections and words-mode arguments.
//!
//! A stage's argument mode is fixed by its head word before anything knows whether the head
//! resolves to a native command or to a program of the same name (ADR-0009), so the words-mode
//! reader below and the expression reader in `expressions` are two ways of reading the same
//! region of source.

use ono_core::Span;

use crate::ast::{
    ArgMode, Argument, ChainOp, ChainedList, CurrentValue, Expr, FieldAccess, OptionArg, Pipeline,
    QualifiedName, RedirectOp, RedirectTarget, Redirection, Stage, StageHead, StageList, StrLit,
    StrPart, Variable, WordArg,
};
use crate::diagnostic::Diagnostic;
use crate::lexer::{LexMode, Token, TokenKind, is_ident_continue, is_ident_start};

use super::literals::current_selector;
use super::state::{MAX_DEPTH, Parser};

/// Tokens that end a stage without belonging to it.
pub(super) const fn ends_stage(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Eof
            | TokenKind::Newline
            | TokenKind::Semi
            | TokenKind::Pipe
            | TokenKind::AndAnd
            | TokenKind::OrOr
            | TokenKind::Amp
            | TokenKind::Comma
            | TokenKind::RParen
            | TokenKind::RBracket
            | TokenKind::RBrace
    )
}

impl Parser<'_> {
    pub(super) fn parse_pipeline(&mut self) -> Pipeline {
        let head = self.parse_stage_list();
        let start = head.span;
        let mut end = head.span;
        let mut tail = Vec::new();
        loop {
            let token = self.peek(LexMode::Words);
            let op = match token.kind {
                TokenKind::AndAnd => ChainOp::And,
                TokenKind::OrOr => ChainOp::Or,
                _ => break,
            };
            self.bump(LexMode::Words);
            self.skip_newlines(LexMode::Words);
            let list = self.parse_stage_list();
            end = list.span;
            tail.push(ChainedList {
                op,
                op_span: token.span,
                list,
            });
        }
        let background = if let Some(amp) = self.eat(LexMode::Words, TokenKind::Amp) {
            end = amp.span;
            true
        } else {
            false
        };
        Pipeline {
            head,
            tail,
            background,
            span: start.join(end),
        }
    }

    pub(super) fn parse_stage_list(&mut self) -> StageList {
        let first = self.parse_stage();
        let start = first.span;
        let mut end = first.span;
        let mut stages = vec![first];
        while self.eat(LexMode::Words, TokenKind::Pipe).is_some() {
            self.skip_newlines(LexMode::Words);
            let stage = self.parse_stage();
            end = stage.span;
            stages.push(stage);
        }
        StageList {
            stages,
            span: start.join(end),
        }
    }

    pub(super) fn parse_stage(&mut self) -> Stage {
        if self.depth >= MAX_DEPTH {
            let token = self.peek(LexMode::Words);
            self.report(Diagnostic::syntax(
                token.span,
                "this line nests more deeply than the parser will follow",
            ));
            if !ends_stage(token.kind) {
                self.bump(LexMode::Words);
            }
            return Stage {
                head: StageHead::Error(Span::at(token.span.start())),
                mode: ArgMode::Words,
                arguments: Vec::new(),
                redirections: Vec::new(),
                span: Span::at(token.span.start()),
            };
        }
        self.depth += 1;
        let stage = self.parse_stage_inner();
        self.depth -= 1;
        stage
    }

    pub(super) fn parse_stage_inner(&mut self) -> Stage {
        let token = self.peek(LexMode::Words);
        let head = match token.kind {
            TokenKind::Word => {
                self.bump(LexMode::Words);
                StageHead::Command(self.qualified_name(token))
            }
            // A value followed by an infix operator is one expression statement — `@ * 2` in a
            // block, `$n + 1` at the prompt — decided by the same lookahead that tells
            // `(ls -la)` from `(a - b)` (ADR-0009, ADR-0071 §1).
            TokenKind::Variable | TokenKind::CurrentValue
                if self.value_starts_expression(token) =>
            {
                StageHead::Value(self.parse_expression())
            }
            TokenKind::Variable => {
                self.bump(LexMode::Words);
                StageHead::Value(self.words_variable(token))
            }
            // `@-1 | where …` reuses a retained result (spec §6.4, §20.2): a current-value
            // reference starts a pipeline exactly as a variable does.
            TokenKind::CurrentValue => {
                self.bump(LexMode::Words);
                StageHead::Value(self.words_current_value(token))
            }
            TokenKind::LParen => {
                let paren = self.parse_paren_value();
                StageHead::Value(Expr::Paren(Box::new(paren)))
            }
            _ => {
                let description = self.describe(token);
                self.report_unexpected(token, format!("expected a command, found {description}"));
                if !ends_stage(token.kind) {
                    self.bump(LexMode::Words);
                }
                StageHead::Error(Span::at(token.span.start()))
            }
        };
        let mode = head.name().map_or(ArgMode::Words, ArgMode::for_head);
        // Owned, because the argument loop borrows `self` mutably and the head is what decides
        // whether a bare `--where` takes an expression rather than the next word (ADR-0138).
        let head_name = head.name().map(str::to_owned);
        let lex_mode = match mode {
            ArgMode::Words => LexMode::Words,
            ArgMode::Expression => LexMode::ExprOperand,
        };

        let mut arguments = Vec::new();
        let mut redirections = Vec::new();
        let mut end = head.span();
        loop {
            let token = self.peek(lex_mode);
            if ends_stage(token.kind) {
                break;
            }
            let before = self.pos;
            match (mode, token.kind) {
                (
                    ArgMode::Words,
                    TokenKind::Gt
                    | TokenKind::GtGt
                    | TokenKind::Lt
                    | TokenKind::GtAmp
                    | TokenKind::LtAmp,
                ) => {
                    let redirection = self.parse_redirection(token);
                    end = redirection.span;
                    redirections.push(redirection);
                }
                (
                    ArgMode::Expression,
                    TokenKind::Gt | TokenKind::Lt | TokenKind::GtEq | TokenKind::LtEq,
                ) => {
                    self.report(
                        Diagnostic::syntax(
                            token.span,
                            "a stage whose arguments are expressions cannot be redirected",
                        )
                        .with_help("serialize the stream first, then redirect: `| to text > file`"),
                    );
                    self.bump(lex_mode);
                }
                (ArgMode::Words, _) => {
                    let argument = self.parse_words_argument(token, head_name.as_deref());
                    end = argument.span();
                    arguments.push(argument);
                }
                (ArgMode::Expression, TokenKind::Word) => {
                    // The lexer only produces a word here for an adjacent `--name` (ADR-0032).
                    self.bump(lex_mode);
                    let text = token.text(self.source);
                    let name = text.strip_prefix("--").unwrap_or(text).to_owned();
                    end = token.span;
                    // `--initial=10`: an `=` written against the option is punctuation between
                    // the option and the expression that is its value, exactly as the space in
                    // `--initial 10` is (ADR-0227). Written apart, `=` is not an operator in
                    // this language and the expression parser reports it.
                    let next = self.peek(LexMode::ExprOperand);
                    let value = if next.kind == TokenKind::Eq
                        && next.span.start() == token.span.end()
                        && !ends_stage(self.peek_after(LexMode::ExprOperand, next).kind)
                    {
                        self.bump(LexMode::ExprOperand);
                        let expression = self.parse_expression();
                        end = expression.span();
                        Some(expression)
                    } else {
                        None
                    };
                    arguments.push(Argument::Option(OptionArg {
                        name,
                        value,
                        span: token.span.join(end),
                    }));
                }
                (ArgMode::Expression, _) => {
                    let argument = Argument::Value(self.parse_expression());
                    end = argument.span();
                    arguments.push(argument);
                }
            }
            if self.pos == before {
                self.bump(lex_mode);
            }
        }
        Stage {
            span: head.span().join(end),
            head,
            mode,
            arguments,
            redirections,
        }
    }

    /// Splits `namespace:name` apart, leaving anything else as a bare name (spec §6.5).
    pub(super) fn qualified_name(&self, token: Token) -> QualifiedName {
        let text = token.text(self.source);
        if let Some((namespace, name)) = text.split_once(':') {
            let is_ident = !namespace.is_empty()
                && namespace.bytes().next().is_some_and(is_ident_start)
                && namespace.bytes().all(is_ident_continue);
            let is_bare = !name.is_empty() && !name.contains([':', '/']);
            if is_ident && is_bare {
                return QualifiedName {
                    namespace: Some(namespace.to_owned()),
                    name: name.to_owned(),
                    span: token.span,
                };
            }
        }
        QualifiedName {
            namespace: None,
            name: text.to_owned(),
            span: token.span,
        }
    }

    pub(super) fn parse_redirection(&mut self, token: Token) -> Redirection {
        self.bump(LexMode::Words);
        let text = token.text(self.source);
        let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
        let fd = if digits.is_empty() {
            None
        } else if let Ok(number) = digits.parse::<u32>() {
            Some(number)
        } else {
            self.report(Diagnostic::syntax(
                token.span,
                "this file descriptor is out of range",
            ));
            None
        };
        let op = match token.kind {
            TokenKind::Gt => RedirectOp::Write,
            TokenKind::GtGt => RedirectOp::Append,
            TokenKind::Lt => RedirectOp::Read,
            TokenKind::GtAmp => RedirectOp::DupWrite,
            _ => RedirectOp::DupRead,
        };
        let (target, end) = match op {
            RedirectOp::DupWrite | RedirectOp::DupRead => self.parse_fd_target(token),
            _ => self.parse_redirect_target(token),
        };
        Redirection {
            fd,
            op,
            target,
            span: token.span.join(end),
        }
    }

    pub(super) fn parse_fd_target(&mut self, operator: Token) -> (RedirectTarget, Span) {
        let token = self.peek(LexMode::Words);
        if token.kind == TokenKind::Word {
            let text = token.text(self.source);
            if let Ok(number) = text.parse::<u32>() {
                self.bump(LexMode::Words);
                return (RedirectTarget::Fd(number), token.span);
            }
        }
        let description = self.describe(token);
        self.report_unexpected(
            token,
            format!("expected a file descriptor to duplicate, found {description}"),
        );
        (
            RedirectTarget::Error(Span::at(token.span.start())),
            operator.span,
        )
    }

    pub(super) fn parse_redirect_target(&mut self, operator: Token) -> (RedirectTarget, Span) {
        let token = self.peek(LexMode::Words);
        match token.kind {
            TokenKind::Word => {
                self.bump(LexMode::Words);
                (
                    RedirectTarget::Word(WordArg {
                        text: token.text(self.source).to_owned(),
                        span: token.span,
                    }),
                    token.span,
                )
            }
            TokenKind::Str
            | TokenKind::RawStr
            | TokenKind::UnterminatedStr
            | TokenKind::UnterminatedRawStr => {
                let value = self.parse_string(token);
                (RedirectTarget::Value(value), token.span)
            }
            TokenKind::Variable => {
                self.bump(LexMode::Words);
                let value = self.words_variable(token);
                (RedirectTarget::Value(value), token.span)
            }
            _ => {
                let description = self.describe(token);
                self.report_unexpected(
                    token,
                    format!("expected a redirection target, found {description}"),
                );
                (
                    RedirectTarget::Error(Span::at(token.span.start())),
                    operator.span,
                )
            }
        }
    }

    pub(super) fn parse_words_argument(&mut self, token: Token, head: Option<&str>) -> Argument {
        match token.kind {
            TokenKind::Word => self.parse_word_argument(token, head),
            TokenKind::Str
            | TokenKind::RawStr
            | TokenKind::UnterminatedStr
            | TokenKind::UnterminatedRawStr => Argument::Value(self.parse_string(token)),
            TokenKind::Variable => {
                self.bump(LexMode::Words);
                Argument::Value(self.words_variable(token))
            }
            TokenKind::CurrentValue => {
                self.bump(LexMode::Words);
                Argument::Value(self.words_current_value(token))
            }
            TokenKind::LParen => {
                let paren = self.parse_paren_value();
                Argument::Value(Expr::Paren(Box::new(paren)))
            }
            TokenKind::LBracket => Argument::Value(self.parse_list()),
            TokenKind::LBrace => Argument::Value(self.parse_brace(token)),
            _ => {
                let description = self.describe(token);
                self.report_unexpected(token, format!("expected an argument, found {description}"));
                self.bump(LexMode::Words);
                Argument::Error(token.span)
            }
        }
    }

    pub(super) fn parse_word_argument(&mut self, token: Token, head: Option<&str>) -> Argument {
        self.bump(LexMode::Words);
        let text = token.text(self.source);
        if let Some(rest) = text.strip_prefix("--")
            && !rest.is_empty()
        {
            let (name, value_text) = match rest.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (rest, None),
            };
            let named = !name.is_empty()
                && name.bytes().next().is_some_and(is_ident_start)
                && name.bytes().all(is_ident_continue);
            if named {
                let value_start = token.span.start() + 2 + name.len() as u32 + 1;
                let value = match value_text {
                    Some(text) => {
                        Some(self.option_value(text, Span::new(value_start, token.span.end())))
                    }
                    // `--where <predicate>`: the value is the expression that follows, read in
                    // expression mode so that `>` compares and a bare identifier is a field
                    // path (ADR-0138). Nothing follows an option at the end of a stage, and the
                    // command reports the missing value with the type it wanted.
                    None if head
                        .is_some_and(|head| ArgMode::option_takes_expression(head, name))
                        && !ends_stage(self.peek(LexMode::ExprOperand).kind) =>
                    {
                        Some(self.parse_expression())
                    }
                    None => None,
                };
                let span = value
                    .as_ref()
                    .map_or(token.span, |value| token.span.join(value.span()));
                return Argument::Option(OptionArg {
                    name: name.to_owned(),
                    value,
                    span,
                });
            }
        }
        Argument::Word(WordArg {
            text: text.to_owned(),
            span: token.span,
        })
    }

    /// The value of `--name=value`: the text after the `=`, or an adjacent quoted or
    /// parenthesised value when the option ends with the `=`.
    pub(super) fn option_value(&mut self, text: &str, span: Span) -> Expr {
        if text.is_empty() {
            let token = self.peek(LexMode::Words);
            if token.span.start() == span.end() {
                match token.kind {
                    TokenKind::Str
                    | TokenKind::RawStr
                    | TokenKind::UnterminatedStr
                    | TokenKind::UnterminatedRawStr => return self.parse_string(token),
                    TokenKind::LParen => {
                        let paren = self.parse_paren_value();
                        return Expr::Paren(Box::new(paren));
                    }
                    TokenKind::Variable => {
                        self.bump(LexMode::Words);
                        return self.words_variable(token);
                    }
                    _ => {}
                }
            }
        }
        Expr::Str(StrLit {
            parts: vec![StrPart::Text {
                text: text.to_owned(),
                span,
            }],
            raw: true,
            span,
        })
    }

    /// Expands a words-mode `$name.field.field` token, whose steps the lexer folded in.
    pub(super) fn words_variable(&mut self, token: Token) -> Expr {
        let text = token.text(self.source);
        let start = token.span.start();
        let mut steps = text.split('.');
        let name = steps.next().unwrap_or("$");
        let mut offset = start + name.len() as u32;
        let mut expression = Expr::Variable(Variable {
            name: name.strip_prefix('$').unwrap_or(name).to_owned(),
            span: Span::new(start, offset),
        });
        for step in steps {
            let field_span = Span::new(offset + 1, offset + 1 + step.len() as u32);
            offset = field_span.end();
            expression = Expr::Field(Box::new(FieldAccess {
                span: Span::new(start, offset),
                base: expression,
                field: step.to_owned(),
                field_span,
                optional: false,
            }));
        }
        expression
    }

    /// Expands a words-mode `@`, `@-1` or `@3` token and any folded `.field` steps.
    pub(super) fn words_current_value(&mut self, token: Token) -> Expr {
        let text = token.text(self.source);
        let start = token.span.start();
        let mut steps = text.split('.');
        let head = steps.next().unwrap_or("@");
        let mut offset = start + head.len() as u32;
        let mut expression = Expr::CurrentValue(CurrentValue {
            selector: current_selector(head),
            span: Span::new(start, offset),
        });
        for step in steps {
            let field_span = Span::new(offset + 1, offset + 1 + step.len() as u32);
            offset = field_span.end();
            expression = Expr::Field(Box::new(FieldAccess {
                span: Span::new(start, offset),
                base: expression,
                field: step.to_owned(),
                field_span,
                optional: false,
            }));
        }
        expression
    }
}

/// Wraps a bare expression as the one-stage pipeline the AST models a binding's value as.
///
/// `let name = "world"` and `let hot = get process | …` then have one shape, so the evaluator has
/// one path rather than two that can drift apart.
pub(super) fn expression_as_pipeline(expression: Expr) -> Pipeline {
    let span = expression.span();
    Pipeline {
        head: StageList {
            stages: vec![Stage {
                head: StageHead::Value(expression),
                mode: ArgMode::Expression,
                arguments: Vec::new(),
                redirections: Vec::new(),
                span,
            }],
            span,
        },
        tail: Vec::new(),
        background: false,
        span,
    }
}
