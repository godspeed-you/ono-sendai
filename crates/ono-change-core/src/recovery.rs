//! Recovery planning (spec v0.6 §24, §25, Appendix C).
//!
//! §5.8 and §24.1 make `recover` a *planner*: it produces a [`RecoveryPlan`] and changes nothing.
//! §24.2 says why — since the original plan ran, new files, new writes, later package updates and
//! newer snapshots may exist, and a naive rollback destroys them. So a recovery plan is a
//! [`crate::ChangePlan`] plus the things §24.3 requires it to show, and the two that carry the
//! weight are [`NewerStateImpact`] and [`UnrecoverableEffect`].
//!
//! Appendix C.1's least-destructive principle is enforced by [`choose_method`] rather than
//! documented beside it: given several candidate methods that satisfy the goal, the one with the
//! smallest recovery blast radius wins, and a full dataset rollback is chosen only when nothing
//! above it can preserve what the operator asked to keep.
//!
//! §25.3 is the rule the verification half exists for: nothing here can say "recovered" without
//! naming the domain it recovered. [`RecoveryOutcome`] has one field per equivalence domain and
//! no summary field for a renderer to reach for.

use std::sync::Arc;

use jiff::Timestamp;
use ono_value::ByteSize;

use crate::asset::RestoreMethod;
use crate::effect::EffectDomain;
use crate::id::{PlanId, RecoveryAssetId};
use crate::plan::ChangePlan;
use crate::verification::EquivalenceDomain;
use crate::vocab::vocabulary;

vocabulary! {
    /// What a recovery is trying to achieve (Appendix C.2).
    ///
    /// Appendix C.2 is explicit that the goal is not automatically "return the whole persistence
    /// domain to the snapshot timestamp". A failed nginx configuration plan wants the
    /// configuration object back and the service healthy, not every file in `/etc` rewound.
    RecoveryGoal {
        RestoreChangedObjects => "restore-changed-objects", "Appendix C.2: put back the objects the original plan changed, and nothing else.";
        RestoreDomain => "restore-domain", "Appendix C.2: return the whole persistence domain to the captured point. Chosen only when the objects cannot be restored individually.";
        CompensateSemantics => "compensate-semantics", "§27.4: run the declared inverse actions. Not a restore of prior state.";
        RestoreServiceHealth => "restore-service-health", "Appendix C.2: bring the service back to a healthy semantic state, accepting new process identities (§25.2).";
    }
}

vocabulary! {
    /// What a recovery method does to state written after the asset was captured (Appendix C.3).
    NewerStateClass {
        PreservedByMethod => "preserved-by-method", "Appendix C.3: the method leaves this newer state alone.";
        DiscardedByMethod => "discarded-by-method", "Appendix C.3: the method removes this newer state.";
        Conflicting => "conflicting", "Appendix C.4: the recovery target and the newer state are the same object, so restoring one discards the other.";
        Unknown => "unknown", "Appendix C.3: whether the method touches this could not be established, which §56.3 makes a reason to block rather than guess.";
    }
}

impl NewerStateClass {
    /// Whether recovery would take this newer state away (§24.3).
    #[must_use]
    pub const fn is_loss(self) -> bool {
        matches!(
            self,
            NewerStateClass::DiscardedByMethod | NewerStateClass::Conflicting
        )
    }

    /// Whether recovery must be gated on explicit acceptance because of this (§24.5, §56.3).
    #[must_use]
    pub const fn requires_acceptance(self) -> bool {
        !matches!(self, NewerStateClass::PreservedByMethod)
    }
}

/// One thing that came into being after the recovery asset was captured (Appendix C.3).
#[derive(Debug, Clone, PartialEq)]
pub struct NewerStateItem {
    object: Arc<str>,
    class: NewerStateClass,
    changed_at: Option<Timestamp>,
    detail: Arc<str>,
}

