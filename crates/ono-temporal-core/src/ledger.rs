//! The ledger contract of v0.5 §39: what a temporal store answers, in the vocabulary of §3.
//!
//! §39.4 is the constraint that shapes this module: "`ono-temporal-core` MUST not expose SQLite
//! types or SQL semantics". Nothing here names a connection, a statement, a table or a file.
//! `ono-temporal-reconstruct` and `ono-temporal-query` therefore compile against these traits,
//! and both `ono-temporal-ledger`'s persistent store and [`crate::SessionLedger`]'s bounded
//! in-memory one satisfy the same contract.

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{SpatialId, SpatialScope, SpatialType};
use ono_value::{ErrorValue, Provenance, RecordValue};

use crate::action::ActionEvent;
use crate::causal::CausalLink;
use crate::coverage::TemporalCoverage;
use crate::event::{EventKind, TemporalEvent};
use crate::evidence::Evidence;
use crate::id::{CheckpointId, EventId, EvidenceId};
use crate::source::EvidenceSource;

/// A half-open interval `[from, until)`, either end open (§13.1's `--since` and `--until`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeRange {
    /// The earliest instant included, or `None` for "as far back as there is".
    pub from: Option<Timestamp>,
    /// The first instant excluded, or `None` for "up to the end".
    pub until: Option<Timestamp>,
}

impl TimeRange {
    /// Everything the ledger holds.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            from: None,
            until: None,
        }
    }

    /// `[from, until)`.
    #[must_use]
    pub const fn between(from: Timestamp, until: Timestamp) -> Self {
        Self {
            from: Some(from),
            until: Some(until),
        }
    }

    /// Everything from `from` onwards.
    #[must_use]
    pub const fn since(from: Timestamp) -> Self {
        Self {
            from: Some(from),
            until: None,
        }
    }

    /// Everything up to `until`.
    #[must_use]
    pub const fn until(until: Timestamp) -> Self {
        Self {
            from: None,
            until: Some(until),
        }
    }

    /// The degenerate range that is one instant — what a point sample covers (§8.4).
    #[must_use]
    pub const fn at(instant: Timestamp) -> Self {
        Self {
            from: Some(instant),
            until: Some(instant),
        }
    }

    /// Whether `instant` falls inside. A degenerate range contains its own instant.
    #[must_use]
    pub fn contains(&self, instant: Timestamp) -> bool {
        let after_start = self.from.is_none_or(|from| instant >= from);
        let before_end = match (self.from, self.until) {
            (Some(from), Some(until)) if from == until => instant == until,
            (_, Some(until)) => instant < until,
            (_, None) => true,
        };
        after_start && before_end
    }

    /// A stable text for the range, so an evidence claim over it digests deterministically.
    pub(crate) fn digest_token(&self) -> String {
        format!(
            "{}\u{1}{}",
            self.from
                .map(|from| from.as_nanosecond().to_string())
                .unwrap_or_default(),
            self.until
                .map(|until| until.as_nanosecond().to_string())
                .unwrap_or_default()
        )
    }
}

/// Which way round a query returns its events (§11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryOrder {
    /// Oldest first, by the instant a human navigates by.
    #[default]
    Ascending,
    /// Newest first.
    Descending,
}

impl QueryOrder {
    /// The name a command option spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            QueryOrder::Ascending => "ascending",
            QueryOrder::Descending => "descending",
        }
    }
}

/// What a caller wants from the event ledger (§11.2, §20.3).
#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    /// Restrict to one boundary and everything inside it. `None` for every scope.
    pub scope: Option<SpatialScope>,
    /// Restrict to events about these subjects. Empty for any subject.
    pub subjects: Vec<SpatialId>,
    /// Restrict to these kinds. Empty for any kind.
    pub kinds: Vec<EventKind>,
    /// The window, by the instant a human navigates by.
    pub range: TimeRange,
    /// At most this many events. `None` for as many as match.
    pub limit: Option<usize>,
    /// Which way round (§11.2).
    pub order: QueryOrder,
}

impl EventQuery {
    /// Every event in `range`.
    #[must_use]
    pub fn in_range(range: TimeRange) -> Self {
        Self {
            range,
            ..Self::default()
        }
    }

    /// Whether `event` matches every restriction this query names.
    #[must_use]
    pub fn matches(&self, event: &TemporalEvent) -> bool {
        if !self.range.contains(event.times.presentation_instant()) {
            return false;
        }
        if let Some(scope) = &self.scope
            && !scope.contains(&event.scope)
        {
            return false;
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&event.kind) {
            return false;
        }
        if !self.subjects.is_empty() {
            let mentions = event
                .subject
                .iter()
                .chain(event.related.iter())
                .filter_map(crate::event::SpatialRef::spatial_id)
                .any(|id| self.subjects.contains(id));
            if !mentions {
                return false;
            }
        }
        true
    }
}

