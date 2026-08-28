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

    /// A sink for a file, laid out at the fixed width redirected output uses.
    ///
    /// Spec §4.6 requires deterministic bytes when the destination is not a terminal, so nothing
    /// about this depends on the terminal that happens to be attached to the shell.
    #[must_use]
    pub fn for_file() -> Self {
        Self {
            width: terminal_width(false),
            presentation: Presentation::Redirect,
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
        let lines = self.render(values);
        if lines.is_empty() {
            return;
        }
        let mut out = std::io::stdout().lock();
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
        let _ = out.flush();
    }

    /// The lines this sink would write, for a caller that sends them somewhere else.
    #[must_use]
    pub fn render(&self, values: &[Value]) -> Vec<String> {
        if values.is_empty() {
            return Vec::new();
        }
        // A graph never renders as a table (spec §13.6): its record revives and draws as the
        // trees it holds, wherever it came from — a live trace, a file, a pipe.
        if let [value] = values
            && let Ok(record) = value.as_record()
            && record.schema_id().to_string() == "ono.graph/1"
            && let Ok(graph) = ono_graph::Graph::from_record(record)
        {
            let layout = Layout::new(self.width);
            return graph
                .trees()
                .iter()
                .flat_map(|tree| layout.render_tree_styled(tree, &self.theme, self.presentation))
                .collect();
        }
        // A place view never renders as a table (spec v0.4 §6.1, §23.1): its headings are
        // presentation over a structured object, and the renderer that knows them is
        // `ono-spatial-render`, which may not invent an exit the view did not declare (§45.4).
        if let [value] = values
            && let Ok(record) = value.as_record()
            && record.schema_id().to_string() == "ono.place-view/1"
        {
            return ono_spatial_render::place_view(record, self.width);
        }
        // Nor does a map (spec v0.4 §23.2): "Every terminal MUST have a non-fullscreen textual
        // map representation", and §39.3 makes that representation adapt to the width rather than
        // wrap. The width is therefore the one the environment states even when stdout is a pipe,
        // because a map laid out for a terminal nobody has is a map that does not fit.
        if let [value] = values
            && let Ok(record) = value.as_record()
            && record.schema_id().to_string() == "ono.spatial-map/1"
        {
            return ono_spatial_render::spatial_map(record, map_width(self.width), map_charset());
        }
        let renderer = Renderer::new();
        let mut layout = Layout::new(self.width);
        if let Some(max_rows) = self.max_rows {
            layout = layout.max_rows(max_rows);
        }

        layout.render_view_styled(&renderer, values, self.view, &self.theme, self.presentation)
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

/// How wide a map may be drawn (spec v0.4 §39.3).
///
/// §39.3 is explicit that a map adapts to the terminal — "At narrow widths, maps MAY collapse
/// into ranked tree/list projections" — and a map is the one view whose whole point is to fit.
/// So `COLUMNS` is honoured wherever it is stated, including for redirected output, which stays
/// deterministic because the environment is part of the run (spec v0.2 §4.6).
fn map_width(fallback: usize) -> usize {
    const NARROWEST_USABLE: usize = 20;
    std::env::var("COLUMNS")
        .ok()
        .and_then(|columns| columns.parse::<usize>().ok())
        .filter(|columns| *columns >= NARROWEST_USABLE)
        .unwrap_or(fallback)
}

/// Whether the terminal can be promised box-drawing characters (spec v0.4 §39.2).
///
/// §39.2 requires an ASCII fallback to exist; this is when it is taken. A terminal that says it
/// is `dumb`, and a locale that does not promise UTF-8, both get ASCII — guessing wrong here
/// prints mojibake, which is worse than a plainer drawing.
#[must_use]
pub fn map_charset() -> ono_spatial_render::Charset {
    let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .is_some_and(|locale| {
            let locale = locale.to_ascii_uppercase();
            locale.contains("UTF-8") || locale.contains("UTF8")
        });
    let dumb = std::env::var("TERM").is_ok_and(|term| term == "dumb");
    if utf8 && !dumb {
        ono_spatial_render::Charset::Unicode
    } else {
        ono_spatial_render::Charset::Ascii
    }
}
