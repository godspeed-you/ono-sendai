//! Impact derivation and the risk rules of Ono-Sendai v0.6 (spec §9, §19, Appendix A.1).
//!
//! Three questions are answered here, and they are deliberately separate ones:
//!
//! - **What could this plan touch?** [`derive()`] walks the v0.4 topology out from a plan's frozen
//!   targets and answers with an [`ImpactGraph`]. §9.1 is careful about the question — *"What
//!   known parts of the system could this plan touch directly or indirectly?"* — and it is not
//!   failure prediction. Nothing here says anything will break.
//! - **How dangerous is it?** [`assess`] runs a registry of named rules over the plan and answers
//!   with a [`RiskAssessment`]. §19.2 says the classes are rule-based and not AI-generated, and
//!   §62.11 repeats it, so every finding names the rule that made it and the reason §40.2 prints
//!   in place of "Are you sure?".
//! - **What state does it change?** [`mutation_domains`] derives Appendix A.1's records, which is
//!   what the protection engine takes as the input to its coverage algorithm.
//!
//! Two rules run through all three:
//!
//! - **Provenance is retained unchanged (§3.5).** Every [`ImpactNode`] carries the relation that
//!   reached it, the v0.4 [`Confidence`] of that edge spelled as v0.4 spells it, and the edge id
//!   as evidence. There is no path through this crate by which an `inferred` edge becomes
//!   anything stronger — [`ImpactRequest::with_history`] is the one place v0.5 evidence enters,
//!   and it may raise a node's *display prominence* and nothing else (§9.4).
//! - **A bound is visible (§9.5, §52.2).** Traversal is bounded in depth and in nodes, and a
//!   graph cut short by a budget says so through [`truncation`]. A traversal whose coarse
//!   estimate is beyond the interactive budget is refused rather than started
//!   ([`derive_within_budget`]), exactly as `map` refuses one.
//!
//! The crate performs no I/O. It reads a [`SpatialIndex`] someone else filled and the actions of
//! a plan someone else built, which is what keeps §2.1's side-effect-free planning true by
//! construction.
//!
//! [`Confidence`]: ono_spatial_core::Confidence
//! [`ImpactGraph`]: ono_change_core::ImpactGraph
//! [`ImpactNode`]: ono_change_core::ImpactNode
//! [`RiskAssessment`]: ono_change_core::RiskAssessment
//! [`truncation`]: ono_change_core::ImpactGraph::truncation
//! [`SpatialIndex`]: ono_spatial_index::SpatialIndex

#![forbid(unsafe_code)]

pub mod derive;
pub mod risk;

pub use derive::{
    DEFAULT_DEPTH, DEFAULT_NODE_BUDGET, HistoricalRelevance, ImpactDerivation, ImpactRequest,
    Prominence, derive, derive_in_detail, derive_within_budget, estimate,
};
pub use risk::{
    ActiveLink, BulkThresholds, REBOOT_REQUIRED, REBOOT_SUGGESTED, RiskRequest, RiskRuleSpec,
    assess, rule, rules,
};
