//! The live view of spec §18.3: rows updating in place, keyed by object identity.
//!
//! Only a terminal gets this. The values are exactly the events a pipe would receive — the live
//! table is a presentation over them (ADR-0024), and nothing about the pipeline changes because
//! somebody is watching.

use std::collections::BTreeMap;
use std::io::Write;

use ono_pipeline::{StreamEvent, ValueStream};
use ono_render::{Layout, Presentation, Renderer, Theme, View};
use ono_value::{ErrorValue, Value};

/// How often the screen repaints at most. Spec §4.4 forbids animation for its own sake: a tick
/// that changed nothing redraws nothing, and changes faster than this coalesce into one frame —
/// the screen can only show the newest state anyway (ADR-0024).
const FRAME: std::time::Duration = std::time::Duration::from_millis(250);

/// Drains `stream` into an in-place table until the stream ends or is cancelled.
///
/// Each event either replaces one row (`snapshot`, `added`, `changed` — by the identity of the
/// object it carries) or removes one (`removed`). Errors go to stderr as they arrive; they scroll
/// above the table rather than corrupting it.
pub async fn show(stream: ValueStream, width: usize, height: usize) -> Vec<ErrorValue> {
    let mut stream = stream;
    let mut rows: BTreeMap<String, Value> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut painted = 0usize;
    let mut dirty = false;
    let renderer = Renderer::new();
    let theme = Theme::default();
    let layout = Layout::new(width).max_rows(height.saturating_sub(3).max(4));

    // The frame deadline is fixed, not restarted per event: a stream busier than the frame rate
    // must still paint — otherwise the fastest watches would be the ones showing nothing.
    let mut next_frame = tokio::time::Instant::now() + FRAME;
    loop {
        let event = tokio::select! {
            event = stream.recv() => event,
            () = tokio::time::sleep_until(next_frame) => {
                if dirty {
                    painted = repaint(&layout, &renderer, &theme, &rows, painted);
                    dirty = false;
                }
                next_frame = tokio::time::Instant::now() + FRAME;
                continue;
            }
        };
        let Some(event) = event else {
            break;
        };
        match event {
            StreamEvent::Value(value) => {
                if apply(&mut rows, &value) {
                    dirty = true;
                }
            }
            StreamEvent::Failure(error) => failures.push(error),
        }
    }

    if dirty {
        repaint(&layout, &renderer, &theme, &rows, painted);
    }
    failures
}

/// Applies one event to the table model, answering whether anything changed.
fn apply(rows: &mut BTreeMap<String, Value>, event: &Value) -> bool {
    let Ok(record) = event.as_record() else {
        return false;
    };
    let kind = record
        .get("kind")
        .and_then(|kind| kind.as_str().ok())
        .unwrap_or("snapshot");

    // The object the event carries is its first record-valued field, whatever the target calls
    // it — `process` on ono.process-event/1, `service` on a service event.
    let object = record
        .schema()
        .fields()
        .iter()
        .filter_map(|field| record.get(field.name()))
        .find_map(|value| value.as_record().ok().map(|_| value.clone()));
    let Some(object) = object else {
        return false;
    };
    let key = object
        .as_record()
        .map(|inner| inner.identity().to_string())
        .unwrap_or_default();

    match kind {
        "removed" => rows.remove(&key).is_some(),
        _ => {
            rows.insert(key, object);
            true
        }
    }
}

/// Repaints the table over the previous frame, answering the new line count.
fn repaint(
    layout: &Layout,
    renderer: &Renderer,
    theme: &Theme,
    rows: &BTreeMap<String, Value>,
    painted: usize,
) -> usize {
    let values: Vec<Value> = rows.values().cloned().collect();
    let lines = layout.render_view_styled(
        renderer,
        &values,
        View::Table,
        theme,
        Presentation::Terminal,
    );

    let mut out = std::io::stdout().lock();
    if painted > 0 {
        // Up over the previous frame, then erase to the end of the screen: the frame below can
        // only shrink through this, never leave stale rows behind.
        let _ = write!(out, "\x1b[{painted}A\x1b[0J");
    }
    for line in &lines {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
    lines.len()
}
