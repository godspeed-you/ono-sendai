//! Text rendering for the spatial interface (spec v0.4 §23, §24, §39, §45.4).
//!
//! §45.4 gives this crate the compact textual place view, and ends with the sentence that
//! constrains everything in it:
//!
//! > It MUST NOT invent semantic nodes/edges.
//!
//! So nothing here asks a provider anything, computes a relationship, or decides what is
//! navigable. It is handed the [`ono.place-view/1`] record `look` built and turns it into lines.
//! Every exit it prints is an exit the view declared; every group it marks as enterable is one
//! the view marked `navigable`, because §24.2 forbids a renderer from implying an exit that is
//! not one; and a group whose state says its contents could not be read shows that state instead
//! of a number, because §35.2 makes "files — permission denied for 14 process FDs" a different
//! fact from "files — 0".
//!
//! The headings are presentation and the object underneath stays structured (§6.1): `look --json`
//! writes the record itself, and this is one way of reading it aloud.
//!
//! [`ono.place-view/1`]: https://example.invalid

#![allow(
    clippy::missing_errors_doc,
    reason = "nothing here fails: a field the view did not carry is simply not printed"
)]

use ono_value::{RecordValue, Value};

pub mod map;
pub mod view;

pub use map::{Charset, MapLine, map_lines, spatial_map};
pub use view::{Action, Effect, Key, Keymap, MapView};

/// How wide a label column is before the counts start.
const LABEL_WIDTH: usize = 14;

/// The lines a `PlaceView` reads as, at a terminal `width` columns wide (§23.1, §39.3).
///
/// The width is honoured rather than assumed: §39.3 requires the rendering to remain usable at 80
/// and at 40 columns, so nothing here is laid out to a fixed page and every line is truncated to
/// what actually fits.
#[must_use]
pub fn place_view(view: &RecordValue, width: usize) -> Vec<String> {
    let width = width.max(20);
    let mut lines = Vec::new();
    lines.push(fit(&heading(view), width));
    // §10.3: "A tombstone MUST be visually distinct." It is a line of its own, in words rather
    // than in colour, because §39.1 forbids colour from carrying meaning on its own.
    if let Some(state) = tombstone_state(view) {
        lines.push(fit(&format!(" tombstone — {state}"), width));
    }

    let groups = list(view, "groups");
    if !groups.is_empty() {
        lines.push(String::new());
        lines.push(" exits".to_owned());
        for group in &groups {
            lines.push(fit(&exit_line(group), width));
        }
    }

    let landmarks = list(view, "landmarks");
    if !landmarks.is_empty() {
        lines.push(String::new());
        lines.push(" landmarks".to_owned());
        for landmark in &landmarks {
            lines.push(fit(&landmark_line(landmark), width));
        }
    }

    if let Some(changed) = record(view, "changed") {
        lines.push(String::new());
        lines.push(" changed".to_owned());
        for line in change_lines(&changed) {
            lines.push(fit(&line, width));
        }
    }

    let hidden = record(view, "neighborhood")
        .and_then(|neighborhood| integer(&neighborhood, "hidden_count"))
        .unwrap_or_default();
    if hidden > 0 {
        lines.push(String::new());
        lines.push(fit(&format!(" {hidden} more not shown"), width));
    }
    lines
}

/// `SYSTEM / web01   local` — the place, the host it belongs to, and where that is (§6.1, §7.1).
///
/// The third column is the place path §21.2 writes into the prompt, so what the heading says and
/// what the prompt says are the same sentence. §5 requires the entry screen to carry "current
/// host/context identity", which is exactly this line: the link, the host and the place.
fn heading(view: &RecordValue) -> String {
    let label = text(view, "label").unwrap_or_default();
    let mut line = match text(view, "hostname") {
        Some(host) if !host.is_empty() => format!("{label} / {host}"),
        _ => label,
    };
    if let Some(path) = place_path(view) {
        line.push_str("   ");
        line.push_str(&path);
    }
    line
}

