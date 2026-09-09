//! Appearance and disappearance derived from snapshots (v0.5 §22.1, §6.3, §8.4).
//!
//! §22.1 permits it and constrains it in one sentence: "the recorder MAY create process
//! appearance/disappearance events by comparing complete-enough snapshots, but provenance MUST
//! say `snapshot_diff` and coverage MUST reflect polling limitations."
//!
//! §6.3 constrains it further, and this is the constraint that is easy to lose: a polling gap
//! MUST NOT emit a disappearance unless the provider contract makes missing-from-a-complete-
//! snapshot meaningful. Three things therefore have to be true before an object that is no longer
//! in the snapshot becomes an `object.disappeared`:
//!
//! 1. the snapshot was complete — a truncated or partly-refused read says nothing about what is
//!    missing from it;
//! 2. the source declared that missing means gone
//!    ([`crate::SourceProfile::meaningful_disappearance`]);
//! 3. the interval since the previous snapshot is one polling interval, not a stretch during
//!    which the recorder was doing something else.
//!
//! Where any of the three fails, the object stays in the differ's memory and the interval becomes
//! a [`TemporalGap`] instead. That is §43.2's preference applied to a polling source: an explicit
//! gap over pretended continuity, in the direction that matters, because the wrong answer here is
//! a process the timeline says died and did not.

use std::collections::BTreeMap;
use std::sync::Arc;

use jiff::Timestamp;
use ono_temporal_core::{
    EventKind, GapReason, TemporalCompleteness, TemporalCoverage, TemporalEvent, TemporalGap,
};
use ono_value::RecordValue;

use crate::normalize::Normalizer;

/// How many polling intervals may pass before a missing object is a gap rather than a departure.
///
/// One interval is the expected cadence; two is a late round. Beyond that the recorder was not
/// looking, and §6.3 forbids reading its own absence as the object's.
pub const MISSED_ROUNDS_BEFORE_GAP: i128 = 2;

/// What one round of snapshot comparison produced (§22.1).
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotRound {
    /// The canonical events the comparison supports.
    pub events: Vec<TemporalEvent>,
    /// What the round covered, with the sampling interval §22.1 requires.
    pub coverage: TemporalCoverage,
    /// The interval nothing can be said about, where the round could not be trusted (§6.3).
    pub gap: Option<TemporalGap>,
    /// How many departures were withheld because §6.3 does not support them.
    pub withheld_disappearances: usize,
}

/// Compares complete-enough snapshots of one source (§22.1).
#[derive(Debug)]
pub struct SnapshotDiff {
    normalizer: Normalizer,
    previous: BTreeMap<Arc<str>, RecordValue>,
    last_at: Option<Timestamp>,
}

impl SnapshotDiff {
    /// A differ for the source `normalizer` speaks for.
    #[must_use]
    pub fn new(normalizer: Normalizer) -> Self {
        Self {
            normalizer,
            previous: BTreeMap::new(),
            last_at: None,
        }
    }

    /// When the last snapshot was taken, or `None` before the first.
    #[must_use]
    pub const fn last_at(&self) -> Option<Timestamp> {
        self.last_at
    }

    /// How many objects the differ is holding.
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.previous.len()
    }

    /// Compares `snapshot` with the previous one (§22.1).
    ///
    /// `complete` is the provider's own statement about whether the read was whole. A partial
    /// read still produces appearances — an object that is there was there — and never a
    /// departure, because §7.4's negative claim needs completeness the read did not have.
    #[must_use]
    pub fn observe(
        &mut self,
        snapshot: &[RecordValue],
        at: Timestamp,
        complete: bool,
        ingested_at: Timestamp,
    ) -> SnapshotRound {
        let current: BTreeMap<Arc<str>, RecordValue> = snapshot
            .iter()
            .map(|record| (key_of(record), record.clone()))
            .collect();
        let first_round = self.last_at.is_none();
        let mut events = Vec::new();

        for (key, record) in &current {
            if !self.previous.contains_key(key) {
                let kind = if first_round {
                    EventKind::ObjectObserved
                } else {
                    EventKind::ObjectAppeared
                };
                events.push(
                    self.normalizer
                        .from_snapshot_diff(kind, record, at, ingested_at),
                );
            }
        }

        let departures: Vec<(Arc<str>, RecordValue)> = self
            .previous
            .iter()
            .filter(|(key, _)| !current.contains_key(*key))
            .map(|(key, record)| (Arc::clone(key), record.clone()))
            .collect();

        let trustworthy = complete
            && self.normalizer.profile().disappearance_is_meaningful
            && self.round_was_timely(at);
        let mut withheld = 0;
        let mut retained = current;
        if trustworthy {
            for (_, record) in &departures {
                events.push(self.normalizer.from_snapshot_diff(
                    EventKind::ObjectDisappeared,
                    record,
                    at,
                    ingested_at,
                ));
            }
        } else {
            withheld = departures.len();
            for (key, record) in departures {
                retained.insert(key, record);
            }
        }

        let from = self.last_at.unwrap_or(at);
        let gap = (!complete || (withheld > 0 && !first_round)).then(|| TemporalGap {
            scope: self.scope_of(),
            from,
            until: at,
            capability: self.normalizer.profile().existence_capability(),
            reason: if complete {
                GapReason::NotRecorded
            } else {
                GapReason::ProviderUnavailable
            },
            source: self.normalizer.profile().source.clone(),
            detail: None,
        });

        let coverage = TemporalCoverage {
            completeness: if complete {
                self.normalizer.profile().completeness()
            } else {
                TemporalCompleteness::PointSample
            },
            ..self.normalizer.profile().coverage_of(
                self.normalizer.profile().existence_capability(),
                &self.scope_of(),
                from,
                at,
                ono_spatial_core::PermissionState::Available,
            )
        };

        self.previous = retained;
        self.last_at = Some(at);
        SnapshotRound {
            events,
            coverage,
            gap,
            withheld_disappearances: withheld,
        }
    }

    /// Whether this round followed the previous one closely enough to trust an absence (§6.3).
    fn round_was_timely(&self, at: Timestamp) -> bool {
        let Some(last) = self.last_at else {
            return false;
        };
        let Some(interval) = self.normalizer.profile().delivery.sampling_interval() else {
            return true;
        };
        let elapsed = at.as_nanosecond() - last.as_nanosecond();
        elapsed <= interval.nanoseconds() * MISSED_ROUNDS_BEFORE_GAP
    }

    fn scope_of(&self) -> ono_spatial_core::SpatialScope {
        self.normalizer.scope().clone()
    }
}

/// The key two observations of one object share.
fn key_of(record: &RecordValue) -> Arc<str> {
    let identity = record.identity();
    let rendered: Vec<String> = identity
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    Arc::from(format!("{}|{}", record.schema_id(), rendered.join("|")))
}
