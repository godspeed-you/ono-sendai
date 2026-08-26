//! Where completion candidates come from.
//!
//! Like highlighting, completion is a trait the editor defines and the shell implements from the
//! command, verb and schema registries (spec §15.1). The editor knows how to ask, how to insert
//! and how to lay the answer out; it knows nothing about what a `process` is.

use ono_core::Span;

/// What a completer found for the word under the cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completion {
    /// The bytes of the line the candidates would replace.
    pub span: Span,
    /// The candidates, in the order they should be offered.
    pub candidates: Vec<String>,
}

impl Completion {
    /// Candidates replacing `span`.
    #[must_use]
    pub fn new(span: Span, candidates: Vec<String>) -> Self {
        Self { span, candidates }
    }

    /// No candidates at all.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether there is nothing to offer.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// The longest prefix every candidate shares, in whole characters.
    #[must_use]
    pub fn common_prefix(&self) -> &str {
        let Some(first) = self.candidates.first() else {
            return "";
        };
        let mut end = first.len();
        for candidate in &self.candidates[1..] {
            let shared = first
                .char_indices()
                .zip(candidate.char_indices())
                .take_while(|((_, left), (_, right))| left == right)
                .map(|((offset, character), _)| offset + character.len_utf8())
                .last()
                .unwrap_or(0);
            end = end.min(shared);
        }
        first.get(..end).unwrap_or("")
    }
}

/// Answers "what could the word under the cursor become?".
pub trait Completer {
    /// The candidates for the word ending at `cursor`, a byte offset into `line`.
    fn complete(&self, line: &str, cursor: usize) -> Completion;
}

/// A completer that never has anything to offer.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCompleter;

impl Completer for NoCompleter {
    fn complete(&self, line: &str, cursor: usize) -> Completion {
        let _ = (line, cursor);
        Completion::none()
    }
}
