//! The canonical prospective-change vocabulary of Ono-Sendai (spec v0.6 §3, §50).
//!
//! §50 splits v0.6 across a family of crates and gives this one the words the others speak in:
//! the plan, its actions, the effects they propose, the impact they reach, the protection that
//! covers them, the risk they carry, the assets a way back rests on and the contracts that decide
//! whether any of it worked. Everything here is a value or a rule over values.
//!
//! Two prohibitions shape the crate, and both come straight out of §50.1:
//!
//! - **Core types do not call providers.** [`RecoveryProvider`] and [`ChangeProvider`] are
//!   declared here and implemented elsewhere, so a provider compiles against the contract and the
//!   contract compiles against nothing.
//! - **Nothing here performs I/O.** §2.1 makes planning side-effect free, and a planner that
//!   cannot open a file cannot break that rule by accident. A [`RecoveryAsset`] a plan mentions is
//!   [`AssetState::Proposed`] until an executor creates it.
//!
//! # The invariants the types enforce
//!
//! Several of §2's invariants are properties of these types rather than rules a reviewer has to
//! remember, and those are the ones worth knowing before reading further:
//!
//! - **A protection level never overstates coverage.** [`ProtectionSummary::level`] computes §10.2's
//!   word from the per-domain matrix beneath it (Appendix A.5, A.7). There is no setter.
//! - **A sealed plan is immutable.** [`ChangePlan::seal`] consumes the plan; every mutator is on
//!   the draft, and [`ChangePlan::revise`] leaves the original in the caller's hands (§4.4, §7.5).
//! - **A plan that mutates carries verification.** [`ChangePlan::seal`] refuses otherwise (§23.1).
//! - **Unknown is never promoted.** [`EffectConfidence::weakest_of`] and
//!   [`ConsistencyClass::weakest_of`] have no counterpart that strengthens (§1.3, §2.4).
//! - **Recovery is gated on its own destructiveness.** [`RecoveryPlan::needs_destructive_acceptance`]
//!   answers `true` for an unanalysed recovery, so §62.8's "recovery without drift analysis" is
//!   not reachable by forgetting to look (§24.5).
//! - **A provider runs a program, never a command line.** [`Execution`] has no variant that holds
//!   one, and [`ToolRunner`] takes an argument vector (§2.17, §12.3, §43.6).
//!
//! # Where a change value becomes an Ono value
//!
//! [`value`] is the single bridge from these types to the schemas of §46, and [`error`] the single
//! source of the refusals of §45. No other crate spells a v0.6 field name or an error code by
//! hand.

#![forbid(unsafe_code)]

mod action;
mod asset;
mod digest;
mod domain;
mod effect;
mod id;
mod impact;
mod plan;
mod protection;
mod provider;
mod recovery;
mod risk;
mod state;
mod strategy;
mod target;
mod tool;
mod verification;
mod vocab;

pub mod error;
pub mod value;

pub use action::{ActionRole, ActionStatus, Execution, Idempotency, PlanAction, topological_order};
pub use asset::{
    AssetState, DEFAULT_RETENTION, RecoveryAsset, RecoveryAssetType, RecoveryCost,
    RecoveryExclusion, RecoveryScope, RecoveryValidation, RestoreMethod, RetentionPolicy,
};
pub use digest::{DigestBuilder, SECTION, UNIT, value_text};
pub use domain::{FilesystemKind, NonPersistentReason, PersistenceDomain, ResolvedMount};
pub use effect::{EffectConfidence, EffectDomain, EffectKind, ProposedEffect};
pub use id::{
    ActionId, CheckId, EffectId, PlanId, RecoveryAssetId, SHORT, shortest_unique_prefixes,
};
pub use impact::{BlastRadius, ImpactClass, ImpactGraph, ImpactNode, UnknownBoundary};
pub use plan::{ChangePlan, Intent, PlanKind, ProviderBinding};
pub use protection::{
    ConsistencyClass, CoverageExclusion, DomainCoverage, DomainProtection, ProtectionLevel,
    ProtectionMode, ProtectionSummary, RecoveryObjective,
};
pub use provider::{
    ChangeCapability, ChangeProvider, PlanFragment, ProtectionAction, ProviderAvailability,
    ProviderCapabilities, RECOVERY_PROVIDER_CONFORMANCE, RecoveryCandidate, RecoveryCapability,
    RecoveryPlanFragment, RecoveryProvider, Support,
};
pub use recovery::{
    DirectoryRestorePolicy, EquivalenceState, MetadataCoverage, NewerStateClass, NewerStateImpact,
    NewerStateItem, RecoveryGoal, RecoveryOutcome, RecoveryPlan, UnrecoverableEffect,
    choose_method,
};
pub use risk::{RequiredAcknowledgement, RiskAssessment, RiskClass, RiskDimension, RiskFinding};
pub use state::{LifecycleEvent, PlanState};
pub use strategy::{Strategy, StrategyKind, Wave};
pub use target::{DriftFinding, DriftVerdict, FrozenTarget, Precondition, PreconditionKind};
pub use tool::{ScriptedRunner, ToolOutput, ToolRunner};
pub use verification::{
    DEFAULT_TIMEOUT, EquivalenceDomain, Verdict, VerificationClass, VerificationContract,
    VerificationResult, VerificationSet, VerificationStatus, duration_value,
};
