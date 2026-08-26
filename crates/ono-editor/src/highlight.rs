//! Where the colours of a line being typed come from.
//!
//! The editor does **not** depend on the parser. Highlighting arrives through this trait, which
//! the shell implements over its incremental parse (spec §24.4). That keeps the layering of
//! ADR-0005 intact — the editor is above the renderer and beside the parser, never below it —
//! and it keeps every behaviour in this crate testable without a grammar.

use ono_core::Span;
use ono_render::Token;

/// Turns the line being typed into painted spans, and says whether it is a finished statement.
pub trait Highlighter {
    /// Spans of the line and the semantic token each should be painted with.
    ///
    /// Spans are byte ranges into `line`. Anything not covered is painted as ordinary
    /// foreground. Overlapping spans are resolved in favour of the one that starts earlier.
    fn highlight(&self, line: &str) -> Vec<(Span, Token)>;

    /// Whether the line is a complete statement, so Enter submits rather than continuing.
    ///
    /// This is exactly the distinction `parse.incomplete` (E0002) draws from `parse.syntax`
    /// (E0001) in ADR-0009: an unclosed quote or brace, or a trailing `|`, is still being typed
    /// and must not be executed.
    fn is_complete(&self, line: &str) -> bool {
        let _ = line;
        true
    }
}

/// A highlighter that paints nothing and accepts every line.
///
/// It is what the editor uses until the shell hands it a real one, and what the editor's own
/// tests use where the highlight is beside the point.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlainHighlighter;

impl Highlighter for PlainHighlighter {
    fn highlight(&self, line: &str) -> Vec<(Span, Token)> {
        let _ = line;
        Vec::new()
    }
}
