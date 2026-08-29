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
pub async fn show(
    stream: ValueStream,
    width: usize,
    height: usize,
    theme: &Theme,
) -> Vec<ErrorValue> {
    let mut stream = stream;
    let mut rows: BTreeMap<String, Value> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut painted = 0usize;
    let mut dirty = false;
    let renderer = Renderer::new();
    let row_limit = height.saturating_sub(3).max(4);
    let layout = Layout::new(width).max_rows(row_limit);

    // The frame deadline is fixed, not restarted per event: a stream busier than the frame rate
    // must still paint — otherwise the fastest watches would be the ones showing nothing.
    let mut next_frame = tokio::time::Instant::now() + FRAME;
    loop {
        let event = tokio::select! {
            event = stream.recv() => event,
            () = tokio::time::sleep_until(next_frame) => {
                if dirty {
                    painted = repaint(&layout, &renderer, theme, &rows, painted);
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
                if absorb(&mut rows, &value, row_limit) {
                    dirty = true;
                }
            }
            StreamEvent::Failure(error) => failures.push(error),
        }
    }

    if dirty {
        repaint(&layout, &renderer, theme, &rows, painted);
    }
    failures
}

/// Absorbs one value into the table model: an event as [`apply`] does, and a plain record —
/// a journal entry streaming from `journalctl -f` (spec v0.3 §1.37) — as a row of its own,
/// keyed by its identity, so a live stream of objects is a growing table (ADR-0059).
///
/// The model keeps only the newest `limit` such rows — a tail, as a log is read — so the
/// screen shows what just happened and a follower that never ends holds nothing it cannot show.
fn absorb(rows: &mut BTreeMap<String, Value>, value: &Value, limit: usize) -> bool {
    if apply(rows, value) {
        return true;
    }
    let Ok(record) = value.as_record() else {
        return false;
    };
    if is_event(value) {
        return false;
    }
    let key = format!("{:020}:{}", rows.len(), record.identity());
    let changed = rows.insert(key, value.clone()) != Some(value.clone());
    while rows.len() > limit {
        let Some(oldest) = rows.keys().next().cloned() else {
            break;
        };
        rows.remove(&oldest);
    }
    changed
}

/// Whether a record is an event carrying an object, rather than an object itself.
fn is_event(value: &Value) -> bool {
    value.as_record().is_ok_and(|record| {
        record
            .schema()
            .fields()
            .iter()
            .filter_map(|field| record.get(field.name()))
            .any(|inner| inner.as_record().is_ok())
    })
}

/// Applies one event to the table model, answering whether anything changed.
///
/// Answers `false` for a value that is not an event at all, which is how a caller folding a
/// mixed stream tells events from ordinary values.
pub(crate) fn apply(rows: &mut BTreeMap<String, Value>, event: &Value) -> bool {
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
        // Spec §4.4: a tick that changed nothing repaints nothing. An event carrying the same
        // state as the row already shows is applied, but it is not a change.
        _ => rows.insert(key, object.clone()) != Some(object),
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    fn event(kind: &str, pid: i128, name: &str) -> Value {
        let schemas = ono_value::builtin_schemas();
        let process_schema = schemas
            .get(&"ono.process/1".parse().expect("a schema id"))
            .expect("the process schema");
        let event_schema = schemas
            .get(&"ono.process-event/1".parse().expect("a schema id"))
            .expect("the event schema");
        let provenance = ono_value::Provenance::local("test", process_schema.id().clone());
        let process = ono_value::RecordValue::builder(process_schema, provenance.clone())
            .set("pid", Value::Int(pid))
            .and_then(|builder| builder.set("name", Value::string(name)))
            .expect("a process record")
            .build();
        let record = ono_value::RecordValue::builder(event_schema, provenance)
            .set("kind", Value::string(kind))
            .and_then(|b| b.set("at", Value::Null))
            .and_then(|b| b.set("process", Value::Record(std::sync::Arc::new(process))))
            .and_then(|b| b.set("source", Value::string("poll")))
            .expect("an event record")
            .build();
        Value::Record(std::sync::Arc::new(record))
    }

    #[test]
    fn should_report_no_change_when_an_event_repeats_the_shown_state() {
        // Spec §4.4: a tick that changed nothing repaints nothing.
        let mut rows = BTreeMap::new();
        assert!(apply(&mut rows, &event("snapshot", 1, "systemd")));
        assert!(
            !apply(&mut rows, &event("snapshot", 1, "systemd")),
            "the same state again is not a change"
        );
        assert!(
            apply(&mut rows, &event("changed", 1, "systemd-renamed")),
            "a different state is"
        );
        assert!(apply(&mut rows, &event("removed", 1, "systemd-renamed")));
        assert!(!apply(&mut rows, &event("removed", 1, "systemd-renamed")));
    }
}
