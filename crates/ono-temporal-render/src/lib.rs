//! Temporal presentation for Ono-Sendai: timeline text, causal explanation, coverage gaps, the
//! temporal HUD and the full-screen timeline (spec v0.5 §11.5, §16.5, §18, §19, §39).
//!
//! §39.3 states the rule this crate is built around:
//!
//! > No provider calls from renderer.
//!
//! So nothing here queries a provider, the ledger or the network. Every function takes a record
//! the temporal query path already produced — `ono.temporal-timeline/1`,
//! `ono.causal-explanation/1`, `ono.temporal-gap/1`, `ono.temporal-context/1`,
//! `ono.temporal-change/1`, `ono.recorder-status/1` — and a terminal width, and returns lines.
//! The crate depends on `ono-value` and nothing else, and the dependency graph is how §39.3 is
//! enforced rather than remembered.
//!
//! Three rules run through every function:
//!
//! - **A gap is never hidden** (§11.7, §55.5). A coverage gap inside a rendered window is drawn
//!   even though events exist on both sides of it.
//! - **Colour never carries meaning alone** (§45.2). `[PAST]`, `gap`, `correlated` and the
//!   connector glyphs are words and characters; this crate emits no escape sequence at all.
//! - **Nothing is invented** (§18.5, §45.1). There is no animation, no interpolated frame between
//!   unsupported states and no instant the input did not state.
//!
//! Every rendering is a pure function of its record, its width and its [`RenderOptions`], which
//! is what makes the output deterministic when it is redirected (v0.2 §50).

#![allow(
    clippy::missing_panics_doc,
    reason = "nothing here panics: a field the record did not carry is simply not printed"
)]

use ono_value::{Duration, RecordValue, Value, canonical_text};

mod causal;
mod changes;
mod hud;
mod timeline;

pub use causal::{causal_explanation, causal_graph};
pub use changes::changes;
pub use hud::{gap_frame, paused_marker, recorder_status, return_to_now, temporal_hud};
pub use timeline::{timeline, timeline_view};

/// The session settings and view state a temporal rendering honours.
///
/// Everything that is not the record itself lives here, so a renderer stays a pure function: the
/// zone offset is passed in rather than read from the system (§39.2 applies the same discipline
/// to the clock), and the view's cursor and expansions arrive as data rather than as state.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// The session's offset from UTC, applied to every wall clock this crate prints (§25.3).
    ///
    /// Timestamps are stored in UTC (§25.2); interactive display defaults to the user's
    /// configured zone, and this is how the caller says which that is.
    pub utc_offset: Duration,
    /// `temporal.ui.show_source_tags` (§33), which decides whether a timeline row carries the
    /// abbreviated `[systemd]` tag of §11.5 at all.
    pub show_source_tags: bool,
    /// Whether repeated rows are grouped in the full-screen timeline (§19.4).
    ///
    /// A grouped row keeps its count and its time span, and a lifecycle change is never folded
    /// into one.
    pub group_repeats: bool,
    /// The event references of grouped rows the view has expanded (§19.5).
    ///
    /// A reference matches the event whose id it prefixes, so the `@e…` a row printed is what a
    /// view stores here.
    pub expanded: Vec<String>,
    /// The event reference the full-screen cursor is on (§19.2).
    pub cursor: Option<String>,
}

impl Default for RenderOptions {
    /// The defaults of §33: UTC, source tags on, nothing grouped and nothing selected.
    fn default() -> Self {
        Self {
            utc_offset: Duration::ZERO,
            show_source_tags: true,
            group_repeats: false,
            expanded: Vec::new(),
            cursor: None,
        }
    }
}

/// How much of a wall clock a line needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Precision {
    /// `12:17` — the window ends of §19.2.
    Minute,
    /// `12:40:18` — the coordinate of §4.6 and the gap boundaries of §18.6.
    Second,
    /// `12:17:51.203` — an event row (§11.5).
    Milli,
}

/// How many hexadecimal digits an event reference shows (§11.6).
///
/// The full id is a 24-digit content digest, which no row has room for. Eight digits identify an
/// event inside any window a person is looking at, and the shell answers a prefix that matches
/// two events with `temporal.ambiguous_event` (§34) rather than choosing one.
pub(crate) const REFERENCE_DIGITS: usize = 8;

/// A field of a record or of the map a nested sub-record is written as.
///
/// The production bridge writes a schema's `record` field as a map and a list of objects that
/// have a schema of their own as records, so a renderer that read only one of the two would
/// break on half its input.
pub(crate) fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::Record(record) => record.get(name),
        Value::Map(map) => map.get(name),
        _ => None,
    }
}

/// A text field, absent where it is null or empty — an empty string is never a value (§35.3).
pub(crate) fn text(value: &Value, name: &str) -> Option<String> {
    match field(value, name) {
        Some(Value::String(text)) if !text.is_empty() => Some(text.to_string()),
        _ => None,
    }
}

/// A list field, empty where the record carries none.
pub(crate) fn items<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    match field(value, name) {
        Some(Value::List(items)) => items,
        _ => &[],
    }
}

/// A list field of a record.
pub(crate) fn record_items<'a>(record: &'a RecordValue, name: &str) -> &'a [Value] {
    match record.get(name) {
        Some(Value::List(items)) => items,
        _ => &[],
    }
}

/// A text field of a record.
pub(crate) fn record_text(record: &RecordValue, name: &str) -> Option<String> {
    match record.get(name) {
        Some(Value::String(text)) if !text.is_empty() => Some(text.to_string()),
        _ => None,
    }
}

