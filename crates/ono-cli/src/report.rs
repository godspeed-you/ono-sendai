//! Showing a person what went wrong.
//!
//! Spec §16.2: errors are terse by default and rich on demand. A parse error additionally points
//! at the text it is about (spec §16.3), because a span nobody can see is a span nobody can use.

use std::io::Write;

use ono_parser::Diagnostic;
use ono_render::{Presentation, Theme, Token};
use ono_value::ErrorValue;

/// Where diagnostics go, and how much decoration they may carry.
pub struct Reporter {
    theme: Theme,
    presentation: Presentation,
}

impl Reporter {
    /// A reporter writing to standard error with the given presentation.
    #[must_use]
    pub fn new(presentation: Presentation) -> Self {
        Self {
            theme: Theme::default(),
            presentation,
        }
    }

    /// Reports a structured error in its terse form (spec §16.2).
    ///
    /// The message is sanitised, not merely the code and the hint. An error message is where
    /// attacker-controlled text most reliably reaches a terminal — a path, a process name, a
    /// command that was not found — and `cd` into a directory whose name carries an OSC sequence
    /// would otherwise retitle the window (ADR-0015 T1).
    pub fn error(&self, error: &ErrorValue) {
        let mut out = std::io::stderr().lock();
        let code = self
            .theme
            .paint(error.code().code(), Token::ErrorCode, self.presentation);
        let _ = writeln!(
            out,
            "{}: {code} {}",
            ono_core::SHORT_NAME,
            ono_render::sanitise(error.message())
        );
        if let Some(help) = error.help() {
            let hint = self.theme.paint(help, Token::ErrorHint, self.presentation);
            let _ = writeln!(out, "  {hint}");
        }
    }

    /// Reports a parse diagnostic, showing the line and marking the span (spec §16.3).
    pub fn diagnostic(&self, source: &str, diagnostic: &Diagnostic) {
        let mut out = std::io::stderr().lock();
        let code = self.theme.paint(
            diagnostic.code().code(),
            Token::ErrorCode,
            self.presentation,
        );
        let (line_number, column) = diagnostic.span().line_column(source);
        // The message quotes the offending token, so it carries whatever was typed — sanitised
        // for the same reason the echoed line below it is (ADR-0015 T1).
        let _ = writeln!(
            out,
            "{}: {code} {} (line {line_number}, column {column})",
            ono_core::SHORT_NAME,
            ono_render::sanitise(diagnostic.message())
        );

        if let Some(line) = source.lines().nth(line_number as usize - 1) {
            let (shown, shift) = window(line, column);
            let _ = writeln!(out, "  {}", ono_render::sanitise(&shown));
            let _ = writeln!(
                out,
                "  {}",
                marker(
                    &shown,
                    column.saturating_sub(shift),
                    diagnostic.span().len()
                )
            );
        }
        if let Some(help) = diagnostic.help() {
            let hint = self.theme.paint(help, Token::ErrorHint, self.presentation);
            let _ = writeln!(out, "  {hint}");
        }
    }
}

/// How much of a line a diagnostic may show, in characters.
///
/// A shell takes whatever is typed at it, and what is typed at it is sometimes a hundred thousand
/// characters on one line. Echoing all of it turns a diagnostic into a wall, and the part that
/// matters is around the span.
const SHOWN_WIDTH: usize = 120;

/// The part of `line` worth showing, and how many characters were cut from its start.
///
/// A line that fits is shown whole. A longer one is windowed around the offending column, with
/// `...` standing in for what was cut, so the reader can see that the line continues.
fn window(line: &str, column: u32) -> (String, u32) {
    const MARKER: &str = "...";

    let characters: Vec<char> = line.chars().collect();
    if characters.len() <= SHOWN_WIDTH {
        return (line.to_owned(), 0);
    }

    let focus = (column.saturating_sub(1) as usize).min(characters.len());
    let half = SHOWN_WIDTH / 2;
    let start = focus.saturating_sub(half);
    let end = (start + SHOWN_WIDTH).min(characters.len());

    let mut shown = String::new();
    if start > 0 {
        shown.push_str(MARKER);
    }
    shown.extend(&characters[start..end]);
    if end < characters.len() {
        shown.push_str(MARKER);
    }

    // The caret has to move with the text. Cutting `start` characters and adding the marker back
    // shifts everything by `start - MARKER.len()`.
    let shift = if start > 0 {
        (start - MARKER.len().min(start)) as u32
    } else {
        0
    };
    (shown, shift)
}

/// A caret run under the offending text, measured in display cells so it lines up.
fn marker(shown: &str, column: u32, width: u32) -> String {
    let start = column.saturating_sub(1) as usize;
    let width = shown
        .chars()
        .skip(start)
        .take(width.max(1) as usize)
        .count()
        .max(1);
    format!("{}{}", " ".repeat(start), "^".repeat(width))
}
