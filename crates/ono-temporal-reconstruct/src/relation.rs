//! Historical relations (v0.5 §9.5, §14.3) and the payload a relation event carries (§6.4).
//!
//! §9.5: "a relation exists at `T` only if reconstruction supports its existence at `T`", and
//! "unknown relation state MUST be distinguishable from absent relation state". The first
//! sentence is [`ReconstructedRelation::presence`]; the second is [`crate::Presence`], which has
//! a third variant so the distinction cannot be lost by a caller reading a boolean.

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{Confidence, SpatialId};
use ono_temporal_core::{EvidenceSource, TemporalCompleteness};

use crate::field::Presence;

/// One relationship as reconstruction supports it at the requested instant (§9.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructedRelation {
    from: SpatialId,
    to: SpatialId,
    relation: Arc<str>,
    presence: Presence,
    valid_from: Option<Timestamp>,
    valid_until: Option<Timestamp>,
    confidence: Confidence,
    completeness: TemporalCompleteness,
    sources: Vec<EvidenceSource>,
}

impl ReconstructedRelation {
    /// An edge with this support behind it.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "every field of the edge is part of §3.5's answer to `why are these related?`"
    )]
    pub fn new(
        from: SpatialId,
        to: SpatialId,
        relation: Arc<str>,
        presence: Presence,
        valid_from: Option<Timestamp>,
        valid_until: Option<Timestamp>,
        confidence: Confidence,
        completeness: TemporalCompleteness,
        sources: Vec<EvidenceSource>,
    ) -> Self {
        Self {
            from,
            to,
            relation,
            presence,
            valid_from,
            valid_until,
            confidence,
            completeness,
            sources,
        }
    }

    /// The identity at the near end.
    #[must_use]
    pub const fn from(&self) -> &SpatialId {
        &self.from
    }

    /// The identity at the far end.
    #[must_use]
    pub const fn to(&self) -> &SpatialId {
        &self.to
    }

    /// The relation type, as v0.4's registry names it.
    #[must_use]
    pub fn relation(&self) -> &str {
        &self.relation
    }

    /// Whether the edge held at the requested instant (§9.5).
    #[must_use]
    pub const fn presence(&self) -> Presence {
        self.presence
    }

    /// When the edge was first supported, where an observation says so.
    #[must_use]
    pub const fn valid_from(&self) -> Option<Timestamp> {
        self.valid_from
    }

    /// When it was observed to end. `None` where no observation ended it (§9.3).
    #[must_use]
    pub const fn valid_until(&self) -> Option<Timestamp> {
        self.valid_until
    }

    /// How well the edge itself is known (v0.4 §11.5).
    #[must_use]
    pub const fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// What the sources behind this relation composed to over the window (§8.5).
    #[must_use]
    pub const fn completeness(&self) -> TemporalCompleteness {
        self.completeness
    }

    /// Every source that spoke to this edge.
    pub fn sources(&self) -> impl Iterator<Item = &EvidenceSource> {
        self.sources.iter()
    }
}
