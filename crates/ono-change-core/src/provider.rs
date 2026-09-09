//! The provider contracts of v0.6 (spec §12.1, §12.2, §48, §51).
//!
//! Two traits, and the reason they are two: §50.1 says recovery providers do not own plan
//! orchestration. A [`RecoveryProvider`] answers questions about protecting and restoring one
//! mechanism's state; a [`ChangeProvider`] answers questions about what an intent means and how to
//! carry it out. Neither of them decides what a plan is, and §51 forbids both from mutating
//! anything during `supports` or `resolve`.
//!
//! §12.2 fixes the capability names, and §48.3 and §48.4 make them the boundary a KUANG/11 plugin
//! is held to: a plugin that describes impact does not thereby gain permission to execute the
//! change. [`ProviderCapabilities`] is the declaration, and it is checked at load rather than at
//! the first call, exactly as the existing `ActionContribution` validation is (ADR-0594).

use std::sync::Arc;

use ono_value::ErrorValue;

use crate::action::PlanAction;
use crate::asset::{RecoveryAsset, RecoveryCost, RecoveryExclusion, RecoveryScope, RestoreMethod};
use crate::domain::PersistenceDomain;
use crate::effect::EffectDomain;
use crate::plan::{ChangePlan, Intent};
use crate::protection::{ConsistencyClass, ProtectionMode, RecoveryObjective};
use crate::recovery::{
    DirectoryRestorePolicy, MetadataCoverage, NewerStateImpact, UnrecoverableEffect,
};
use crate::target::{FrozenTarget, Precondition};
use crate::verification::{VerificationContract, VerificationResult};
use crate::vocab::vocabulary;

vocabulary! {
    /// The capabilities a recovery provider declares (§12.2) and a plugin is granted (§48.3).
    RecoveryCapability {
        Discover => "recovery.discover", "§12.2: find candidate protection for a target. Read-only.";
        Prepare => "recovery.prepare", "§12.2: create a recovery asset. This mutates the storage or control plane (§5.5).";
        Restore => "recovery.restore", "§12.2: use an asset to put state back. The most consequential capability v0.6 defines.";
        Cleanup => "recovery.cleanup", "§12.2: remove an asset (§37).";
        EstimateCost => "recovery.estimate-cost", "§12.2: report what an asset costs to create and keep (§38).";
        Quiesce => "recovery.quiesce", "§12.2, optional: pause an application so a snapshot is application-consistent (§39.3).";
        Transaction => "recovery.transaction", "§12.2, optional: begin, prepare, commit and roll back inside the provider's own boundary (§27.1).";
    }
}

impl RecoveryCapability {
    /// The capabilities §12.2 requires of every recovery provider.
    pub const REQUIRED: &'static [RecoveryCapability] = &[
        RecoveryCapability::Discover,
        RecoveryCapability::Prepare,
        RecoveryCapability::Restore,
        RecoveryCapability::Cleanup,
        RecoveryCapability::EstimateCost,
    ];

    /// Whether exercising this capability changes the system (§5.5, §43.2).
    #[must_use]
    pub const fn mutates(self) -> bool {
        matches!(
            self,
            RecoveryCapability::Prepare
                | RecoveryCapability::Restore
                | RecoveryCapability::Cleanup
                | RecoveryCapability::Quiesce
                | RecoveryCapability::Transaction
        )
    }
}

vocabulary! {
    /// The capabilities a change provider or plugin declares (§48.3).
    ChangeCapability {
        PlanRead => "change.plan.read", "§48.3: read plans and their impact. Read-only, and §48.4 makes it no route to execution.";
        PlanContribute => "change.plan.contribute", "§48.3: contribute actions, impact or risk findings to a plan. Still not permission to run anything.";
        ActionExecute => "change.action.execute", "§48.3: carry out a mutating plan action.";
        VerificationObserve => "verification.observe", "§48.3: observe a verification contract and report the result.";
    }
}

