//! The recoverable recursive-descent parser of ADR-0009.
//!
//! The parser never fails and never panics: it always returns a tree, collecting problems as
//! diagnostics instead of throwing them. Every construct has a recovery point — the end of a
//! stage at `|`, the end of a statement at a newline or `;`, the closing delimiter of a
//! bracketed form — so a half-typed line still produces something the editor can highlight.

use crate::ast::{Argument, Program};
use crate::diagnostic::Diagnostic;
use crate::lexer::{LexMode, Token};

use self::pipelines::ends_stage;
use self::state::Parser;

mod blocks;
mod diagnostics;
mod expressions;
mod literals;
mod pipelines;
mod recovery;
mod state;
mod statements;

/// The result of parsing a source: a tree, plus everything that was wrong with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    program: Program,
    diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    /// The tree. Always present, even for input that was rejected outright.
    #[must_use]
    pub fn program(&self) -> &Program {
        &self.program
    }

    /// Everything the parser found wrong, in source order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Whether the input ends outside every construct it opened.
    ///
    /// A line that is merely unfinished — an open quote, an open bracket, a trailing `|` — is
    /// not complete but is not wrong either, which is the distinction the editor needs
    /// (ADR-0009).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(crate::diagnostic::Diagnostic::is_incomplete)
    }

    /// Whether the input contains something no amount of further typing can rescue.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| !diagnostic.is_incomplete())
    }

    /// Takes the tree and the diagnostics apart, for a caller that wants to own both.
    #[must_use]
    pub fn into_parts(self) -> (Program, Vec<Diagnostic>) {
        (self.program, self.diagnostics)
    }
}

/// Parses a source into a tree and a list of diagnostics.
///
/// The call never panics and always returns a tree, whatever it is handed.
///
/// ```
/// let parsed = ono_parser::parse("get process | where cpu > 20");
/// assert!(parsed.diagnostics().is_empty());
/// assert_eq!(parsed.program().statements.len(), 1);
/// ```
#[must_use]
pub fn parse(source: &str) -> Parsed {
    let mut parser = Parser::new(source);
    let program = parser.parse_program();
    Parsed {
        program,
        diagnostics: parser.diagnostics,
    }
}

/// Lexes a source into the tokens the parser actually read, for syntax highlighting.
///
/// Because a token's class depends on the mode its stage is in, the stream is produced by the
/// parser rather than by a standalone lexer: `>` comes back as a redirection in `cat a > b` and
/// as a comparison in `where a > b`. Tokens are ordered, non-overlapping and inside the source.
///
/// ```
/// use ono_parser::TokenKind;
/// let kinds: Vec<TokenKind> = ono_parser::tokens("ls -la").iter().map(|t| t.kind).collect();
/// assert_eq!(kinds, vec![TokenKind::Word, TokenKind::Word]);
/// ```
#[must_use]
pub fn tokens(source: &str) -> Vec<Token> {
    let mut parser = Parser::new(source);
    parser.record = Some(Vec::new());
    let _ = parser.parse_program();
    parser.record.unwrap_or_default()
}

/// The arguments `text` reads as, in words mode.
///
/// A stage's argument mode is fixed at parse time by its head word (ADR-0009), before anything
/// knows whether the head resolves to a native command or to a program of the same name. Where
/// resolution chooses the program, its arguments are the words the user typed — `sort -r
/// /tmp/a` is a flag and a path, not the arithmetic that `-r / tmp / a` also is — and this
/// re-reads exactly that region of the source in the mode the program deserves (ADR-0260).
///
/// The spans of the returned arguments are offsets into `text`, so an option's value expression
/// must be evaluated against `text` and not against the whole line.
///
/// ```
/// use ono_parser::{Argument, words_arguments};
/// let arguments = words_arguments("-r /tmp/a");
/// assert_eq!(arguments.len(), 2);
/// assert!(matches!(&arguments[0], Argument::Word(word) if word.text == "-r"));
/// ```
#[must_use]
pub fn words_arguments(text: &str) -> Vec<Argument> {
    let mut parser = Parser::new(text);
    let mut arguments = Vec::new();
    loop {
        let token = parser.peek(LexMode::Words);
        if ends_stage(token.kind) {
            break;
        }
        let before = parser.pos;
        arguments.push(parser.parse_words_argument(token, None));
        if parser.pos == before {
            parser.bump(LexMode::Words);
        }
    }
    arguments
}
