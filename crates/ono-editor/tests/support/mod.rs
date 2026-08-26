//! Fixtures shared by the editor's behaviour tests.
//!
//! The editor is deliberately parser-free: highlighting and completion arrive through traits.
//! These fixtures are the test's own implementations of those traits, so every behaviour below
//! is observable without a parser, a terminal or a filesystem.
#![allow(
    dead_code,
    reason = "each integration test file uses a different part of the shared fixture"
)]
#![allow(
    clippy::panic,
    reason = "a shared test helper states its preconditions the way a test does"
)]

use ono_core::Span;
use ono_editor::{Completer, Completion, Editor, Highlighter, KeyPress};
use ono_render::Token;

/// A highlighter with just enough of the Ono grammar to drive the editor's tests.
///
/// The head word is painted as the accent, double-quoted runs as strings; a line with an open
/// quote, an open brace or a trailing `|` is incomplete, which is what `parse.incomplete` means
/// for the real parser.
pub struct DemoHighlighter;

impl Highlighter for DemoHighlighter {
    fn highlight(&self, line: &str) -> Vec<(Span, Token)> {
        let mut spans = Vec::new();
        let head = line.len() - line.trim_start().len();
        let head_end = head
            + line[head..]
                .find(char::is_whitespace)
                .unwrap_or(line.len() - head);
        if head_end > head {
            spans.push((Span::new(head as u32, head_end as u32), Token::Accent));
        }
        let mut start: Option<usize> = None;
        for (offset, character) in line.char_indices() {
            if character == '"' {
                match start {
                    None => start = Some(offset),
                    Some(open) => {
                        spans.push((
                            Span::new(open as u32, (offset + 1) as u32),
                            Token::ValueString,
                        ));
                        start = None;
                    }
                }
            }
        }
        if let Some(open) = start {
            spans.push((
                Span::new(open as u32, line.len() as u32),
                Token::ValueString,
            ));
        }
        spans
    }

    fn is_complete(&self, line: &str) -> bool {
        let quotes = line.chars().filter(|c| *c == '"').count();
        let opens = line.chars().filter(|c| *c == '{').count();
        let closes = line.chars().filter(|c| *c == '}').count();
        quotes % 2 == 0 && opens <= closes && !line.trim_end().ends_with('|')
    }
}

/// A completer over a fixed candidate list, matching the word before the cursor.
pub struct WordCompleter {
    candidates: Vec<String>,
}

impl WordCompleter {
    /// A completer offering `candidates`.
    pub fn new<S: Into<String>>(candidates: Vec<S>) -> Self {
        Self {
            candidates: candidates.into_iter().map(Into::into).collect(),
        }
    }
}

impl Completer for WordCompleter {
    fn complete(&self, line: &str, cursor: usize) -> Completion {
        let start = line[..cursor].rfind(' ').map_or(0, |index| index + 1);
        let prefix = &line[start..cursor];
        let matches = self
            .candidates
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .cloned()
            .collect();
        Completion::new(Span::new(start as u32, cursor as u32), matches)
    }
}

/// Feeds every character of `text` to the editor as an unmodified key press.
pub fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        editor.feed(KeyPress::char(character));
    }
}