impl ChangeCapability {
    /// Whether exercising this capability changes the system (§48.4).
    #[must_use]
    pub const fn mutates(self) -> bool {
        matches!(self, ChangeCapability::ActionExecute)
    }
}

/// What one provider declares it can do (§12.2, §48.3, Appendix G.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCapabilities {
    provider: Arc<str>,
    conformance: Arc<str>,
    recovery: Vec<RecoveryCapability>,
    change: Vec<ChangeCapability>,
    tool_versions: Vec<(Arc<str>, Arc<str>)>,
}

/// The conformance version of §12 and Appendix G.5.
pub const RECOVERY_PROVIDER_CONFORMANCE: &str = "ono.recovery-provider/1";

impl ProviderCapabilities {
    /// Declares `provider` at the current conformance version.
    #[must_use]
    pub fn new(provider: impl Into<Arc<str>>) -> Self {
        Self {
            provider: provider.into(),
            conformance: Arc::from(RECOVERY_PROVIDER_CONFORMANCE),
            recovery: Vec::new(),
            change: Vec::new(),
            tool_versions: Vec::new(),
        }
    }

    /// Declares a recovery capability.
    #[must_use]
    pub fn recovering(mut self, capability: RecoveryCapability) -> Self {
        if !self.recovery.contains(&capability) {
            self.recovery.push(capability);
        }
        self
    }

    /// Declares a change capability.
    #[must_use]
    pub fn changing(mut self, capability: ChangeCapability) -> Self {
        if !self.change.contains(&capability) {
            self.change.push(capability);
        }
        self
    }

    /// Records the version of an external tool the provider validated against (Appendix G.4).
    #[must_use]
    pub fn tested_against(
        mut self,
        tool: impl Into<Arc<str>>,
        version: impl Into<Arc<str>>,
    ) -> Self {
        self.tool_versions.push((tool.into(), version.into()));
        self
    }

    /// The provider's id.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The conformance version it advertises (Appendix G.5).
    #[must_use]
    pub fn conformance(&self) -> &str {
        &self.conformance
    }

    /// The recovery capabilities.
    #[must_use]
    pub fn recovery(&self) -> &[RecoveryCapability] {
        &self.recovery
    }

    /// The change capabilities.
    #[must_use]
    pub fn change(&self) -> &[ChangeCapability] {
        &self.change
    }

    /// The tool versions the provider validated against (Appendix G.4).
    #[must_use]
    pub fn tool_versions(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.tool_versions
    }

    /// Whether the provider declared `capability`.
    #[must_use]
    pub fn has_recovery(&self, capability: RecoveryCapability) -> bool {
        self.recovery.contains(&capability)
    }

    /// Whether the provider declared `capability`.
    #[must_use]
    pub fn has_change(&self, capability: ChangeCapability) -> bool {
        self.change.contains(&capability)
    }

    /// The §12.2 capabilities a recovery provider declares and this one does not.
    ///
    /// A provider that cannot restore is not a recovery provider, however good its discovery is:
    /// a candidate nobody can use is snapshot theatre (§62.1).
    #[must_use]
    pub fn missing_required(&self) -> Vec<RecoveryCapability> {
        RecoveryCapability::REQUIRED
            .iter()
            .copied()
            .filter(|capability| !self.has_recovery(*capability))
            .collect()
    }

    /// Whether a contributor with these capabilities may execute a mutating action (§48.4).
    #[must_use]
    pub fn may_execute(&self) -> bool {
        self.has_change(ChangeCapability::ActionExecute)
    }
}

/// A protection opportunity a provider found, before anything has been created (Appendix A.3).
///
/// §2.1 keeps a candidate a description: it names what would be made, what it would cost and what
/// it would leave out, and creating it is a PREPARE action the plan shows first.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryCandidate {
    provider: Arc<str>,
    scope: RecoveryScope,
    domain: EffectDomain,
    objective: RecoveryObjective,
    consistency: ConsistencyClass,
    restore_method: RestoreMethod,
    cost: RecoveryCost,
    exclusions: Vec<RecoveryExclusion>,
    creation_requirements: Vec<Arc<str>>,
    restore_requirements: Vec<Arc<str>>,
    detail: Arc<str>,
}