/// The place as §21.2 spells it: `local`, `local/compute`, `local/process/nginx`.
///
/// A canonical space is declared geography and carries no provider reference, and its
/// `place_path` already names the place itself; an observed object's `place_path` names the
/// parent chain it is filed under (§27.2), so the object is written under its link, its kind and
/// its name instead.
fn place_path(view: &RecordValue) -> Option<String> {
    let place = record(view, "place")?;
    let path = text(&place, "place_path").filter(|path| !path.is_empty())?;
    if place.get("canonical_ref").is_none_or(Value::is_null) {
        return Some(path);
    }
    let link = path.split('/').next().unwrap_or("local");
    let kind = text(&place, "spatial_type")?.to_ascii_lowercase();
    let name = text(&place, "display_name")?;
    Some(format!("{link}/{kind}/{name}"))
}

/// What a tombstoned place says about itself, where the view carries one (§10.3).
fn tombstone_state(view: &RecordValue) -> Option<String> {
    let place = record(view, "place")?;
    let Some(Value::Map(tombstone)) = place.get("tombstone") else {
        return None;
    };
    match tombstone.get("state") {
        Some(Value::String(state)) => Some(state.to_string()),
        _ => Some("gone".to_owned()),
    }
}

/// One exit: its label, and either how many places lie behind it or why nobody could say (§24.2).
fn exit_line(group: &RecordValue) -> String {
    let label = text(group, "label").unwrap_or_default();
    let state = text(group, "state").unwrap_or_default();
    // §35.2: a state that is not `available` or `empty` replaces the count. A number in its place
    // would be the false-empty rendering §42.4 forbids.
    let right = if state == "available" || state == "empty" {
        integer(group, "count").unwrap_or_default().to_string()
    } else {
        match text(group, "detail") {
            Some(detail) if !detail.is_empty() => format!("{} — {detail}", state.replace('_', " ")),
            _ => state.replace('_', " "),
        }
    };
    format!("   {label:<LABEL_WIDTH$} {right}")
}

/// One landmark, with the reason §3.7 makes mandatory.
fn landmark_line(landmark: &RecordValue) -> String {
    let name = text(landmark, "name").unwrap_or_default();
    let reason = text(landmark, "reason")
        .unwrap_or_default()
        .replace('_', " ");
    let evidence = text(landmark, "evidence").unwrap_or_default();
    if evidence.is_empty() {
        format!("   {name:<LABEL_WIDTH$} {reason}")
    } else {
        format!("   {name:<LABEL_WIDTH$} {reason} — {evidence}")
    }
}

/// The change section, which never invents a change (§24.3).
fn change_lines(changed: &RecordValue) -> Vec<String> {
    let state = text(changed, "state").unwrap_or_default();
    let entries = list(changed, "entries");
    if entries.is_empty() {
        let reason = match text(changed, "source") {
            Some(source) if !source.is_empty() => format!("{state} ({source})"),
            _ => state.replace('_', " "),
        };
        return vec![format!("   {reason}")];
    }
    entries
        .iter()
        .map(|entry| {
            let name = text(entry, "object")
                .or_else(|| text(entry, "id"))
                .unwrap_or_default();
            let what = text(entry, "change").unwrap_or_default();
            format!("   {name:<LABEL_WIDTH$} {what}")
        })
        .collect()
}

pub(crate) fn fit(line: &str, width: usize) -> String {
    if line.chars().count() <= width {
        return line.trim_end().to_owned();
    }
    line.chars()
        .take(width)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

pub(crate) fn text(record: &RecordValue, field: &str) -> Option<String> {
    match record.get(field) {
        Some(Value::String(text)) => Some(text.to_string()),
        _ => None,
    }
}

pub(crate) fn integer(record: &RecordValue, field: &str) -> Option<i128> {
    match record.get(field) {
        Some(Value::Int(number)) => Some(*number),
        _ => None,
    }
}

pub(crate) fn record(view: &RecordValue, field: &str) -> Option<RecordValue> {
    match view.get(field) {
        Some(Value::Record(record)) => Some(RecordValue::clone(record)),
        _ => None,
    }
}

pub(crate) fn list(view: &RecordValue, field: &str) -> Vec<RecordValue> {
    match view.get(field) {
        Some(Value::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::Record(record) => Some(RecordValue::clone(record)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}
