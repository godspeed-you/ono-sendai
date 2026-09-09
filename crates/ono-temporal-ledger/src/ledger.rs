//! One handle for a caller that does not care whether recording is on (§10.7, §32.1).
//!
//! §32.1: "with persistent recording disabled and no historical query executed, v0.5 MUST add less
//! than 5 ms p95 to Ono interactive startup", and "temporal storage initialization MUST be lazy
//! when recording is disabled". [`Ledger::session`] therefore touches no filesystem at all — it
//! constructs the in-memory ledger `ono-temporal-core` already owns and returns. Nothing opens a
//! database until [`Ledger::persistent`] is called, which is the moment `start recorder` happens.
//!
//! The in-memory ledger is `ono_temporal_core::SessionLedger`, used rather than reimplemented: it
//! is §10.7's own bounded ledger and it already satisfies the same contract.

use jiff::Timestamp;
use ono_spatial_core::SpatialScope;
use ono_temporal_core::{
    ActionEvent, Appended, CausalLink, Checkpoint, CoverageQuery, EventId, EventQuery, Evidence,
    EvidenceId, LedgerRead, LedgerWrite, RetentionState, SessionLedger, TemporalCoverage,
    TemporalEvent, TimeRange,
};
use ono_value::ErrorValue;

use crate::retention::Swept;
use crate::store::{LedgerStore, StoreOptions};

/// The ledger a session holds, recording or not (§10.7, §32.1).
#[derive(Debug)]
pub enum Ledger {
    /// The bounded in-memory ledger every session has (§10.7).
    Session(SessionLedger),
    /// The persistent store the recorder writes to (§31).
    Persistent(LedgerStore),
}

impl Default for Ledger {
    /// §10.2: "persistent recording MUST be disabled by default."
    fn default() -> Self {
        Self::session()
    }
}

impl Ledger {
    /// The in-memory ledger of §10.7, holding §33's `temporal.session.max_events` events.
    ///
    /// Nothing is opened, created or read: this is the path §32.1 budgets under 5 ms.
    #[must_use]
    pub fn session() -> Self {
        Ledger::Session(SessionLedger::new())
    }

    /// The in-memory ledger with a ceiling other than §10.7's default.
    #[must_use]
    pub fn session_with_capacity(capacity: usize) -> Self {
        Ledger::Session(SessionLedger::with_capacity(capacity))
    }

    /// Opens the persistent store, which is what `start recorder` does (§10.8).
    ///
    /// # Errors
    ///
    /// Returns `temporal.store_unavailable` or `temporal.store_corrupt` (§34).
    pub fn persistent(options: &StoreOptions) -> Result<Self, ErrorValue> {
        LedgerStore::open_with(options).map(Ledger::Persistent)
    }

    /// Whether history outlives the session (§10.2).
    #[must_use]
    pub const fn is_persistent(&self) -> bool {
        matches!(self, Ledger::Persistent(_))
    }

    /// The persistent store behind this ledger, or `None` when recording is off.
    #[must_use]
    pub const fn store(&self) -> Option<&LedgerStore> {
        match self {
            Ledger::Persistent(store) => Some(store),
            Ledger::Session(_) => None,
        }
    }

    /// Removes expired history where there is a store to remove it from (§10.4, §31.8).
    ///
    /// The session ledger enforces its own ceiling on every append, so a sweep there removes
    /// nothing and says so.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn sweep(&self, now: Timestamp) -> Result<Swept, ErrorValue> {
        match self {
            Ledger::Persistent(store) => store.sweep(now),
            Ledger::Session(_) => Ok(Swept {
                complete: true,
                ..Swept::default()
            }),
        }
    }

    /// The earliest instant history reaches, for §12.3's refusal to name.
    #[must_use]
    pub fn earliest_retained(&self) -> Option<Timestamp> {
        self.retention().earliest
    }

    /// Whether `at` is older than anything retained, which §34 distinguishes from never recorded.
    ///
    /// `temporal.out_of_retention` says history existed and expired; `temporal.not_recorded` says
    /// it was never there. A ledger holding nothing at all cannot claim the first.
    #[must_use]
    pub fn is_out_of_retention(&self, at: Timestamp) -> bool {
        self.retention()
            .earliest
            .is_some_and(|earliest| at < earliest)
    }
}

impl LedgerRead for Ledger {
    fn events(&self, query: &EventQuery) -> Result<Vec<TemporalEvent>, ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.events(query),
            Ledger::Persistent(store) => store.events(query),
        }
    }

    fn event(&self, id: &EventId) -> Result<Option<TemporalEvent>, ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.event(id),
            Ledger::Persistent(store) => store.event(id),
        }
    }

    fn shortest_unique_prefixes(
        &self,
        ids: &[EventId],
        minimum: usize,
    ) -> Result<Vec<usize>, ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.shortest_unique_prefixes(ids, minimum),
            Ledger::Persistent(store) => store.shortest_unique_prefixes(ids, minimum),
        }
    }

    fn evidence(&self, ids: &[EvidenceId]) -> Result<Vec<Evidence>, ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.evidence(ids),
            Ledger::Persistent(store) => store.evidence(ids),
        }
    }

    fn causal_links(&self, effect: &EventId) -> Result<Vec<CausalLink>, ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.causal_links(effect),
            Ledger::Persistent(store) => store.causal_links(effect),
        }
    }

    fn coverage(&self, query: &CoverageQuery) -> Result<Vec<TemporalCoverage>, ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.coverage(query),
            Ledger::Persistent(store) => store.coverage(query),
        }
    }

    fn checkpoint_before(
        &self,
        scope: &SpatialScope,
        at: Timestamp,
    ) -> Result<Option<Checkpoint>, ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.checkpoint_before(scope, at),
            Ledger::Persistent(store) => store.checkpoint_before(scope, at),
        }
    }

    fn actions(&self, range: TimeRange) -> Result<Vec<ActionEvent>, ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.actions(range),
            Ledger::Persistent(store) => store.actions(range),
        }
    }

    fn retention(&self) -> RetentionState {
        match self {
            Ledger::Session(ledger) => ledger.retention(),
            Ledger::Persistent(store) => store.retention(),
        }
    }
}

impl LedgerWrite for Ledger {
    fn append(
        &self,
        events: &[TemporalEvent],
        evidence: &[Evidence],
    ) -> Result<Appended, ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.append(events, evidence),
            Ledger::Persistent(store) => store.append(events, evidence),
        }
    }

    fn append_links(&self, links: &[CausalLink]) -> Result<usize, ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.append_links(links),
            Ledger::Persistent(store) => store.append_links(links),
        }
    }

    fn record_coverage(&self, intervals: &[TemporalCoverage]) -> Result<(), ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.record_coverage(intervals),
            Ledger::Persistent(store) => store.record_coverage(intervals),
        }
    }

    fn record_action(&self, action: &ActionEvent) -> Result<(), ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.record_action(action),
            Ledger::Persistent(store) => store.record_action(action),
        }
    }

    fn write_checkpoint(&self, checkpoint: &Checkpoint) -> Result<(), ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.write_checkpoint(checkpoint),
            Ledger::Persistent(store) => store.write_checkpoint(checkpoint),
        }
    }

    fn flush(&self) -> Result<(), ErrorValue> {
        match self {
            Ledger::Session(ledger) => ledger.flush(),
            Ledger::Persistent(store) => store.flush(),
        }
    }
}