impl RecoveryCandidate {
    /// Declares that `provider` could protect `scope` for `objective` in `domain`.
    #[must_use]
    pub fn new(
        provider: impl Into<Arc<str>>,
        scope: RecoveryScope,
        domain: EffectDomain,
        objective: RecoveryObjective,
        detail: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            provider: provider.into(),
            scope,
            domain,
            objective,
            consistency: ConsistencyClass::Unknown,
            restore_method: RestoreMethod::SelectiveFileRestore,
            cost: RecoveryCost::unknown(),
            exclusions: Vec::new(),
            creation_requirements: Vec::new(),
            restore_requirements: Vec::new(),
            detail: detail.into(),
        }
    }

    /// States the consistency the mechanism would achieve (§11.3).
    #[must_use]
    pub const fn at_consistency(mut self, consistency: ConsistencyClass) -> Self {
        self.consistency = consistency;
        self
    }

    /// States how the resulting asset would be used to restore (Appendix C.1).
    #[must_use]
    pub const fn restored_by(mut self, method: RestoreMethod) -> Self {
        self.restore_method = method;
        self
    }

    /// States what creating it costs (§38).
    #[must_use]
    pub fn costing(mut self, cost: RecoveryCost) -> Self {
        self.cost = cost;
        self
    }

    /// States something it would not cover.
    #[must_use]
    pub fn excluding(mut self, exclusion: RecoveryExclusion) -> Self {
        self.exclusions.push(exclusion);
        self
    }

    /// States what creating it needs — a capability, free space, a quiesce window.
    #[must_use]
    pub fn needing_to_create(mut self, requirement: impl Into<Arc<str>>) -> Self {
        self.creation_requirements.push(requirement.into());
        self
    }

    /// States what restoring from it needs — a reboot, an unmount, a stronger privilege (§43.4).
    #[must_use]
    pub fn needing_to_restore(mut self, requirement: impl Into<Arc<str>>) -> Self {
        self.restore_requirements.push(requirement.into());
        self
    }

    /// The provider that offered it.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// What it would protect.
    #[must_use]
    pub const fn scope(&self) -> &RecoveryScope {
        &self.scope
    }

    /// The domain it covers.
    #[must_use]
    pub const fn domain(&self) -> EffectDomain {
        self.domain
    }

    /// The objective it would satisfy.
    #[must_use]
    pub const fn objective(&self) -> RecoveryObjective {
        self.objective
    }

    /// The consistency it would achieve.
    #[must_use]
    pub const fn consistency(&self) -> ConsistencyClass {
        self.consistency
    }

    /// How it would be restored.
    #[must_use]
    pub const fn restore_method(&self) -> RestoreMethod {
        self.restore_method
    }

    /// What it costs.
    #[must_use]
    pub const fn cost(&self) -> &RecoveryCost {
        &self.cost
    }

    /// What it would not cover.
    #[must_use]
    pub fn exclusions(&self) -> &[RecoveryExclusion] {
        &self.exclusions
    }

    /// What creating it needs.
    #[must_use]
    pub fn creation_requirements(&self) -> &[Arc<str>] {
        &self.creation_requirements
    }

    /// What restoring from it needs.
    #[must_use]
    pub fn restore_requirements(&self) -> &[Arc<str>] {
        &self.restore_requirements
    }

    /// The sentence the plan view shows.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// How many objects the scope holds, which is Appendix A.4's second preference key.
    #[must_use]
    pub fn scope_width(&self) -> usize {
        self.scope.covers().len()
    }
}