/// An instant, in nanoseconds since the epoch.
pub(crate) fn nanos(value: &Value) -> Option<i128> {
    match value {
        Value::Timestamp(instant) => Some(instant.as_nanosecond()),
        _ => None,
    }
}

/// An instant held in a field.
pub(crate) fn instant(value: &Value, name: &str) -> Option<i128> {
    field(value, name).and_then(nanos)
}

/// An instant held in a field of a record.
pub(crate) fn record_instant(record: &RecordValue, name: &str) -> Option<i128> {
    record.get(name).and_then(nanos)
}

/// The instant a person navigates by: the source's own time where it gave one, else the
/// observation (§3.3, `EventTimes::presentation_instant`).
pub(crate) fn presentation_instant(event: &Value) -> Option<i128> {
    instant(event, "source_time").or_else(|| instant(event, "observed_at"))
}

/// A wall clock, in the session's zone (§25.3).
///
/// Only the time of day is printed, so the arithmetic is a shift and a remainder and needs no
/// calendar. A negative instant — an event before the epoch — floors the same way any other
/// does, because `div_euclid` keeps the remainder positive.
pub(crate) fn clock(instant: i128, options: &RenderOptions, precision: Precision) -> String {
    let shifted = instant.saturating_add(options.utc_offset.nanoseconds());
    let seconds = shifted.div_euclid(1_000_000_000);
    let subsecond = shifted.rem_euclid(1_000_000_000);
    let day = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (day / 3600, (day % 3600) / 60, day % 60);
    match precision {
        Precision::Minute => format!("{hour:02}:{minute:02}"),
        Precision::Second => format!("{hour:02}:{minute:02}:{second:02}"),
        Precision::Milli => {
            let millis = subsecond / 1_000_000;
            format!("{hour:02}:{minute:02}:{second:02}.{millis:03}")
        }
    }
}

/// The span between two instants, in the house duration form — `4m 12s`, `843ms`.
pub(crate) fn span(from: i128, until: i128) -> String {
    Duration::from_nanoseconds(until.saturating_sub(from)).to_string()
}

/// The `@e…` reference §11.6 requires a rendered event to carry.
pub(crate) fn reference(event_id: &str) -> String {
    let digits: String = event_id
        .strip_prefix('e')
        .unwrap_or(event_id)
        .chars()
        .take(REFERENCE_DIGITS)
        .collect();
    format!("@e{digits}")
}

/// Whether a reference a view holds names this event.
///
/// The stored form may be the `@e…` a row printed or the bare id the record carries, and the
/// printed form is a prefix of the full digest, so a prefix match is what identifies the event.
pub(crate) fn refers_to(stored: &str, event_id: &str) -> bool {
    let trimmed = stored.trim().trim_start_matches('@');
    let trimmed = trimmed.strip_prefix('e').unwrap_or(trimmed);
    let body = event_id.strip_prefix('e').unwrap_or(event_id);
    !trimmed.is_empty() && body.starts_with(trimmed)
}

/// A value as a person reads it, with an unknown staying the word `unknown` (v0.2 §10.5).
pub(crate) fn value_text(value: &Value) -> String {
    match value {
        Value::Null => "unknown".to_owned(),
        // v0.2 §33.5 asks for the human form where a renderer is what is doing the asking:
        // `512.00 MiB` and `4m 12s` read as themselves, and the canonical spelling stays what
        // `to json` and `to text` write.
        Value::ByteSize(size) => size.to_string(),
        Value::Duration(span) => span.to_string(),
        Value::Path(path) => path.display().to_string(),
        other => canonical_text(other).unwrap_or_else(|_| other.type_name().to_owned()),
    }
}

/// The §7.1 source, abbreviated the way §11.5 asks for: short, and derived by a stated rule so it
/// stays inspectable.
///
/// | source | tag |
/// |---|---|
/// | `ono.session`, `ono.recorder` | `ono` |
/// | `linux.systemd-dbus` | `systemd` |
/// | `linux.procfs`, `linux.journald`, `linux.netlink` | `procfs`, `journald`, `netlink` |
/// | `adapter:ps` | `ps` |
/// | `remote:web01/procfs` | `web01` |
/// | `kuang:dev.example.eye/probe` | `eye` |
pub(crate) fn abbreviate_source(source: &str) -> String {
    if let Some(rest) = source.strip_prefix("adapter:") {
        return rest.to_owned();
    }
    if let Some(rest) = source.strip_prefix("remote:") {
        return rest.split('/').next().unwrap_or(rest).to_owned();
    }
    if let Some(rest) = source.strip_prefix("kuang:") {
        let package = rest.split('/').next().unwrap_or(rest);
        return package.rsplit('.').next().unwrap_or(package).to_owned();
    }
    if source.starts_with("ono.") {
        return "ono".to_owned();
    }
    let tail = source.rsplit('.').next().unwrap_or(source);
    tail.split('-').next().unwrap_or(tail).to_owned()
}

/// A line clipped to the terminal, with trailing padding dropped.
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

/// Text padded to a column, or shortened to it with an ellipsis when it overflows.
pub(crate) fn pad(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count == width {
        return text.to_owned();
    }
    if count > width {
        if width <= 1 {
            return text.chars().take(width).collect();
        }
        let mut shortened: String = text.chars().take(width - 1).collect();
        shortened.push('…');
        return shortened;
    }
    format!("{text}{}", " ".repeat(width - count))
}

/// The width the subject column gets at this terminal width (v0.4 §39.3).
pub(crate) fn subject_width(width: usize) -> usize {
    (width / 3).clamp(8, 22)
}
