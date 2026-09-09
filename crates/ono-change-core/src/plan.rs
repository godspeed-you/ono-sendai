//! The `ChangePlan` (spec v0.6 §3.2, §4, §46.1).
//!
//! A plan is a typed object, not a rehearsal. §1.2 lists what separates it from a dry run —
//! resolved targets, an ordered action graph, preconditions, effects at four confidence classes,
//! risk, protection opportunities, recovery assets and methods, verification criteria, provider
//! provenance, approval requirements, strategy, and a revision and integrity identity — and this
//! type carries all of it.
//!
//! Three of the core invariants are enforced by the shape rather than by the caller:
//!
//! - **§2.1, planning is side-effect free.** Nothing in this module performs I/O. A plan
//!   describes protection actions; creating the assets is the executor's job at PREPARE.
//! - **§4.4, a sealed plan is immutable.** [`ChangePlan::seal`] consumes the plan and returns a
//!   sealed one; every mutator is on the draft. Changing a sealed plan means [`ChangePlan::revise`],
//!   which produces a new revision and leaves the original alone (§7.5).
//! - **§23.1, a plan that mutates carries verification.** [`ChangePlan::seal`] refuses otherwise,
//!   so an unverifiable plan cannot reach a state `apply` accepts.

use std::sync::Arc;

use jiff::Timestamp;
use ono_value::ErrorValue;

use crate::action::{ActionRole, ActionStatus, PlanAction, topological_order};
use crate::digest::DigestBuilder;
use crate::effect::ProposedEffect;
use crate::id::PlanId;
use crate::impact::ImpactGraph;
use crate::protection::{ProtectionMode, ProtectionSummary};
use crate::risk::{RequiredAcknowledgement, RiskAssessment};
use crate::state::{LifecycleEvent, PlanState};
use crate::strategy::Strategy;
use crate::target::FrozenTarget;
use crate::verification::VerificationSet;
use crate::vocab::vocabulary;

vocabulary! {
    /// Whether a plan changes the system forward or restores it (§24.1).
    PlanKind {
        Change => "change", "§3.2: an ordinary proposed change.";
        Recovery => "recovery", "§3.8 and §24.1: a plan produced to recover from a prior plan or asset. It is a ChangePlan, and §2.12 puts it through the same lifecycle.";
    }
}

/// The operator's requested change, at the highest semantic level (§3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    text: Arc<str>,
    source: Arc<str>,
}

impl Intent {
    /// Records `text` as the intent, spelled by the operator as `source`.
    #[must_use]
    pub fn new(text: impl Into<Arc<str>>, source: impl Into<Arc<str>>) -> Self {
        Self {
            text: text.into(),
            source: source.into(),
        }
    }

    /// The intent, as a person reads it.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// What the operator actually typed.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// A provider the plan is bound to, and the version whose semantics it was resolved against.
///
/// §4.4 puts these in the seal because a plan resolved against one version of a provider is not
/// the same plan against another, and Appendix G.4 requires a provider to degrade to unsupported
/// rather than execute semantics it has not validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderBinding {
    id: Arc<str>,
    version: Arc<str>,
}

impl ProviderBinding {
    /// Binds `id` at `version`.
    #[must_use]
    pub fn new(id: impl Into<Arc<str>>, version: impl Into<Arc<str>>) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
        }
    }

    /// The provider's id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The version the plan was resolved against.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// A versioned description of one proposed change (§3.2, §46.1).
#[derive(Debug, Clone, PartialEq)]
pub struct ChangePlan {
    id: PlanId,
    revision: u32,
    kind: PlanKind,
    state: PlanState,
    intent: Intent,
    session: Arc<str>,
    created_at: Timestamp,
    sealed_at: Option<Timestamp>,
    expires_at: Option<Timestamp>,
    targets: Vec<FrozenTarget>,
    actions: Vec<PlanAction>,
    impact: ImpactGraph,
    protection: ProtectionSummary,
    protection_mode: ProtectionMode,
    risk: RiskAssessment,
    strategy: Strategy,
    verification: VerificationSet,
    providers: Vec<ProviderBinding>,
    digest: Option<Arc<str>>,
    supersedes: Option<u32>,
}