/// One protection step a provider proposes, which becomes a PREPARE action (§3.3, §12.1).
#[derive(Debug, Clone, PartialEq)]
pub struct ProtectionAction {
    provider: Arc<str>,
    summary: Arc<str>,
    candidate: RecoveryCandidate,
    proposed_asset: RecoveryAsset,
    required: bool,
}

impl ProtectionAction {
    /// Proposes creating `proposed_asset` from `candidate`.
    #[must_use]
    pub fn new(
        provider: impl Into<Arc<str>>,
        summary: impl Into<Arc<str>>,
        candidate: RecoveryCandidate,
        proposed_asset: RecoveryAsset,
    ) -> Self {
        Self {
            provider: provider.into(),
            summary: summary.into(),
            candidate,
            proposed_asset,
            required: true,
        }
    }

    /// Marks the action as one whose failure does not stop the apply (§17.2's `maximize`).
    ///
    /// Under `prefer` and `require` a planned protection action is required, and §2.3 aborts
    /// before mutation when it fails. `maximize` may add extra coverage on top, and a failure
    /// there degrades the coverage matrix instead of the plan.
    #[must_use]
    pub const fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    /// The provider that will carry it out.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The line the plan view shows.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The candidate it came from.
    #[must_use]
    pub const fn candidate(&self) -> &RecoveryCandidate {
        &self.candidate
    }

    /// The asset it would produce, still `PROPOSED`.
    #[must_use]
    pub const fn proposed_asset(&self) -> &RecoveryAsset {
        &self.proposed_asset
    }

    /// Whether a failure here must abort before mutation (§2.3).
    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.required
    }
}

/// A fragment of a recovery plan, contributed by one provider (§12.1).
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryPlanFragment {
    provider: Arc<str>,
    method: RestoreMethod,
    actions: Vec<PlanAction>,
    newer_state: NewerStateImpact,
    unrecoverable: Vec<UnrecoverableEffect>,
    verification: Vec<VerificationContract>,
    metadata: MetadataCoverage,
    directory_policy: DirectoryRestorePolicy,
    requires_reboot: bool,
    requires_offline: bool,
}

impl RecoveryPlanFragment {
    /// Declares that `provider` would recover by `method`.
    #[must_use]
    pub fn new(provider: impl Into<Arc<str>>, method: RestoreMethod) -> Self {
        Self {
            provider: provider.into(),
            method,
            actions: Vec::new(),
            newer_state: NewerStateImpact::unanalysed(),
            unrecoverable: Vec::new(),
            verification: Vec::new(),
            metadata: MetadataCoverage::none(),
            directory_policy: DirectoryRestorePolicy::KeepExtraFiles,
            requires_reboot: false,
            requires_offline: false,
        }
    }