impl NewerStateItem {
    /// Records that `object` changed after the asset, and what the method would do to it.
    #[must_use]
    pub fn new(
        object: impl Into<Arc<str>>,
        class: NewerStateClass,
        detail: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            object: object.into(),
            class,
            changed_at: None,
            detail: detail.into(),
        }
    }

    /// Records when it changed.
    #[must_use]
    pub const fn changed_at(mut self, at: Timestamp) -> Self {
        self.changed_at = Some(at);
        self
    }

    /// The object.
    #[must_use]
    pub fn object(&self) -> &str {
        &self.object
    }

    /// What the method would do to it.
    #[must_use]
    pub const fn class(&self) -> NewerStateClass {
        self.class
    }

    /// When it changed, where that is known.
    #[must_use]
    pub const fn changed_instant(&self) -> Option<Timestamp> {
        self.changed_at
    }

    /// The sentence a person reads.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Everything a recovery would do to state written since the asset (§24.3, Appendix C.3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewerStateImpact {
    items: Vec<NewerStateItem>,
    destroyed_assets: Vec<Arc<str>>,
    discarded_bytes: Option<ByteSize>,
    complete: bool,
}

impl NewerStateImpact {
    /// An impact analysis that has not been performed.
    ///
    /// It is deliberately not "nothing would be lost": §62.8 names recovering without drift
    /// analysis as a failure mode, and [`NewerStateImpact::is_complete`] answers `false` until an
    /// analyser says otherwise.
    #[must_use]
    pub const fn unanalysed() -> Self {
        Self {
            items: Vec::new(),
            destroyed_assets: Vec::new(),
            discarded_bytes: None,
            complete: false,
        }
    }

    /// An analysis that ran to completion over `items`.
    #[must_use]
    pub fn analysed(items: Vec<NewerStateItem>) -> Self {
        Self {
            items,
            destroyed_assets: Vec::new(),
            discarded_bytes: None,
            complete: true,
        }
    }

    /// Rebuilds an impact analysis out of the fields a store read back.
    ///
    /// `complete` travels rather than being re-derived, because §62.8 makes "the analysis did not
    /// run" a distinct state from "the analysis found nothing", and only the writer knows which.
    #[must_use]
    pub(crate) fn restore(
        items: Vec<NewerStateItem>,
        destroyed_assets: Vec<Arc<str>>,
        discarded_bytes: Option<ByteSize>,
        complete: bool,
    ) -> Self {
        Self {
            items,
            destroyed_assets,
            discarded_bytes,
            complete,
        }
    }

    /// Names a provider-native object recovery would destroy — a newer snapshot, a bookmark, a
    /// clone (§13.6, §24.5, Appendix D.5).
    #[must_use]
    pub fn destroying(mut self, asset: impl Into<Arc<str>>) -> Self {
        self.destroyed_assets.push(asset.into());
        self
    }

    /// Records how much live data would be discarded (§24.5).
    #[must_use]
    pub const fn discarding(mut self, bytes: ByteSize) -> Self {
        self.discarded_bytes = Some(bytes);
        self
    }

    /// The newer objects and what would happen to them.
    #[must_use]
    pub fn items(&self) -> &[NewerStateItem] {
        &self.items
    }

    /// The provider-native objects recovery would destroy.
    #[must_use]
    pub fn destroyed_assets(&self) -> &[Arc<str>] {
        &self.destroyed_assets
    }

    /// How much live data would be discarded, where it could be estimated.
    #[must_use]
    pub const fn discarded_bytes(&self) -> Option<ByteSize> {
        self.discarded_bytes
    }