impl ChangePlan {
    /// Opens a draft for `intent` in `session` at `created_at` (§4.2).
    #[must_use]
    pub fn draft(intent: Intent, session: impl Into<Arc<str>>, created_at: Timestamp) -> Self {
        let session = session.into();
        let id = PlanId::of(&session, &created_at.to_string(), intent.text());
        Self {
            id,
            revision: 1,
            kind: PlanKind::Change,
            state: PlanState::Draft,
            intent,
            session,
            created_at,
            sealed_at: None,
            expires_at: None,
            targets: Vec::new(),
            actions: Vec::new(),
            impact: ImpactGraph::empty(),
            protection: ProtectionSummary::empty(),
            protection_mode: ProtectionMode::Prefer,
            risk: RiskAssessment::empty(),
            strategy: Strategy::Sequential,
            verification: VerificationSet::empty(),
            providers: Vec::new(),
            digest: None,
            supersedes: None,
        }
    }

    /// Marks this plan as a recovery plan (§3.8).
    #[must_use]
    pub const fn as_recovery(mut self) -> Self {
        self.kind = PlanKind::Recovery;
        self
    }

    /// Freezes `targets` and moves the plan to `RESOLVED` (§4.3).
    ///
    /// # Errors
    ///
    /// Refuses when the plan is already sealed: §2.6 forbids the target set moving afterwards.
    pub fn resolve(mut self, targets: Vec<FrozenTarget>) -> Result<Self, ErrorValue> {
        if self.state.is_sealed() {
            return Err(crate::error::plan_not_editable(&self.id, self.state));
        }
        self.targets = targets;
        self.state = self
            .state
            .after(LifecycleEvent::Resolve)
            .unwrap_or(PlanState::Resolved);
        Ok(self)
    }

    /// Adds an action to a draft.
    ///
    /// # Errors
    ///
    /// Refuses on a sealed plan (§4.4).
    pub fn with_action(mut self, action: PlanAction) -> Result<Self, ErrorValue> {
        if self.state.is_sealed() {
            return Err(crate::error::plan_not_editable(&self.id, self.state));
        }
        self.actions.push(action);
        Ok(self)
    }

    /// Sets the impact graph.
    #[must_use]
    pub fn with_impact(mut self, impact: ImpactGraph) -> Self {
        self.impact = impact;
        self
    }

    /// Sets the coverage matrix (§10.3).
    #[must_use]
    pub fn with_protection(mut self, protection: ProtectionSummary) -> Self {
        self.protection = protection;
        self
    }

    /// Sets the protection policy mode this plan runs under (§17.3).
    #[must_use]
    pub const fn with_protection_mode(mut self, mode: ProtectionMode) -> Self {
        self.protection_mode = mode;
        self
    }

    /// Sets the risk assessment (§19).
    #[must_use]
    pub fn with_risk(mut self, risk: RiskAssessment) -> Self {
        self.risk = risk;
        self
    }