    /// Adds an action the recovery would take.
    #[must_use]
    pub fn acting(mut self, action: PlanAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Records what the method would do to newer state (Appendix C.3).
    #[must_use]
    pub fn with_newer_state(mut self, impact: NewerStateImpact) -> Self {
        self.newer_state = impact;
        self
    }

    /// Records something the recovery cannot reverse.
    #[must_use]
    pub fn leaving(mut self, effect: UnrecoverableEffect) -> Self {
        self.unrecoverable.push(effect);
        self
    }

    /// Adds a verification contract for the recovery (§25).
    #[must_use]
    pub fn verifying(mut self, contract: VerificationContract) -> Self {
        self.verification.push(contract);
        self
    }

    /// Records which file metadata this provider's restore actually puts back (Appendix C.7).
    ///
    /// Appendix C.7 requires missing metadata support to be visible, and the provider is the only
    /// thing that knows: whether ACLs can be restored depends on the filesystem, and whether
    /// ownership can depends on the privilege the process holds. A fragment that could not carry
    /// it would leave the recovery plan asserting a coverage nobody measured.
    #[must_use]
    pub const fn restoring_metadata(mut self, metadata: MetadataCoverage) -> Self {
        self.metadata = metadata;
        self
    }

    /// Records what this provider's restore does with files the asset never held (Appendix C.6).
    ///
    /// The default keeps them, and Appendix C.6 is explicit that deleting newer extra files is a
    /// choice the objective has to require rather than a default a directory restore falls into.
    /// The provider states it because the provider is what will do it.
    #[must_use]
    pub const fn restoring_directories(mut self, policy: DirectoryRestorePolicy) -> Self {
        self.directory_policy = policy;
        self
    }

    /// Records that the recovery needs a reboot (§13.7, §14.6).
    #[must_use]
    pub const fn needing_reboot(mut self) -> Self {
        self.requires_reboot = true;
        self
    }

    /// Records that the recovery needs the filesystem offline.
    #[must_use]
    pub const fn needing_offline(mut self) -> Self {
        self.requires_offline = true;
        self
    }

    /// Which metadata this provider's restore returns (Appendix C.7).
    #[must_use]
    pub const fn metadata(&self) -> MetadataCoverage {
        self.metadata
    }

    /// What this provider's restore does with files the asset never held (Appendix C.6).
    #[must_use]
    pub const fn directory_policy(&self) -> DirectoryRestorePolicy {
        self.directory_policy
    }

    /// The provider that contributed it.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The method.
    #[must_use]
    pub const fn method(&self) -> RestoreMethod {
        self.method
    }

    /// The actions.
    #[must_use]
    pub fn actions(&self) -> &[PlanAction] {
        &self.actions
    }

    /// What it would do to newer state.
    #[must_use]
    pub const fn newer_state(&self) -> &NewerStateImpact {
        &self.newer_state
    }

    /// What it cannot reverse.
    #[must_use]
    pub fn unrecoverable(&self) -> &[UnrecoverableEffect] {
        &self.unrecoverable
    }

    /// Its verification contracts.
    #[must_use]
    pub fn verification(&self) -> &[VerificationContract] {
        &self.verification
    }

    /// Whether it needs a reboot.
    #[must_use]
    pub const fn requires_reboot(&self) -> bool {
        self.requires_reboot
    }

    /// Whether it needs the filesystem offline.
    #[must_use]
    pub const fn requires_offline(&self) -> bool {
        self.requires_offline
    }
}

/// Discovers, creates, validates, restores and cleans up assets for one mechanism (§12.1).
///
/// The methods correspond to §12.1's conceptual trait. Two departures from its sketch are
/// deliberate and recorded in an ADR: `plan_protection` takes the resolved [`PersistenceDomain`]
/// rather than a bare object reference, because §11.2 requires a path to be mapped before
/// protection is claimed and a provider that does the mapping itself will eventually do it
/// differently from the one next to it; and `plan_recovery` takes the plan being recovered from,
/// because Appendix C.2 makes the recovery goal a property of what the original plan changed.
pub trait RecoveryProvider: Send + Sync + std::fmt::Debug {
    /// The provider's id, such as `ono.recovery.zfs`.
    fn id(&self) -> &str;

    /// What the provider declares it can do (§12.2).
    fn capabilities(&self) -> ProviderCapabilities;

    /// Whether the provider can run here at all — its tool present, its module loaded.
    ///
    /// A provider that is unavailable says so rather than answering `discover` with nothing:
    /// §55.6 case 29 requires unknown recovery semantics to stay unknown, and an empty candidate
    /// list is indistinguishable from "there is nothing to protect".
    fn availability(&self) -> ProviderAvailability;

    /// Maps `path` to the persistence object that actually holds its state (Appendix B).
    ///
    /// Returns `Ok(None)` when the path is not this provider's business at all.
    ///
    /// # Errors
    ///
    /// A structured error when the mapping could not be established, which §56.3 turns into a
    /// refusal rather than an assumption.
    fn resolve_domain(&self, path: &str) -> Result<Option<PersistenceDomain>, ErrorValue>;