/// What a caller wants from the coverage record (§8.1).
#[derive(Debug, Clone, Default)]
pub struct CoverageQuery {
    /// Restrict to one boundary and everything inside it. `None` for every scope.
    pub scope: Option<SpatialScope>,
    /// Restrict to these capabilities. Empty for every capability.
    pub capabilities: Vec<Arc<str>>,
    /// The window.
    pub range: TimeRange,
}

impl CoverageQuery {
    /// Whether `interval` matches every restriction this query names.
    #[must_use]
    pub fn matches(&self, interval: &TemporalCoverage) -> bool {
        if let Some(scope) = &self.scope
            && !scope.contains(&interval.scope)
        {
            return false;
        }
        if !self.capabilities.is_empty() && !self.capabilities.contains(&interval.capability) {
            return false;
        }
        let after_start = self.range.from.is_none_or(|from| interval.until >= from);
        let before_end = self.range.until.is_none_or(|until| interval.from <= until);
        after_start && before_end
    }
}

/// What an append actually did (§6.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Appended {
    /// How many events were new.
    pub stored: usize,
    /// How many were already there, by identity rather than by rendered text (§6.8).
    pub duplicates: usize,
}

/// How much history there is and under which limits (§10.4, §12.3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetentionState {
    /// The earliest instant still retained — the boundary `at` refuses beyond (§12.3).
    pub earliest: Option<Timestamp>,
    /// The most recent instant retained.
    pub latest: Option<Timestamp>,
    /// How many events are held now.
    pub events: u64,
    /// How many have been removed by retention or by the session ceiling (§10.7).
    pub evicted: u64,
    /// How much space the history occupies, where it occupies any.
    pub stored_size: Option<ono_value::ByteSize>,
    /// `temporal.retention.max_age`, where a policy is in force (§10.4).
    pub max_age: Option<ono_value::Duration>,
    /// `temporal.retention.max_size`, where a policy is in force (§10.4).
    pub max_size: Option<ono_value::ByteSize>,
}

/// One source's answer to "how far back can you see?", for §34's `temporal.not_recorded`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAvailability {
    /// The §7.1 source that was asked.
    pub source: EvidenceSource,
    /// The earliest instant it can answer for. `None` where it reaches nothing.
    pub earliest: Option<Timestamp>,
    /// Whether it answered at all.
    pub available: bool,
    /// Why it cannot answer, where that needs saying.
    pub detail: Option<Arc<str>>,
}

/// A canonical object as one source observed it, inside a checkpoint (§42.2).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectState {
    /// The v0.4 identity.
    pub id: SpatialId,
    /// What kind of object it is.
    pub object_type: SpatialType,
    /// What a person calls it.
    pub label: Arc<str>,
    /// The provider's own record, as observed (§42.2).
    pub record: RecordValue,
    /// When it was observed.
    pub observed_at: Timestamp,
    /// The §7.1 source that observed it.
    pub source: EvidenceSource,
}

/// A spatial relationship as one source observed it, inside a checkpoint (§42.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationState {
    /// The identity at the near end.
    pub from: SpatialId,
    /// The identity at the far end.
    pub to: SpatialId,
    /// The relation type, as v0.4's registry names it.
    pub relation: Arc<str>,
    /// How well the edge is known (v0.4 §11.5).
    pub confidence: ono_spatial_core::Confidence,
    /// When it was observed.
    pub observed_at: Timestamp,
    /// The §7.1 source that observed it.
    pub source: EvidenceSource,
}

/// A bounded retained state projection (§3.6, §42).
///
/// §3.6: "a checkpoint is not automatically complete just because it serializes many objects" —
/// which is why it carries its own coverage rather than implying any.
#[derive(Debug, Clone, PartialEq)]
pub struct Checkpoint {
    /// The identity.
    pub checkpoint_id: CheckpointId,
    /// The boundary the projection covers (§42.1).
    pub scope: SpatialScope,
    /// The instant it captured.
    pub captured_at: Timestamp,
    /// What the capture was actually able to see (§3.6).
    pub coverage: Vec<TemporalCoverage>,
    /// The objects it holds.
    pub objects: Vec<ObjectState>,
    /// The relationships it holds.
    pub relations: Vec<RelationState>,
    /// Where the record came from (v0.2 §25.2).
    pub provenance: Provenance,
}

/// Reading the temporal ledger (§39).
///
/// Every method names events, evidence, coverage, actions or checkpoints. None names a store:
/// §39.4 keeps SQL out of the core types, so a caller here cannot tell — and must not care —
/// whether the answer came from memory, from a file or from a link.
pub trait LedgerRead: Send + Sync + std::fmt::Debug {
    /// The events matching `query`, in the order it asks for.
    ///
    /// # Errors
    ///
    /// Returns a §34 refusal where the store cannot answer — `temporal.store_unavailable`,
    /// `temporal.store_corrupt` or `temporal.permission_denied`.
    fn events(&self, query: &EventQuery) -> Result<Vec<TemporalEvent>, ErrorValue>;

