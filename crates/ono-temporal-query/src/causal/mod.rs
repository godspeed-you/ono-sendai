//! The causal engine (spec v0.5 §15, §16, §41): registered rules, the evidence each one
//! requires, and `why`.
//!
//! The rule that governs every line here is §2 invariant 7: temporal proximity alone MUST NEVER
//! create a `caused_by` relation.
//!
//! It is enforced in three places rather than asserted in one:
//!
//! - a rule returns a [`CausalFinding`], and the only constructor for one demands the registered
//!   rule, the §7.1 source and at least one [`ono_temporal_core::EvidenceId`];
//! - [`CausalEngine::links`] re-checks every finding against the rule's own registry row — the
//!   evidence must resolve, the strength must not exceed the weakest record behind it (§7.2), the
//!   source must be one the row admits, and both ends must be kinds the row declares;
//! - none of the ten built-in rules has a time-only join. The seven causal rules join on a
//!   published identity — a systemd job path, an `ActionId` propagated into a provider
//!   transaction, a kernel-reported parent, a cgroup membership, an explicit provider token — and
//!   the three correlation rules, which do use a window, can emit nothing but `correlated_with`.
//!
//! `docs/contracts/temporal/causality.yaml` is the contract, `tests/causality.rs` holds this
//! module against it in both directions, and [`BUILTIN_RULE_IDS`] is what `xtask spec-check`
//! reads so the gate compares the registry with the engine rather than with a copy of the
//! specification.

mod facts;
mod link;
mod registry;
mod rule;
mod rules;
mod why;

pub use facts::{
    CONNECTED_TO, CONTROLS_PROCESS, JOB_TYPE_FIELD, PARENT_OF, SYSTEMD_JOB_NEW,
    SYSTEMD_JOB_REMOVED, UNIT_STATE_FIELD, UNIT_STATE_FIELD_SHORT,
};
pub use link::{CausalFinding, LinkRefused};
pub use registry::{
    BUILTIN_CAUSAL_RULE_IDS, BUILTIN_CORRELATION_RULE_IDS, BUILTIN_RULE_IDS, CausalEngine,
};
pub use rule::{
    CausalContext, CausalRule, EventSet, EvidenceRequirements, RuleDescription, SourceConstraint,
};
pub use rules::{
    ActionLaunchedProcess, ActionToTransaction, ChangeBeforeFailure, ProcessParent,
    ProviderCausalToken, RemoteEndpointRetrySpike, ResourcePressureOverlap, ServiceControlsProcess,
    SystemdJobResult, SystemdJobToUnitState,
};
pub use why::{
    CausalExplanation, CausalNode, CausalStep, DEFAULT_DEPTH, DEFAULT_MAX_CANDIDATES,
    TemporalAssociation, WhyOptions, WhyRequest,
};