    /// Finds what this provider could protect for `domain` at `objective` (Appendix A.3).
    ///
    /// # Errors
    ///
    /// A structured error when discovery itself failed. An empty list means "nothing here",
    /// which is a different answer.
    fn discover(
        &self,
        domain: &PersistenceDomain,
        objective: RecoveryObjective,
    ) -> Result<Vec<RecoveryCandidate>, ErrorValue>;

    /// Turns candidates into the PREPARE actions a plan will show (§12.1, §17.1).
    ///
    /// # Errors
    ///
    /// A structured error when the policy cannot be satisfied by this provider.
    fn plan_protection(
        &self,
        candidates: &[RecoveryCandidate],
        mode: ProtectionMode,
    ) -> Result<Vec<ProtectionAction>, ErrorValue>;

    /// Creates the asset a protection action proposes (§4.5).
    ///
    /// # Errors
    ///
    /// A structured error when creation failed. §2.3 then aborts before any mutation.
    fn create(&self, action: &ProtectionAction) -> Result<RecoveryAsset, ErrorValue>;

    /// Checks an asset against §11.4's list.
    ///
    /// # Errors
    ///
    /// A structured error when the check could not be made at all, which is not the same as a
    /// check that was made and failed.
    fn validate(
        &self,
        asset: &RecoveryAsset,
    ) -> Result<crate::asset::RecoveryValidation, ErrorValue>;

    /// Builds this provider's part of a recovery plan (§12.1, §24).
    ///
    /// # Errors
    ///
    /// A structured error when a critical recovery fact could not be established. §56.3 makes
    /// that a block rather than a guess.
    fn plan_recovery(
        &self,
        asset: &RecoveryAsset,
        source: Option<&ChangePlan>,
        goal: crate::recovery::RecoveryGoal,
    ) -> Result<RecoveryPlanFragment, ErrorValue>;

    /// Carries out one recovery action (§4.1's `RECOVERING`).
    ///
    /// # Errors
    ///
    /// A structured error when the action failed. Appendix F then preserves the remaining assets
    /// and the exact partial state.
    fn restore(&self, action: &PlanAction, asset: &RecoveryAsset) -> Result<(), ErrorValue>;

    /// Removes an asset (§37).
    ///
    /// # Errors
    ///
    /// A structured error when removal failed. §37.4 surfaces the retained asset rather than
    /// forgetting it.
    fn cleanup(&self, asset: &RecoveryAsset) -> Result<(), ErrorValue>;

    /// Estimates what an asset costs now (§38).
    ///
    /// # Errors
    ///
    /// A structured error when the provider could not measure. The estimate is labelled
    /// estimated either way (§37.5).
    fn estimate_cost(&self, asset: &RecoveryAsset) -> Result<RecoveryCost, ErrorValue>;
}

/// Whether a provider can run here (§12.2, Appendix G.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderAvailability {
    /// The provider can run, at the tool version it names.
    Available {
        /// The version of the underlying tool, so Appendix G.4's variance is inspectable.
        version: Arc<str>,
    },
    /// The provider cannot run, and says why.
    Unavailable {
        /// The reason, in a sentence a person can act on.
        reason: Arc<str>,
    },
    /// The provider is present at a version it has not validated against (Appendix G.4).
    ///
    /// It degrades to unsupported rather than executing semantics it has not tested, which is
    /// what Appendix G.4 requires and what §56.3 makes the safe direction.
    Unsupported {
        /// The version that was found.
        version: Arc<str>,
        /// Why it is not supported.
        reason: Arc<str>,
    },
}

impl ProviderAvailability {
    /// Whether the provider may be asked to do anything.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(self, ProviderAvailability::Available { .. })
    }

    /// The sentence to show when it cannot.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            ProviderAvailability::Available { .. } => None,
            ProviderAvailability::Unavailable { reason }
            | ProviderAvailability::Unsupported { reason, .. } => Some(reason),
        }
    }
}

/// How well a provider supports one intent (§51).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Support {
    /// The provider can plan this intent.
    Full,
    /// The provider can plan part of it and names what it cannot.
    Partial(Arc<str>),
    /// The provider does not handle this intent.
    None,
}