    /// One event by reference, which may be shortened (§11.6).
    ///
    /// # Errors
    ///
    /// Returns `temporal.ambiguous_event` where a shortened reference names more than one, and a
    /// store refusal otherwise. A reference that names none is `Ok(None)`.
    fn event(&self, id: &EventId) -> Result<Option<TemporalEvent>, ErrorValue>;

    /// How long a prefix of each of `ids` has to be to name at most one retained event (§11.6).
    ///
    /// The answer is a length in characters of [`EventId::as_str`] — `4` is `e34b` — parallel to
    /// `ids` and never below `minimum`, and it is the inverse of the question
    /// [`LedgerRead::event`] answers: whoever prints a reference the reader can type back has to
    /// know which prefixes this store would call ambiguous, and only the store knows that. It is
    /// batched because a timeline mints one reference per rendered event and asks once for all of
    /// them, and `minimum` is what lets a store stop distinguishing below the shortest length its
    /// caller would ever print.
    ///
    /// The default answers with the whole identity, which is always unambiguous and never short.
    /// A store that can order identities overrides it with something a person can type.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn shortest_unique_prefixes(
        &self,
        ids: &[EventId],
        minimum: usize,
    ) -> Result<Vec<usize>, ErrorValue> {
        let _ = minimum;
        Ok(ids.iter().map(|id| id.as_str().len()).collect())
    }

    /// The evidence records behind `ids` (§7.3).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn evidence(&self, ids: &[EvidenceId]) -> Result<Vec<Evidence>, ErrorValue>;

    /// The causal links whose effect is `effect` (§16.7).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn causal_links(&self, effect: &EventId) -> Result<Vec<CausalLink>, ErrorValue>;

    /// The coverage intervals matching `query` (§8.1).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn coverage(&self, query: &CoverageQuery) -> Result<Vec<TemporalCoverage>, ErrorValue>;

    /// The most recent checkpoint for `scope` at or before `at` (§9.1).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn checkpoint_before(
        &self,
        scope: &SpatialScope,
        at: Timestamp,
    ) -> Result<Option<Checkpoint>, ErrorValue>;

    /// The actions requested through Ono in `range` (§17.4).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn actions(&self, range: TimeRange) -> Result<Vec<ActionEvent>, ErrorValue>;

    /// How much history there is and under which limits (§10.4).
    fn retention(&self) -> RetentionState;
}

/// Writing the temporal ledger (§39).
///
/// §6.7 makes the ledger append-only: there is no method here that rewrites an event, because
/// "corrections are represented by new events or evidence records referencing the prior event".
pub trait LedgerWrite: Send + Sync + std::fmt::Debug {
    /// Appends events and the evidence behind them, deduplicating by identity (§6.8).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn append(
        &self,
        events: &[TemporalEvent],
        evidence: &[Evidence],
    ) -> Result<Appended, ErrorValue>;

    /// Appends causal links, returning how many were new (§15).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn append_links(&self, links: &[CausalLink]) -> Result<usize, ErrorValue>;

    /// Records what a source was able to observe over an interval (§8.1).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn record_coverage(&self, intervals: &[TemporalCoverage]) -> Result<(), ErrorValue>;

    /// Records an action the operator requested through the shell (§17.4).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn record_action(&self, action: &ActionEvent) -> Result<(), ErrorValue>;

    /// Stores a bounded state projection (§42).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn write_checkpoint(&self, checkpoint: &Checkpoint) -> Result<(), ErrorValue>;

    /// Makes everything appended so far durable (§10.8: `stop recorder` MUST flush).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    fn flush(&self) -> Result<(), ErrorValue>;
}

/// How long a prefix of `id` has to be for `neighbour` not to share it (§11.6).
///
/// One character more than the two have in common, and never more than `id` itself is: the rule
/// every [`LedgerRead::shortest_unique_prefixes`] implementation applies to whichever retained
/// identities it found nearest.
#[must_use]
pub fn distinguishing_length(id: &str, neighbour: &str) -> usize {
    let shared = id
        .chars()
        .zip(neighbour.chars())
        .take_while(|(left, right)| left == right)
        .count();
    shared.saturating_add(1).min(id.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(text: &str) -> Timestamp {
        text.parse().unwrap_or(Timestamp::UNIX_EPOCH)
    }

    #[test]
    fn should_exclude_the_upper_end_when_a_range_is_half_open() {
        let range = TimeRange::between(
            instant("2026-01-01T00:00:00Z"),
            instant("2026-01-01T01:00:00Z"),
        );
        assert!(range.contains(instant("2026-01-01T00:00:00Z")));
        assert!(range.contains(instant("2026-01-01T00:59:59Z")));
        assert!(!range.contains(instant("2026-01-01T01:00:00Z")));
        assert!(TimeRange::all().contains(instant("2026-01-01T01:00:00Z")));
    }

    #[test]
    fn should_contain_its_own_instant_when_the_range_is_a_point() {
        let point = TimeRange::at(instant("2026-01-01T00:00:00Z"));
        assert!(point.contains(instant("2026-01-01T00:00:00Z")));
        assert!(!point.contains(instant("2026-01-01T00:00:01Z")));
    }
}
