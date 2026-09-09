//! The one door a causal link comes through (spec v0.5 §15.2, §15.8, §48.5 scenario 27).
//!
//! §15.2 closes with the sentence this module makes structural: "Temporal proximity is
//! insufficient." A rule therefore never returns a [`CausalLink`] directly. It returns a
//! [`CausalFinding`], whose only constructor demands the registered rule that emitted it, the
//! §7.1 source that produced it and at least one [`EvidenceId`]. A link asserting causation with
//! nothing behind it is unconstructable on this path, which is the guarantee §55.3 exists to
//! protect.

use ono_temporal_core::{
    CausalLink, CausalLinkId, CausalRelation, CausalRuleId, EventId, EvidenceId, EvidenceSource,
    EvidenceStrength,
};

/// Why a rule's proposed link was refused before it became one (§15.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkRefused {
    /// The rule cited no evidence, so the only thing joining the two events is time.
    NoEvidence,
    /// Cause and effect are the same event, which explains nothing.
    SelfLink,
}

impl LinkRefused {
    /// The reason, in the words a diagnostic uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LinkRefused::NoEvidence => "no evidence",
            LinkRefused::SelfLink => "cause and effect are one event",
        }
    }
}

impl std::fmt::Display for LinkRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A link a registered rule found, carrying everything §15.8 requires of one.
///
/// The inner [`CausalLink`] is private and reachable only through [`CausalFinding::emit`], so a
/// rule cannot hand the engine a causal claim it did not join on published evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalFinding {
    link: CausalLink,
}

impl CausalFinding {
    /// The link, if the rule supplied what §15.8 requires.
    ///
    /// The evidence list is sorted and deduplicated, so one rule firing on one pair of events
    /// produces one link whatever order it visited the evidence in — §41's determinism
    /// requirement, enforced where the link is made rather than checked afterwards.
    ///
    /// # Errors
    ///
    /// [`LinkRefused::NoEvidence`] when `evidence` is empty, and [`LinkRefused::SelfLink`] when
    /// `cause` and `effect` name one event.
    pub fn emit(
        rule: &CausalRuleId,
        relation: CausalRelation,
        cause: &EventId,
        effect: &EventId,
        evidence: Vec<EvidenceId>,
        strength: EvidenceStrength,
        source: EvidenceSource,
    ) -> Result<Self, LinkRefused> {
        if evidence.is_empty() {
            return Err(LinkRefused::NoEvidence);
        }
        if cause == effect {
            return Err(LinkRefused::SelfLink);
        }
        let mut evidence = evidence;
        evidence.sort();
        evidence.dedup();
        Ok(Self {
            link: CausalLink {
                link_id: CausalLinkId::of(rule, relation, cause, effect),
                relation,
                cause: cause.clone(),
                effect: effect.clone(),
                rule: rule.clone(),
                evidence,
                strength,
                source,
            },
        })
    }

    /// The link, for inspection.
    #[must_use]
    pub fn link(&self) -> &CausalLink {
        &self.link
    }

    /// The link, for a caller that owns the finding.
    #[must_use]
    pub fn into_link(self) -> CausalLink {
        self.link
    }
}
