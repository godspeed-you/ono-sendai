//! The temporal HUD, the gap frame, the paused marker, the return-to-now summary and the
//! recorder's own status (spec v0.5 §4.6, §8.6, §10.3, §18.2, §18.6, §18.7, §30.1, §45.1).
//!
//! §4.6: "Historical context MUST be visually obvious", and "the distinction MUST remain visible
//! in monochrome and plain text". So the marker is a word. §18.6 requires a view that steps into
//! a gap to show the gap and forbids it from continuing to show the last state with a silently
//! advancing timestamp, which is why [`gap_frame`] states the interval, the reason and the last
//! instant anything was actually known, and nothing else.

use std::collections::BTreeMap;

use ono_value::{RecordValue, Value};

use crate::{
    Precision, RenderOptions, clock, field, fit, items, nanos, record_instant, record_text, text,
    value_text,
};

/// §4.6's temporal HUD: the coordinate and the marker, in words.
///
/// The present carries nothing, because §4.6 makes the marker the sign of historical context and
/// a marker that always appeared would mark nothing.
#[must_use]
pub fn temporal_hud(context: &RecordValue, width: usize, options: &RenderOptions) -> Vec<String> {
    let width = width.max(20);
    if record_text(context, "mode").as_deref() != Some("historical") {
        return Vec::new();
    }
    let coordinate = record_instant(context, "resolved_at").map_or_else(String::new, |at| {
        format!("@{}", clock(at, options, Precision::Second))
    });
    let marker = record_text(context, "marker").unwrap_or_else(|| "[PAST]".to_owned());
    let mut head = format!("{coordinate} {marker}");
    if let Some(requested) = record_text(context, "requested") {
        head.push_str(&format!("  {requested}"));
    }
    let mut lines = vec![fit(head.trim_start(), width)];

    // §8.6: `[PAST?]` is a claim about coverage, so the HUD says what the coverage was rather
    // than leaving the question mark to speak for itself.
    if let Some(coverage) = context.get("coverage").filter(|value| !value.is_null()) {
        let headline = text(coverage, "headline").unwrap_or_else(|| "unknown".to_owned());
        if headline != "complete" {
            let gaps = items(coverage, "gaps").len();
            let mut line = format!("coverage {headline}");
            if gaps > 0 {
                line.push_str(&format!(
                    " — {gaps} {}",
                    if gaps == 1 { "gap" } else { "gaps" }
                ));
            }
            lines.push(fit(&line, width));
        }
    }
    lines
}

/// §18.2's `PAUSED @14:03:12.410`.
///
/// Pausing freezes the view's temporal cursor and stops nothing else, so the marker states the
/// instant the view is showing. A value that is not an instant is no coordinate, and the marker
/// then says only that the view is paused.
#[must_use]
pub fn paused_marker(at: &Value, options: &RenderOptions) -> String {
    match nanos(at) {
        Some(at) => format!("PAUSED @{}", clock(at, options, Precision::Milli)),
        None => "PAUSED".to_owned(),
    }
}

/// §18.6's gap frame, for a cursor that stepped into a coverage gap.
///
/// ```text
/// HISTORY GAP
/// 12:40:18 - 12:44:30
/// recorder disconnected
///
/// last supported state shown at 12:40:18
/// ```
///
/// Nothing is animated and no frame is invented (§18.5, §45.1): the only instants in the frame
/// are the two the record states and the one derived from them.
#[must_use]
pub fn gap_frame(gap: &RecordValue, width: usize, options: &RenderOptions) -> Vec<String> {
    let width = width.max(20);
    let from = record_instant(gap, "from");
    let until = record_instant(gap, "until");
    let mut lines = vec![fit("HISTORY GAP", width)];
    if let (Some(from), Some(until)) = (from, until) {
        lines.push(fit(
            &format!(
                "{} - {}",
                clock(from, options, Precision::Second),
                clock(until, options, Precision::Second)
            ),
            width,
        ));
    }
    let reason = record_text(gap, "detail").unwrap_or_else(|| {
        record_text(gap, "reason")
            .unwrap_or_else(|| "not recorded".to_owned())
            .replace('_', " ")
    });
    lines.push(fit(&reason, width));
    if let Some(from) = from {
        lines.push(String::new());
        lines.push(fit(
            &format!(
                "last supported state shown at {}",
                clock(from, options, Precision::Second)
            ),
            width,
        ));
    }
    lines
}

