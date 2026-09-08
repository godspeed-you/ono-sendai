//! The bounded session ledger of v0.5 §10.7.
//!
//! "Even without the recorder, the interactive session SHOULD maintain a bounded in-memory
//! ledger for current-session temporal features", with a default ceiling of 100 000 events. It
//! lives beside the contract that defines it rather than in the persistent store's crate,
//! because it *is* the contract with nothing behind it: `ono-temporal-reconstruct` and
//! `ono-temporal-query` run against this in a session with no recorder.
//!
//! The ceiling is the interesting part. A ledger that silently drops its oldest events reports a
//! quiet morning where there was a full one, so eviction leaves a [`TemporalGap`] with the reason
//! §7.5 gives it and a coverage interval that composes into that gap (§8.5).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use jiff::Timestamp;
use ono_spatial_core::{PermissionState, SpatialScope};
use ono_value::ErrorValue;

use crate::action::ActionEvent;
use crate::causal::CausalLink;
use crate::coverage::{GapReason, TemporalCompleteness, TemporalCoverage, TemporalGap};
use crate::error;
use crate::event::TemporalEvent;
use crate::evidence::Evidence;
use crate::id::{EventId, EvidenceId};
use crate::ledger::{
    Appended, Checkpoint, CoverageQuery, EventQuery, LedgerRead, LedgerWrite, QueryOrder,
    RetentionState, TimeRange,
};
use crate::order::presentation_order;
use crate::source::EvidenceSource;
use crate::time::EventAnchors;

/// The ceiling `temporal.session.max_events` defaults to (§10.7, §33).
pub const DEFAULT_SESSION_MAX_EVENTS: usize = 100_000;

/// The capability an evicted stretch of the session ledger stops covering.
const LEDGER_CAPABILITY: &str = "temporal.events";

/// The in-memory ledger a session keeps whether or not the recorder runs (§10.7).
///
/// It satisfies the same [`LedgerRead`] and [`LedgerWrite`] contract as the persistent store, so
/// nothing above it has to know which one it is talking to (§39).
#[derive(Debug)]
pub struct SessionLedger {
    capacity: usize,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    events: VecDeque<TemporalEvent>,
    evidence: Vec<Evidence>,
    links: Vec<CausalLink>,
    coverage: Vec<TemporalCoverage>,
    actions: Vec<ActionEvent>,
    checkpoints: Vec<Checkpoint>,
    evicted: u64,
    eviction: Option<TemporalGap>,
}

impl Default for SessionLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionLedger {
    /// A ledger with §10.7's default ceiling.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_SESSION_MAX_EVENTS)
    }

    /// A ledger holding at most `capacity` events.
    ///
    /// A capacity of zero is raised to one: a ledger that can hold nothing would evict every
    /// event it was given, which is a configuration mistake rather than a behaviour to honour.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            state: Mutex::new(State::default()),
        }
    }

    /// The ceiling in force.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The stretch the ceiling has already dropped, or `None` where it has dropped nothing.
    ///
    /// §7.5 makes a gap a typed object rather than an absence, so this is what a timeline draws
    /// at the start of a long session and what §11.7 forbids hiding.
    #[must_use]
    pub fn eviction_boundary(&self) -> Option<TemporalGap> {
        self.locked().eviction.clone()
    }

    /// How many events it holds now.
    #[must_use]
    pub fn len(&self) -> usize {
        self.locked().events.len()
    }

    /// Whether it holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locked().events.is_empty()
    }

    /// The state behind the lock, ignoring poisoning.
    ///
    /// A panic elsewhere must not turn every later temporal query into a refusal: the data is a
    /// queue of immutable events, so it is exactly as valid after a poisoning as before one.
    fn locked(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    /// Drops the oldest events until the ceiling holds, recording what went (§10.7).
    fn enforce(&mut self, capacity: usize) {
        while self.events.len() > capacity {
            let Some(dropped) = self.events.pop_front() else {
                break;
            };
            self.evicted += 1;
            let at = dropped.times.presentation_instant();
            match &mut self.eviction {
                Some(gap) => gap.until = gap.until.max(at),
                None => {
                    self.eviction = Some(TemporalGap {
                        scope: dropped.scope.clone(),
                        from: at,
                        until: at,
                        capability: Arc::from(LEDGER_CAPABILITY),
                        reason: GapReason::RetentionExpired,
                        source: EvidenceSource::session(),
                    });
                }
            }
        }
        if let Some(gap) = &self.eviction {
            let boundary = TemporalCoverage {
                scope: gap.scope.clone(),
                capability: Arc::from(LEDGER_CAPABILITY),
                from: gap.from,
                until: gap.until,
                completeness: TemporalCompleteness::Unavailable,
                sampling_interval: None,
                source: EvidenceSource::session(),
                permission: PermissionState::Unknown,
            };
            match self.coverage.iter_mut().find(|interval| {
                interval.completeness == TemporalCompleteness::Unavailable
                    && &*interval.capability == LEDGER_CAPABILITY
            }) {
                Some(existing) => *existing = boundary,
                None => self.coverage.push(boundary),
            }
        }
    }
}

impl EventAnchors for SessionLedger {
    fn instant_of(&self, id: &EventId) -> Option<Timestamp> {
        let state = self.locked();
        let mut matches = state
            .events
            .iter()
            .filter(|event| event.event_id.as_str().starts_with(id.as_str()));
        let first = matches.next()?;
        matches
            .next()
            .is_none()
            .then(|| first.times.presentation_instant())
    }
}

