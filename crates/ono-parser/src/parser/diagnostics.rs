//! Diagnostic construction: how the parser says what is wrong, and how it names what it saw.
//!
//! `expect` and `close` are here rather than beside their callers because a missing token is
//! reported in one shape wherever it is missing — the distinction between "still typing" and
//! "broken" (ADR-0009) is decided by `Diagnostic::incomplete` and nothing else.

use ono_core::Span;

use crate::diagnostic::Diagnostic;
use crate::lexer::{LexMode, Token, TokenKind};

use super::state::Parser;

impl Parser<'_> {
    pub(super) fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Reports that `token` was not what the grammar expected here.
    pub(super) fn report_unexpected(&mut self, token: Token, message: impl Into<String>) {
        let diagnostic = if self.is_possibly_unfinished(token) {
            Diagnostic::incomplete(token.span, message)
        } else {
            Diagnostic::syntax(token.span, message)
        };
        self.report(diagnostic);
    }

    pub(super) fn describe(&self, token: Token) -> String {
        match token.kind {
            TokenKind::Eof => "the end of the input".to_owned(),
            TokenKind::Newline => "the end of the line".to_owned(),
            _ => format!("`{}`", token.text(self.source)),
        }
    }

    pub(super) fn expect_ident(&mut self, what: &str) -> (String, Span) {
        let token = self.peek(LexMode::Expr);
        if token.kind == TokenKind::Ident {
            self.bump(LexMode::Expr);
            return (token.text(self.source).to_owned(), token.span);
        }
        let description = self.describe(token);
        self.report_unexpected(token, format!("expected {what}, found {description}"));
        (String::new(), Span::at(token.span.start()))
    }

    pub(super) fn expect(&mut self, mode: LexMode, kind: TokenKind, what: &str) -> Option<Token> {
        let token = self.peek(mode);
        if token.kind == kind {
            return Some(self.bump(mode));
        }
        let description = self.describe(token);
        self.report_unexpected(token, format!("expected {what}, found {description}"));
        None
    }

    /// Consumes the closing delimiter of a bracketed form, or reports that it is missing.
    pub(super) fn close(&mut self, kind: TokenKind, open: Token, what: &str) -> Option<Span> {
        self.skip_newlines(LexMode::Expr);
        if let Some(token) = self.eat(LexMode::Expr, kind) {
            return Some(token.span);
        }
        let token = self.peek(LexMode::Expr);
        if token.kind == TokenKind::Eof {
            let opener = open.text(self.source);
            self.report(Diagnostic::incomplete(
                open.span,
                format!("this `{opener}` is never closed"),
            ));
        } else {
            let description = self.describe(token);
            self.report_unexpected(token, format!("expected {what}, found {description}"));
        }
        None
    }
}