    /// Whether the analysis actually ran (§62.8).
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.complete
    }

    /// The newer state recovery would take away (§24.3).
    #[must_use]
    pub fn losses(&self) -> Vec<&NewerStateItem> {
        self.items
            .iter()
            .filter(|item| item.class().is_loss())
            .collect()
    }

    /// The newer state recovery would leave alone (§24.4's "newer state preserved").
    #[must_use]
    pub fn preserved(&self) -> Vec<&NewerStateItem> {
        self.items
            .iter()
            .filter(|item| item.class() == NewerStateClass::PreservedByMethod)
            .collect()
    }

    /// Whether §24.5's explicit gate applies to this recovery.
    ///
    /// Three things trigger it, and any one is enough: newer state would be discarded, a
    /// provider-native object would be destroyed, or the analysis could not be completed. The
    /// last is §56.3's fail-closed rule — an unanalysed recovery is gated, not waved through.
    #[must_use]
    pub fn requires_destructive_acceptance(&self) -> bool {
        !self.complete
            || !self.destroyed_assets.is_empty()
            || self
                .items
                .iter()
                .any(|item| item.class().requires_acceptance())
    }
}

/// An effect of the original plan that recovery cannot reverse (§24.3, §35.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnrecoverableEffect {
    subject: Arc<str>,
    domain: EffectDomain,
    reason: Arc<str>,
    compensation: Option<Arc<str>>,
}

impl UnrecoverableEffect {
    /// Records that `subject` in `domain` cannot be reversed, and why.
    #[must_use]
    pub fn new(
        subject: impl Into<Arc<str>>,
        domain: EffectDomain,
        reason: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            subject: subject.into(),
            domain,
            reason: reason.into(),
            compensation: None,
        }
    }

    /// Names an inverse action that restores an acceptable semantic state (§35.3).
    ///
    /// §35.3 is explicit that this is `COMPENSATABLE` and not rollback, so the effect stays in
    /// this list even once a compensation is named.
    #[must_use]
    pub fn compensated_by(mut self, action: impl Into<Arc<str>>) -> Self {
        self.compensation = Some(action.into());
        self
    }

    /// What cannot be reversed.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The domain it belongs to.
    #[must_use]
    pub const fn domain(&self) -> EffectDomain {
        self.domain
    }

    /// Why.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The declared inverse action, where one exists.
    #[must_use]
    pub fn compensation(&self) -> Option<&str> {
        self.compensation.as_deref()
    }
}

/// Which pieces of file metadata a restore actually puts back (Appendix C.7).
///
/// Appendix C.7 requires missing metadata support to be visible, because a restore that returns
/// the bytes and loses the SELinux label has not returned the file. Every field defaults to
/// `false`: a provider states what it can do, and silence is not a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MetadataCoverage {
    /// The file's contents.
    pub content: bool,
    /// The permission bits.
    pub mode: bool,
    /// Owning user and group.
    pub owner: bool,
    /// POSIX ACLs.
    pub acl: bool,
    /// Extended attributes.
    pub xattrs: bool,
    /// File capabilities.
    pub capabilities: bool,
    /// SELinux labels.
    pub selinux: bool,
    /// Hard-link relationships between restored files.
    pub hardlinks: bool,
}

impl MetadataCoverage {
    /// The coverage a provider that restores only bytes can claim.
    #[must_use]
    pub const fn content_only() -> Self {
        Self {
            content: true,
            ..Self::none()
        }
    }

    /// No coverage at all.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            content: false,
            mode: false,
            owner: false,
            acl: false,
            xattrs: false,
            capabilities: false,
            selinux: false,
            hardlinks: false,
        }
    }

    /// The pieces this coverage does not restore, named for the plan view (Appendix C.7).
    #[must_use]
    pub fn gaps(self) -> Vec<&'static str> {
        let mut gaps = Vec::new();
        for (present, name) in [
            (self.content, "content"),
            (self.mode, "mode"),
            (self.owner, "owner/group"),
            (self.acl, "ACLs"),
            (self.xattrs, "extended attributes"),
            (self.capabilities, "file capabilities"),
            (self.selinux, "SELinux labels"),
            (self.hardlinks, "hard-link relationships"),
        ] {
            if !present {
                gaps.push(name);
            }
        }
        gaps
    }
}

