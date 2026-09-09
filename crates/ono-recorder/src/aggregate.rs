//! Declared aggregation for high-frequency change (v0.5 §43.3, §22.6).
//!
//! §43.3 permits folding "high-frequency metrics-like changes ... when they are not semantically
//! relevant to exact topology", and attaches two conditions: "aggregation rules MUST be declared
//! and must not hide object/relation lifecycle changes".
//!
//! Both are structural here. The rules are a list a caller can read back through
//! [`AggregationRules::declared`], and the only events an [`Aggregator`] will ever fold are
//! `object.changed` events every one of whose changed fields is on that list. An appearance, a
//! disappearance, a relation event, an action or a change touching one undeclared field passes
//! through whatever its rate — which is what stops §43.3 from becoming a way to lose the
//! lifecycle transitions §9 reconstructs from.
//!
//! §22.6 is the reason the default list looks the way it does: "filesystem usage percentage is
//! not a default high-frequency time series. It MAY be captured at checkpoints or landmark
//! transition time."

use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::SpatialId;
use ono_temporal_core::{EventKind, TemporalEvent};
use ono_value::Duration;

/// The fields §43.3 and §22.6 make foldable by default.
///
/// Each is a sample of a continuously moving quantity, and none of them is what makes an object
/// the object it is. A field not on this list is never folded, however fast it moves.
pub const DEFAULT_AGGREGATED_FIELDS: &[&str] = &[
    "cpu",
    "cpu_window",
    "memory",
    "virtual_mem",
    "rss",
    "usage",
    "used",
    "available",
    "free",
    "rx_bytes",
    "tx_bytes",
    "bytes",
    "packets",
];

/// The window inside which one folded field yields one event (§32.5's flush interval).
pub const DEFAULT_AGGREGATION_WINDOW: Duration = Duration::from_nanoseconds(2 * 1_000_000_000);

/// The declared aggregation rules of §43.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregationRules {
    fields: Vec<Arc<str>>,
    window: Duration,
}

impl Default for AggregationRules {
    /// §43.3's rules as this build declares them.
    fn default() -> Self {
        Self {
            fields: DEFAULT_AGGREGATED_FIELDS
                .iter()
                .map(|field| Arc::from(*field))
                .collect(),
            window: DEFAULT_AGGREGATION_WINDOW,
        }
    }
}

impl AggregationRules {
    /// Rules that fold nothing, for a recorder that would rather keep every sample.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            fields: Vec::new(),
            window: DEFAULT_AGGREGATION_WINDOW,
        }
    }

    /// Rules folding exactly these fields.
    #[must_use]
    pub fn folding<'a>(fields: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            fields: fields.into_iter().map(Arc::from).collect(),
            window: DEFAULT_AGGREGATION_WINDOW,
        }
    }

    /// The same rules over a different window.
    #[must_use]
    pub const fn within(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    /// Every field these rules fold — §43.3's "aggregation rules MUST be declared".
    pub fn declared(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().map(Arc::as_ref)
    }

    /// The window one folded field yields one event inside.
    #[must_use]
    pub const fn window(&self) -> Duration {
        self.window
    }

    /// Whether §43.3 allows this event to be folded into a preceding one.
    ///
    /// Only an `object.changed` whose every changed field is declared. A change with no field
    /// list says which object moved and not what about it, so it is never folded.
    #[must_use]
    pub fn may_fold(&self, event: &TemporalEvent) -> bool {
        if event.kind != EventKind::ObjectChanged || event.changed_fields.is_empty() {
            return false;
        }
        event
            .changed_fields
            .iter()
            .all(|change| self.fields.iter().any(|field| **field == *change.field))
    }

    /// An aggregator applying these rules.
    #[must_use]
    pub fn aggregator(&self) -> Aggregator {
        Aggregator {
            rules: self.clone(),
            last: HashMap::new(),
        }
    }
}

/// The state one stream of events is folded against (§43.3).
#[derive(Debug)]
pub struct Aggregator {
    rules: AggregationRules,
    last: HashMap<(Option<SpatialId>, Arc<str>), Timestamp>,
}

impl Aggregator {
    /// The event to record, or `None` where §43.3 folds it into one already recorded.
    ///
    /// A lifecycle event is never `None`. That is the whole of §43.3's second condition, and it
    /// is checked before the window rather than after it.
    pub fn admit(&mut self, event: TemporalEvent) -> Option<TemporalEvent> {
        if !self.rules.may_fold(&event) {
            return Some(event);
        }
        let at = event.times.presentation_instant();
        let subject = event
            .subject
            .as_ref()
            .and_then(|subject| subject.spatial_id().cloned());
        let mut folded = true;
        for change in &event.changed_fields {
            let key = (subject.clone(), Arc::clone(&change.field));
            match self.last.get(&key) {
                Some(previous) if within(*previous, at, self.rules.window()) => {}
                _ => {
                    folded = false;
                }
            }
            self.last.insert(key, at);
        }
        (!folded).then_some(event)
    }

    /// The rules this aggregator applies, so a caller can report them (§43.3).
    #[must_use]
    pub const fn rules(&self) -> &AggregationRules {
        &self.rules
    }
}

/// Whether `at` falls inside `window` of `previous`.
fn within(previous: Timestamp, at: Timestamp, window: Duration) -> bool {
    let elapsed = at.as_nanosecond() - previous.as_nanosecond();
    elapsed >= 0 && elapsed <= window.nanoseconds()
}