impl LedgerRead for SessionLedger {
    fn events(&self, query: &EventQuery) -> Result<Vec<TemporalEvent>, ErrorValue> {
        let state = self.locked();
        let mut found: Vec<TemporalEvent> = state
            .events
            .iter()
            .filter(|event| query.matches(event))
            .cloned()
            .collect();
        presentation_order(&mut found);
        if query.order == QueryOrder::Descending {
            found.reverse();
        }
        if let Some(limit) = query.limit {
            found.truncate(limit);
        }
        Ok(found)
    }

    fn event(&self, id: &EventId) -> Result<Option<TemporalEvent>, ErrorValue> {
        let state = self.locked();
        let matches: Vec<&TemporalEvent> = state
            .events
            .iter()
            .filter(|event| event.event_id.as_str().starts_with(id.as_str()))
            .collect();
        match matches.as_slice() {
            [] => Ok(None),
            [only] => Ok(Some((*only).clone())),
            many => Err(error::ambiguous_event(
                &many
                    .iter()
                    .map(|event| event.event_id.clone())
                    .collect::<Vec<_>>(),
            )),
        }
    }

    fn evidence(&self, ids: &[EvidenceId]) -> Result<Vec<Evidence>, ErrorValue> {
        let state = self.locked();
        Ok(state
            .evidence
            .iter()
            .filter(|evidence| ids.contains(&evidence.evidence_id))
            .cloned()
            .collect())
    }

    fn causal_links(&self, effect: &EventId) -> Result<Vec<CausalLink>, ErrorValue> {
        let state = self.locked();
        Ok(state
            .links
            .iter()
            .filter(|link| &link.effect == effect)
            .cloned()
            .collect())
    }

    fn coverage(&self, query: &CoverageQuery) -> Result<Vec<TemporalCoverage>, ErrorValue> {
        let state = self.locked();
        Ok(state
            .coverage
            .iter()
            .filter(|interval| query.matches(interval))
            .cloned()
            .collect())
    }

    fn checkpoint_before(
        &self,
        scope: &SpatialScope,
        at: Timestamp,
    ) -> Result<Option<Checkpoint>, ErrorValue> {
        let state = self.locked();
        Ok(state
            .checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.captured_at <= at && scope.contains(&checkpoint.scope))
            .max_by_key(|checkpoint| checkpoint.captured_at)
            .cloned())
    }

    fn actions(&self, range: TimeRange) -> Result<Vec<ActionEvent>, ErrorValue> {
        let state = self.locked();
        Ok(state
            .actions
            .iter()
            .filter(|action| range.contains(action.requested_at))
            .cloned()
            .collect())
    }

    fn retention(&self) -> RetentionState {
        let state = self.locked();
        RetentionState {
            earliest: state
                .events
                .iter()
                .map(|event| event.times.presentation_instant())
                .min(),
            latest: state
                .events
                .iter()
                .map(|event| event.times.presentation_instant())
                .max(),
            events: state.events.len() as u64,
            evicted: state.evicted,
            stored_size: None,
            max_age: None,
            max_size: None,
        }
    }
}

impl LedgerWrite for SessionLedger {
    fn append(
        &self,
        events: &[TemporalEvent],
        evidence: &[Evidence],
    ) -> Result<Appended, ErrorValue> {
        let mut state = self.locked();
        let mut appended = Appended::default();
        for event in events {
            if state
                .events
                .iter()
                .any(|held| held.event_id == event.event_id)
            {
                appended.duplicates += 1;
                continue;
            }
            state.events.push_back(event.clone());
            appended.stored += 1;
        }
        for record in evidence {
            if !state
                .evidence
                .iter()
                .any(|held| held.evidence_id == record.evidence_id)
            {
                state.evidence.push(record.clone());
            }
        }
        let capacity = self.capacity;
        state.enforce(capacity);
        Ok(appended)
    }

    fn append_links(&self, links: &[CausalLink]) -> Result<usize, ErrorValue> {
        let mut state = self.locked();
        let mut stored = 0;
        for link in links {
            if !state.links.iter().any(|held| held.link_id == link.link_id) {
                state.links.push(link.clone());
                stored += 1;
            }
        }
        Ok(stored)
    }

    fn record_coverage(&self, intervals: &[TemporalCoverage]) -> Result<(), ErrorValue> {
        let mut state = self.locked();
        state.coverage.extend(intervals.iter().cloned());
        Ok(())
    }

    fn record_action(&self, action: &ActionEvent) -> Result<(), ErrorValue> {
        let mut state = self.locked();
        match state
            .actions
            .iter_mut()
            .find(|held| held.action_id == action.action_id)
        {
            // §17.2's lifecycle updates one action rather than appending a second: the identity
            // is minted before execution and the result arrives later.
            Some(held) => *held = action.clone(),
            None => state.actions.push(action.clone()),
        }
        Ok(())
    }

    fn write_checkpoint(&self, checkpoint: &Checkpoint) -> Result<(), ErrorValue> {
        let mut state = self.locked();
        state.checkpoints.push(checkpoint.clone());
        Ok(())
    }

    fn flush(&self) -> Result<(), ErrorValue> {
        // Nothing outlives the session, so there is nothing to make durable (§10.7).
        Ok(())
    }
}