/// Turns an intent into plan actions, and carries them out (§51).
///
/// §51 is explicit: providers MUST NOT mutate state during `supports` or `resolve`, which is what
/// makes §2.1 hold for the whole planner rather than only for the code that calls it.
pub trait ChangeProvider: Send + Sync + std::fmt::Debug {
    /// The provider's id.
    fn id(&self) -> &str;

    /// What the provider declares it can do (§48.3).
    fn capabilities(&self) -> ProviderCapabilities;

    /// Whether this provider handles `intent`. Never mutates (§51).
    fn supports(&self, intent: &Intent) -> Support;

    /// Turns `intent` over `targets` into actions. Never mutates (§51).
    ///
    /// # Errors
    ///
    /// `change.action_not_plannable` when the intent has no provider contract that declares
    /// everything §6.1 requires.
    fn resolve(
        &self,
        intent: &Intent,
        targets: &[FrozenTarget],
    ) -> Result<PlanFragment, ErrorValue>;

    /// Rechecks one action's preconditions against the world (§7.3).
    ///
    /// # Errors
    ///
    /// A structured error when the check could not be made, which §7.3 treats as drift.
    fn revalidate(
        &self,
        action: &PlanAction,
    ) -> Result<Vec<crate::target::DriftFinding>, ErrorValue>;

    /// Carries out one action (§4.7).
    ///
    /// # Errors
    ///
    /// A structured error when the action failed. Appendix F preserves the evidence.
    fn execute(&self, action: &PlanAction) -> Result<ono_value::ActionResult, ErrorValue>;

    /// Observes one verification contract (§23).
    ///
    /// # Errors
    ///
    /// A structured error when the observation could not be made. §23.3's `UNKNOWN` is the
    /// result of a check that ran and could not answer; an error is a check that did not run.
    fn verify(&self, contract: &VerificationContract) -> Result<VerificationResult, ErrorValue>;
}

/// What one change provider contributes to a plan (§51).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlanFragment {
    actions: Vec<PlanAction>,
    preconditions: Vec<Precondition>,
    verification: Vec<VerificationContract>,
    provider_version: Option<Arc<str>>,
}

