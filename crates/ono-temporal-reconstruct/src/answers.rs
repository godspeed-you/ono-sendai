//! Provider-owned historical answers (v0.5 §9.1 step 3, §21.4).
//!
//! §9.1 puts these third, after the checkpoint and the replayed events: "merge provider-owned
//! historical queries that directly answer fields at `T`". A provider that can answer about the
//! past directly is stronger evidence about that instant than a value carried forward from an
//! earlier reading, and §9.3's `valid_from`/`valid_until` come from exactly here — from the
//! source's own semantics rather than from arithmetic between two of Ono's readings.
//!
//! This crate reaches no provider (§39.2, §39.3). The answers arrive as values a caller already
//! obtained, which is what lets the whole engine be tested against an in-memory ledger.

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{SpatialId, SpatialType};
use ono_temporal_core::{EvidenceSource, EvidenceStrength};
use ono_value::RecordValue;

/// One provider's answer about one object at one instant (§21.4).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoricalAnswer {
    pub(crate) subject: SpatialId,
    pub(crate) object_type: SpatialType,
    pub(crate) label: Arc<str>,
    pub(crate) record: RecordValue,
    pub(crate) observed_at: Timestamp,
    pub(crate) valid_from: Option<Timestamp>,
    pub(crate) valid_until: Option<Timestamp>,
    pub(crate) source: EvidenceSource,
    pub(crate) strength: EvidenceStrength,
}

impl HistoricalAnswer {
    /// An answer about `subject` as `source` reports it for `observed_at`.
    ///
    /// The strength starts at [`EvidenceStrength::Observational`], the weakest of §7.2's five.
    /// A caller that knows the source owns the fact says so with
    /// [`with_strength`](Self::with_strength); nothing in this crate raises it afterwards.
    #[must_use]
    pub fn new(
        subject: SpatialId,
        object_type: SpatialType,
        label: &str,
        record: RecordValue,
        observed_at: Timestamp,
        source: EvidenceSource,
    ) -> Self {
        Self {
            subject,
            object_type,
            label: Arc::from(label),
            record,
            observed_at,
            valid_from: None,
            valid_until: None,
            source,
            strength: EvidenceStrength::Observational,
        }
    }

    /// The answer with the strength its source actually supports (§7.2).
    #[must_use]
    pub const fn with_strength(mut self, strength: EvidenceStrength) -> Self {
        self.strength = strength;
        self
    }

    /// The interval the source itself says the answer holds over (§9.3).
    #[must_use]
    pub const fn valid_over(mut self, from: Option<Timestamp>, until: Option<Timestamp>) -> Self {
        self.valid_from = from;
        self.valid_until = until;
        self
    }

    /// The object the answer is about.
    #[must_use]
    pub const fn subject(&self) -> &SpatialId {
        &self.subject
    }

    /// Whether the answer's own validity interval covers `at` — §9.1's "directly answer fields
    /// at `T`", as opposed to a reading that merely sits before it.
    #[must_use]
    pub fn speaks_to(&self, at: Timestamp) -> bool {
        match (self.valid_from, self.valid_until) {
            (Some(from), Some(until)) => from <= at && at <= until,
            (Some(from), None) => from <= at,
            (None, Some(until)) => at <= until,
            (None, None) => self.observed_at == at,
        }
    }
}

/// The provider-owned answers a caller obtained for one reconstruction.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HistoricalAnswers {
    answers: Vec<HistoricalAnswer>,
}

impl HistoricalAnswers {
    /// No provider was asked, or none could answer.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The answers a caller obtained.
    #[must_use]
    pub fn of(answers: Vec<HistoricalAnswer>) -> Self {
        Self { answers }
    }

    /// Every answer, in the order the caller supplied them.
    pub fn iter(&self) -> impl Iterator<Item = &HistoricalAnswer> {
        self.answers.iter()
    }

    /// Whether any provider answered at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.answers.is_empty()
    }
}
