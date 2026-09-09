//! When to take a checkpoint, and why (v0.5 §31.9, §18.4, §27.1).
//!
//! §31.9 is two sentences and both are load-bearing: "default checkpoint interval is 5 minutes,
//! but providers MAY trigger additional checkpoints around high-significance topology changes if
//! doing so is cheap and bounded", and "checkpoints MUST not block the interactive prompt".
//!
//! This module is the first sentence. The second is the recorder's, which runs the projection on
//! a task of its own ([`crate::PendingCheckpoint`]) so the only thing the prompt ever waits for
//! is the write, and never the walk over what a scope currently holds.
//!
//! *Bounded* is the word that turns "additional checkpoints around high-significance changes"
//! into something that survives an event storm. A restart loop that appears and disappears a
//! hundred processes a second is exactly a high-significance change happening a hundred times a
//! second, and taking a checkpoint each time would cost more than the history is worth. So an
//! interval holds at most [`CheckpointSchedule::MAX_SIGNIFICANT_PER_INTERVAL`] extra checkpoints
//! and the rest of the storm rides on the events themselves, which lose nothing.

use jiff::Timestamp;
use ono_temporal_core::{EventKind, TemporalEvent};
use ono_value::Duration;

/// Why a checkpoint is being taken (§31.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointReason {
    /// The interval elapsed.
    Scheduled,
    /// A high-significance topology change arrived.
    Significant,
}

impl CheckpointReason {
    /// The word a status or a landmark reads.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CheckpointReason::Scheduled => "scheduled",
            CheckpointReason::Significant => "significant",
        }
    }
}

/// Whether an event is the kind §31.9 calls a high-significance topology change.
///
/// Appearance, disappearance and both relation kinds change what the world *is* rather than what
/// one of its fields reads, and those are the four a reconstruction has to replay from a
/// checkpoint to get right. §18.4 and §27.1 step by the same predicate.
#[must_use]
pub fn is_significant(event: &TemporalEvent) -> bool {
    matches!(
        event.kind,
        EventKind::ObjectAppeared
            | EventKind::ObjectDisappeared
            | EventKind::RelationAdded
            | EventKind::RelationRemoved
            | EventKind::LandmarkAdded
            | EventKind::LandmarkRemoved
    )
}

/// The cadence of §31.9.
#[derive(Debug, Clone)]
pub struct CheckpointSchedule {
    interval: Duration,
    last: Option<Timestamp>,
    significant: Vec<Timestamp>,
}

impl Default for CheckpointSchedule {
    /// §10.4, §31.9: five minutes.
    fn default() -> Self {
        Self::every(crate::settings::DEFAULT_CHECKPOINT_INTERVAL)
    }
}

impl CheckpointSchedule {
    /// How many extra checkpoints one interval may hold (§31.9's "cheap and bounded").
    pub const MAX_SIGNIFICANT_PER_INTERVAL: usize = 4;

    /// A schedule taking a checkpoint every `interval`.
    ///
    /// An interval of zero or less is raised to the default: a schedule that is always due would
    /// checkpoint in a loop, which is the opposite of bounded.
    #[must_use]
    pub fn every(interval: Duration) -> Self {
        let interval = if interval.nanoseconds() <= 0 {
            crate::settings::DEFAULT_CHECKPOINT_INTERVAL
        } else {
            interval
        };
        Self {
            interval,
            last: None,
            significant: Vec::new(),
        }
    }

    /// The interval between scheduled checkpoints.
    #[must_use]
    pub const fn interval(&self) -> Duration {
        self.interval
    }

    /// When the last checkpoint was taken, or `None` where none has been.
    #[must_use]
    pub const fn last(&self) -> Option<Timestamp> {
        self.last
    }

    /// The instant the next scheduled checkpoint is due, or `None` where one is due now.
    #[must_use]
    pub fn next_due(&self) -> Option<Timestamp> {
        self.last.map(|last| last + span_of(self.interval))
    }

    /// Whether a checkpoint is due at `now`, and why (§31.9).
    ///
    /// A recorder that has never checkpointed is always due: without one there is nothing for
    /// §9.1's reconstruction to stand on, and the events alone reach only as far back as the
    /// oldest of them.
    #[must_use]
    pub fn due(&self, now: Timestamp) -> Option<CheckpointReason> {
        match self.last {
            None => Some(CheckpointReason::Scheduled),
            Some(last) if now >= last + span_of(self.interval) => Some(CheckpointReason::Scheduled),
            Some(_) => None,
        }
    }

    /// Whether `event` earns an extra checkpoint at `now` (§31.9).
    ///
    /// `Some` at most [`Self::MAX_SIGNIFICANT_PER_INTERVAL`] times per interval; the storm after
    /// that rides on the events, which lose nothing by not being checkpointed.
    pub fn on_change(&mut self, event: &TemporalEvent, now: Timestamp) -> Option<CheckpointReason> {
        if let Some(reason) = self.due(now) {
            return Some(reason);
        }
        if !is_significant(event) {
            return None;
        }
        let window_start = now - span_of(self.interval);
        self.significant.retain(|at| *at > window_start);
        if self.significant.len() >= Self::MAX_SIGNIFICANT_PER_INTERVAL {
            return None;
        }
        self.significant.push(now);
        Some(CheckpointReason::Significant)
    }

    /// Records that a checkpoint was taken at `at`.
    pub fn taken(&mut self, at: Timestamp) {
        self.last = Some(at);
    }
}

/// The span a duration is, saturating at zero for a negative one.
fn span_of(duration: Duration) -> jiff::Span {
    let nanoseconds = i64::try_from(duration.nanoseconds().max(0)).unwrap_or(i64::MAX);
    jiff::Span::new().nanoseconds(nanoseconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_treat_a_zero_interval_as_the_default_when_a_schedule_is_built() {
        let schedule = CheckpointSchedule::every(Duration::from_nanoseconds(0));
        assert_eq!(
            schedule.interval(),
            crate::settings::DEFAULT_CHECKPOINT_INTERVAL
        );
    }
}
