//! Where a native pipeline's values go when nothing else consumes them.
//!
//! Spec §13.1 keeps presentation out of the language and out of providers: a pipeline carries
//! typed values, and this is the one place they become characters. Spec §4.6 decides how many:
//! a terminal gets a table, a pipe and a file get the same table with no escape sequences, and
//! nothing about the values themselves changes either way.

use std::io::{IsTerminal, Write};

use ono_pipeline::{StreamEvent, ValueStream};
use ono_render::{Layout, Presentation, Renderer, Theme, View};
use ono_value::{ErrorValue, Value};

/// How wide the output is, and how much decoration it may carry.
#[derive(Debug, Clone)]
pub struct Sink {
    width: usize,
    presentation: Presentation,
    theme: Theme,
    view: View,
    max_rows: Option<usize>,
}

impl Sink {
    /// A sink for the shell's standard output, sized and styled for whatever is on the other end.
    #[must_use]
    pub fn for_stdout(environment: &[(&str, &str)]) -> Self {
        let is_terminal = std::io::stdout().is_terminal();
        Self {
            width: terminal_width(is_terminal),
            presentation: Presentation::choose(is_terminal, environment),
            theme: Theme::default(),
            view: View::Table,
            max_rows: None,
        }
    }

    /// Renders in a particular view rather than letting the values choose (spec §13.6).
    #[must_use]
    pub fn with_view(mut self, view: View) -> Self {
        self.view = view;
        self
    }

    /// Shows at most `max_rows` rows, and says how many were left out.
    #[must_use]
    pub fn with_max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows);
        self
    }

    /// The presentation this sink was built for.
    #[must_use]
    pub fn presentation(&self) -> Presentation {
        self.presentation
    }

    /// Drains `stream`, rendering its values and collecting its errors.
    ///
    /// Errors do not stop the rendering. A bulk operation reports what succeeded *and* what
    /// failed (spec §16.5), so the values that arrived are shown and the failures are returned to
    /// the caller to report and to derive a status from.
    pub async fn drain(&self, mut stream: ValueStream) -> Vec<ErrorValue> {
        let mut values = Vec::new();
        let mut failures = Vec::new();

        while let Some(event) = stream.recv().await {
            match event {
                StreamEvent::Value(value) => values.push(value),
                StreamEvent::Failure(error) => failures.push(error),
            }
        }

        self.write(&values);
        failures
    }

    /// Writes already-collected values.
    ///
    /// Rendering a table needs to know its widest cell, so a table is necessarily collected
    /// first. That is a property of tables, not of the pipeline: `to json` and the other
    /// serialising views stream, and a future in-place `watch` renderer (spec §18.3) will too.
    pub fn write(&self, values: &[Value]) {
        if values.is_empty() {
            return;
        }
        let renderer = Renderer::new();
        let mut layout = Layout::new(self.width);
        if let Some(max_rows) = self.max_rows {
            layout = layout.max_rows(max_rows);
        }

        let lines =
            layout.render_view_styled(&renderer, values, self.view, &self.theme, self.presentation);

        let mut out = std::io::stdout().lock();
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
        let _ = out.flush();
    }
}

/// The terminal's width, or the width a redirected stream is laid out for.
///
/// Redirected output is laid out at a fixed width so it is byte-for-byte reproducible: spec §4.6
/// requires deterministic behaviour when output is not a terminal, and a table whose column
/// widths depended on the terminal that happened to be attached would not be.
fn terminal_width(is_terminal: bool) -> usize {
    const REDIRECTED: usize = 80;
    const NARROWEST_USABLE: usize = 20;

    if !is_terminal {
        return REDIRECTED;
    }
    if let Ok(columns) = std::env::var("COLUMNS")
        && let Ok(columns) = columns.parse::<usize>()
        && columns >= NARROWEST_USABLE
    {
        return columns;
    }
    ono_editor::terminal_size()
        .ok()
        .map(|(columns, _)| columns)
        .filter(|columns| *columns >= NARROWEST_USABLE)
        .unwrap_or(REDIRECTED)
}
