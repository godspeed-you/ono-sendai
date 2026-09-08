//! The evidence model of v0.5 §7: what Ono observed, how strongly, and what a derived claim
//! rests on.
//!
//! §7.2 states the rule this module exists to make unbreakable: "evidence strength MUST NOT be
//! automatically upgraded by renderers, AI assistants or plugins". There is therefore no API
//! that raises a strength — no `promote`, no `max`, no `upgrade`, only
//! [`EvidenceStrength::weakest_of`].

use std::cmp::Ordering as CmpOrdering;
use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{SpatialId, SpatialScope};
use ono_value::{Provenance, Value};

use crate::digest::token;
use crate::id::EvidenceId;
use crate::ledger::TimeRange;
use crate::source::EvidenceSource;

/// How strongly a source supports a claim (§7.2).
///
/// Ordered strongest first: `authoritative > asserted > derived > correlated > observational`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidenceStrength {
    /// The source owns the fact for the queried scope — a unit state reported by systemd.
    Authoritative,
    /// A source explicitly reports the fact but is not its sole authority.
    Asserted,
    /// Ono computed the claim through a deterministic documented rule from stronger evidence.
    Derived,
    /// The evidence establishes association, never causation (§15.5).
    Correlated,
    /// A sample or a snapshot, with no exhaustive coverage guarantee.
    Observational,
}

impl EvidenceStrength {
    /// Every strength, strongest first, as §7.2 lists them.
    pub const ALL: &'static [EvidenceStrength] = &[
        EvidenceStrength::Authoritative,
        EvidenceStrength::Asserted,
        EvidenceStrength::Derived,
        EvidenceStrength::Correlated,
        EvidenceStrength::Observational,
    ];

    /// The name §7.2 and `ono.temporal-evidence/1` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            EvidenceStrength::Authoritative => "authoritative",
            EvidenceStrength::Asserted => "asserted",
            EvidenceStrength::Derived => "derived",
            EvidenceStrength::Correlated => "correlated",
            EvidenceStrength::Observational => "observational",
        }
    }

    /// The strength with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|strength| strength.as_str() == name)
    }

    /// The weaker of two strengths — the only way this crate combines them (§7.2).
    ///
    /// A chain is exactly as strong as its weakest link, and there is deliberately no operation
    /// that answers the other question.
    #[must_use]
    pub fn weakest_of(self, other: Self) -> Self {
        if self.rank() <= other.rank() {
            self
        } else {
            other
        }
    }

    /// Whether a negative claim may rest on this strength (§7.4).
    #[must_use]
    pub fn can_support_absence(self) -> bool {
        matches!(self, EvidenceStrength::Authoritative)
    }

    /// Where the strength sits in §7.2's order; higher is stronger.
    const fn rank(self) -> u8 {
        match self {
            EvidenceStrength::Observational => 0,
            EvidenceStrength::Correlated => 1,
            EvidenceStrength::Derived => 2,
            EvidenceStrength::Asserted => 3,
            EvidenceStrength::Authoritative => 4,
        }
    }
}

impl PartialOrd for EvidenceStrength {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for EvidenceStrength {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.rank().cmp(&other.rank())
    }
}

impl std::fmt::Display for EvidenceStrength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A provider-owned handle to material Ono did not copy (§3.4, §7.6).
///
/// §3.4: "raw log content or secret-bearing payloads MUST NOT be copied into evidence merely for
/// convenience". This is the reference that lets a user go and look instead — a journal cursor,
/// a segment name — under the permissions they already have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaqueReference {
    source: EvidenceSource,
    handle: Arc<str>,
}

impl OpaqueReference {
    /// A handle `source` understands and Ono does not interpret.
    #[must_use]
    pub fn new(source: EvidenceSource, handle: &str) -> Self {
        Self {
            source,
            handle: Arc::from(handle),
        }
    }

    /// The source that owns the material.
    #[must_use]
    pub fn source(&self) -> &EvidenceSource {
        &self.source
    }

    /// The handle, as the source spells it.
    #[must_use]
    pub fn handle(&self) -> &str {
        &self.handle
    }
}

/// What a piece of evidence actually claims (§3.4).
#[derive(Debug, Clone, PartialEq)]
pub enum EvidenceClaim {
    /// The object existed at an instant.
    ObjectExisted {
        /// When.
        at: Timestamp,
    },
    /// The object did not exist over an interval. §7.4 allows this only where the source had
    /// coverage capable of proving it.
    ObjectAbsent {
        /// Over which interval.
        over: TimeRange,
    },
    /// A field held a value at an instant.
    FieldValue {
        /// Which field.
        field: Arc<str>,
        /// What it held.
        value: Value,
        /// When.
        at: Timestamp,
    },
    /// A relationship held over an interval (§6.4).
    RelationHeld {
        /// Which relation.
        relation: Arc<str>,
        /// The other end.
        other: SpatialId,
        /// Over which interval.
        over: TimeRange,
    },
    /// A field went from one value to another at an instant.
    Transition {
        /// Which field.
        field: Arc<str>,
        /// What it was.
        from: Value,
        /// What it became.
        to: Value,
        /// When.
        at: Timestamp,
    },
    /// A transaction token the source exposes — a systemd job, an [`crate::ActionId`], a request
    /// id. §21.6 makes this the evidence a direct causal link is built on.
    Transaction {
        /// The token, as the source spells it.
        token: Arc<str>,
        /// When it was seen.
        at: Timestamp,
    },
    /// A structured statement a source made that maps onto no other claim (§6.5).
    SourceStatement {
        /// The parsed statement, never a raw log body (§3.4).
        text: Arc<str>,
    },
}