vocabulary! {
    /// What a directory restore does with files that exist now and did not then (Appendix C.6).
    DirectoryRestorePolicy {
        KeepExtraFiles => "keep-extra-files", "Appendix C.6: the default. A newer file the asset never held is left alone.";
        ExactTree => "exact-tree", "Appendix C.6: the tree is made to match the asset exactly, which deletes newer files. Chosen only when the objective requires it.";
    }
}

/// A plan produced to recover from a prior plan or asset (§3.8, §24.1, §46.5).
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryPlan {
    plan: ChangePlan,
    source_plan: Option<PlanId>,
    source_assets: Vec<RecoveryAssetId>,
    goal: RecoveryGoal,
    method: RestoreMethod,
    target_state: Arc<str>,
    restores: Vec<Arc<str>>,
    newer_state: NewerStateImpact,
    unrecoverable: Vec<UnrecoverableEffect>,
    metadata: MetadataCoverage,
    directory_policy: DirectoryRestorePolicy,
    requires_reboot: bool,
    requires_offline: bool,
    destructive_accepted: bool,
}

impl RecoveryPlan {
    /// Builds a recovery plan around `plan`, which §2.12 puts through the ordinary lifecycle.
    #[must_use]
    pub fn new(
        plan: ChangePlan,
        goal: RecoveryGoal,
        method: RestoreMethod,
        target_state: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            plan: plan.as_recovery(),
            source_plan: None,
            source_assets: Vec::new(),
            goal,
            method,
            target_state: target_state.into(),
            restores: Vec::new(),
            newer_state: NewerStateImpact::unanalysed(),
            unrecoverable: Vec::new(),
            metadata: MetadataCoverage::none(),
            directory_policy: DirectoryRestorePolicy::KeepExtraFiles,
            requires_reboot: false,
            requires_offline: false,
            destructive_accepted: false,
        }
    }

    /// Rebuilds a recovery plan out of the fields a store read back (§36.1).
    ///
    /// `pub(crate)`, reached only through [`crate::value`]. The field that must survive the round
    /// trip intact is `destructive_accepted`: §24.5's gate is recorded in the stored plan, and a
    /// recovery plan that came back with the acceptance dropped would ask again — or, worse, with
    /// it invented, would not.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub(crate) fn restore(
        plan: ChangePlan,
        source_plan: Option<PlanId>,
        source_assets: Vec<RecoveryAssetId>,
        goal: RecoveryGoal,
        method: RestoreMethod,
        target_state: Arc<str>,
        restores: Vec<Arc<str>>,
        newer_state: NewerStateImpact,
        unrecoverable: Vec<UnrecoverableEffect>,
        metadata: MetadataCoverage,
        directory_policy: DirectoryRestorePolicy,
        requires_reboot: bool,
        requires_offline: bool,
        destructive_accepted: bool,
    ) -> Self {
        Self {
            plan,
            source_plan,
            source_assets,
            goal,
            method,
            target_state,
            restores,
            newer_state,
            unrecoverable,
            metadata,
            directory_policy,
            requires_reboot,
            requires_offline,
            destructive_accepted,
        }
    }

    /// Names the plan being recovered from (§46.5's `source_plan`).
    #[must_use]
    pub fn recovering(mut self, plan: PlanId) -> Self {
        self.source_plan = Some(plan);
        self
    }

    /// Names an asset the recovery will use (§46.5's `source_assets`).
    #[must_use]
    pub fn using(mut self, asset: RecoveryAssetId) -> Self {
        self.source_assets.push(asset);
        self
    }

    /// Names an object the recovery will restore (§24.4's "will restore").
    #[must_use]
    pub fn restoring(mut self, object: impl Into<Arc<str>>) -> Self {
        self.restores.push(object.into());
        self
    }

    /// Records the newer-state analysis (§24.2, Appendix C.3).
    #[must_use]
    pub fn with_newer_state(mut self, impact: NewerStateImpact) -> Self {
        self.newer_state = impact;
        self
    }

    /// Records an effect recovery cannot reverse (§24.3).
    #[must_use]
    pub fn leaving(mut self, effect: UnrecoverableEffect) -> Self {
        self.unrecoverable.push(effect);
        self
    }

    /// Records which file metadata the restore actually puts back (Appendix C.7).
    #[must_use]
    pub const fn restoring_metadata(mut self, metadata: MetadataCoverage) -> Self {
        self.metadata = metadata;
        self
    }

    /// Sets what happens to files the asset never held (Appendix C.6).
    #[must_use]
    pub const fn directory_policy(mut self, policy: DirectoryRestorePolicy) -> Self {
        self.directory_policy = policy;
        self
    }

    /// Records that recovery needs a reboot (§13.7, §14.6).
    #[must_use]
    pub const fn needing_reboot(mut self) -> Self {
        self.requires_reboot = true;
        self
    }

    /// Records that recovery needs the filesystem offline (§13.7, §14.6).
    #[must_use]
    pub const fn needing_offline(mut self) -> Self {
        self.requires_offline = true;
        self
    }

    /// Records the operator's acceptance of the newer state this recovery destroys (§24.5).
    #[must_use]
    pub const fn destruction_accepted(mut self) -> Self {
        self.destructive_accepted = true;
        self
    }

    /// The plan this recovery is (§2.12).
    #[must_use]
    pub const fn plan(&self) -> &ChangePlan {
        &self.plan
    }

    /// The plan being recovered from.
    #[must_use]
    pub const fn source_plan(&self) -> Option<&PlanId> {
        self.source_plan.as_ref()
    }

    /// The assets the recovery uses.
    #[must_use]
    pub fn source_assets(&self) -> &[RecoveryAssetId] {
        &self.source_assets
    }

    /// What the recovery is trying to achieve (Appendix C.2).
    #[must_use]
    pub const fn goal(&self) -> RecoveryGoal {
        self.goal
    }

    /// The method it will use (§13.5, §14.4).
    #[must_use]
    pub const fn method(&self) -> RestoreMethod {
        self.method
    }

    /// The state that would be restored (§46.5's `target_state`).
    #[must_use]
    pub fn target_state(&self) -> &str {
        &self.target_state
    }

    /// The objects it will restore.
    #[must_use]
    pub fn restores(&self) -> &[Arc<str>] {
        &self.restores
    }

    /// What it would do to newer state (§24.2).
    #[must_use]
    pub const fn newer_state(&self) -> &NewerStateImpact {
        &self.newer_state
    }

    /// What it cannot reverse (§24.3).
    #[must_use]
    pub fn unrecoverable(&self) -> &[UnrecoverableEffect] {
        &self.unrecoverable
    }

    /// Which metadata the restore returns (Appendix C.7).
    #[must_use]
    pub const fn metadata(&self) -> MetadataCoverage {
        self.metadata
    }

    /// What happens to files the asset never held (Appendix C.6).
    #[must_use]
    pub const fn directory_restore_policy(&self) -> DirectoryRestorePolicy {
        self.directory_policy
    }

    /// Whether recovery needs a reboot.
    #[must_use]
    pub const fn requires_reboot(&self) -> bool {
        self.requires_reboot
    }

    /// Whether recovery needs the filesystem offline.
    #[must_use]
    pub const fn requires_offline(&self) -> bool {
        self.requires_offline
    }

    /// Whether §24.5's explicit gate is still outstanding.
    ///
    /// This is the predicate `apply` consults on a recovery plan. §24.5 is unambiguous: *"No
    /// recovery execution occurs without the explicit gate."*
    #[must_use]
    pub fn needs_destructive_acceptance(&self) -> bool {
        self.newer_state.requires_destructive_acceptance() && !self.destructive_accepted
    }

    /// Whether every object the recovery set out to restore is actually covered.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unrecoverable.is_empty() && self.newer_state.is_complete()
    }
}

