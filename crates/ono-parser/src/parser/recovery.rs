//! Recovery and incomplete input: where the parser resumes after something it could not read.
//!
//! Every construct has a recovery point — the end of a statement at a newline or `;`, the
//! closing brace of a block — and the rules for reaching one live here, together with the test
//! for input that is merely unfinished rather than wrong.

use ono_core::Span;

use crate::lexer::{LexMode, Token, TokenKind};

use super::state::Parser;

impl Parser<'_> {
    /// Whether more typing at the end of the input could still turn `token` into something valid.
    ///
    /// The end of the input is the one place where a token may be a fragment of a longer one, so
    /// a problem there is reported as unfinished rather than as wrong. A closing delimiter is
    /// the exception: nothing appended after it can rescue it.
    pub(super) fn is_possibly_unfinished(&self, token: Token) -> bool {
        token.kind == TokenKind::Eof
            || (token.span.end() as usize == self.limit && !token.kind.is_closing_delimiter())
    }

    pub(super) fn finish_statement(&mut self, block_terminated: bool) {
        let token = self.peek(LexMode::Words);
        match token.kind {
            TokenKind::Semi | TokenKind::Newline => {
                self.bump(LexMode::Words);
            }
            TokenKind::Eof | TokenKind::RBrace => {}
            _ if block_terminated => {}
            _ => {
                let description = self.describe(token);
                self.report_unexpected(
                    token,
                    format!("expected the end of the statement, found {description}"),
                );
                self.recover_to_statement_end();
            }
        }
    }

    pub(super) fn recover_to_statement_end(&mut self) {
        loop {
            let token = self.peek(LexMode::Words);
            match token.kind {
                TokenKind::Eof | TokenKind::RBrace => return,
                TokenKind::Semi | TokenKind::Newline => {
                    self.bump(LexMode::Words);
                    return;
                }
                _ => {
                    self.bump(LexMode::Words);
                }
            }
        }
    }

    /// Consumes tokens up to the `}` that closes the block whose `{` was just taken.
    ///
    /// Counting braces rather than recursing is the point: the input that gets here is the input
    /// that was too deep to recurse through.
    pub(super) fn skip_balanced_block(&mut self) -> Span {
        let mut open_braces = 1usize;
        loop {
            let token = self.peek(LexMode::Words);
            match token.kind {
                TokenKind::Eof => return token.span,
                TokenKind::LBrace => open_braces += 1,
                TokenKind::RBrace => {
                    open_braces -= 1;
                    if open_braces == 0 {
                        return self.bump(LexMode::Words).span;
                    }
                }
                _ => {}
            }
            self.bump(LexMode::Words);
        }
    }
}
