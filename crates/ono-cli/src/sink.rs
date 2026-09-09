//! Where a native pipeline's values go when nothing else consumes them.
//!
//! Spec §13.1 keeps presentation out of the language and out of providers: a pipeline carries
//! typed values, and this is the one place they become characters. Spec §4.6 decides how many:
//! a terminal gets a table, a pipe and a file get the same table with no escape sequences, and
//! nothing about the values themselves changes either way.

use std::io::{IsTerminal, Write};
use std::sync::Arc;

use ono_pipeline::{StreamEvent, ValueStream};
use ono_render::{Layout, Presentation, Renderer, Theme, View};
use ono_value::{ErrorValue, RecordValue, Value};

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

    /// Paints with `theme` rather than the default one (spec §44, ADR-0332).
    #[must_use]
    pub fn with_theme(mut self, theme: &Theme) -> Self {
        self.theme = theme.clone();
        self
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
        // §13.3 renders a whole comparison at once: the classes are headings and the objects are
        // grouped under them, so a stream of `ono.temporal-change/1` is one rendering rather than
        // one per row.
        if values.len() > 1
            && let Some(changes) = every_change(values)
        {
            return ono_temporal_render::changes(&changes, self.width, &temporal_options());
        }
        // §11.4 makes `timeline` a stream of events and the renderer "only a presentation"; §11.7
        // obliges that presentation to draw a coverage gap inside the window. The window is not in
        // the stream, so the command published it and this is where the two meet again: the events
        // at hand — filtered or not — are drawn against the interval they came from (ADR-0778).
        if let Some(view) = timeline_view_of(values) {
            return ono_temporal_render::timeline(&view, self.width, &temporal_options());
        }
        // The temporal views are presentation over one record each, and the renderer that knows
        // them is `ono-temporal-render` (v0.5 §39.3). Every arm is keyed on one schema id, and
        // the options — the session's UTC offset and `temporal.ui.show_source_tags` — are handed
        // over rather than read, because §39.2 gives a renderer its settings (ADR-0694).
        if let [value] = values
            && let Ok(record) = value.as_record()
        {
            let options = temporal_options();
            match record.schema_id().to_string().as_str() {
                "ono.temporal-change/1" => {
                    return ono_temporal_render::changes(
                        &[RecordValue::clone(record)],
                        self.width,
                        &options,
                    );
                }
                "ono.temporal-timeline/1" => {
                    return ono_temporal_render::timeline(record, self.width, &options);
                }
                "ono.causal-explanation/1" => {
                    return ono_temporal_render::causal_explanation(record, self.width, &options);
                }
                "ono.recorder-status/1" => {
                    return ono_temporal_render::recorder_status(record, self.width, &options);
                }
                "ono.temporal-context/1" => {
                    return ono_temporal_render::temporal_hud(record, self.width, &options);
                }
                "ono.temporal-gap/1" => {
                    return ono_temporal_render::gap_frame(record, self.width, &options);
                }
                _ => {}
            }
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

/// The window the last `timeline` in this process was a statement about (§11.2, §11.7, §8.5).
///
/// §11.4 makes the value a stream of events and the renderer "only a presentation", which leaves
/// the presentation needing something the stream does not carry: the bounds of the window, the
/// coverage behind it and the gaps §11.7 obliges a renderer to draw. The command publishes it here
/// rather than wrapping it around the events, which is what lets both rules hold at once — the
/// pipeline filters events, and this file still knows what interval it is drawing.
///
/// One slot per process, written by `timeline` and read on the same turn. A stage that filters the
/// stream narrows what is drawn and leaves the window it was drawn from intact, which is the
/// honest reading: the gap was in the interval whether or not a `where` kept the events on either
/// side of it.
fn published_timeline() -> &'static std::sync::RwLock<Option<Arc<RecordValue>>> {
    static PUBLISHED: std::sync::OnceLock<std::sync::RwLock<Option<Arc<RecordValue>>>> =
        std::sync::OnceLock::new();
    PUBLISHED.get_or_init(|| std::sync::RwLock::new(None))
}

/// Records the window `timeline` just answered over, for the renderer that draws it.
pub fn publish_timeline(record: RecordValue) {
    if let Ok(mut held) = published_timeline().write() {
        *held = Some(Arc::new(record));
    }
}

/// The `ono.temporal-timeline/1` these values are a window on, where they are a timeline's.
///
/// Every value has to be an `ono.temporal-event/1` and the last `timeline` in this process has to
/// have published its window; anything else is a stream of events from somewhere else — `find
/// event`, a `--kind` filter over a saved list — and the ordinary renderer draws it as rows.
fn timeline_view_of(values: &[Value]) -> Option<RecordValue> {
    if values.is_empty() {
        return None;
    }
    if !values.iter().all(|value| {
        value
            .as_record()
            .is_ok_and(|record| record.schema_id().to_string() == "ono.temporal-event/1")
    }) {
        return None;
    }
    let published = published_timeline().read().ok()?.clone()?;
    let schema = Arc::clone(published.schema());
    let mut builder = RecordValue::builder(Arc::clone(&schema), published.provenance().clone());
    for (index, field) in schema.fields().iter().enumerate() {
        let held = if field.name() == "events" {
            Value::list(values.to_vec())
        } else {
            published.field_at(index).cloned().unwrap_or(Value::Null)
        };
        builder = builder.set(field.name(), held).ok()?;
    }
    Some(builder.build())
}

/// The render options the temporal renderers are handed (v0.5 §39.2, §33).
///
/// The session's zone offset and `temporal.ui.show_source_tags` reach the renderer as data. §39.2
/// keeps the clock out of pure logic and the same discipline applies to settings: a renderer that
/// read the configuration itself could not be tested and could not be told what to draw.
pub(crate) fn temporal_options() -> ono_temporal_render::RenderOptions {
    let now = jiff::Timestamp::now();
    let offset = i128::from(jiff::tz::TimeZone::system().to_offset(now).seconds());
    ono_temporal_render::RenderOptions {
        utc_offset: ono_value::Duration::from_nanoseconds(offset.saturating_mul(1_000_000_000)),
        show_source_tags: crate::temporal::session::show_source_tags(),
        ..ono_temporal_render::RenderOptions::default()
    }
}

/// Every value as an `ono.temporal-change/1`, or `None` where one of them is something else.
fn every_change(values: &[Value]) -> Option<Vec<RecordValue>> {
    values
        .iter()
        .map(|value| {
            let record = value.as_record().ok()?;
            (record.schema_id().to_string() == "ono.temporal-change/1")
                .then(|| RecordValue::clone(record))
        })
        .collect()
}