/// Picks the least-destructive method that satisfies `goal` from `candidates` (Appendix C.1).
///
/// The order is Appendix C.1's, and the tie-break is the asset's own preference — a provider that
/// says a dataset rollback is the only way to restore a given object is believed, because it is
/// the one that knows. `None` means nothing on offer can achieve the goal, which §56.3 makes a
/// reason to block rather than to pick the biggest hammer.
#[must_use]
pub fn choose_method(goal: RecoveryGoal, candidates: &[RestoreMethod]) -> Option<RestoreMethod> {
    let mut usable: Vec<RestoreMethod> = candidates
        .iter()
        .copied()
        .filter(|method| satisfies(goal, *method))
        .collect();
    usable.sort_by_key(|method| method.destructiveness());
    usable.first().copied()
}

const fn satisfies(goal: RecoveryGoal, method: RestoreMethod) -> bool {
    match goal {
        RecoveryGoal::RestoreChangedObjects => matches!(
            method,
            RestoreMethod::ProviderNativeRestore
                | RestoreMethod::SelectiveFileRestore
                | RestoreMethod::CloneAndCopy
                | RestoreMethod::SubvolumeReplacement
                | RestoreMethod::DatasetRollback
                | RestoreMethod::OfflineRootRecovery
        ),
        RecoveryGoal::RestoreDomain => matches!(
            method,
            RestoreMethod::SubvolumeReplacement
                | RestoreMethod::DatasetRollback
                | RestoreMethod::OfflineRootRecovery
        ),
        RecoveryGoal::CompensateSemantics => matches!(method, RestoreMethod::Compensation),
        RecoveryGoal::RestoreServiceHealth => matches!(
            method,
            RestoreMethod::Compensation | RestoreMethod::ProviderNativeRestore
        ),
    }
}

