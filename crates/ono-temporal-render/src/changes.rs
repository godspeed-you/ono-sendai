//! `changes` as text (spec v0.5 §13.2, §13.3, §13.4).
//!
//! §13.3 fixes the shape: a section per change class, an object on a line of its own, and a
//! changed object's fields beneath it as `before -> after`.
//!
//! §13.4 fixes the rule that matters, and it is why this renderer exists rather than a `to text`
//! of the record:
//!
//! > If one side lacks enough evidence, the field MUST be reported as unknown rather than
//! > fabricated
//!
//! So a side with no evidence is drawn in §13.4's own expanded form — `from`, `to` and the
//! coverage that explains the ignorance — and never as a zero, an empty string or a blank
//! column. `ono.temporal-change/1` carries the certainty of each field change and the composed
//! coverage of the window, which is exactly what that form needs, and the renderer states what
//! the record says rather than deciding anything for itself.

use ono_value::{RecordValue, Value};

use crate::{
    Precision, RenderOptions, clock, field, fit, pad, record_instant, record_items, record_text,
    text, timeline, value_text,
};

/// §13.2's five change classes, in the order §13.3 prints them.
///
/// §13.3's example contains three of them and shows three headings. The other two are classes of
/// their own — §6.4 makes a relation a different thing from an object — so they get headings of
/// their own rather than being folded into `ADDED` and `REMOVED`, where a reader could not tell
/// an object that appeared from an edge that was drawn.
const SECTIONS: [(&str, &str); 5] = [
    ("added", "ADDED"),
    ("removed", "REMOVED"),
    ("changed", "CHANGED"),
    ("relation_added", "RELATION ADDED"),
    ("relation_removed", "RELATION REMOVED"),
];

/// How wide the field-name column is before the `before -> after` of §13.3.
const FIELD_WIDTH: usize = 18;

/// How wide the `from` / `to` / `coverage` labels of §13.4's expanded form are.
const LABEL_WIDTH: usize = 9;

/// §13.3's rendering of a stream of `ono.temporal-change/1`.
///
/// ```text
/// ADDED
///   process/7128
///
/// REMOVED
///   process/6902
///
/// CHANGED
///   service/backup
///     state             running -> failed
///
///   filesystem/data
///     used
///       from      unknown
///       to        94.1%
///       coverage  partial before 12:00
/// ```
///
/// An empty window renders as no lines at all: §13.3's headings describe what happened, and a
/// heading over nothing would say something did.
#[must_use]
pub fn changes(records: &[RecordValue], width: usize, options: &RenderOptions) -> Vec<String> {
    let width = width.max(20);
    let mut lines = Vec::new();
    for (kind, title) in SECTIONS {
        let members: Vec<&RecordValue> = records
            .iter()
            .filter(|record| record_text(record, "kind").as_deref() == Some(kind))
            .collect();
        if members.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(fit(title, width));
        for (index, change) in members.iter().enumerate() {
            // §13.3 separates one changed object's block from the next; a bare list needs none.
            if index > 0 && !record_items(change, "field_changes").is_empty() {
                lines.push(String::new());
            }
            lines.extend(object_rows(change, width, options));
        }
    }
    lines
}

/// One change: the object it is about, and the fields beneath it (§13.3).
fn object_rows(change: &RecordValue, width: usize, options: &RenderOptions) -> Vec<String> {
    let mut lines = vec![fit(&format!("  {}", headline(change)), width)];
    for field_change in record_items(change, "field_changes") {
        lines.extend(field_rows(change, field_change, width, options));
    }
    lines
}

/// What the change is about: the object, or the edge where the class is a relation (§6.4).
fn headline(change: &RecordValue) -> String {
    let subject = change
        .get("subject")
        .map(timeline::subject_label_of)
        .unwrap_or_default();
    let Some(edge) = change.get("relation").filter(|edge| !edge.is_null()) else {
        return subject;
    };
    let ends = format!(
        "{} -> {}",
        end_label(edge, "from", &subject),
        end_label(edge, "to", "")
    );
    match text(edge, "relation") {
        Some(relation) => format!("{ends}   {relation}"),
        None => ends,
    }
}

/// One end of an edge, falling back to `fallback` where the record names none.
fn end_label(edge: &Value, name: &str, fallback: &str) -> String {
    let label = field(edge, name)
        .map(timeline::subject_label_of)
        .unwrap_or_default();
    if label.is_empty() {
        fallback.to_owned()
    } else {
        label
    }
}

/// One field change: §13.3's `before -> after`, or §13.4's expanded form for an unknown side.
fn field_rows(
    change: &RecordValue,
    field_change: &Value,
    width: usize,
    options: &RenderOptions,
) -> Vec<String> {
    let name = text(field_change, "field").unwrap_or_default();
    let before = field(field_change, "before");
    let after = field(field_change, "after");
    // §6.2 keeps the reason a side is null with the change, so `unknown` certainty and a null
    // side say the same thing and either is enough to owe the reader §13.4's explanation.
    let unobserved = |side: Option<&Value>| matches!(side, None | Some(Value::Null));
    let unknown = unobserved(before)
        || unobserved(after)
        || text(field_change, "certainty").as_deref() == Some("unknown");
    if !unknown {
        return vec![fit(
            &format!(
                "    {} {} -> {}",
                pad(&name, FIELD_WIDTH),
                side(before),
                side(after)
            ),
            width,
        )];
    }
    vec![
        fit(&format!("    {name}"), width),
        fit(
            &format!("      {} {}", pad("from", LABEL_WIDTH), side(before)),
            width,
        ),
        fit(
            &format!("      {} {}", pad("to", LABEL_WIDTH), side(after)),
            width,
        ),
        fit(
            &format!(
                "      {} {}",
                pad("coverage", LABEL_WIDTH),
                coverage_note(change, unobserved(before), unobserved(after), options)
            ),
            width,
        ),
    ]
}

/// One side of a field change. A side nothing observed is the word `unknown` (§13.4, v0.2 §10.5).
fn side(value: Option<&Value>) -> String {
    value.map_or_else(|| "unknown".to_owned(), value_text)
}

/// §13.4's `partial before 12:00`: the composed headline, and the boundary it holds up to.
///
/// The window's own ends are what the ignorance is bounded by, and the record carries both, so
/// nothing here is looked up (§39.3).
fn coverage_note(
    change: &RecordValue,
    before_unknown: bool,
    after_unknown: bool,
    options: &RenderOptions,
) -> String {
    let headline = change
        .get("coverage")
        .and_then(|coverage| text(coverage, "headline"))
        .unwrap_or_else(|| "unknown".to_owned());
    let bound = |name: &str, word: &str| {
        record_instant(change, name).map(|at| {
            format!(
                "{headline} {word} {}",
                clock(at, options, Precision::Minute)
            )
        })
    };
    let stated = match (before_unknown, after_unknown) {
        (true, false) => bound("from_time", "before"),
        (false, true) => bound("to_time", "after"),
        _ => None,
    };
    stated.unwrap_or_else(|| format!("{headline} over the window"))
}
