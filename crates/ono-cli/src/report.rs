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
        // Both halves of the identity spec §43 gives an error are shown: the stable code a
        // report quotes, and the dotted name a script catches by (`catch e { $e.name }`). v0.4
        // §40 names its fourteen conditions in that vocabulary — `spatial.no_relation` — and a
        // condition a user can act on must be readable where the refusal appears (ADR-0148).
        let code = self.theme.paint(
            &format!("{} {}", error.code().code(), error.code().name()),
            Token::ErrorCode,
            self.presentation,
        );
        let _ = writeln!(
            out,
            "{}: {code} {}",
            ono_core::SHORT_NAME,
            ono_render::sanitise(error.message())
        );
        // §29.3's refusals list what they could not choose between, and a list is lines. The
        // *shell* decided there are several of them, so the structure is carried as data —
        // `details` — instead of as newlines inside the message, where the render boundary
        // could not tell them from the ones a filename brought with it (ADR-0211, ADR-0015 T1).
        // Each entry is still sanitised on its own, so a name cannot forge a line of its own.
        let listed = details(error);
        for line in listed.iter().take(SHOWN_DETAILS) {
            let _ = writeln!(out, "  {}", ono_render::sanitise(line));
        }
        if let Some(rest) = listed
            .len()
            .checked_sub(SHOWN_DETAILS)
            .filter(|rest| *rest > 0)
        {
            let _ = writeln!(out, "  … {rest} more");
        }
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

/// How many of a refusal's listed candidates reach the terminal.
///
/// The message already says how many there are, and a diagnostic that fills the screen with
/// ninety of them is not a diagnostic. The whole list stays on the error value, so a script that
/// catches it reads every candidate; this bounds only what is painted.
const SHOWN_DETAILS: usize = 10;

/// The lines a refusal listed under its message, where it listed any (ADR-0211).
///
/// The convention is one metadata entry, `details`, holding a list of strings. Anything else in
/// the metadata is machine detail — an errno, a provider id — and is not shown here.
fn details(error: &ErrorValue) -> Vec<String> {
    match error.metadata().get("details") {
        Some(ono_value::Value::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                ono_value::Value::String(text) => Some(text.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
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
