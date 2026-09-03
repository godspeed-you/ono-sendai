//! The parser's own state, and the token access every other module reads the source through.
//!
//! Nothing here decides what a construct means: `peek`, `bump` and `eat` hand the next token
//! over in the lexical mode the caller asks for, and the recording hook behind them is what
//! `tokens` uses to reconstruct the stream a highlighter needs.

use crate::diagnostic::Diagnostic;
use crate::lexer::{LexMode, Token, TokenKind, next_token};

/// How deeply constructs may nest before the parser stops descending.
///
/// Adversarial input is a given for a shell (spec §35.6); the limit keeps a line of ten thousand
/// open parentheses a diagnostic rather than a stack overflow.
pub(super) const MAX_DEPTH: u32 = 96;

pub(super) struct Parser<'a> {
    pub(super) source: &'a str,
    pub(super) limit: usize,
    pub(super) pos: u32,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) record: Option<Vec<Token>>,
    pub(super) depth: u32,
    pub(super) no_brace: bool,
}

impl<'a> Parser<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        Self {
            source,
            limit: source.len(),
            pos: 0,
            diagnostics: Vec::new(),
            record: None,
            depth: 0,
            no_brace: false,
        }
    }

    pub(super) fn text(&self) -> &'a str {
        self.source.get(..self.limit).unwrap_or(self.source)
    }
}

impl Parser<'_> {
    /// The next significant token, without consuming it. Comments are consumed on sight.
    pub(super) fn peek(&mut self, mode: LexMode) -> Token {
        loop {
            let token = next_token(self.text(), self.pos, mode);
            if token.kind == TokenKind::Comment {
                self.pos = token.span.end();
                if let Some(record) = &mut self.record {
                    record.push(token);
                }
                continue;
            }
            return token;
        }
    }

    /// The token after `token`, without consuming anything.
    pub(super) fn peek_after(&self, mode: LexMode, token: Token) -> Token {
        let mut at = token.span.end();
        loop {
            let next = next_token(self.text(), at, mode);
            if next.kind == TokenKind::Comment {
                at = next.span.end();
                continue;
            }
            return next;
        }
    }

    pub(super) fn bump(&mut self, mode: LexMode) -> Token {
        let token = self.peek(mode);
        if token.kind != TokenKind::Eof {
            self.pos = token.span.end();
            if let Some(record) = &mut self.record {
                record.push(token);
            }
        }
        token
    }

    pub(super) fn eat(&mut self, mode: LexMode, kind: TokenKind) -> Option<Token> {
        let token = self.peek(mode);
        (token.kind == kind).then(|| self.bump(mode))
    }

    pub(super) fn at_keyword(&mut self, mode: LexMode, keyword: &str) -> bool {
        let token = self.peek(mode);
        matches!(token.kind, TokenKind::Ident | TokenKind::Word)
            && token.text(self.source) == keyword
    }

    pub(super) fn skip_newlines(&mut self, mode: LexMode) {
        while self.peek(mode).kind == TokenKind::Newline {
            self.bump(mode);
        }
    }

    pub(super) fn skip_separators(&mut self) {
        while matches!(
            self.peek(LexMode::Words).kind,
            TokenKind::Newline | TokenKind::Semi
        ) {
            self.bump(LexMode::Words);
        }
    }
}
