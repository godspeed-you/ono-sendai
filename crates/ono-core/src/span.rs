//! Byte ranges into a source line, used by every diagnostic.

use std::fmt;

/// A half-open byte range `[start, end)` into the source text a value came from.
///
/// Spec §16.3 requires parse errors to point at the relevant span, so spans travel with tokens,
/// AST nodes and diagnostics alike. Offsets are byte offsets, not character indices, because
/// that is what slicing a `&str` needs and what a terminal column calculation starts from.
///
/// ```
/// use ono_core::Span;
/// let source = "get process";
/// assert_eq!(Span::new(4, 11).of(source), "process");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Span {
    start: u32,
    end: u32,
}

impl Span {
    /// Creates a span covering `[start, end)`.
    ///
    /// The ends are ordered on construction, so a caller that computed them backwards gets a
    /// usable span rather than a panic in the middle of rendering a diagnostic.
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    /// A span of zero width at `offset`, used to point between two characters.
    #[must_use]
    pub const fn at(offset: u32) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    /// First byte offset covered by the span.
    #[must_use]
    pub const fn start(self) -> u32 {
        self.start
    }

    /// One past the last byte offset covered by the span.
    #[must_use]
    pub const fn end(self) -> u32 {
        self.end
    }

    /// Width of the span in bytes.
    #[must_use]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    /// Whether the span covers no bytes at all.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// The smallest span covering both operands, regardless of their order.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// The text this span points at, or `""` if it points outside `source`.
    ///
    /// Rendering a diagnostic must never panic, however wrong the span it was handed is.
    #[must_use]
    pub fn of(self, source: &str) -> &str {
        let start = self.start as usize;
        let end = self.end as usize;
        if start > end || end > source.len() {
            return "";
        }
        source.get(start..end).unwrap_or("")
    }

    /// One-based line and column of the span's start within `source`.
    ///
    /// The column counts characters, not bytes, because it exists to be shown to a person.
    #[must_use]
    pub fn line_column(self, source: &str) -> (u32, u32) {
        let offset = (self.start as usize).min(source.len());
        let before = source.get(..offset).unwrap_or("");
        let line = before.bytes().filter(|b| *b == b'\n').count() as u32 + 1;
        let column = before
            .rfind('\n')
            .map_or(before, |index| &before[index + 1..])
            .chars()
            .count() as u32
            + 1;
        (line, column)
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

impl From<std::ops::Range<u32>> for Span {
    fn from(range: std::ops::Range<u32>) -> Self {
        Self::new(range.start, range.end)
    }
}

impl From<Span> for std::ops::Range<usize> {
    fn from(span: Span) -> Self {
        span.start as usize..span.end as usize
    }
}
