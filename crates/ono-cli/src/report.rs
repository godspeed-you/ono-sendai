//! Showing a person what went wrong.
//!
//! Spec §16.2: errors are terse by default and rich on demand. A parse error additionally points
//! at the text it is about (spec §16.3), because a span nobody can see is a span nobody can use.

use std::io::Write;

use ono_core::Span;
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
    pub fn error(&self, error: &ErrorValue) {
        let mut out = std::io::stderr().lock();
        let code = self
            .theme
            .paint(error.code().code(), Token::ErrorCode, self.presentation);
        let _ = writeln!(out, "{}: {code} {}", ono_core::SHORT_NAME, error.message());
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
        let _ = writeln!(
            out,
            "{}: {code} {} (line {line_number}, column {column})",
            ono_core::SHORT_NAME,
            diagnostic.message()
        );

        if let Some(line) = source.lines().nth(line_number as usize - 1) {
            let shown = ono_render::sanitise(line);
            let _ = writeln!(out, "  {shown}");
            let _ = writeln!(out, "  {}", marker(line, diagnostic.span(), source));
        }
        if let Some(help) = diagnostic.help() {
            let hint = self.theme.paint(help, Token::ErrorHint, self.presentation);
            let _ = writeln!(out, "  {hint}");
        }
    }
}

/// A caret run under the offending text, measured in display cells so it lines up.
fn marker(line: &str, span: Span, source: &str) -> String {
    let (line_number, column) = span.line_column(source);
    let _ = line_number;
    let start = column.saturating_sub(1) as usize;
    let width = line
        .chars()
        .skip(start)
        .take(span.len().max(1) as usize)
        .count()
        .max(1);
    format!("{}{}", " ".repeat(start), "^".repeat(width))
}
