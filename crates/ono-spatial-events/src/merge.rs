//! The provider event merge (spec v0.4 §25.1, §2.16; v0.2 §18.2, ADR-0024).
//!
//! §25.1: "`map --live` MUST subscribe to available provider events and/or explicit polling
//! sources." The shell already has such a runtime — `watch <target>` — and §2.16 forbids the
//! spatial layer from becoming a second source of system truth, so this merge reads that
//! runtime's own envelope rather than defining a spatial event beside it:
//!
//! ```text
//! kind    snapshot | added | changed | removed
//! at      when the change was observed
//! <target> the object as it now is, or as it last was for a removal
//! source  subscription | poll
//! ```
//!
//! Two things the merge derives from the stream and nothing else. The first is §25.3's freshness:
//! a view is `event_driven` only while every source it reads says `subscription`, and `polled` as
//! soon as one of them says `poll`. The second is whether the stream has *settled* — ADR-0024
//! makes the opening events `snapshot`, so until one has arrived an `added` cannot be told from a
//! state the view simply had not seen yet, and §43.6 forbids reporting a startup artefact as a
//! real change.

use jiff::Timestamp;
use ono_value::{RecordValue, Value};

use crate::change::{Freshness, ObservedAt};

/// What the runtime says happened (v0.2 §18.2's `kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// The opening state of the stream (ADR-0024).
    Snapshot,
    /// An object the stream had not seen before.
    Added,
    /// The same object, with fields that moved.
    Changed,
    /// An object the provider no longer answers for.
    Removed,
}

impl EventKind {
    /// The event kind a `kind` field names, where it names one.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "snapshot" => Some(EventKind::Snapshot),
            "added" => Some(EventKind::Added),
            "changed" => Some(EventKind::Changed),
            "removed" => Some(EventKind::Removed),
            _ => None,
        }
    }

    /// The word for it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Snapshot => "snapshot",
            EventKind::Added => "added",
            EventKind::Changed => "changed",
            EventKind::Removed => "removed",
        }
    }
}

/// How the runtime saw it (v0.2 §18.2's `source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSource {
    /// The provider announced it.
    Subscription,
    /// The runtime compared snapshots at its interval.
    Poll,
}

impl EventSource {
    /// The source a `source` field names, where it names one.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "subscription" => Some(EventSource::Subscription),
            "poll" => Some(EventSource::Poll),
            _ => None,
        }
    }

    /// What §25.3 calls a view fed by this source.
    #[must_use]
    pub fn freshness(self) -> Freshness {
        match self {
            EventSource::Subscription => Freshness::EventDriven,
            EventSource::Poll => Freshness::Polled,
        }
    }
}

/// One event, read out of the runtime's envelope.
#[derive(Debug, Clone)]
pub struct ObservedEvent {
    kind: EventKind,
    at: Option<Timestamp>,
    changed: Vec<String>,
    source: EventSource,
    object: Option<RecordValue>,
}

impl ObservedEvent {
    /// What happened.
    #[must_use]
    pub fn kind(&self) -> EventKind {
        self.kind
    }

    /// When the runtime observed it (v0.2 §18.2's `at`, v0.5 §3.3's `observed_at`).
    ///
    /// `None` where the envelope stated no time. v0.5 §3.3 keeps source time, observed time and
    /// ingestion time apart, so the one instant this envelope carries is reported as the one
    /// instant it is, and an envelope carrying none reports none.
    #[must_use]
    pub fn at(&self) -> Option<Timestamp> {
        self.at
    }

    /// When it was observed, in the terms v0.5 §9.2 allows a consumer to use.
    ///
    /// This is the seam a change built from a provider event takes its time from: the event
    /// states an instant, so the change states that instant, and an event that states none
    /// produces a change that states none.
    #[must_use]
    pub fn observed(&self) -> ObservedAt {
        self.at.map_or(ObservedAt::Unknown, ObservedAt::At)
    }

    /// The names of the fields whose values moved (v0.2 §18.2's `changed`, v0.5 §6.2).
    ///
    /// Empty where the envelope named none — for every kind but `changed` it names none, and
    /// §35.3 forbids inventing field names to fill the gap.
    #[must_use]
    pub fn changed_fields(&self) -> &[String] {
        &self.changed
    }

    /// How it was seen.
    #[must_use]
    pub fn source(&self) -> EventSource {
        self.source
    }

    /// The object it happened to — the provider's own record, unchanged (§2.16).
    #[must_use]
    pub fn object(&self) -> Option<&RecordValue> {
        self.object.as_ref()
    }
}

/// The events of every stream a live view reads, merged into one picture of how fresh it is.
#[derive(Debug, Clone, Default)]
pub struct EventMerge {
    freshness: Option<Freshness>,
    settled: bool,
}

impl EventMerge {
    /// A merge that has seen nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads one value from a watch stream.
    ///
    /// Returns `None` for anything that is not one of the runtime's events — §24.3 forbids
    /// inventing a change, and a value that announces nothing is not a change.
    pub fn absorb(&mut self, value: &Value) -> Option<ObservedEvent> {
        let Value::Record(record) = value else {
            return None;
        };
        if !record.schema().id().name().ends_with("-event") {
            return None;
        }
        let kind = EventKind::from_word(word(record, "kind")?)?;
        let source = EventSource::from_word(word(record, "source")?)?;

        self.freshness = Some(match self.freshness {
            Some(known) => known.weaker(source.freshness()),
            None => source.freshness(),
        });
        if kind == EventKind::Snapshot {
            self.settled = true;
        }
        Some(ObservedEvent {
            kind,
            at: instant_of(record),
            changed: changed_fields_of(record),
            source,
            object: object_of(record),
        })
    }

    /// How live the merged view is (§25.3).
    ///
    /// Before the first event, `cached`: the view shows what was read, and nothing is watching it
    /// yet. §2.17 makes that difference one the user has to be able to see.
    #[must_use]
    pub fn freshness(&self) -> Freshness {
        self.freshness.unwrap_or(Freshness::Cached)
    }

    /// Whether an opening snapshot has arrived (ADR-0024).
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.settled
    }
}

/// A string field of an event.
fn word<'a>(record: &'a RecordValue, field: &str) -> Option<&'a str> {
    record.get(field).and_then(|value| value.as_str().ok())
}

/// When the envelope says the change was observed, where it says so (v0.2 §18.2's `at`).
fn instant_of(record: &RecordValue) -> Option<Timestamp> {
    match record.get("at") {
        Some(Value::Timestamp(at)) => Some(*at),
        _ => None,
    }
}

/// The field names the envelope lists as moved (v0.2 §18.2's `changed`).
fn changed_fields_of(record: &RecordValue) -> Vec<String> {
    match record.get("changed") {
        Some(Value::List(fields)) => fields
            .iter()
            .filter_map(|field| field.as_str().ok().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// The object an event carries, under whichever target field the envelope names it.
///
/// The field is the watched target's own word — `socket`, `process`, `service` — so rather than
/// hard-coding a table that would have to grow with every target, the object is the one record
/// field the envelope holds beside the four it declares for itself (v0.2 §31.14).
fn object_of(record: &RecordValue) -> Option<RecordValue> {
    for field in record.schema().fields() {
        if matches!(field.name(), "kind" | "at" | "changed" | "source") {
            continue;
        }
        if let Some(Value::Record(object)) = record.get(field.name()) {
            return Some(RecordValue::clone(object));
        }
    }
    None
}