/// What a recovery actually achieved, per equivalence domain (§25.1, §25.2).
///
/// There is deliberately no overall verdict field. §25.3 forbids the global sentence, so a caller
/// that wants to say something about a recovery has to say which domain it is talking about.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecoveryOutcome {
    persistent: Vec<(Arc<str>, EquivalenceState)>,
    runtime: Vec<(Arc<str>, EquivalenceState)>,
    external: Vec<(Arc<str>, EquivalenceState)>,
}

impl RecoveryOutcome {
    /// An outcome with nothing recorded yet.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            persistent: Vec::new(),
            runtime: Vec::new(),
            external: Vec::new(),
        }
    }

    /// Records what happened to `subject` in `domain`.
    #[must_use]
    pub fn recording(
        mut self,
        domain: EquivalenceDomain,
        subject: impl Into<Arc<str>>,
        state: EquivalenceState,
    ) -> Self {
        let entry = (subject.into(), state);
        match domain {
            EquivalenceDomain::PersistentState => self.persistent.push(entry),
            EquivalenceDomain::RuntimeState => self.runtime.push(entry),
            EquivalenceDomain::ExternalSideEffect => self.external.push(entry),
        }
        self
    }

    /// What happened to persistent state.
    #[must_use]
    pub fn persistent(&self) -> &[(Arc<str>, EquivalenceState)] {
        &self.persistent
    }

    /// What happened to runtime state.
    #[must_use]
    pub fn runtime(&self) -> &[(Arc<str>, EquivalenceState)] {
        &self.runtime
    }

    /// What happened to external side effects.
    #[must_use]
    pub fn external(&self) -> &[(Arc<str>, EquivalenceState)] {
        &self.external
    }

    /// Whether every persistent subject came back (§25.2's "PERSISTENT STATE VERIFIED").
    #[must_use]
    pub fn persistent_state_verified(&self) -> bool {
        !self.persistent.is_empty()
            && self
                .persistent
                .iter()
                .all(|(_, state)| *state == EquivalenceState::Restored)
    }

    /// Whether anything at all is unrecoverable, which §25.2 prints beside the verdict.
    #[must_use]
    pub fn has_unrecoverable(&self) -> bool {
        self.persistent
            .iter()
            .chain(&self.runtime)
            .chain(&self.external)
            .any(|(_, state)| *state == EquivalenceState::NotRecoverable)
    }
}