    /// Sets the execution strategy (§28.4).
    #[must_use]
    pub const fn with_strategy(mut self, strategy: Strategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Sets the verification contracts (§23.1).
    #[must_use]
    pub fn with_verification(mut self, verification: VerificationSet) -> Self {
        self.verification = verification;
        self
    }

    /// Binds a provider and the version the plan was resolved against (§4.4).
    #[must_use]
    pub fn binding(mut self, binding: ProviderBinding) -> Self {
        if !self.providers.contains(&binding) {
            self.providers.push(binding);
        }
        self
    }

    /// Sets when the sealed plan stops being appliable (§4.1's `EXPIRED`).
    #[must_use]
    pub const fn expiring_at(mut self, at: Timestamp) -> Self {
        self.expires_at = Some(at);
        self
    }

    /// Seals the plan, computing the canonical digest of §4.4.
    ///
    /// # Errors
    ///
    /// - `change.plan_not_sealed` when the plan is already sealed.
    /// - `change.action_not_plannable` when the action graph has a cycle (§3.2).
    /// - `change.verification_missing` when a plan with a MUTATE action carries no contract
    ///   (§23.1).
    pub fn seal(mut self, at: Timestamp) -> Result<Self, ErrorValue> {
        if self.state.is_sealed() {
            return Err(crate::error::plan_not_editable(&self.id, self.state));
        }
        if let Err(cycle) = topological_order(&self.actions) {
            return Err(crate::error::action_graph_cyclic(&self.id, &cycle));
        }
        if self.mutates() && self.verification.is_empty() {
            return Err(crate::error::verification_missing(&self.id));
        }
        self.sealed_at = Some(at);
        self.state = PlanState::Sealed;
        self.digest = Some(Arc::from(self.compute_digest().as_str()));
        Ok(self)
    }

    /// Opens a new revision of a sealed plan, leaving this one untouched (§7.5, §4.4).
    ///
    /// The returned plan keeps the identity and carries revision + 1 in `DRAFT`. The caller holds
    /// the original by value, so "rebase MUST NOT mutate the sealed original" is a property of the
    /// signature rather than a rule.
    #[must_use]
    pub fn revise(&self) -> Self {
        let mut next = self.clone();
        next.revision = self.revision.saturating_add(1);
        next.supersedes = Some(self.revision);
        next.state = PlanState::Draft;
        next.sealed_at = None;
        next.digest = None;
        next.actions = next
            .actions
            .into_iter()
            .map(|action| action.with_status(ActionStatus::Pending))
            .collect();
        next
    }

    /// Records a lifecycle transition, or refuses one §4.1 does not draw.
    ///
    /// # Errors
    ///
    /// `change.plan_state_invalid` when no edge exists.
    pub fn advance(mut self, event: LifecycleEvent) -> Result<Self, ErrorValue> {
        let next = self
            .state
            .after(event)
            .ok_or_else(|| crate::error::invalid_transition(&self.id, self.state, event))?;
        self.state = next;
        Ok(self)
    }

    /// The plan's identity.
    #[must_use]
    pub const fn id(&self) -> &PlanId {
        &self.id
    }

    /// The revision, which increases and never repeats (§3.2).
    #[must_use]
    pub const fn revision(&self) -> u32 {
        self.revision
    }

    /// Whether this is a change plan or a recovery plan (§24.1).
    #[must_use]
    pub const fn kind(&self) -> PlanKind {
        self.kind
    }

    /// Where the plan is in §4.1.
    #[must_use]
    pub const fn state(&self) -> PlanState {
        self.state
    }

    /// The intent.
    #[must_use]
    pub const fn intent(&self) -> &Intent {
        &self.intent
    }

    /// The session that created the plan.
    #[must_use]
    pub fn session(&self) -> &str {
        &self.session
    }

    /// When it was created.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// When it was sealed.
    #[must_use]
    pub const fn sealed_at(&self) -> Option<Timestamp> {
        self.sealed_at
    }

    /// When it stops being appliable.
    #[must_use]
    pub const fn expires_at(&self) -> Option<Timestamp> {
        self.expires_at
    }

    /// The frozen targets (§4.3).
    #[must_use]
    pub fn targets(&self) -> &[FrozenTarget] {
        &self.targets
    }

    /// The actions.
    #[must_use]
    pub fn actions(&self) -> &[PlanAction] {
        &self.actions
    }

    /// The actions of one role.
    #[must_use]
    pub fn actions_of(&self, role: ActionRole) -> Vec<&PlanAction> {
        self.actions
            .iter()
            .filter(|action| action.role() == role)
            .collect()
    }

    /// Every effect every action declares (§8).
    #[must_use]
    pub fn effects(&self) -> Vec<&ProposedEffect> {
        self.actions.iter().flat_map(PlanAction::effects).collect()
    }

    /// The impact graph (§9).
    #[must_use]
    pub const fn impact(&self) -> &ImpactGraph {
        &self.impact
    }

    /// The coverage matrix (§10.3).
    #[must_use]
    pub const fn protection(&self) -> &ProtectionSummary {
        &self.protection
    }

    /// The protection policy mode.
    #[must_use]
    pub const fn protection_mode(&self) -> ProtectionMode {
        self.protection_mode
    }

    /// The risk assessment (§19).
    #[must_use]
    pub const fn risk(&self) -> &RiskAssessment {
        &self.risk
    }

    /// The execution strategy (§28.4).
    #[must_use]
    pub const fn strategy(&self) -> Strategy {
        self.strategy
    }

    /// The verification contracts (§23).
    #[must_use]
    pub const fn verification(&self) -> &VerificationSet {
        &self.verification
    }

    /// The providers the plan is bound to.
    #[must_use]
    pub fn providers(&self) -> &[ProviderBinding] {
        &self.providers
    }

    /// The seal digest, present only on a sealed plan (§4.4).
    #[must_use]
    pub fn digest(&self) -> Option<&str> {
        self.digest.as_deref()
    }

    /// The revision this one was derived from, where it was.
    #[must_use]
    pub const fn supersedes(&self) -> Option<u32> {
        self.supersedes
    }

    /// Whether the plan contains an action that changes the system (§23.1, §4.7).
    #[must_use]
    pub fn mutates(&self) -> bool {
        self.actions
            .iter()
            .any(|action| action.role().mutates_target())
    }

    /// Whether the plan contains an action Ono cannot reason about (§6.3).
    #[must_use]
    pub fn has_opaque_action(&self) -> bool {
        self.actions
            .iter()
            .any(|action| action.execution().is_opaque())
    }

    /// Whether any action needs elevated privilege (§43.3).
    #[must_use]
    pub fn needs_privilege(&self) -> bool {
        self.actions.iter().any(PlanAction::needs_privilege)
    }

    /// Whether the plan has expired as of `now` (§5.6).
    #[must_use]
    pub fn is_expired_at(&self, now: Timestamp) -> bool {
        self.state == PlanState::Expired || self.expires_at.is_some_and(|at| now >= at)
    }

    /// The acknowledgements §19.4 still requires before apply.
    #[must_use]
    pub fn outstanding_acknowledgements(&self) -> Vec<RequiredAcknowledgement> {
        self.risk.outstanding_acknowledgements()
    }

    /// Whether the seal still describes the plan, which is what §63.2 asks to be verifiable.
    #[must_use]
    pub fn digest_holds(&self) -> bool {
        match self.digest.as_deref() {
            None => false,
            Some(recorded) => recorded == self.compute_digest(),
        }
    }

    /// The canonical digest of §4.4, over everything the seal covers.
    #[must_use]
    pub fn compute_digest(&self) -> String {
        let mut actions = String::new();
        for action in &self.actions {
            actions.push_str(&action.digest_text());
            actions.push(crate::digest::UNIT);
        }
        let mut targets = String::new();
        for target in &self.targets {
            targets.push_str(&target.digest_text());
            targets.push(crate::digest::UNIT);
        }
        let mut providers = String::new();
        for provider in &self.providers {
            use std::fmt::Write as _;
            let _ = write!(
                providers,
                "{}@{}{}",
                provider.id(),
                provider.version(),
                crate::digest::UNIT
            );
        }
        DigestBuilder::new()
            .section("revision", &self.revision.to_string())
            .section("kind", self.kind.as_str())
            .section("intent", self.intent.text())
            .section("targets", &targets)
            .section("actions", &actions)
            .section("providers", &providers)
            .section("protection-mode", self.protection_mode.as_str())
            .section("protection", &self.protection.digest_text())
            .section("strategy", &self.strategy.digest_text())
            .section("verification", &self.verification.digest_text())
            .section("risk", &self.risk.digest_text())
            .finish()
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
    use crate::action::Execution;
    use crate::verification::{VerificationClass, VerificationContract};

    fn instant() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    fn draft() -> ChangePlan {
        ChangePlan::draft(
            Intent::new(
                "replace nginx configuration and restart service",
                "plan { replace file /etc/nginx/nginx.conf from ./nginx.conf }",
            ),
            "session-1",
            instant(),
        )
    }

    fn mutate(plan: &ChangePlan, ordinal: usize, summary: &str) -> PlanAction {
        PlanAction::new(
            plan.id(),
            ordinal,
            ActionRole::Mutate,
            summary,
            Execution::ProviderAction {
                provider: Arc::from("linux.files"),
                operation: Arc::from("ono.file.write"),
                arguments: Vec::new(),
            },
        )
    }

    fn verified(plan: &ChangePlan) -> VerificationSet {
        VerificationSet::empty().with(VerificationContract::new(
            plan.id(),
            VerificationClass::Required,
            "nginx.service",
            "state == running",
        ))
    }

    fn sealed() -> ChangePlan {
        let plan = draft();
        let action = mutate(&plan, 1, "replace nginx.conf");
        let verification = verified(&plan);
        plan.with_action(action)
            .expect("a draft accepts an action")
            .with_verification(verification)
            .seal(instant())
            .expect("a plan with a required contract seals")
    }

    #[test]
    fn should_refuse_to_seal_a_mutating_plan_with_no_verification() {
        let plan = draft();
        let action = mutate(&plan, 1, "replace nginx.conf");
        let error = plan
            .with_action(action)
            .expect("a draft accepts an action")
            .seal(instant())
            .expect_err("§23.1 requires at least one verification contract");
        assert_eq!(error.code().name(), "change.verification_missing");
    }

    #[test]
    fn should_refuse_to_edit_a_sealed_plan() {
        let plan = sealed();
        let action = mutate(&plan, 2, "restart nginx");
        let error = plan
            .with_action(action)
            .expect_err("§4.4: a sealed plan is immutable");
        assert_eq!(error.code().name(), "change.plan_sealed");
    }

    #[test]
    fn should_refuse_to_seal_a_plan_whose_actions_form_a_cycle() {
        let plan = draft();
        let mut first = mutate(&plan, 1, "a");
        let second = mutate(&plan, 2, "b").after(first.id().clone());
        first = first.after(second.id().clone());
        let error = plan
            .with_action(first)
            .and_then(|plan| plan.with_action(second))
            .and_then(|plan| {
                plan.with_verification(VerificationSet::empty())
                    .seal(instant())
            })
            .expect_err("§3.2 requires an acyclic action graph");
        assert_eq!(error.code().name(), "change.action_not_plannable");
    }

    #[test]
    fn should_carry_a_digest_only_once_sealed() {
        let plan = draft();
        assert!(
            plan.digest().is_none(),
            "§4.4 puts the digest on the seal, and a draft has none"
        );
        assert!(sealed().digest().is_some());
        assert!(
            sealed().digest_holds(),
            "§63.2: a sealed plan must be digest-verifiable"
        );
    }

    #[test]
    fn should_change_the_digest_when_the_strategy_changes() {
        let one = sealed();
        let two = one
            .revise()
            .with_strategy(Strategy::batch(4).expect("valid"))
            .seal(instant())
            .expect("re-seals");
        assert_ne!(
            one.digest(),
            two.digest(),
            "§55.1 case 3: the plan digest changes when strategy or target changes"
        );
    }

    #[test]
    fn should_change_the_digest_when_a_target_changes() {
        let one = sealed();
        let two = one
            .revise()
            .resolve(vec![FrozenTarget::new(
                "ono.service/1",
                "nginx.service",
                "nginx",
            )])
            .expect("a draft resolves")
            .seal(instant())
            .expect("re-seals");
        assert_ne!(one.digest(), two.digest());
    }

    #[test]
    fn should_leave_the_original_untouched_when_a_plan_is_revised() {
        let original = sealed();
        let revised = original.revise();
        assert_eq!(original.revision(), 1);
        assert_eq!(original.state(), PlanState::Sealed);
        assert_eq!(
            revised.revision(),
            2,
            "§7.5: rebase creates a new plan revision"
        );
        assert_eq!(
            revised.state(),
            PlanState::Draft,
            "a new revision is editable again"
        );
        assert_eq!(revised.id(), original.id(), "the identity is the plan's");
        assert_eq!(revised.supersedes(), Some(1));
    }

    #[test]
    fn should_refuse_a_lifecycle_transition_section_four_does_not_draw() {
        let error = sealed()
            .advance(LifecycleEvent::Verified)
            .expect_err("a sealed plan has not verified anything");
        assert_eq!(error.code().name(), "change.plan_state_invalid");
    }

    #[test]
    fn should_report_a_plan_as_expired_once_its_window_closed() {
        let later = Timestamp::from_second(3600).expect("a valid instant");
        let plan = draft()
            .with_verification(VerificationSet::empty())
            .expiring_at(later)
            .seal(instant())
            .expect("a plan with no mutation needs no contract");
        assert!(!plan.is_expired_at(instant()));
        assert!(
            plan.is_expired_at(later),
            "§5.6: apply MUST refuse an expired plan"
        );
    }

    #[test]
    fn should_seal_a_plan_that_changes_nothing_without_a_contract() {
        let plan = draft()
            .seal(instant())
            .expect("§23.1 applies to plans containing a MUTATE action");
        assert!(!plan.mutates());
    }

    #[test]
    fn should_report_the_protection_level_out_of_the_matrix_it_holds() {
        use crate::effect::EffectDomain;
        use crate::protection::{DomainCoverage, DomainProtection, RecoveryObjective};
        let plan = sealed()
            .revise()
            .with_protection(ProtectionSummary::of(vec![DomainCoverage::new(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
                "zfs snapshot",
            )]));
        assert_eq!(
            plan.protection().level(),
            crate::protection::ProtectionLevel::Protected,
            "§20.4: the plan's protection is read off its own matrix"
        );
    }
}