impl PlanFragment {
    /// An empty fragment.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            actions: Vec::new(),
            preconditions: Vec::new(),
            verification: Vec::new(),
            provider_version: None,
        }
    }

    /// Adds an action.
    #[must_use]
    pub fn acting(mut self, action: PlanAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Adds a precondition (§7.2).
    #[must_use]
    pub fn requiring(mut self, precondition: Precondition) -> Self {
        self.preconditions.push(precondition);
        self
    }

    /// Adds a verification contract (§23.1).
    #[must_use]
    pub fn verifying(mut self, contract: VerificationContract) -> Self {
        self.verification.push(contract);
        self
    }

    /// Records the provider version the fragment was resolved against (§4.4).
    #[must_use]
    pub fn at_version(mut self, version: impl Into<Arc<str>>) -> Self {
        self.provider_version = Some(version.into());
        self
    }

    /// The actions.
    #[must_use]
    pub fn actions(&self) -> &[PlanAction] {
        &self.actions
    }

    /// The preconditions.
    #[must_use]
    pub fn preconditions(&self) -> &[Precondition] {
        &self.preconditions
    }

    /// The verification contracts.
    #[must_use]
    pub fn verification(&self) -> &[VerificationContract] {
        &self.verification
    }

    /// The provider version.
    #[must_use]
    pub fn provider_version(&self) -> Option<&str> {
        self.provider_version.as_deref()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_refuse_to_call_a_provider_that_cannot_restore_a_recovery_provider() {
        let partial = ProviderCapabilities::new("dev.example.recovery")
            .recovering(RecoveryCapability::Discover)
            .recovering(RecoveryCapability::Prepare);
        assert!(
            partial
                .missing_required()
                .contains(&RecoveryCapability::Restore),
            "§62.1: a candidate nobody can use is snapshot theatre"
        );
    }

    #[test]
    fn should_accept_a_provider_declaring_every_required_capability() {
        let complete = RecoveryCapability::REQUIRED.iter().fold(
            ProviderCapabilities::new("ono.recovery.zfs"),
            |carry, capability| carry.recovering(*capability),
        );
        assert!(complete.missing_required().is_empty());
    }

    #[test]
    fn should_not_let_describing_impact_become_permission_to_execute() {
        let describer = ProviderCapabilities::new("dev.example.postgres")
            .changing(ChangeCapability::PlanRead)
            .changing(ChangeCapability::PlanContribute);
        assert!(
            !describer.may_execute(),
            "§48.4: a plugin that can describe impact MUST NOT gain permission to execute"
        );
    }

    #[test]
    fn should_classify_which_capabilities_change_the_system() {
        assert!(RecoveryCapability::Restore.mutates());
        assert!(RecoveryCapability::Prepare.mutates());
        assert!(
            !RecoveryCapability::Discover.mutates(),
            "§5.5: discovery is read-only; creating the asset is the mutation"
        );
        assert!(!ChangeCapability::PlanRead.mutates());
        assert!(ChangeCapability::ActionExecute.mutates());
    }

    #[test]
    fn should_degrade_to_unsupported_rather_than_run_untested_semantics() {
        let unsupported = ProviderAvailability::Unsupported {
            version: Arc::from("0.1-alpha"),
            reason: Arc::from("this provider has not been validated against 0.1-alpha"),
        };
        assert!(
            !unsupported.is_available(),
            "Appendix G.4: degrade to unsupported rather than execute unvalidated semantics"
        );
        assert!(unsupported.reason().is_some());
    }

    #[test]
    fn should_carry_the_tool_version_a_provider_validated_against() {
        let capabilities =
            ProviderCapabilities::new("ono.recovery.zfs").tested_against("zfs", "2.4.1");
        assert_eq!(capabilities.tool_versions().len(), 1);
        assert_eq!(
            capabilities.conformance(),
            RECOVERY_PROVIDER_CONFORMANCE,
            "Appendix G.5: a provider advertises a conformance version"
        );
    }

    #[test]
    fn should_report_no_metadata_coverage_until_a_provider_measures_it() {
        let fragment =
            RecoveryPlanFragment::new("ono.recovery.zfs", RestoreMethod::DatasetRollback);
        assert_eq!(
            fragment.metadata(),
            MetadataCoverage::none(),
            "Appendix C.7: missing metadata support reduces coverage and MUST be visible, so \
             silence is not a claim that everything comes back"
        );
        let measured = fragment.restoring_metadata(MetadataCoverage::content_only());
        assert!(
            measured.metadata().gaps().contains(&"owner/group"),
            "a provider that restores only bytes says so, and the gap travels to the plan"
        );
    }

    #[test]
    fn should_keep_a_protection_action_required_by_default() {
        let scope = RecoveryScope::new("zfs-dataset", "tank/data", "localhost");
        let candidate = RecoveryCandidate::new(
            "ono.recovery.zfs",
            scope.clone(),
            EffectDomain::FilesystemPersistent,
            RecoveryObjective::PreserveExact,
            "snapshot tank/data",
        );
        let asset = RecoveryAsset::proposed(
            "ono.recovery.zfs",
            crate::asset::RecoveryAssetType::ZfsSnapshot,
            "tank/data@ono-a82f",
            scope,
            jiff::Timestamp::UNIX_EPOCH,
        );
        let action =
            ProtectionAction::new("ono.recovery.zfs", "snapshot tank/data", candidate, asset);
        assert!(
            action.is_required(),
            "§2.3: if a required recovery asset cannot be created, mutation MUST NOT begin"
        );
        assert!(!action.optional().is_required());
    }
}