/// §18.7's summary of what changed while the cursor was in the past.
///
/// The input is the canonical `ono.temporal-change/1` answer, so the summary is a reading of the
/// `changes` engine rather than a second one. Appearances and disappearances are counted by
/// object type; a state change is named, because a count would lose the thing worth seeing.
#[must_use]
pub fn return_to_now(changes: &[RecordValue], width: usize) -> Vec<String> {
    let width = width.max(20);
    let mut added: BTreeMap<String, usize> = BTreeMap::new();
    let mut removed: BTreeMap<String, usize> = BTreeMap::new();
    let mut altered: Vec<String> = Vec::new();

    for change in changes {
        let kind = record_text(change, "kind").unwrap_or_default();
        let subject = change.get("subject");
        let object_type = subject
            .and_then(|subject| text(subject, "object_type"))
            .unwrap_or_else(|| "object".to_owned());
        match kind.as_str() {
            "added" => *added.entry(object_type).or_default() += 1,
            "removed" => *removed.entry(object_type).or_default() += 1,
            "relation_added" => *added.entry("relation".to_owned()).or_default() += 1,
            "relation_removed" => *removed.entry("relation".to_owned()).or_default() += 1,
            _ => altered.push(alteration(change, subject)),
        }
    }

    let mut lines = vec![fit("returned to now", width)];
    for (object_type, count) in &added {
        lines.push(fit(
            &format!("  +{count} {}", plural(object_type, *count)),
            width,
        ));
    }
    for (object_type, count) in &removed {
        lines.push(fit(
            &format!("  -{count} {}", plural(object_type, *count)),
            width,
        ));
    }
    for line in altered {
        lines.push(fit(&format!("  {line}"), width));
    }
    lines
}

/// `nginx.service  active -> failed` (§18.7).
fn alteration(change: &RecordValue, subject: Option<&Value>) -> String {
    let label = subject
        .and_then(|subject| text(subject, "label"))
        .unwrap_or_else(|| "object".to_owned());
    let fields = match change.get("field_changes") {
        Some(Value::List(items)) => items.as_ref(),
        _ => &[][..],
    };
    match fields.split_first() {
        Some((only, [])) => {
            let before = field(only, "before").map_or_else(|| "unknown".to_owned(), value_text);
            let after = field(only, "after").map_or_else(|| "unknown".to_owned(), value_text);
            format!("{label}  {before} -> {after}")
        }
        Some((_, _)) => format!("{label}  {} fields changed", fields.len()),
        None => format!("{label}  changed"),
    }
}

/// An English plural for a count of objects of one type.
fn plural(word: &str, count: usize) -> String {
    if count == 1 {
        return word.to_owned();
    }
    if word.ends_with('s') || word.ends_with('x') || word.ends_with("ch") || word.ends_with("sh") {
        return format!("{word}es");
    }
    format!("{word}s")
}

/// `get recorder`, in the terms §30.1 requires to be legible.
///
/// > The user must know when Ono is retaining system history and how much it retains.
///
/// So the status states whether it runs, where the store is, what the limits are and how much is
/// held — and §43.2 forbids silent loss, so the dropped count is part of it.
#[must_use]
pub fn recorder_status(status: &RecordValue, width: usize, options: &RenderOptions) -> Vec<String> {
    let width = width.max(20);
    let running = matches!(status.get("running"), Some(Value::Bool(true)));
    let enabled = matches!(status.get("enabled"), Some(Value::Bool(true)));
    let health = record_text(status, "health").unwrap_or_else(|| "unknown".to_owned());
    let mut lines = vec![fit(
        &format!(
            "recorder {}   {health}",
            if running { "running" } else { "stopped" }
        ),
        width,
    )];

    let mut row = |label: &str, value: String| {
        if !value.is_empty() {
            lines.push(fit(&format!("  {label:<20} {value}"), width));
        }
    };
    row(
        "recording",
        if enabled {
            "enabled".to_owned()
        } else {
            "disabled".to_owned()
        },
    );
    row(
        "since",
        record_instant(status, "since")
            .map_or_else(String::new, |at| clock(at, options, Precision::Second)),
    );
    row(
        "store",
        status.get("store").map_or_else(String::new, |store| {
            if store.is_null() {
                "none — session only".to_owned()
            } else {
                value_text(store)
            }
        }),
    );
    row(
        "retention",
        match (status.get("max_age"), status.get("max_size")) {
            (Some(age), Some(size)) if !age.is_null() && !size.is_null() => {
                format!("{} / {}", value_text(age), value_text(size))
            }
            _ => String::new(),
        },
    );
    row(
        "events",
        status.get("events").map_or_else(String::new, value_text),
    );
    row(
        "size",
        status
            .get("size")
            .and_then(|size| (!size.is_null()).then(|| value_text(size)))
            .unwrap_or_default(),
    );
    row(
        "retained",
        match (
            record_instant(status, "earliest"),
            record_instant(status, "latest"),
        ) {
            (Some(earliest), Some(latest)) => format!(
                "{} - {}",
                clock(earliest, options, Precision::Second),
                clock(latest, options, Precision::Second)
            ),
            _ => String::new(),
        },
    );
    let sources: Vec<String> = match status.get("sources") {
        Some(Value::List(items)) => items
            .iter()
            .filter_map(|source| match source {
                Value::String(source) => Some(source.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    row("sources", sources.join(", "));
    // §43.2: overflow is never silent, so the count of what the bounded queues discarded is part
    // of the status rather than a log line nobody reads.
    row(
        "dropped",
        status.get("dropped").map_or_else(String::new, value_text),
    );
    lines
}