impl EvidenceClaim {
    /// The name `ono.temporal-evidence/1` spells for this kind of claim.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            EvidenceClaim::ObjectExisted { .. } => "object_existed",
            EvidenceClaim::ObjectAbsent { .. } => "object_absent",
            EvidenceClaim::FieldValue { .. } => "field_value",
            EvidenceClaim::RelationHeld { .. } => "relation_held",
            EvidenceClaim::Transition { .. } => "transition",
            EvidenceClaim::Transaction { .. } => "transaction",
            EvidenceClaim::SourceStatement { .. } => "source_statement",
        }
    }

    /// Whether this claim asserts an absence, which §7.4 guards.
    #[must_use]
    pub const fn is_negative(&self) -> bool {
        matches!(self, EvidenceClaim::ObjectAbsent { .. })
    }

    /// A stable text for the claim, so equal claims produce equal [`EvidenceId`]s.
    pub(crate) fn digest_token(&self) -> String {
        match self {
            EvidenceClaim::ObjectExisted { at } => format!("existed\u{1}{}", at.as_nanosecond()),
            EvidenceClaim::ObjectAbsent { over } => format!("absent\u{1}{}", over.digest_token()),
            EvidenceClaim::FieldValue { field, value, at } => format!(
                "field\u{1}{field}\u{1}{}\u{1}{}",
                token(value),
                at.as_nanosecond()
            ),
            EvidenceClaim::RelationHeld {
                relation,
                other,
                over,
            } => format!(
                "relation\u{1}{relation}\u{1}{}\u{1}{}",
                other.as_str(),
                over.digest_token()
            ),
            EvidenceClaim::Transition {
                field,
                from,
                to,
                at,
            } => format!(
                "transition\u{1}{field}\u{1}{}\u{1}{}\u{1}{}",
                token(from),
                token(to),
                at.as_nanosecond()
            ),
            EvidenceClaim::Transaction { token, at } => {
                format!("transaction\u{1}{token}\u{1}{}", at.as_nanosecond())
            }
            EvidenceClaim::SourceStatement { text } => format!("statement\u{1}{text}"),
        }
    }
}

/// Why Ono believes a historical claim (§3.4).
#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    /// The identity a chain and an event's evidence list refer to.
    pub evidence_id: EvidenceId,
    /// The §7.1 source class that made the observation.
    pub source: EvidenceSource,
    /// When the observing component made it.
    pub observed_at: Timestamp,
    /// When the source says the fact held. `None` where it gave no time of its own.
    pub source_time: Option<Timestamp>,
    /// The v0.4 boundary the claim is about.
    pub scope: SpatialScope,
    /// What the claim is about. `None` for evidence about no single object.
    pub subject: Option<SpatialId>,
    /// The claim itself.
    pub claim: EvidenceClaim,
    /// How strongly the source supports it (§7.2).
    pub strength: EvidenceStrength,
    /// A provider-owned handle to the raw material (§3.4, §7.6).
    pub raw_ref: Option<OpaqueReference>,
    /// The evidence this claim was derived from (§7.3).
    pub derived_from: Vec<EvidenceId>,
    /// Where the record came from (v0.2 §25.2).
    pub provenance: Provenance,
}

impl Evidence {
    /// Whether the claim carries the chain §7.3 requires of a derived claim.
    ///
    /// "A derived claim MUST reference the evidence it derives from." A `derived` record with an
    /// empty chain is a claim with nothing to walk, and this is how a caller catches one.
    #[must_use]
    pub fn has_required_chain(&self) -> bool {
        self.strength != EvidenceStrength::Derived || !self.derived_from.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_report_a_missing_chain_when_a_derived_claim_cites_nothing() {
        assert!(EvidenceStrength::Authoritative.can_support_absence());
        assert!(!EvidenceStrength::Observational.can_support_absence());
    }

    #[test]
    fn should_produce_different_tokens_when_two_claims_differ() {
        let existed = EvidenceClaim::ObjectExisted {
            at: Timestamp::UNIX_EPOCH,
        };
        let absent = EvidenceClaim::ObjectAbsent {
            over: TimeRange::all(),
        };
        assert_ne!(existed.digest_token(), absent.digest_token());
        assert!(absent.is_negative());
        assert!(!existed.is_negative());
    }
}
