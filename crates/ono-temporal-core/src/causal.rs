//! The causal model of v0.5 §15.
//!
//! §15.2: "temporal proximity is insufficient". A link therefore names the registered rule that
//! emitted it (§15.8) and the evidence the rule matched on, and [`CausalRelation::is_causal`] is
//! a structural fact a renderer keys on rather than a judgement it makes — §15.6 forbids
//! rendering `preceded_by` with causal language, and §15.5 requires correlation to stay visually
//! and structurally distinct from causation.

use std::sync::Arc;

use crate::evidence::EvidenceStrength;
use crate::id::{CausalLinkId, EventId, EvidenceId};
use crate::source::EvidenceSource;

/// One of the five canonical relationship classes (§15.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CausalRelation {
    /// Evidence supports a direct causal statement under a registered rule (§15.2).
    CausedBy,
    /// A initiated a mechanism that reached B through intermediate steps (§15.3).
    TriggeredBy,
    /// The forward-facing projection of a known effect chain (§15.4).
    ResultedIn,
    /// A registered correlation rule found association without causal evidence (§15.5).
    CorrelatedWith,
    /// Order according to the supported ordering model, and nothing more (§15.6).
    PrecededBy,
}

impl CausalRelation {
    /// Every class, in the order §15.1 lists them.
    pub const ALL: &'static [CausalRelation] = &[
        CausalRelation::CausedBy,
        CausalRelation::TriggeredBy,
        CausalRelation::ResultedIn,
        CausalRelation::CorrelatedWith,
        CausalRelation::PrecededBy,
    ];

    /// The name §15.1 and `ono.causal-link/1` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CausalRelation::CausedBy => "caused_by",
            CausalRelation::TriggeredBy => "triggered_by",
            CausalRelation::ResultedIn => "resulted_in",
            CausalRelation::CorrelatedWith => "correlated_with",
            CausalRelation::PrecededBy => "preceded_by",
        }
    }

    /// The class with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|relation| relation.as_str() == name)
    }

    /// The same link read from the cause's side, in §15.1's own words.
    #[must_use]
    pub const fn inverse_label(self) -> &'static str {
        match self {
            CausalRelation::CausedBy => "caused",
            CausalRelation::TriggeredBy => "triggered",
            CausalRelation::ResultedIn => "result",
            CausalRelation::CorrelatedWith => "correlated_with",
            CausalRelation::PrecededBy => "followed_by",
        }
    }

    /// Whether the class asserts causation.
    ///
    /// A renderer keys its edge style and its wording on this: §15.5 keeps correlation distinct
    /// from causation, and §15.6 forbids "because", "therefore", "led to" or "caused" for
    /// `preceded_by`.
    #[must_use]
    pub const fn is_causal(self) -> bool {
        matches!(
            self,
            CausalRelation::CausedBy | CausalRelation::TriggeredBy | CausalRelation::ResultedIn
        )
    }
}

impl std::fmt::Display for CausalRelation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The registered rule that emitted a link (§15.8).
///
/// "No renderer may create causal language outside this registry", so every link names the rule
/// it came from and a reader can go and read what that rule requires.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CausalRuleId(Arc<str>);

impl CausalRuleId {
    /// A rule id — `ono.action-to-job` for a built-in, publisher-namespaced for a plugin rule.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(Arc::from(id))
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the rule belongs to Ono itself rather than to a package (v0.2 §31.5).
    #[must_use]
    pub fn is_builtin(&self) -> bool {
        self.0.starts_with("ono.")
    }
}

impl std::fmt::Display for CausalRuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A typed relationship between two events (§3.8, §15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalLink {
    /// The identity an event's `causal_parents` refer to.
    pub link_id: CausalLinkId,
    /// The class, read from the effect's side (§15.1).
    pub relation: CausalRelation,
    /// The event at the cause end.
    pub cause: EventId,
    /// The event at the effect end.
    pub effect: EventId,
    /// The registered rule that emitted the link (§15.8).
    pub rule: CausalRuleId,
    /// The evidence the rule matched on.
    pub evidence: Vec<EvidenceId>,
    /// The weakest strength in the chain (§7.2).
    pub strength: EvidenceStrength,
    /// The §7.1 source that produced the link.
    pub source: EvidenceSource,
}

impl CausalLink {
    /// Whether a renderer may use causal language for this link (§15.6).
    #[must_use]
    pub fn is_causal(&self) -> bool {
        self.relation.is_causal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_keep_a_plugin_rule_apart_from_a_built_in_one_when_a_link_is_inspected() {
        assert!(CausalRuleId::new("ono.action-to-job").is_builtin());
        assert!(!CausalRuleId::new("dev.example.packet-eye.retry").is_builtin());
    }
}
