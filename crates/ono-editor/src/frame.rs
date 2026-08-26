//! What the editor draws, and where the terminal cursor belongs within it.

use ono_render::{Presentation, Theme, Token};
use unicode_width::UnicodeWidthChar;

/// Cells between two columns of the candidate list, matching the gap `ono-render` puts between
/// table columns.
const COLUMN_GAP: usize = 2;

/// The marker that tells a reader a candidate was shortened, as spec §13.3 requires.
const TRUNCATION_MARKER: &str = "...";

/// One rendered picture of the editor's state.
///
/// The lines are ready to print. `cursor_row` indexes `lines`, and `cursor_column` counts
/// display cells from the left edge, so a caller can place the real cursor without knowing
/// anything about the characters that got it there.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Frame {
    /// The lines to print, top to bottom.
    pub lines: Vec<String>,
    /// The row the cursor sits on, as an index into `lines`.
    pub cursor_row: usize,
    /// The column the cursor sits at, in display cells.
    pub cursor_column: usize,
}

/// A display row under construction: a finished prefix and the painted runs after it.
pub(crate) struct RowDraft {
    prefix: String,
    segments: Vec<(String, Token)>,
}

impl RowDraft {
    pub(crate) fn new(prefix: String) -> Self {
        Self {
            prefix,
            segments: Vec::new(),
        }
    }

    /// Appends `text`, joining it to the previous run when the token has not changed so the
    /// finished line carries one escape per run rather than one per character.
    pub(crate) fn push(&mut self, text: &str, token: Token) {
        if let Some(last) = self.segments.last_mut()
            && last.1 == token
        {
            last.0.push_str(text);
            return;
        }
        self.segments.push((text.to_owned(), token));
    }

    pub(crate) fn finish(self, theme: &Theme, presentation: Presentation) -> String {
        let mut line = self.prefix;
        for (text, token) in self.segments {
            line.push_str(&theme.paint(&text, token, presentation));
        }
        line
    }
}

/// The display form of one character, and how many cells it occupies.
///
/// Control characters become the visible escape `ono-render` uses, because a line the user
/// pasted must never be able to drive the terminal (spec §49). A tab becomes one space, so the
/// reported cursor column is exactly where the cursor is.
pub(crate) fn display_char(character: char, scratch: &mut String) -> (&str, usize) {
    scratch.clear();
    match character {
        '\t' => {
            scratch.push(' ');
            (scratch.as_str(), 1)
        }
        control if control.is_control() => {
            scratch.push_str(&format!("\\u{{{:x}}}", control as u32));
            let width = scratch.chars().count();
            (scratch.as_str(), width)
        }
        other => {
            scratch.push(other);
            (scratch.as_str(), other.width().unwrap_or(0))
        }
    }
}

/// Lays candidates out in columns that fit `width` display cells.
///
/// Candidates run down each column before moving to the next, the order `ls` uses, so a long
/// alphabetical list stays readable.
pub(crate) fn candidate_lines(candidates: &[String], width: usize) -> Vec<String> {
    if candidates.is_empty() {
        return Vec::new();
    }
    let widest = candidates
        .iter()
        .map(|candidate| cell_width(candidate))
        .max()
        .unwrap_or(0)
        .max(1);
    let columns = ((width + COLUMN_GAP) / (widest + COLUMN_GAP)).max(1);
    let rows = candidates.len().div_ceil(columns);

    let mut lines = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut line = String::new();
        for column in 0..columns {
            let index = column * rows + row;
            let Some(candidate) = candidates.get(index) else {
                continue;
            };
            if column > 0 {
                line.push_str(&" ".repeat(COLUMN_GAP));
            }
            let text = shorten(candidate, width);
            let padding = widest.saturating_sub(cell_width(&text));
            line.push_str(&text);
            if index + rows < candidates.len() {
                line.push_str(&" ".repeat(padding));
            }
        }
        lines.push(line);
    }
    lines
}

/// The width of `text` in terminal cells, counting control characters as they are displayed.
pub(crate) fn cell_width(text: &str) -> usize {
    let mut scratch = String::new();
    text.chars()
        .map(|character| display_char(character, &mut scratch).1)
        .sum()
}

/// Shortens `text` to at most `width` cells, never splitting a character.
fn shorten(text: &str, width: usize) -> String {
    if cell_width(text) <= width {
        return text.to_owned();
    }
    let budget = width.saturating_sub(TRUNCATION_MARKER.len());
    let mut used = 0;
    let mut kept = String::new();
    let mut scratch = String::new();
    for character in text.chars() {
        let (display, cells) = display_char(character, &mut scratch);
        if used + cells > budget {
            break;
        }
        used += cells;
        kept.push_str(display);
    }
    if width > TRUNCATION_MARKER.len() {
        kept.push_str(TRUNCATION_MARKER);
    }
    kept
}