vocabulary! {
    /// What happened to one subject of a recovery (§25.2).
    EquivalenceState {
        Restored => "restored", "§25.2: the subject came back as it was.";
        DifferentAsExpected => "different-as-expected", "§25.2: the subject differs in a way recovery never claimed to fix — new worker PIDs after a restart.";
        NotRecoverable => "not-recoverable", "§25.2: the subject cannot be brought back at all — a TCP session, a webhook already delivered.";
        NotRestored => "not-restored", "§25.2: the subject should have come back and did not.";
        Unknown => "unknown", "§25.2: whether the subject came back could not be established.";
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
    use crate::plan::Intent;

    fn instant() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    fn base() -> ChangePlan {
        ChangePlan::draft(
            Intent::new("restore nginx configuration", "recover plan/a82f"),
            "session-1",
            instant(),
        )
    }

    fn recovery(impact: NewerStateImpact) -> RecoveryPlan {
        RecoveryPlan::new(
            base(),
            RecoveryGoal::RestoreChangedObjects,
            RestoreMethod::SelectiveFileRestore,
            "rpool/ROOT/debian@ono-a82f",
        )
        .with_newer_state(impact)
    }

    #[test]
    fn should_gate_a_recovery_whose_drift_was_never_analysed() {
        let plan = recovery(NewerStateImpact::unanalysed());
        assert!(
            plan.needs_destructive_acceptance(),
            "§62.8: recovering without drift analysis is a failure mode, not a fast path"
        );
    }

    #[test]
    fn should_not_gate_a_recovery_that_preserves_everything_newer() {
        let plan = recovery(NewerStateImpact::analysed(vec![
            NewerStateItem::new(
                "/etc/ssh/sshd_config",
                NewerStateClass::PreservedByMethod,
                "changed after the plan, outside the restore set",
            ),
            NewerStateItem::new(
                "/etc/hosts",
                NewerStateClass::PreservedByMethod,
                "changed after the plan, outside the restore set",
            ),
        ]));
        assert!(
            !plan.needs_destructive_acceptance(),
            "§24.4: a selective restore that preserves unrelated newer state needs no gate"
        );
    }

    #[test]
    fn should_gate_a_recovery_that_discards_newer_state() {
        let plan = recovery(NewerStateImpact::analysed(vec![NewerStateItem::new(
            "/var/lib/app",
            NewerStateClass::DiscardedByMethod,
            "18 GiB changed since the snapshot",
        )]));
        assert!(
            plan.needs_destructive_acceptance(),
            "§24.5: no recovery execution occurs without the explicit gate"
        );
    }

    #[test]
    fn should_gate_a_recovery_that_destroys_newer_snapshots() {
        let plan = recovery(
            NewerStateImpact::analysed(Vec::new())
                .destroying("tank/data@later-1")
                .destroying("tank/data@later-2"),
        );
        assert!(
            plan.needs_destructive_acceptance(),
            "§13.6: destruction of newer snapshots requires explicit acceptance"
        );
        assert_eq!(plan.newer_state().destroyed_assets().len(), 2);
    }

    #[test]
    fn should_clear_the_gate_only_once_acceptance_is_recorded() {
        let plan = recovery(NewerStateImpact::analysed(vec![NewerStateItem::new(
            "/etc/nginx/nginx.conf",
            NewerStateClass::Conflicting,
            "edited again at 15:12",
        )]))
        .destruction_accepted();
        assert!(!plan.needs_destructive_acceptance());
    }

    #[test]
    fn should_report_a_conflict_on_the_object_recovery_would_overwrite() {
        // Appendix C.4's own example.
        let impact = NewerStateImpact::analysed(vec![NewerStateItem::new(
            "/etc/nginx/nginx.conf",
            NewerStateClass::Conflicting,
            "the user edited it again after the plan wrote it",
        )]);
        let losses = impact.losses();
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].object(), "/etc/nginx/nginx.conf");
    }

    #[test]
    fn should_prefer_the_least_destructive_method_that_meets_the_goal() {
        let chosen = choose_method(
            RecoveryGoal::RestoreChangedObjects,
            &[
                RestoreMethod::DatasetRollback,
                RestoreMethod::SelectiveFileRestore,
                RestoreMethod::CloneAndCopy,
            ],
        );
        assert_eq!(
            chosen,
            Some(RestoreMethod::SelectiveFileRestore),
            "Appendix C.1 and §59.6: prefer selective restore over a full rollback"
        );
    }

    #[test]
    fn should_fall_back_to_a_bigger_method_only_when_nothing_smaller_is_offered() {
        let chosen = choose_method(
            RecoveryGoal::RestoreChangedObjects,
            &[RestoreMethod::DatasetRollback],
        );
        assert_eq!(chosen, Some(RestoreMethod::DatasetRollback));
    }

    #[test]
    fn should_answer_nothing_when_no_offered_method_can_meet_the_goal() {
        assert_eq!(
            choose_method(
                RecoveryGoal::RestoreDomain,
                &[
                    RestoreMethod::Compensation,
                    RestoreMethod::ProviderNativeRestore
                ],
            ),
            None,
            "§56.3: where a critical recovery fact cannot be established, block rather than guess"
        );
    }

    #[test]
    fn should_keep_an_external_effect_unrecoverable_even_with_a_compensation() {
        let effect = UnrecoverableEffect::new(
            "deployment webhook",
            EffectDomain::ExternalSideEffect,
            "the request was already served",
        )
        .compensated_by("post a rollback notification");
        assert!(
            effect.compensation().is_some(),
            "§35.3: a provider may define a compensating action"
        );
        let plan = recovery(NewerStateImpact::analysed(Vec::new())).leaving(effect);
        assert!(
            !plan.is_complete(),
            "§35.3: compensation is COMPENSATABLE, not rollback, so the effect stays visible"
        );
    }

    #[test]
    fn should_report_recovery_per_domain_and_never_as_one_word() {
        let outcome = RecoveryOutcome::empty()
            .recording(
                EquivalenceDomain::PersistentState,
                "nginx.conf",
                EquivalenceState::Restored,
            )
            .recording(
                EquivalenceDomain::RuntimeState,
                "worker PIDs",
                EquivalenceState::DifferentAsExpected,
            )
            .recording(
                EquivalenceDomain::ExternalSideEffect,
                "1 webhook request",
                EquivalenceState::NotRecoverable,
            );
        assert!(
            outcome.persistent_state_verified(),
            "§25.2: persistent state verified is a statement about persistent state"
        );
        assert!(
            outcome.has_unrecoverable(),
            "§25.2: full world equivalence is not claimed"
        );
    }

    #[test]
    fn should_not_claim_persistent_state_when_something_did_not_come_back() {
        let outcome = RecoveryOutcome::empty()
            .recording(
                EquivalenceDomain::PersistentState,
                "nginx.conf",
                EquivalenceState::Restored,
            )
            .recording(
                EquivalenceDomain::PersistentState,
                "package version",
                EquivalenceState::NotRestored,
            );
        assert!(!outcome.persistent_state_verified());
    }

    #[test]
    fn should_keep_extra_files_when_restoring_a_directory_by_default() {
        let plan = recovery(NewerStateImpact::analysed(Vec::new()));
        assert_eq!(
            plan.directory_restore_policy(),
            DirectoryRestorePolicy::KeepExtraFiles,
            "Appendix C.6: default selective directory restore MUST NOT delete newer extra files"
        );
    }

    #[test]
    fn should_name_the_metadata_a_restore_does_not_return() {
        let gaps = MetadataCoverage::content_only().gaps();
        assert!(
            gaps.contains(&"SELinux labels"),
            "Appendix C.7: missing metadata support reduces coverage and MUST be visible"
        );
        assert!(!gaps.contains(&"content"));
    }
}
