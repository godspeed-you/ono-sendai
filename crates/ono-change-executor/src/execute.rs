//! The lifecycle engine: PREPARING, PROTECTED, APPLYING, VERIFYING, CLOSED (spec v0.6 §4.5–§4.9).
//!
//! §5.6 makes `apply` the commitment point, and everything in this module is arranged around the
//! one question an operator asks after a failure: *did anything change?* The order of the steps is
//! what answers it. Claim, revalidation, capability and gate all refuse with the plan still
//! `SEALED` and nothing created (§42.4, §7.3, §43.2, §19.4). Preparation may create recovery
//! assets and still leave the targets untouched, which is why §4.1 gives it a terminal state of
//! its own (§4.5, §2.3). Only from `APPLYING` onwards is "something may have happened" the honest
//! sentence (§4.7, Appendix F).
//!
//! # Why `apply` returns an outcome rather than a result
//!
//! Appendix F's thirteen rows differ in four independent facts: where it failed, whether mutation
//! occurred, the resulting state and what the operator must be told. A `Result` carries one of
//! those. [`ApplyOutcome`] carries all of them, and [`FailurePoint`] names the row.
//!
//! # Two rules that look like implementation details and are not
//!
//! - **An unestablished outcome is never guessed.** [`ExecutionOutcome::Unknown`] records
//!   [`ActionStatus::Unknown`] and leaves the plan `APPLYING`, because Appendix F.2 makes it an
//!   uncertainty boundary for recovery planning rather than a failure or a success.
//! - **Cleanup never obscures the failure that caused it.** Appendix F.1's last sentence is a
//!   requirement about what is reported: [`prepare`] returns the original refusal whatever the
//!   cleanup did, and the cleanup's own failures are reported beside it.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ActionId, ActionRole, ActionStatus, ChangeCapability, ChangePlan, DriftFinding, DriftVerdict,
    LifecycleEvent, PlanAction, PlanKind, PlanState, ProtectionAction, ProtectionLevel,
    RecoveryAsset, RecoveryAssetId, RecoveryCapability, RecoveryValidation, Verdict,
    VerificationClass, VerificationContract, VerificationResult, VerificationStatus, error,
    topological_order,
};
use ono_change_plan::{Claim, PlanStore};
use ono_change_protection::ProviderRegistry;
use ono_value::{ErrorValue, Value};

use crate::strategy::{TargetResult, run_waves};

/// What executing one action established (§4.7, Appendix F.2).
///
/// The third variant is the one that matters. A provider that cannot say what happened has not
/// said that nothing happened, and §29.3 forbids marking such an action failed or successful
/// without evidence.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionOutcome {
    /// The action completed and the provider said so.
    Succeeded,
    /// The action did not complete, and the provider said so.
    Failed(ErrorValue),
    /// The outcome could not be established (Appendix F.2).
    Unknown(ErrorValue),
}

impl ExecutionOutcome {
    /// The status this outcome records against the action (§4.7).
    #[must_use]
    pub const fn status(&self) -> ActionStatus {
        match self {
            ExecutionOutcome::Succeeded => ActionStatus::Succeeded,
            ExecutionOutcome::Failed(_) => ActionStatus::Failed,
            ExecutionOutcome::Unknown(_) => ActionStatus::Unknown,
        }
    }

    /// The structured refusal, where the outcome carries one.
    #[must_use]
    pub const fn error(&self) -> Option<&ErrorValue> {
        match self {
            ExecutionOutcome::Succeeded => None,
            ExecutionOutcome::Failed(error) | ExecutionOutcome::Unknown(error) => Some(error),
        }
    }
}

/// What observing one verification contract established (§23.3, §23.5).
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    /// The check ran and answered.
    Answered {
        /// What it answered (§23.3).
        status: VerificationStatus,
        /// What was actually seen, where anything was.
        observed: Option<Value>,
    },
    /// The check exceeded its timeout, and the contract decides what that means (§23.5).
    TimedOut,
    /// The check could not be run at all, which is a different fact from a check that answered.
    Unobservable(ErrorValue),
}

impl Observation {
    /// A check that passed.
    #[must_use]
    pub const fn passed() -> Self {
        Observation::Answered {
            status: VerificationStatus::Passed,
            observed: None,
        }
    }

    /// A check that failed.
    #[must_use]
    pub const fn failed() -> Self {
        Observation::Answered {
            status: VerificationStatus::Failed,
            observed: None,
        }
    }
}

/// Where in Appendix F's matrix a failure happened.
///
/// Appendix F's first column is a behavioural contract rather than a diagnostic nicety: the row
/// decides whether the operator is told "nothing was changed", "assets exist and the targets are
/// untouched" or "part of it ran". Naming the row in the outcome is what lets a renderer and a
/// test agree on which sentence is owed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePoint {
    /// Another session holds the apply claim (§42.4).
    Claim,
    /// The plan is in a state `apply` does not accept, or it has expired (§5.6).
    PlanState,
    /// A frozen precondition no longer holds (§7.3).
    TargetRevalidation,
    /// A capability or privilege the apply needs is not held (§43.2, §43.3).
    PrivilegeCheck,
    /// The policy requires protection the plan could not discover (§17.2).
    RecoveryDiscovery,
    /// An outstanding acknowledgement blocks the apply (§19.4, §40.2).
    Gate,
    /// A recovery asset could not be created (§2.3).
    RecoveryAssetCreation,
    /// A created asset did not validate (§11.4).
    RecoveryValidation,
    /// An application could not be quiesced or resumed (§18.4).
    ApplicationQuiesce,
    /// A mutating action was never started: its in-flight record could not be written, or the
    /// apply claim could not be renewed before it (§41.2, §42.3). Nothing ran *at this point*;
    /// whether earlier actions of the run did is [`ApplyOutcome::has_mutated`]'s answer.
    ActionNotStarted,
    /// The first mutating action failed, so nothing else in the chain ran.
    FirstMutateAction,
    /// A later mutating action failed, after earlier ones had already changed the system.
    MiddleMutateAction,
    /// A remote action's outcome could not be established (§29.3).
    RemoteDisconnect,
    /// A local action's outcome could not be established (Appendix F.2).
    UnknownOutcome,
    /// A required postcondition failed (§23.2).
    RequiredVerification,
    /// A verification check exceeded its timeout (§23.5).
    VerificationTimeout,
    /// A recovery action failed (§4.1's `RECOVERING`).
    RecoveryAction,
    /// Recovery verification did not hold (§25.3).
    RecoveryVerification,
    /// An asset could not be removed when the plan closed (§37.4).
    Cleanup,
}

impl FailurePoint {
    /// Whether reaching this point means the target system may already have been changed.
    ///
    /// This is Appendix F's second column, and it is a property of the point rather than of the
    /// message: everything up to and including preparation leaves the targets untouched, and
    /// §2.3 is what makes that true rather than likely.
    #[must_use]
    pub const fn may_have_mutated(self) -> bool {
        matches!(
            self,
            FailurePoint::FirstMutateAction
                | FailurePoint::MiddleMutateAction
                | FailurePoint::RemoteDisconnect
                | FailurePoint::UnknownOutcome
                | FailurePoint::RequiredVerification
                | FailurePoint::VerificationTimeout
                | FailurePoint::RecoveryAction
                | FailurePoint::RecoveryVerification
        )
    }

    /// The name Appendix F's first column spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            FailurePoint::Claim => "apply-claim",
            FailurePoint::PlanState => "plan-state",
            FailurePoint::TargetRevalidation => "target-revalidation",
            FailurePoint::PrivilegeCheck => "privilege-check",
            FailurePoint::RecoveryDiscovery => "recovery-discovery",
            FailurePoint::Gate => "risk-gate",
            FailurePoint::RecoveryAssetCreation => "recovery-asset-creation",
            FailurePoint::RecoveryValidation => "recovery-validation",
            FailurePoint::ApplicationQuiesce => "application-quiesce",
            FailurePoint::ActionNotStarted => "action-not-started",
            FailurePoint::FirstMutateAction => "first-mutate-action",
            FailurePoint::MiddleMutateAction => "middle-mutate-action",
            FailurePoint::RemoteDisconnect => "remote-disconnect",
            FailurePoint::UnknownOutcome => "unknown-execution-outcome",
            FailurePoint::RequiredVerification => "verification-required-failed",
            FailurePoint::VerificationTimeout => "verification-timeout",
            FailurePoint::RecoveryAction => "recovery-action",
            FailurePoint::RecoveryVerification => "recovery-verification",
            FailurePoint::Cleanup => "cleanup",
        }
    }
}

impl std::fmt::Display for FailurePoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What this session is allowed to do (§43.2, §43.3).
///
/// §43.2 splits the answer in two — "applying requires target action capabilities plus protection
/// provider capabilities" — so a session that may plan is not thereby a session that may change
/// anything, and one that may change a service is not thereby one that may take a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authority {
    change: Vec<ChangeCapability>,
    recovery: Vec<RecoveryCapability>,
    elevated: bool,
}

impl Authority {
    /// Everything §43.2 names, for a session that holds it all.
    #[must_use]
    pub fn full() -> Self {
        Self {
            change: vec![
                ChangeCapability::PlanRead,
                ChangeCapability::PlanContribute,
                ChangeCapability::ActionExecute,
                ChangeCapability::VerificationObserve,
            ],
            recovery: RecoveryCapability::REQUIRED.to_vec(),
            elevated: true,
        }
    }

    /// The authority of the session the shell is actually running as (§43.3).
    ///
    /// `effective_uid` is the process's effective user id, and `effective_capabilities` the
    /// kernel's effective capability set (`CapEff` in `/proc/self/status`) where it could be read.
    /// Elevation is `CAP_SYS_ADMIN` in that set: a root process whose capabilities were dropped —
    /// a container, a hardened unit — cannot do what an elevated action needs, and a non-root
    /// process granted the capability can. Only where the set is unknown does uid 0 decide.
    ///
    /// The §43.2 capabilities are the shell's own grants rather than the kernel's, so they are
    /// [`Authority::full`]'s; a caller narrows them with [`Authority::without_change`].
    #[must_use]
    pub fn for_session(effective_uid: u32, effective_capabilities: Option<u64>) -> Self {
        /// `CAP_SYS_ADMIN` is capability 21 (`linux/capability.h`).
        const CAP_SYS_ADMIN: u64 = 1 << 21;
        let elevated = match effective_capabilities {
            Some(set) => set & CAP_SYS_ADMIN != 0,
            None => effective_uid == 0,
        };
        Self {
            elevated,
            ..Self::full()
        }
    }

    /// A session that may read a plan and change nothing (§43.2's "planning MAY be available
    /// without mutation capability").
    #[must_use]
    pub fn planning_only() -> Self {
        Self {
            change: vec![ChangeCapability::PlanRead],
            recovery: Vec::new(),
            elevated: false,
        }
    }

    /// The same authority with `capability` added.
    #[must_use]
    pub fn changing(mut self, capability: ChangeCapability) -> Self {
        if !self.change.contains(&capability) {
            self.change.push(capability);
        }
        self
    }

    /// The same authority with `capability` added.
    #[must_use]
    pub fn recovering(mut self, capability: RecoveryCapability) -> Self {
        if !self.recovery.contains(&capability) {
            self.recovery.push(capability);
        }
        self
    }

    /// The same authority with `capability` removed, for a session that lost it.
    #[must_use]
    pub fn without_change(mut self, capability: ChangeCapability) -> Self {
        self.change.retain(|held| *held != capability);
        self
    }

    /// The same authority with `capability` removed.
    #[must_use]
    pub fn without_recovery(mut self, capability: RecoveryCapability) -> Self {
        self.recovery.retain(|held| *held != capability);
        self
    }

    /// The same authority without the elevation §43.3's privileged actions need.
    #[must_use]
    pub const fn unprivileged(mut self) -> Self {
        self.elevated = false;
        self
    }

    /// Whether the session holds `capability`.
    #[must_use]
    pub fn has_change(&self, capability: ChangeCapability) -> bool {
        self.change.contains(&capability)
    }

    /// Whether the session holds `capability`.
    #[must_use]
    pub fn has_recovery(&self, capability: RecoveryCapability) -> bool {
        self.recovery.contains(&capability)
    }

    /// Whether the session may run an action that declares it needs privilege (§43.3).
    #[must_use]
    pub const fn is_elevated(&self) -> bool {
        self.elevated
    }
}

impl Default for Authority {
    fn default() -> Self {
        Self::full()
    }
}

/// Pausing and resuming an application for an application-consistent asset (§18.4, §39.3).
///
/// The trait is here rather than in a provider because §18.4 puts the *bound* on PREPARE: the
/// window is the executor's to close, and resuming when creation fails is the executor's to do.
pub trait Quiesce: std::fmt::Debug {
    /// Pauses `application` so a snapshot can be application-consistent.
    ///
    /// # Errors
    ///
    /// A structured error when the application could not be paused, which §18.4 turns into a
    /// prepare failure with nothing mutated.
    fn pause(&self, application: &str) -> Result<(), ErrorValue>;

    /// Resumes `application`.
    ///
    /// # Errors
    ///
    /// A structured error when the application is still paused, which §18.4 makes a critical
    /// result surfaced separately from whatever else happened.
    fn resume(&self, application: &str) -> Result<(), ErrorValue>;
}

/// The quiesce window preparation must bound (§18.4).
#[derive(Debug, Clone, Copy)]
pub struct Quiescing<'a> {
    application: &'a str,
    window: std::time::Duration,
    provider: &'a dyn Quiesce,
}

impl<'a> Quiescing<'a> {
    /// Bounds `application`'s pause to `window`, carried out by `provider`.
    #[must_use]
    pub const fn new(
        application: &'a str,
        window: std::time::Duration,
        provider: &'a dyn Quiesce,
    ) -> Self {
        Self {
            application,
            window,
            provider,
        }
    }

    /// The application that is paused.
    #[must_use]
    pub const fn application(&self) -> &'a str {
        self.application
    }

    /// The bound §18.4 requires PREPARE to place on the pause.
    #[must_use]
    pub const fn window(&self) -> std::time::Duration {
        self.window
    }
}

/// What happened to the quiesce window (§18.4).
#[derive(Debug, Clone, PartialEq)]
pub struct QuiesceReport {
    application: Arc<str>,
    window: std::time::Duration,
    paused: bool,
    resumed: bool,
    critical: Option<ErrorValue>,
}

impl QuiesceReport {
    /// The application the window covered.
    #[must_use]
    pub fn application(&self) -> &str {
        &self.application
    }

    /// The bound the window was given (§18.4).
    #[must_use]
    pub const fn window(&self) -> std::time::Duration {
        self.window
    }

    /// Whether the application was actually paused.
    #[must_use]
    pub const fn was_paused(&self) -> bool {
        self.paused
    }

    /// Whether it was put back the way it was found.
    #[must_use]
    pub const fn was_resumed(&self) -> bool {
        self.resumed
    }

    /// The critical result §18.4 requires to be surfaced separately: it is still quiesced.
    #[must_use]
    pub const fn critical(&self) -> Option<&ErrorValue> {
        self.critical.as_ref()
    }
}

/// What to do with the assets a failed preparation had already created (Appendix F.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CleanupDecision {
    /// Keep them until an operator decides, which is Appendix F's default for the row.
    #[default]
    Retain,
    /// Remove them where Appendix F.1's three conditions all hold, and keep them where they do not.
    RemoveWherePermitted,
}

/// What a cleanup attempt did, and what it left behind (Appendix F.1, §37.4).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CleanupReport {
    removed: Vec<RecoveryAssetId>,
    retained: Vec<(RecoveryAssetId, Arc<str>)>,
    failures: Vec<ErrorValue>,
}

impl CleanupReport {
    /// The assets that were removed.
    #[must_use]
    pub fn removed(&self) -> &[RecoveryAssetId] {
        &self.removed
    }

    /// The assets that were kept, each with the reason Appendix F.1 gives for keeping it.
    #[must_use]
    pub fn retained(&self) -> &[(RecoveryAssetId, Arc<str>)] {
        &self.retained
    }

    /// The refusals cleanup itself raised, which never replace the failure that caused it.
    #[must_use]
    pub fn failures(&self) -> &[ErrorValue] {
        &self.failures
    }
}

/// An optional protection action that did not produce validated protection (§4.6, §17.2).
///
/// `maximize`'s extras may fail without stopping the apply, and §4.6 forbids counting them as
/// protection when they do. Both halves of that are reported here: the asset that exists and did
/// not validate (it still occupies storage and is retained), and the one that was never created.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtectionShortfall {
    provider: Arc<str>,
    summary: Arc<str>,
    asset: Option<RecoveryAsset>,
    reason: ErrorValue,
}

impl ProtectionShortfall {
    /// The provider that was asked.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The protection action's line (§17.1).
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The asset that was created and did not validate, where one was created (`INVALID`).
    #[must_use]
    pub const fn asset(&self) -> Option<&RecoveryAsset> {
        self.asset.as_ref()
    }

    /// Why it is not protection: the creation refusal, or `recovery.asset_invalid`.
    #[must_use]
    pub const fn reason(&self) -> &ErrorValue {
        &self.reason
    }
}

/// The functions the executor reaches the world through. `revalidate` rechecks one action's
/// preconditions (§7.3).
type Revalidate<'a> = dyn Fn(&PlanAction) -> Result<Vec<DriftFinding>, ErrorValue> + 'a;

/// Carries out one action (§4.7).
type Execute<'a> = dyn Fn(&PlanAction) -> ExecutionOutcome + 'a;

/// Observes one verification contract (§23).
type Observe<'a> = dyn Fn(&VerificationContract) -> Observation + 'a;

/// Everything preparation needs, and the assets it produced on the way (§4.5).
///
/// The request is taken by `&mut` because Appendix F's asset rows are about what exists after a
/// failure: [`prepare`] answers `Err` and the four snapshots that were created are still facts,
/// so they are written back here rather than lost with the error.
#[derive(Debug)]
pub struct PrepareRequest<'a> {
    plan: &'a ChangePlan,
    protection: &'a [ProtectionAction],
    providers: &'a ProviderRegistry,
    now: Timestamp,
    authority: Authority,
    quiesce: Option<Quiescing<'a>>,
    cleanup: CleanupDecision,
    created: Vec<RecoveryAsset>,
    shortfall: Vec<ProtectionShortfall>,
    cleanup_report: Option<CleanupReport>,
    quiesce_report: Option<QuiesceReport>,
    failure_point: Option<FailurePoint>,
}

impl<'a> PrepareRequest<'a> {
    /// Prepares `plan` by asking `providers` to carry out `protection`, at `now`.
    #[must_use]
    pub fn new(
        plan: &'a ChangePlan,
        protection: &'a [ProtectionAction],
        providers: &'a ProviderRegistry,
        now: Timestamp,
    ) -> Self {
        Self {
            plan,
            protection,
            providers,
            now,
            authority: Authority::full(),
            quiesce: None,
            cleanup: CleanupDecision::Retain,
            created: Vec::new(),
            shortfall: Vec::new(),
            cleanup_report: None,
            quiesce_report: None,
            failure_point: None,
        }
    }

    /// States what the session actually holds (§43.2).
    #[must_use]
    pub fn with_authority(mut self, authority: Authority) -> Self {
        self.authority = authority;
        self
    }

    /// Bounds an application quiesce around asset creation (§18.4).
    #[must_use]
    pub const fn quiescing(mut self, quiescing: Quiescing<'a>) -> Self {
        self.quiesce = Some(quiescing);
        self
    }

    /// States what to do with sibling assets when preparation fails (Appendix F.1).
    #[must_use]
    pub const fn cleaning_up(mut self, decision: CleanupDecision) -> Self {
        self.cleanup = decision;
        self
    }

    /// The assets preparation created, whether or not it went on to succeed.
    #[must_use]
    pub fn created(&self) -> &[RecoveryAsset] {
        &self.created
    }

    /// The optional protection that did not become protection (§17.2), failed or not.
    #[must_use]
    pub fn shortfall(&self) -> &[ProtectionShortfall] {
        &self.shortfall
    }

    /// What cleanup did after a failed preparation (Appendix F.1).
    #[must_use]
    pub const fn cleanup_report(&self) -> Option<&CleanupReport> {
        self.cleanup_report.as_ref()
    }

    /// What happened to the quiesce window (§18.4).
    #[must_use]
    pub const fn quiesce_report(&self) -> Option<&QuiesceReport> {
        self.quiesce_report.as_ref()
    }

    /// Which of Appendix F's rows preparation stopped on, where it stopped.
    #[must_use]
    pub const fn failure_point(&self) -> Option<FailurePoint> {
        self.failure_point
    }

    /// The identities of the assets that still exist (Appendix F's "retain … sibling assets").
    #[must_use]
    pub fn retained(&self) -> Vec<RecoveryAssetId> {
        let removed: Vec<&RecoveryAssetId> = self
            .cleanup_report
            .as_ref()
            .map(|report| report.removed.iter().collect())
            .unwrap_or_default();
        self.created
            .iter()
            .filter(|asset| !removed.contains(&asset.id()))
            .map(|asset| asset.id().clone())
            .collect()
    }
}

/// What preparation left behind when it succeeded (§4.6).
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    assets: Vec<RecoveryAsset>,
    shortfall: Vec<ProtectionShortfall>,
    quiesce: Option<QuiesceReport>,
}

impl Prepared {
    /// Nothing prepared, for a plan with no protection to create or one already past PREPARE.
    const fn nothing() -> Self {
        Self {
            assets: Vec::new(),
            shortfall: Vec::new(),
            quiesce: None,
        }
    }

    /// The validated assets protection produced (§4.6, §11.4). Only these are protection.
    #[must_use]
    pub fn assets(&self) -> &[RecoveryAsset] {
        &self.assets
    }

    /// The optional protection that failed or did not validate (§17.2). An `INVALID` asset here
    /// still exists; the caller persists and surfaces it like any other.
    #[must_use]
    pub fn shortfall(&self) -> &[ProtectionShortfall] {
        &self.shortfall
    }

    /// What happened to the quiesce window (§18.4).
    #[must_use]
    pub const fn quiesce(&self) -> Option<&QuiesceReport> {
        self.quiesce.as_ref()
    }

    /// The state §4.6 gives a plan whose every required protection action completed.
    ///
    /// §4.6 forbids the word as a label for partial protection, and this is the only constructor
    /// of it: [`prepare`] returns `Err` where any required asset failed or did not validate.
    #[must_use]
    pub const fn state(&self) -> PlanState {
        PlanState::Protected
    }
}

/// Creates and validates the recovery assets the policy requires, before any mutation (§4.5).
///
/// The order inside preparation is §4.5's own, and each step's failure is a different row of
/// Appendix F: a quiesce that could not be taken or given back is `application quiesce`, a
/// provider that could not create an asset is `recovery asset creation`, and an asset that exists
/// and did not check out is `recovery validation`. All three leave the targets untouched, which is
/// what §2.3 requires and what makes `PREPARE_FAILED` a state of its own.
///
/// # Errors
///
/// The refusal for the row it stopped on. The assets already created stay reachable through
/// [`PrepareRequest::created`] and [`PrepareRequest::retained`], because Appendix F's row is about
/// what still exists rather than about what the message says.
pub fn prepare(request: &mut PrepareRequest<'_>) -> Result<Prepared, ErrorValue> {
    request.failure_point = None;
    request.created.clear();
    request.shortfall.clear();
    request.cleanup_report = None;
    request.quiesce_report = None;

    if let Some(quiescing) = request.quiesce
        && let Err(failure) = quiescing.provider.pause(quiescing.application)
    {
        request.failure_point = Some(FailurePoint::ApplicationQuiesce);
        // §18.4: the application is resumed when protection could not be taken, and whether that
        // worked is recorded even though the pause itself never happened.
        request.quiesce_report = Some(QuiesceReport {
            application: Arc::from(quiescing.application),
            window: quiescing.window,
            paused: false,
            resumed: true,
            critical: None,
        });
        return Err(failure);
    }
    if let Some(quiescing) = request.quiesce {
        request.quiesce_report = Some(QuiesceReport {
            application: Arc::from(quiescing.application),
            window: quiescing.window,
            paused: true,
            resumed: false,
            critical: None,
        });
    }

    let mut failure: Option<(FailurePoint, ErrorValue)> = None;
    // ADR-0825: the domains this preparation tried to protect, and the ones it did.
    let mut attempted: Vec<ono_change_core::EffectDomain> = Vec::new();
    let mut protected: Vec<ono_change_core::EffectDomain> = Vec::new();
    for action in request.protection {
        attempted.push(action.candidate().domain());
        let required = requires_protection(request.plan, action);
        if !request.authority.has_recovery(RecoveryCapability::Prepare) {
            // §43.2: creating an asset is the protection provider's capability, and a session
            // without it asks no provider for anything.
            let refusal = error::capability_missing(
                request.plan.id().as_str(),
                RecoveryCapability::Prepare.as_str(),
                false,
            );
            if required {
                failure = Some((FailurePoint::PrivilegeCheck, refusal));
                break;
            }
            request.shortfall.push(shortfall_of(action, None, refusal));
            continue;
        }
        match create_and_validate(request.providers, action, request.now) {
            Ok(asset) => {
                // §4.6: only a validated asset is protection. One that exists and did not
                // validate is still a real object — retained, reported — and never counted.
                let invalid = (!asset.is_usable()).then(|| {
                    let failures = asset
                        .validation()
                        .map(RecoveryValidation::failures)
                        .unwrap_or_default();
                    error::asset_invalid(asset.id(), &failures)
                });
                if let Some(refusal) = &invalid
                    && !required
                {
                    request.shortfall.push(shortfall_of(
                        action,
                        Some(asset.clone()),
                        refusal.clone(),
                    ));
                }
                if asset.is_usable() {
                    protected.push(action.candidate().domain());
                }
                request.created.push(asset);
                if let Some(refusal) = invalid
                    && required
                {
                    failure = Some((FailurePoint::RecoveryValidation, refusal));
                    break;
                }
            }
            Err(refusal) => {
                if required {
                    failure = Some((FailurePoint::RecoveryAssetCreation, refusal));
                    break;
                }
                // §17.2: a failed extra degrades the coverage, and a degradation nobody is told
                // about is the silent partial protection §4.6 forbids.
                request.shortfall.push(shortfall_of(action, None, refusal));
            }
        }
    }

    // ADR-0825 and §2.3: the operator approved the matrix the plan was sealed with. A domain it
    // showed protected, and that this preparation set out to protect, ends with a validated asset
    // or nothing mutates. An optional action failing there is the plan's protection failing: the
    // silent downgrade to unprotected execution §2.3 forbids, not an extra degrading.
    if failure.is_none()
        && let Some(row) = request.plan.protection().rows().iter().find(|row| {
            row.is_required()
                && row.is_satisfied()
                && attempted.contains(&row.domain())
                && !protected.contains(&row.domain())
        })
    {
        let cause = request.shortfall.last().map_or_else(
            || {
                error::asset_create_failed(
                    "the protection provider",
                    row.domain().as_str(),
                    "no validated asset came out of the preparation",
                )
            },
            |shortfall| shortfall.reason.clone(),
        );
        failure = Some((FailurePoint::RecoveryAssetCreation, cause));
    }

    // §18.4: the window closes whether creation worked or not, and failing to close it is a
    // critical result of its own rather than a footnote on the creation failure.
    let resume_failure = release_quiesce(request);

    match (failure, resume_failure) {
        (Some((point, refusal)), _) => {
            request.failure_point = Some(point);
            run_cleanup(request);
            Err(prepare_refusal(request, point, &refusal))
        }
        (None, Some(critical)) => {
            request.failure_point = Some(FailurePoint::ApplicationQuiesce);
            run_cleanup(request);
            Err(critical)
        }
        (None, None) => Ok(Prepared {
            assets: request
                .created
                .iter()
                .filter(|asset| asset.is_usable())
                .cloned()
                .collect(),
            shortfall: request.shortfall.clone(),
            quiesce: request.quiesce_report.clone(),
        }),
    }
}

/// Whether a failure of `action` must stop the apply before mutation (§2.3, §4.6, §17.2).
///
/// This is the one predicate for "required protection": the discovery check, the capability
/// check and preparation all ask it. Under `require` nothing planned is optional, whatever the
/// action says, because §17.2's `require` refuses to apply without the protection; elsewhere the
/// action's own flag decides (§17.2's `maximize` extras).
fn requires_protection(plan: &ChangePlan, action: &ProtectionAction) -> bool {
    action.is_required() || plan.protection_mode().refuses_shortfall()
}

/// The report for an optional protection action that did not become protection.
fn shortfall_of(
    action: &ProtectionAction,
    asset: Option<RecoveryAsset>,
    reason: ErrorValue,
) -> ProtectionShortfall {
    ProtectionShortfall {
        provider: Arc::from(action.provider()),
        summary: Arc::from(action.summary()),
        asset,
        reason,
    }
}

/// Closes the quiesce window, returning §18.4's critical result where it would not close.
fn release_quiesce(request: &mut PrepareRequest<'_>) -> Option<ErrorValue> {
    let quiescing = request.quiesce?;
    let report = request.quiesce_report.as_mut()?;
    if !report.paused {
        return None;
    }
    match quiescing.provider.resume(quiescing.application) {
        Ok(()) => {
            report.resumed = true;
            None
        }
        Err(failure) => {
            report.resumed = false;
            report.critical = Some(failure.clone());
            Some(failure)
        }
    }
}

/// Asks one provider to create an asset and then to check it against §11.4's list.
fn create_and_validate(
    providers: &ProviderRegistry,
    action: &ProtectionAction,
    now: Timestamp,
) -> Result<RecoveryAsset, ErrorValue> {
    let Some(provider) = providers.get(action.provider()) else {
        return Err(error::provider_unavailable(
            action.provider(),
            "the plan named a recovery provider this session has not registered",
        ));
    };
    let asset = provider.create(action)?;
    match provider.validate(&asset) {
        Ok(validation) => Ok(asset.validated(validation)),
        // §11.4 separates a check that was made and failed from a check nobody could make. The
        // second is still not protection, so the asset is INVALID and the refusal says why.
        Err(refusal) => Ok(asset
            .validated(RecoveryValidation::none(now, refusal.message().to_owned()).existing(true))),
    }
}

/// Appendix F.1's three conditions, applied to every asset a failed preparation created.
fn run_cleanup(request: &mut PrepareRequest<'_>) {
    let mut report = CleanupReport::default();
    if request.cleanup == CleanupDecision::RemoveWherePermitted {
        let quiesce_holds = request
            .quiesce_report
            .as_ref()
            .is_some_and(|report| !report.resumed);
        let depended_on: BTreeSet<RecoveryAssetId> = request
            .created
            .iter()
            .flat_map(|asset| asset.dependencies().iter().cloned())
            .collect();
        for asset in &request.created {
            if let Some(reason) = retention_reason(request, asset, quiesce_holds, &depended_on) {
                report
                    .retained
                    .push((asset.id().clone(), Arc::from(reason)));
                continue;
            }
            let removal = request.providers.get(asset.provider()).map_or_else(
                || {
                    Err(error::provider_unavailable(
                        asset.provider(),
                        "the provider that created the asset is not registered here",
                    ))
                },
                |provider| provider.cleanup(asset),
            );
            match removal {
                Ok(()) => report.removed.push(asset.id().clone()),
                Err(failure) => {
                    // Appendix F.1: "cleanup failure must not obscure the original prepare
                    // failure". It is reported beside it and never in place of it.
                    report.failures.push(failure);
                    report.retained.push((
                        asset.id().clone(),
                        Arc::from("the provider could not remove it"),
                    ));
                }
            }
        }
    } else {
        for asset in &request.created {
            report.retained.push((
                asset.id().clone(),
                Arc::from("retained until a cleanup decision (Appendix F)"),
            ));
        }
    }
    request.cleanup_report = Some(report);
}

/// Why Appendix F.1 forbids removing this asset, or `None` where all three conditions hold.
fn retention_reason(
    request: &PrepareRequest<'_>,
    asset: &RecoveryAsset,
    quiesce_holds: bool,
    depended_on: &BTreeSet<RecoveryAssetId>,
) -> Option<&'static str> {
    if asset.source_plan() != Some(request.plan.id()) {
        return Some("it was not created solely for this prepare");
    }
    if !request
        .providers
        .get(asset.provider())
        .is_some_and(|provider| {
            provider
                .capabilities()
                .has_recovery(RecoveryCapability::Cleanup)
        })
    {
        return Some("its provider does not declare recovery.cleanup");
    }
    if !request.authority.has_recovery(RecoveryCapability::Cleanup) {
        return Some("this session does not hold recovery.cleanup");
    }
    if quiesce_holds {
        return Some("the quiesce window did not close, so the asset may still be needed");
    }
    if depended_on.contains(asset.id()) {
        return Some("another recovery asset depends on it");
    }
    if asset.retention().is_held() {
        return Some("an explicit retention hold is on it (§37.2)");
    }
    None
}

/// Wraps a prepare-time refusal in §2.3's sentence, naming the assets that survive it.
fn prepare_refusal(
    request: &PrepareRequest<'_>,
    point: FailurePoint,
    cause: &ErrorValue,
) -> ErrorValue {
    let retained: Vec<String> = request
        .retained()
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    error::prepare_failed(
        request.plan.id(),
        point.as_str(),
        cause.message(),
        &retained,
    )
    .with_metadata("cause", Value::string(cause.code().name()))
    .with_metadata("failure_point", Value::string(point.as_str()))
}

/// Everything `apply` reaches the world through (§5.6).
///
/// The request is `&mut` because §41.2's reconstruction reads what the executor wrote: each action
/// status is persisted as it settles, and the assets, the touched targets and the verification
/// results accumulate here as the plan walks §4.1.
pub struct ApplyRequest<'a> {
    prepare: PrepareRequest<'a>,
    store: &'a PlanStore,
    session: Arc<str>,
    revalidate: &'a Revalidate<'a>,
    execute: &'a Execute<'a>,
    observe: &'a Observe<'a>,
    store_failures: Vec<ErrorValue>,
    clock: Option<&'a dyn Fn() -> Timestamp>,
    resume: bool,
}

impl std::fmt::Debug for ApplyRequest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplyRequest")
            .field("plan", &self.prepare.plan.id().as_str())
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

impl<'a> ApplyRequest<'a> {
    /// Applies `plan` on behalf of `session`, at `now`.
    ///
    /// `revalidate`, `execute` and `observe` are the whole of the outside world: §7.3's recheck,
    /// §4.7's mutation and §23's observation. Nothing else in this crate reaches past them, which
    /// is what makes §54.5's injected failures a scripted answer rather than a broken machine.
    ///
    /// The nine arguments are nine facts the executor is forbidden to obtain for itself: §39.2
    /// keeps the clock a parameter, §50.1 keeps providers out of the core, and §54.5 needs every
    /// one of them replaceable by a scripted answer.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        plan: &'a ChangePlan,
        store: &'a PlanStore,
        session: impl Into<Arc<str>>,
        now: Timestamp,
        protection: &'a [ProtectionAction],
        providers: &'a ProviderRegistry,
        revalidate: &'a Revalidate<'a>,
        execute: &'a Execute<'a>,
        observe: &'a Observe<'a>,
    ) -> Self {
        Self {
            prepare: PrepareRequest::new(plan, protection, providers, now),
            store,
            session: session.into(),
            revalidate,
            execute,
            observe,
            store_failures: Vec::new(),
            clock: None,
            resume: false,
        }
    }

    /// Continues an interrupted apply rather than starting one (§41.2, §41.3).
    ///
    /// The plan may then be `applying`, `apply-failed` or `verifying` as well as sealed. Under the
    /// claim, the persisted action records decide: an action that succeeded is not run again; one
    /// left `running` or `unknown` is rerun only where its idempotency permits a blind retry, and
    /// a failed one only where its contract accepts a retry (§41.1) — otherwise the whole resume
    /// refuses with `change.resume_refused` naming the actions, and nothing runs. Preparation is
    /// not repeated once mutation began: an asset taken now would capture the half-changed state
    /// rather than the one the plan protected. Instead, the stored asset behind every required
    /// protection action is validated again through its provider, and the resume refuses with
    /// `recovery.coverage_insufficient` where one is gone or no longer validates (§4.6, §17.2).
    /// Verification follows as for a fresh apply.
    #[must_use]
    pub const fn resuming(mut self) -> Self {
        self.resume = true;
        self
    }

    /// Stamps each settled action with the instant `clock` answers when it settles (§41.2).
    ///
    /// Without one, every action carries the instant `apply` was given. That is right for a
    /// scripted run (§39.2), and wrong for a real one: the plan's own write happens after that
    /// instant, and Appendix C.4 tells the write from a later edit by when it settled.
    #[must_use]
    pub const fn stamping_with(mut self, clock: &'a dyn Fn() -> Timestamp) -> Self {
        self.clock = Some(clock);
        self
    }

    /// States what the session actually holds (§43.2, §43.3).
    #[must_use]
    pub fn with_authority(mut self, authority: Authority) -> Self {
        self.prepare.authority = authority;
        self
    }

    /// Bounds an application quiesce around asset creation (§18.4).
    #[must_use]
    pub const fn quiescing(mut self, quiescing: Quiescing<'a>) -> Self {
        self.prepare.quiesce = Some(quiescing);
        self
    }

    /// States what to do with sibling assets when preparation fails (Appendix F.1).
    #[must_use]
    pub const fn cleaning_up(mut self, decision: CleanupDecision) -> Self {
        self.prepare.cleanup = decision;
        self
    }

    /// The plan being applied.
    #[must_use]
    pub const fn plan(&self) -> &ChangePlan {
        self.prepare.plan
    }

    /// The refusals the plan store raised while §41.2's records were being written.
    #[must_use]
    pub fn store_failures(&self) -> &[ErrorValue] {
        &self.store_failures
    }
}

/// What one apply established, in every fact Appendix F distinguishes.
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyOutcome {
    state: PlanState,
    statuses: Vec<(ActionId, ActionStatus)>,
    assets: Vec<RecoveryAsset>,
    retained: Vec<RecoveryAssetId>,
    verification: Vec<VerificationResult>,
    touched: Vec<Arc<str>>,
    untouched: Vec<Arc<str>>,
    uncertain: Vec<ActionId>,
    failure_point: Option<FailurePoint>,
    error: Option<ErrorValue>,
    quiesce: Option<QuiesceReport>,
    cleanup: Option<CleanupReport>,
    shortfall: Vec<ProtectionShortfall>,
    executed: bool,
}

impl ApplyOutcome {
    /// Where §4.1 leaves the plan.
    #[must_use]
    pub const fn state(&self) -> PlanState {
        self.state
    }

    /// Whether the target system may already have been changed (Appendix F's second column).
    ///
    /// It is a fact about *this run*, not about the plan's history. A plan that already applied
    /// is refused before anything is prepared, and the refusal leaves the plan in the state it
    /// was already in — so the state alone would answer "yes, it mutated", about the earlier run.
    /// Appendix F's column asks what the operator in front of the refusal has to worry about, and
    /// [`FailurePoint::may_have_mutated`] is what answers that.
    #[must_use]
    pub const fn has_mutated(&self) -> bool {
        match self.failure_point {
            // The point itself changed nothing; whether an earlier action of the run did is the
            // answer, and only this run's executions count.
            Some(FailurePoint::ActionNotStarted) => self.executed,
            Some(point) => point.may_have_mutated(),
            None => self.state.has_mutated(),
        }
    }

    /// Every action and what is known about it (§4.7's "every action result MUST be recorded").
    #[must_use]
    pub fn statuses(&self) -> &[(ActionId, ActionStatus)] {
        &self.statuses
    }

    /// What is known about one action.
    #[must_use]
    pub fn status_of(&self, action: &ActionId) -> Option<ActionStatus> {
        self.statuses
            .iter()
            .find(|(id, _)| id == action)
            .map(|(_, status)| *status)
    }

    /// The recovery assets this apply created (§4.5).
    #[must_use]
    pub fn assets(&self) -> &[RecoveryAsset] {
        &self.assets
    }

    /// The assets that still exist, which Appendix F requires a failure to surface.
    #[must_use]
    pub fn retained_assets(&self) -> &[RecoveryAssetId] {
        &self.retained
    }

    /// The verification results (§23.3).
    #[must_use]
    pub fn verification(&self) -> &[VerificationResult] {
        &self.verification
    }

    /// The targets a mutating action actually ran against (§28.6, §55.10 case 43).
    #[must_use]
    pub fn touched_targets(&self) -> &[Arc<str>] {
        &self.touched
    }

    /// The targets nothing ran against.
    #[must_use]
    pub fn untouched_targets(&self) -> &[Arc<str>] {
        &self.untouched
    }

    /// The actions whose outcome could not be established (Appendix F.2).
    ///
    /// Appendix F.2 calls this an uncertainty boundary, and recovery planning has to treat it as
    /// one: an action here is neither a thing that happened nor a thing that did not.
    #[must_use]
    pub fn uncertainty_boundary(&self) -> &[ActionId] {
        &self.uncertain
    }

    /// Whether any action's outcome is unestablished (Appendix F.2).
    #[must_use]
    pub fn is_outcome_unknown(&self) -> bool {
        !self.uncertain.is_empty()
    }

    /// Which of Appendix F's rows this outcome is.
    #[must_use]
    pub const fn failure_point(&self) -> Option<FailurePoint> {
        self.failure_point
    }

    /// The structured refusal, where there is one.
    #[must_use]
    pub const fn error(&self) -> Option<&ErrorValue> {
        self.error.as_ref()
    }

    /// Whether the apply reached its intended end.
    #[must_use]
    pub fn is_success(&self) -> bool {
        // A refusal is never a success, whatever state it leaves the plan in. Applying a plan
        // that already verified is refused *at* `verified`, and reading the state alone would
        // report the earlier run's success as this run's — §2.14 and §62.9's exact mistake, one
        // layer up.
        self.error.is_none()
            && matches!(
                self.state,
                PlanState::Verified | PlanState::Recovered | PlanState::RecoveryVerified
            )
    }

    /// Whether a `RecoveryPlan` may be built from here (§24.1, Appendix F's middle-action row).
    ///
    /// Appendix F says "do not blindly reverse; offer a RecoveryPlan", and both halves are load
    /// bearing: the executor reverses nothing on its own, and it says that recovery is available
    /// rather than performing it. §5.8 keeps building the plan a separate, inspectable step.
    #[must_use]
    pub const fn offers_recovery(&self) -> bool {
        self.state.is_recoverable()
    }

    /// What happened to the quiesce window (§18.4).
    #[must_use]
    pub const fn quiesce(&self) -> Option<&QuiesceReport> {
        self.quiesce.as_ref()
    }

    /// What cleanup did after a failed preparation (Appendix F.1).
    #[must_use]
    pub const fn cleanup(&self) -> Option<&CleanupReport> {
        self.cleanup.as_ref()
    }

    /// The optional protection that did not become protection (§17.2), which the operator is
    /// told about rather than left to infer from a shorter asset list.
    #[must_use]
    pub fn protection_shortfall(&self) -> &[ProtectionShortfall] {
        &self.shortfall
    }
}

/// Runs §5.6's commitment point: claim, revalidate, check, gate, prepare, mutate, verify.
///
/// Every step before preparation refuses with the plan untouched and unprepared, which is what
/// Appendix F's first three rows require; preparation may leave assets behind and never a changed
/// target (§2.3); and from the first mutating action onwards the outcome says what ran.
///
/// §4.1 and §41.2: the state the plan reaches is durable, so `apply` on an applied plan is a
/// refusal rather than a second mutation, and a shell that stopped in the middle is found in the
/// state it stopped in. Every durable write happens under the claim: a session refused at the
/// claim writes nothing, because the state it holds may be older than the one the store holds.
#[must_use]
pub fn apply(request: &mut ApplyRequest<'_>) -> ApplyOutcome {
    let plan = request.prepare.plan;
    let now = request.prepare.now;
    let resume = request.resume;

    // §42.4 before §4.1: a plan another session is applying reads `applying` here, and only the
    // claim below can say which session that is. The durable state is checked again under it.
    let in_flight = matches!(
        plan.state(),
        PlanState::Preparing | PlanState::Applying | PlanState::Verifying
    );
    if (!in_flight || resume)
        && let Some(refusal) = appliability(plan, plan.state(), now, resume)
    {
        return refused(plan, FailurePoint::PlanState, refusal);
    }

    // §4.4: the seal is what makes a sealed plan immutable. A plan whose content no longer matches
    // its recorded digest was changed after the operator approved it — in memory or in the store —
    // and none of it runs.
    if !plan.digest_holds() {
        return refused(
            plan,
            FailurePoint::PlanState,
            error::store_corrupt(&format!(
                "plan {} does not match the digest it was sealed with, so it is not the plan that \
                 was approved (§4.4). Nothing was changed; `rebase plan {}` seals it again from \
                 the world as it is",
                plan.id().short(),
                plan.id().short()
            )),
        );
    }

    // 1. §42.4: two sessions MUST NOT apply one sealed plan at once. The claim is held for the
    //    whole of this function and released on the way out, including on an early return (§42.3).
    let mut claim = match request.store.claim(plan.id(), &request.session, now) {
        Ok(claim) => claim,
        Err(refusal) => return refused(plan, FailurePoint::Claim, refusal),
    };

    // §42.4 again, under the claim: the copy of the plan this session holds may have been read
    // before another session applied it. The store is the evidence, and nobody can change it
    // while the claim is held.
    let (durable, recorded) = match durable_evidence(plan, request.store) {
        Ok(evidence) => evidence,
        Err(refusal) => return refused(plan, FailurePoint::PlanState, refusal),
    };
    if let Some(refusal) = appliability(plan, durable, now, resume) {
        return refused_on_evidence(plan, FailurePoint::PlanState, refusal, durable, &recorded);
    }
    let began = mutation_began(plan, &recorded);
    if began && !resume {
        // §2.7 and §41.2: the state write never landed, and an action record did. The plan reads
        // sealed and may already have changed the world, so it is resumed rather than rerun.
        let refusal = error::plan_not_sealed(plan.id(), PlanState::Applying).with_help(
            "v0.6 §41.2: the persisted action records say this plan already began executing. \
             `resume plan` decides, action by action, what may continue"
                .to_owned(),
        );
        return refused_on_evidence(plan, FailurePoint::PlanState, refusal, durable, &recorded);
    }
    // §41.3, under the claim: the records decide which actions may continue, and a plan with any
    // action that may not is refused whole rather than resumed around it.
    let prior = if resume {
        let decision = crate::resume::resume(plan, request.store, now);
        if let Some(refusal) = decision.refusal() {
            return refused_on_evidence(
                plan,
                FailurePoint::PlanState,
                refusal.clone(),
                durable,
                &recorded,
            );
        }
        recorded.clone()
    } else {
        BTreeMap::new()
    };

    // 2. §7.3: revalidate before anything is prepared. Material drift and an unanswerable check
    //    both stop the apply, because §2.4 forbids promoting unknown to expected.
    if let Some(refusal) = revalidate_plan(plan, request.revalidate, &prior) {
        return refused(plan, FailurePoint::TargetRevalidation, refusal);
    }

    // 3. §43.2, §43.3: capability and privilege, before prepare.
    if let Some(refusal) =
        check_authority(plan, &request.prepare.authority, request.prepare.protection)
    {
        return refused(plan, FailurePoint::PrivilegeCheck, refusal);
    }
    if let Some(refusal) = check_discovery(plan, request.prepare.protection) {
        return refused(plan, FailurePoint::RecoveryDiscovery, refusal);
    }

    // 4. §19.4: an outstanding acknowledgement refuses before prepare. §40.3 makes this the whole
    //    of a script's gate: nothing here prompts, so a missing flag is a refusal.
    if let Some(refusal) = check_gates(plan) {
        return refused(plan, FailurePoint::Gate, refusal);
    }

    // 5. §4.5: create and validate the recovery assets the policy requires. The state is written
    //    before the first asset, because §2.3's rule — mutation MUST NOT begin when a required
    //    asset could not be created — is only checkable afterwards if the store says preparation
    //    had begun. A resumed plan that already began mutating is past PREPARE: its protection is
    //    the one taken before the first change, and a second one would capture the half-changed
    //    state.
    let prepared = if began {
        // §4.6 and §17.2 on resume: the protection taken before the first change is what this
        // plan rests on, so it is checked again rather than assumed — and never retaken.
        match reestablish_protection(request) {
            Ok(prepared) => Ok(prepared),
            Err(refusal) => {
                return refused_on_evidence(
                    plan,
                    FailurePoint::RecoveryValidation,
                    refusal,
                    durable,
                    &recorded,
                );
            }
        }
    } else if request.prepare.protection.is_empty() {
        Ok(Prepared::nothing())
    } else {
        write_state(request, PlanState::Preparing);
        prepare(&mut request.prepare)
    };
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(refusal) => {
            let point = request
                .prepare
                .failure_point
                .unwrap_or(FailurePoint::RecoveryAssetCreation);
            let mut outcome = refused(plan, point, refusal);
            outcome.state = PlanState::PrepareFailed;
            outcome.assets = request.prepare.created.clone();
            outcome.retained = request.prepare.retained();
            outcome.quiesce = request.prepare.quiesce_report.clone();
            outcome.cleanup = request.prepare.cleanup_report.clone();
            outcome.shortfall = request.prepare.shortfall.clone();
            outcome.untouched = target_labels(plan);
            let created = request.prepare.created.clone();
            persist_assets(request, &created);
            write_state(request, outcome.state);
            drop(claim);
            return outcome;
        }
    };

    // 6. §4.7: the mutating actions, in dependency order, by the plan's strategy. `applying` is
    //    durable before the first one runs, so a shell killed between here and the end is found
    //    as a plan that may have mutated rather than as one that never started (§41.1, F.2).
    // §41.2: the protection a resume will rest on is durable before the first change, because
    // a crash between here and the end leaves nothing else to find it by.
    let kept: Vec<RecoveryAsset> = prepared
        .assets
        .iter()
        .cloned()
        .chain(
            prepared
                .shortfall
                .iter()
                .filter_map(|shortfall| shortfall.asset.clone()),
        )
        .collect();
    persist_assets(request, &kept);
    write_state(request, PlanState::Applying);
    let (outcome, held) = mutate(request, &prepared, &prior, &mut claim);
    // §42.4: a session that lost the claim to another writes nothing over what that one holds.
    if held {
        write_state(request, outcome.state);
    }
    drop(claim);
    outcome
}

/// Persists the assets this apply created, attributed to the plan (§11.1, §41.2).
fn persist_assets(request: &mut ApplyRequest<'_>, assets: &[RecoveryAsset]) {
    let plan = request.prepare.plan;
    for asset in assets {
        let attributed = match asset.source_plan() {
            Some(_) => asset.clone(),
            None => asset.clone().for_plan(plan.id().clone()),
        };
        if let Err(failure) = request.store.put_asset(&attributed) {
            request.store_failures.push(failure);
        }
    }
}

/// Checks, through its provider, the stored asset behind every required protection action of a
/// resumed plan (§4.6, §17.2, §41.3).
///
/// A fresh asset is never taken instead: after the first change it would capture the half-changed
/// state, which is not the state the plan protected. An asset that is gone, unrecorded, or no
/// longer validates — or whose provider is not here to say — is protection the plan no longer
/// has, and `require`'s refusal is the answer.
fn reestablish_protection(request: &ApplyRequest<'_>) -> Result<Prepared, ErrorValue> {
    let plan = request.prepare.plan;
    let required: Vec<&ProtectionAction> = request
        .prepare
        .protection
        .iter()
        .filter(|action| requires_protection(plan, action))
        .collect();
    if required.is_empty() {
        return Ok(Prepared::nothing());
    }
    // An asset the store cannot decode is one this resume cannot rest on, the same as a missing
    // one; the refusal below names the domain either way.
    let stored: Vec<RecoveryAsset> = request
        .store
        .assets_for(plan.id())?
        .iter()
        .filter_map(|id| request.store.get_asset(id).ok())
        .collect();
    let mut assets = Vec::new();
    let mut missing = Vec::new();
    for action in required {
        let domain = action.proposed_asset().scope().domain();
        let Some(asset) = stored.iter().find(|asset| {
            asset.provider() == action.provider() && asset.scope().domain() == domain
        }) else {
            missing.push(format!(
                "{domain}: no recovery asset from the first run is recorded"
            ));
            continue;
        };
        let Some(provider) = request.prepare.providers.get(asset.provider()) else {
            missing.push(format!(
                "{domain}: {} is not registered, so {} cannot be checked",
                asset.provider(),
                asset.id().as_str()
            ));
            continue;
        };
        match provider.validate(asset) {
            Ok(validation) => {
                let checked = asset.clone().validated(validation);
                if checked.is_usable() {
                    assets.push(checked);
                } else {
                    missing.push(format!(
                        "{domain}: {} no longer validates",
                        asset.id().as_str()
                    ));
                }
            }
            Err(refusal) => missing.push(format!("{domain}: {}", refusal.message())),
        }
    }
    if missing.is_empty() {
        return Ok(Prepared {
            assets,
            shortfall: Vec::new(),
            quiesce: None,
        });
    }
    Err(error::coverage_insufficient(
        plan.id(),
        plan.protection().level(),
        ProtectionLevel::Protected,
        &missing,
    )
    .with_help(
        "v0.6 §4.6, §17.2 and §41.3: the protection this plan was applied under cannot be \
         established again, so it does not go on mutating. A new asset now would capture the \
         half-changed state; a recovery or rebase decision is required"
            .to_owned(),
    ))
}

/// A refusal made after reading the store, reporting what the store holds rather than `pending`.
///
/// The plan is refused in the state the store has it in, and every action carries its persisted
/// record: an operator told "nothing ran" about an action recorded `running` would be told the
/// one thing Appendix F.2 forbids guessing.
fn refused_on_evidence(
    plan: &ChangePlan,
    point: FailurePoint,
    refusal: ErrorValue,
    durable: PlanState,
    recorded: &BTreeMap<String, ActionStatus>,
) -> ApplyOutcome {
    let mut outcome = refused(plan, point, refusal);
    outcome.state = durable;
    for (id, status) in &mut outcome.statuses {
        if let Some(record) = recorded.get(id.as_str()) {
            *status = *record;
        }
    }
    outcome.uncertain = outcome
        .statuses
        .iter()
        .filter(|(_, status)| matches!(status, ActionStatus::Unknown | ActionStatus::Running))
        .map(|(id, _)| id.clone())
        .collect();
    outcome
}

/// Writes a §4.1 transition, keeping a refusal the store raised beside the outcome (§41.2).
fn write_state(request: &mut ApplyRequest<'_>, state: PlanState) {
    let plan = request.prepare.plan;
    if let Err(failure) = request
        .store
        .record_state(plan.id(), plan.revision(), state)
    {
        request.store_failures.push(failure);
    }
}

/// What the store holds about this plan revision: its durable state and its action records.
fn durable_evidence(
    plan: &ChangePlan,
    store: &PlanStore,
) -> Result<(PlanState, BTreeMap<String, ActionStatus>), ErrorValue> {
    let state = store.get_revision(plan.id(), plan.revision())?.state();
    let recorded = store.action_statuses(plan.id(), plan.revision())?;
    Ok((state, recorded))
}

/// Whether a persisted record says a mutating action may already have changed the system.
fn mutation_began(plan: &ChangePlan, recorded: &BTreeMap<String, ActionStatus>) -> bool {
    plan.actions()
        .iter()
        .filter(|action| action.role().mutates_target())
        .any(|action| {
            recorded
                .get(action.id().as_str())
                .is_some_and(|status| status.may_have_mutated())
        })
}

/// §5.6: `apply` MUST refuse drafts and expired plans; §41.3 lets `resume` pick up the states an
/// interrupted apply leaves behind, and nothing past a verdict.
fn appliability(
    plan: &ChangePlan,
    state: PlanState,
    now: Timestamp,
    resume: bool,
) -> Option<ErrorValue> {
    if plan.is_expired_at(now) || state == PlanState::Expired {
        return Some(error::plan_expired(plan.id()));
    }
    if !resume {
        return (!state.is_appliable()).then(|| error::plan_not_sealed(plan.id(), state));
    }
    let resumable = state.is_appliable()
        || matches!(
            state,
            PlanState::Preparing
                | PlanState::Applying
                | PlanState::ApplyFailed
                | PlanState::Verifying
        );
    if resumable {
        return None;
    }
    if state.is_terminal() || state.is_verdict() {
        return Some(error::resume_complete(plan.id(), state));
    }
    Some(error::plan_not_sealed(plan.id(), state))
}

/// §7.3: material drift, or a precondition nobody could check, stops the apply before prepare.
///
/// An action a resumed apply already completed is not rechecked: its target changed because the
/// action changed it, and reading that as drift would refuse every resume (§41.3).
fn revalidate_plan(
    plan: &ChangePlan,
    revalidate: &Revalidate<'_>,
    prior: &BTreeMap<String, ActionStatus>,
) -> Option<ErrorValue> {
    let mut blocking: Vec<(String, String, String)> = Vec::new();
    let mut vanished: Option<String> = None;
    for action in plan
        .actions()
        .iter()
        .filter(|action| prior.get(action.id().as_str()).copied() != Some(ActionStatus::Succeeded))
    {
        match revalidate(action) {
            Ok(findings) => {
                for finding in &findings {
                    if finding.verdict().blocks_apply() {
                        if finding.kind() == ono_change_core::PreconditionKind::Existence
                            && finding.verdict() == DriftVerdict::Material
                            && vanished.is_none()
                        {
                            vanished = Some(finding.subject().to_owned());
                        }
                        blocking.push(drift_row(finding));
                    }
                }
            }
            // A check that could not be made has not held, and §7.3 treats it as drift.
            Err(refusal) => blocking.push((
                action.target().unwrap_or(action.summary()).to_owned(),
                action.id().as_str().to_owned(),
                refusal.message().to_owned(),
            )),
        }
    }
    // §7.3: a target that is gone, or is no longer the object the plan froze, is refused as the
    // changed target it is — the rest of the drift rides along on the metadata.
    if let Some(subject) = vanished {
        return Some(
            error::target_changed(
                &subject,
                "it no longer exists, or is no longer the object the plan was sealed against; \
                 nothing was changed",
            )
            .with_metadata(
                "drift",
                ono_value::Value::list(blocking.iter().map(|(subject, field, detail)| {
                    ono_value::Value::string(&format!("{subject}.{field}: {detail}"))
                })),
            ),
        );
    }
    (!blocking.is_empty()).then(|| error::drift_detected(plan.id(), &blocking))
}

/// The `(subject, field, detail)` triple `change.plan_drift_detected` renders.
fn drift_row(finding: &DriftFinding) -> (String, String, String) {
    let detail = match finding.verdict() {
        DriftVerdict::Material => "changed materially since the plan was sealed".to_owned(),
        DriftVerdict::Unknown => "could not be observed, which is not a pass (§2.4)".to_owned(),
        DriftVerdict::Tolerated => "changed within the contract's declared tolerance".to_owned(),
        DriftVerdict::Unchanged => "still holds".to_owned(),
    };
    (
        finding.subject().to_owned(),
        finding.field().to_owned(),
        detail,
    )
}

/// §43.2 and §43.3: what applying needs, checked before anything is prepared.
fn check_authority(
    plan: &ChangePlan,
    authority: &Authority,
    protection: &[ProtectionAction],
) -> Option<ErrorValue> {
    if plan.mutates() && !authority.has_change(ChangeCapability::ActionExecute) {
        return Some(error::capability_missing(
            plan.id().as_str(),
            ChangeCapability::ActionExecute.as_str(),
            plan.kind() == PlanKind::Recovery,
        ));
    }
    if let Some(action) = plan
        .actions()
        .iter()
        .find(|action| action.needs_privilege() && !authority.is_elevated())
    {
        return Some(error::privilege_required(
            action.summary(),
            "elevated privilege",
            plan.kind() == PlanKind::Recovery,
        ));
    }
    // §43.2: "applying requires target action capabilities plus protection provider capabilities."
    if protection
        .iter()
        .any(|action| requires_protection(plan, action))
        && !authority.has_recovery(RecoveryCapability::Prepare)
    {
        return Some(error::capability_missing(
            plan.id().as_str(),
            RecoveryCapability::Prepare.as_str(),
            false,
        ));
    }
    None
}

/// §17.2's `require`: a policy that demands protection refuses when discovery found none.
fn check_discovery(plan: &ChangePlan, protection: &[ProtectionAction]) -> Option<ErrorValue> {
    if !plan.protection_mode().refuses_shortfall() || !plan.mutates() {
        return None;
    }
    let shortfall: Vec<String> = plan
        .protection()
        .shortfall()
        .iter()
        .map(|row| row.domain().as_str().to_owned())
        .collect();
    let protected = protection
        .iter()
        .any(|action| requires_protection(plan, action));
    if protected && shortfall.is_empty() {
        return None;
    }
    Some(error::coverage_insufficient(
        plan.id(),
        plan.protection().level(),
        ProtectionLevel::Protected,
        &shortfall,
    ))
}

/// §19.4 and §40.2: the acknowledgements the sealed revision must already carry.
fn check_gates(plan: &ChangePlan) -> Option<ErrorValue> {
    use ono_change_core::RequiredAcknowledgement;
    let outstanding = plan.outstanding_acknowledgements();
    let irreversible = outstanding
        .iter()
        .any(|needed| matches!(needed, RequiredAcknowledgement::Irreversible));
    if irreversible {
        let reasons: Vec<String> = plan
            .risk()
            .findings()
            .iter()
            .map(|finding| finding.reason().to_owned())
            .collect();
        return Some(error::irreversible_not_accepted(plan.id(), &reasons));
    }
    let risk = outstanding.iter().find_map(|needed| match needed {
        RequiredAcknowledgement::Risk(class) => Some(*class),
        RequiredAcknowledgement::Irreversible => None,
    })?;
    let reasons: Vec<String> = plan
        .risk()
        .leading()
        .iter()
        .map(|finding| finding.reason().to_owned())
        .collect();
    Some(error::risk_not_accepted(plan.id(), risk.as_str(), &reasons))
}

/// The label a target is known by in an outcome.
fn target_labels(plan: &ChangePlan) -> Vec<Arc<str>> {
    plan.targets()
        .iter()
        .map(|target| Arc::from(target.identity()))
        .collect()
}

/// An outcome for a refusal that left the plan where it was.
fn refused(plan: &ChangePlan, point: FailurePoint, error: ErrorValue) -> ApplyOutcome {
    ApplyOutcome {
        state: plan.state(),
        statuses: plan
            .actions()
            .iter()
            .map(|action| (action.id().clone(), ActionStatus::Pending))
            .collect(),
        assets: Vec::new(),
        retained: Vec::new(),
        verification: Vec::new(),
        touched: Vec::new(),
        untouched: target_labels(plan),
        uncertain: Vec::new(),
        failure_point: Some(point),
        error: Some(error),
        quiesce: None,
        cleanup: None,
        shortfall: Vec::new(),
        executed: false,
    }
}

/// §4.7: the mutating actions, in dependency order, by the plan's strategy, then §4.8.
///
/// `prior` holds what a resumed apply found persisted (§41.2): an action it records as succeeded
/// is reported as such and not run again, and every other action starts from its record.
fn mutate(
    request: &mut ApplyRequest<'_>,
    prepared: &Prepared,
    prior: &BTreeMap<String, ActionStatus>,
    claim: &mut Claim<'_>,
) -> (ApplyOutcome, bool) {
    // §42.3: the claim is a lease, renewed before each action, so an apply longer than the lease
    // is still this session's. A renewal another session refused means the plan is not ours.
    let claim = RefCell::new(claim);
    let lost = Cell::new(false);
    let executed = Cell::new(false);
    let plan = request.prepare.plan;
    let now = request.prepare.now;
    let clock = request.clock;
    let stamp = || clock.map_or(now, |clock| clock());
    let order = match topological_order(plan.actions()) {
        Ok(order) => order,
        Err(cycle) => {
            let mut outcome = refused(
                plan,
                FailurePoint::PlanState,
                error::action_graph_cyclic(plan.id(), &cycle),
            );
            outcome.assets = prepared.assets.to_vec();
            return (outcome, true);
        }
    };

    let store_failures: RefCell<Vec<ErrorValue>> = RefCell::new(Vec::new());
    // §17.1 puts the protection actions in the plan, and §4.5 has already run them by the time
    // `mutate` starts. An asset whose scope names this action's target is the asset it planned,
    // so the action is settled here rather than left `pending` beside a recovery point that
    // demonstrably exists — Appendix E's PREPARE line reads these, and a plan that protected
    // itself and says `pending` is describing the wrong run.
    let prepared_domains: Vec<&str> = prepared
        .assets
        .iter()
        .map(|asset| asset.scope().domain())
        .collect();
    let statuses: RefCell<Vec<(ActionId, ActionStatus)>> = RefCell::new(
        plan.actions()
            .iter()
            .map(|action| {
                let settled = action.role() == ActionRole::Prepare
                    && action
                        .target()
                        .is_some_and(|target| prepared_domains.contains(&target));
                let recorded = prior.get(action.id().as_str()).copied();
                (
                    action.id().clone(),
                    if settled {
                        ActionStatus::Succeeded
                    } else {
                        recorded.unwrap_or(ActionStatus::Pending)
                    },
                )
            })
            .collect(),
    );
    for action in plan.actions() {
        if action.role() == ActionRole::Prepare
            && action
                .target()
                .is_some_and(|target| prepared_domains.contains(&target))
            && let Err(failure) = request.store.record_action_status(
                plan.id(),
                plan.revision(),
                action.id(),
                action.ordinal(),
                ActionStatus::Succeeded,
                stamp(),
                None,
            )
        {
            store_failures.borrow_mut().push(failure);
        }
    }
    let stop: RefCell<Option<(FailurePoint, ErrorValue, ActionId)>> = RefCell::new(None);
    let gate_results: RefCell<Vec<VerificationResult>> = RefCell::new(Vec::new());
    let mutating: Vec<&PlanAction> = order
        .iter()
        .map(|index| &plan.actions()[*index])
        .filter(|action| action.role().mutates_target())
        .collect();
    let recovery = plan.kind() == PlanKind::Recovery;

    let record = |action: &PlanAction, status: ActionStatus, detail: Option<&str>| {
        // §4.7: "every action result MUST be recorded independently", and §41.2 reads those
        // records back after a crash, so they are written as each action settles.
        if let Err(failure) = request.store.record_action_status(
            plan.id(),
            plan.revision(),
            action.id(),
            action.ordinal(),
            status,
            stamp(),
            detail,
        ) {
            store_failures.borrow_mut().push(failure);
        }
        let mut statuses = statuses.borrow_mut();
        if let Some(entry) = statuses.iter_mut().find(|(id, _)| id == action.id()) {
            entry.1 = status;
        }
    };

    let submit = |_wave: &ono_change_core::Wave, targets: &[Arc<str>]| -> Vec<TargetResult> {
        let mut results = Vec::new();
        for target in targets {
            if stop.borrow().is_some() {
                break;
            }
            let mut status = ActionStatus::Skipped;
            let mut failure_seen = None;
            for action in mutating
                .iter()
                .filter(|action| action_covers(action, target, plan))
            {
                if stop.borrow().is_some() {
                    break;
                }
                if prior.get(action.id().as_str()).copied() == Some(ActionStatus::Succeeded) {
                    // §41.3: a resumed apply continues after what already succeeded.
                    status = ActionStatus::Succeeded;
                    continue;
                }
                // §42.3: renewed before the action, so the claim cannot lapse under it. §41.2 and
                // Appendix F.2: `running` is durable before the action starts, so a shell killed
                // mid-action leaves an in-flight record rather than `pending` — the one resume
                // would rerun blindly. Where either could not be done, the action is not started.
                let started = claim.borrow_mut().renew(stamp()).and_then(|()| {
                    request.store.record_action_status(
                        plan.id(),
                        plan.revision(),
                        action.id(),
                        action.ordinal(),
                        ActionStatus::Running,
                        stamp(),
                        None,
                    )
                });
                if let Err(failure) = started {
                    if failure.code().name() == "change.plan_already_applying" {
                        lost.set(true);
                    } else {
                        store_failures.borrow_mut().push(failure.clone());
                    }
                    status = ActionStatus::Skipped;
                    failure_seen = Some(failure.clone());
                    *stop.borrow_mut() =
                        Some((FailurePoint::ActionNotStarted, failure, action.id().clone()));
                    break;
                }
                if let Some(entry) = statuses
                    .borrow_mut()
                    .iter_mut()
                    .find(|(id, _)| id == action.id())
                {
                    entry.1 = ActionStatus::Running;
                }
                executed.set(true);
                let outcome = (request.execute)(action);
                let settled = outcome.status();
                record(action, settled, outcome.error().map(ErrorValue::message));
                status = settled;
                match &outcome {
                    ExecutionOutcome::Succeeded => {}
                    ExecutionOutcome::Failed(refusal) => {
                        let point = if statuses
                            .borrow()
                            .iter()
                            .any(|(_, seen)| *seen == ActionStatus::Succeeded)
                        {
                            FailurePoint::MiddleMutateAction
                        } else {
                            FailurePoint::FirstMutateAction
                        };
                        failure_seen = Some(refusal.clone());
                        *stop.borrow_mut() = Some((point, refusal.clone(), action.id().clone()));
                    }
                    ExecutionOutcome::Unknown(refusal) => {
                        let point = if is_remote(action, plan) {
                            FailurePoint::RemoteDisconnect
                        } else {
                            FailurePoint::UnknownOutcome
                        };
                        failure_seen = Some(refusal.clone());
                        *stop.borrow_mut() = Some((point, refusal.clone(), action.id().clone()));
                    }
                }
            }
            results.push(TargetResult::new(Arc::clone(target), status, failure_seen));
        }
        results
    };

    let every_target = target_labels(plan);
    let reached = std::cell::RefCell::new(Vec::<Arc<str>>::new());
    let gate = |_wave: &ono_change_core::Wave, targets: &[Arc<str>]| -> Verdict {
        // §28.6: for a canary strategy the plan's *required* verification must pass before the
        // remaining batches continue. Advisory checks say nothing about whether to go on, and a
        // check about a target no wave has reached yet says nothing about the batch that ran:
        // it would fail for the one reason the batches exist, that the rest has not changed yet.
        reached.borrow_mut().extend(targets.iter().cloned());
        let reached = reached.borrow();
        let required: Vec<&VerificationContract> = plan
            .verification()
            .contracts()
            .iter()
            .filter(|contract| contract.class() == VerificationClass::Required)
            .filter(|contract| about_reached(contract.subject(), &every_target, &reached))
            .collect();
        let mut results = Vec::new();
        for contract in required {
            results.push(observe_one(plan, contract, request.observe, now));
        }
        let verdict = ono_change_core::VerificationSet::verdict(&results);
        gate_results.borrow_mut().extend(results);
        verdict
    };

    let run = run_waves(plan.strategy(), &target_labels(plan), &submit, &gate);

    let mut statuses = statuses.into_inner();
    let stopped = stop.into_inner();
    request.store_failures.extend(store_failures.into_inner());

    // Appendix F: "stop the dependency chain, preserve evidence". Everything downstream of the
    // action that stopped is SKIPPED, which is a different fact from FAILED.
    if let Some((_, _, failed)) = &stopped {
        let blocked = dependents_of(plan.actions(), failed);
        for (id, status) in &mut statuses {
            if *status == ActionStatus::Pending && blocked.contains(id) {
                *status = ActionStatus::Skipped;
            }
        }
    }

    let uncertain: Vec<ActionId> = statuses
        .iter()
        .filter(|(_, status)| *status == ActionStatus::Unknown)
        .map(|(id, _)| id.clone())
        .collect();
    // Every asset this apply created is reported and retained, the ones that did not validate
    // included: they exist, and §4.6 only forbids calling them protection.
    let assets: Vec<RecoveryAsset> = prepared
        .assets
        .iter()
        .cloned()
        .chain(
            prepared
                .shortfall
                .iter()
                .filter_map(|shortfall| shortfall.asset.clone()),
        )
        .collect();
    let retained = assets.iter().map(|asset| asset.id().clone()).collect();

    let mut outcome = ApplyOutcome {
        state: PlanState::Applying,
        statuses,
        assets,
        retained,
        verification: gate_results.into_inner(),
        touched: run.touched().to_vec(),
        untouched: run.untouched().to_vec(),
        uncertain,
        failure_point: None,
        error: None,
        quiesce: request.prepare.quiesce_report.clone(),
        cleanup: request.prepare.cleanup_report.clone(),
        shortfall: prepared.shortfall.clone(),
        executed: executed.get(),
    };

    if let Some((FailurePoint::ActionNotStarted, refusal, unstarted)) = &stopped {
        // Nothing ran at this point. The plan is part-applied where an earlier action may have
        // changed something — this run's or a resumed one's — and otherwise it is still what it
        // was before `applying` was written: protected where assets exist, sealed where not.
        let began = mutating.iter().any(|action| {
            outcome
                .status_of(action.id())
                .is_some_and(ActionStatus::may_have_mutated)
        });
        outcome.failure_point = Some(FailurePoint::ActionNotStarted);
        outcome.state = if began && recovery {
            PlanState::RecoveryFailed
        } else if began {
            PlanState::ApplyFailed
        } else if !prepared.assets.is_empty() {
            PlanState::Protected
        } else if plan.state().is_appliable() {
            plan.state()
        } else {
            PlanState::Sealed
        };
        outcome.error = Some(
            refusal
                .clone()
                .with_metadata(
                    "failure_point",
                    Value::string(FailurePoint::ActionNotStarted.as_str()),
                )
                .with_metadata("not_started", Value::string(unstarted.as_str())),
        );
        return (outcome, !lost.get());
    }

    if let Some((point, refusal, _)) = stopped {
        outcome.failure_point = Some(point);
        outcome.state = if matches!(
            point,
            FailurePoint::RemoteDisconnect | FailurePoint::UnknownOutcome
        ) {
            // Appendix F.2: an unestablished outcome is not resolved by a state word. The plan
            // stays APPLYING and the uncertainty boundary travels with it.
            PlanState::Applying
        } else if recovery {
            PlanState::RecoveryFailed
        } else {
            PlanState::ApplyFailed
        };
        outcome.error = Some(apply_refusal(plan, point, &refusal, &outcome, recovery));
        return (outcome, !lost.get());
    }

    if let Some(verdict) = run.gate_verdict()
        && verdict != Verdict::Verified
    {
        // §28.6 and §55.10 case 43: the canary's required verification did not pass, so the rest
        // never ran and the outcome says exactly which targets were touched.
        outcome.failure_point = Some(FailurePoint::RequiredVerification);
        outcome.state = if recovery {
            PlanState::RecoveryFailed
        } else {
            match verdict {
                Verdict::Failed => PlanState::Failed,
                _ => PlanState::Degraded,
            }
        };
        outcome.error = Some(verification_refusal(plan, &outcome.verification, recovery));
        return (outcome, !lost.get());
    }

    // 7. §4.8: verification starts after mutation completes.
    let verification = verify(&VerifyRequest {
        plan,
        now,
        observe: request.observe,
    });
    outcome.verification = verification.results.clone();
    outcome.state = verification.state_for(recovery);
    outcome.failure_point = verification.failure_point;
    outcome.error = verification.error;
    (outcome, !lost.get())
}

/// Whether `action` is one of the mutating actions this target's wave should run.
///
/// An action that names no target belongs to the first wave: a plan-wide step has no other place
/// to be, and §28.2 froze the membership, so the first wave is the one that certainly exists.
fn action_covers(action: &PlanAction, target: &str, plan: &ChangePlan) -> bool {
    match action.target() {
        Some(named) => named == target,
        None => plan
            .targets()
            .first()
            .is_some_and(|first| first.identity() == target),
    }
}

/// Whether a contract about `subject` belongs to a batch that has run (§28.6).
///
/// A contract names its target as `<word> <identity>` or as the identity itself. One that names a
/// target no wave has reached waits for the verification after the last wave; one that names no
/// target of the plan is plan-wide and holds at every gate.
fn about_reached(subject: &str, targets: &[Arc<str>], reached: &[Arc<str>]) -> bool {
    let names =
        |target: &Arc<str>| subject == target.as_ref() || subject.ends_with(&format!(" {target}"));
    !targets.iter().any(names) || reached.iter().any(names)
}

/// Whether the action's frozen target lives on another host (§7.1, §29.3).
fn is_remote(action: &PlanAction, plan: &ChangePlan) -> bool {
    action.target().is_some_and(|identity| {
        plan.targets()
            .iter()
            .any(|target| target.identity() == identity && target.host().is_some())
    })
}

/// Everything that transitively depends on `failed`, which Appendix F stops rather than runs.
fn dependents_of(actions: &[PlanAction], failed: &ActionId) -> Vec<ActionId> {
    let mut blocked: Vec<ActionId> = vec![failed.clone()];
    let mut grew = true;
    while grew {
        grew = false;
        for action in actions {
            if blocked.contains(action.id()) {
                continue;
            }
            if action
                .depends_on()
                .iter()
                .any(|dependency| blocked.contains(dependency))
            {
                blocked.push(action.id().clone());
                grew = true;
            }
        }
    }
    blocked.retain(|id| id != failed);
    blocked
}

/// Appendix F's sentence for a mutating action that failed, or for one nobody could resolve.
fn apply_refusal(
    plan: &ChangePlan,
    point: FailurePoint,
    cause: &ErrorValue,
    outcome: &ApplyOutcome,
    recovery: bool,
) -> ErrorValue {
    let completed = outcome
        .statuses
        .iter()
        .filter(|(_, status)| *status == ActionStatus::Succeeded)
        .count();
    let not_executed = outcome
        .statuses
        .iter()
        .filter(|(_, status)| !status.is_settled())
        .count();
    let base = match point {
        FailurePoint::RemoteDisconnect | FailurePoint::UnknownOutcome => {
            error::remote_state_unknown(
                outcome
                    .untouched
                    .first()
                    .map_or("this host", |target| target.as_ref()),
                cause.message(),
            )
        }
        _ if recovery => error::recovery_apply_failed(point.as_str(), cause.message()),
        _ => error::apply_failed(plan.id(), point.as_str(), completed, not_executed),
    };
    base.with_metadata("failure_point", Value::string(point.as_str()))
        .with_metadata("cause", Value::string(cause.code().name()))
}

/// §23.2's refusal for a plan whose required postconditions did not hold.
fn verification_refusal(
    plan: &ChangePlan,
    results: &[VerificationResult],
    recovery: bool,
) -> ErrorValue {
    let failures: Vec<String> = results
        .iter()
        .filter(|result| {
            result.class() == VerificationClass::Required
                && result.status() != VerificationStatus::Passed
        })
        .map(|result| format!("{}: {}", result.subject(), result.expression()))
        .collect();
    if recovery {
        // §25.3: nothing may claim the state was recovered, so the refusal names the domains it
        // could not vouch for rather than the plan.
        let domains: Vec<String> = results
            .iter()
            .filter(|result| result.status() != VerificationStatus::Passed)
            .map(|result| {
                result
                    .equivalence()
                    .map_or_else(|| result.subject().to_owned(), |domain| domain.to_string())
            })
            .collect();
        return error::recovery_verification_failed(&domains);
    }
    error::verification_failed(plan.id(), &failures)
}

/// Everything `verify` needs (§5.7, §23).
pub struct VerifyRequest<'a> {
    /// The plan whose contracts are being observed.
    pub plan: &'a ChangePlan,
    /// The instant every result is stamped with. §39.2's rule: the clock is a parameter.
    pub now: Timestamp,
    /// How one contract is observed.
    pub observe: &'a Observe<'a>,
}

impl std::fmt::Debug for VerifyRequest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifyRequest")
            .field("plan", &self.plan.id().as_str())
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

/// What verification established (§4.8, §23.3).
#[derive(Debug, Clone, PartialEq)]
pub struct VerifyOutcome {
    results: Vec<VerificationResult>,
    verdict: Verdict,
    failure_point: Option<FailurePoint>,
    error: Option<ErrorValue>,
    timed_out: Vec<Arc<str>>,
}

impl VerifyOutcome {
    /// The per-check results (§23.3).
    #[must_use]
    pub fn results(&self) -> &[VerificationResult] {
        &self.results
    }

    /// The plan-level verdict the results compose to (§23.2).
    #[must_use]
    pub const fn verdict(&self) -> Verdict {
        self.verdict
    }

    /// The state §4.8 gives the plan.
    #[must_use]
    pub fn state(&self) -> PlanState {
        self.state_for(false)
    }

    /// The state §4.8 gives the plan, on the recovery branch where `recovery` (§4.1, §25.3).
    #[must_use]
    pub fn state_for(&self, recovery: bool) -> PlanState {
        if recovery {
            return match self.verdict {
                Verdict::Verified => PlanState::RecoveryVerified,
                // §25.3 and Appendix F: a recovery whose verification did not hold MUST NOT be
                // reported as recovered, and DEGRADED is not a word the recovery branch offers.
                Verdict::Degraded | Verdict::Failed => PlanState::RecoveryFailed,
            };
        }
        match self.verdict {
            Verdict::Verified => PlanState::Verified,
            Verdict::Degraded => PlanState::Degraded,
            Verdict::Failed => PlanState::Failed,
        }
    }

    /// Which of Appendix F's verification rows this is.
    #[must_use]
    pub const fn failure_point(&self) -> Option<FailurePoint> {
        self.failure_point
    }

    /// The structured refusal, where a required postcondition did not hold.
    #[must_use]
    pub const fn error(&self) -> Option<&ErrorValue> {
        self.error.as_ref()
    }

    /// The checks that exceeded their timeout (§23.5).
    #[must_use]
    pub fn timed_out(&self) -> &[Arc<str>] {
        &self.timed_out
    }
}

/// Observes every contract the plan carries and composes §4.8's verdict (§5.7, §23).
///
/// §2.14 is what makes this a separate function from [`apply`]: a command that returned success
/// has proved that it returned success, and whether the intended state exists is asked against the
/// world afterwards. §5.7 lets an operator ask it again later, which is why it takes no claim and
/// changes nothing.
#[must_use]
pub fn verify(request: &VerifyRequest<'_>) -> VerifyOutcome {
    let mut results = Vec::new();
    let mut timed_out = Vec::new();
    for contract in request.plan.verification().contracts() {
        let observation = (request.observe)(contract);
        if matches!(observation, Observation::TimedOut) {
            timed_out.push(Arc::from(contract.subject()));
        }
        results.push(build_result(
            request.plan,
            contract,
            &observation,
            request.now,
        ));
    }
    let verdict = ono_change_core::VerificationSet::verdict(&results);
    let recovery = request.plan.kind() == PlanKind::Recovery;
    let failure_point = match verdict {
        Verdict::Verified => None,
        _ if !timed_out.is_empty() => Some(FailurePoint::VerificationTimeout),
        _ if recovery => Some(FailurePoint::RecoveryVerification),
        _ => Some(FailurePoint::RequiredVerification),
    };
    let error = failure_point.map(|_| verification_refusal(request.plan, &results, recovery));
    VerifyOutcome {
        results,
        verdict,
        failure_point,
        error,
        timed_out,
    }
}

/// Observes one contract, for the canary gate of §28.6.
fn observe_one(
    plan: &ChangePlan,
    contract: &VerificationContract,
    observe: &Observe<'_>,
    now: Timestamp,
) -> VerificationResult {
    let observation = observe(contract);
    build_result(plan, contract, &observation, now)
}

/// Turns an observation into §23.3's result, applying §23.5's timeout contract.
fn build_result(
    plan: &ChangePlan,
    contract: &VerificationContract,
    observation: &Observation,
    now: Timestamp,
) -> VerificationResult {
    match observation {
        Observation::Answered { status, observed } => {
            let mut result = VerificationResult::new(plan.id().clone(), contract, *status, now);
            if let Some(value) = observed {
                result = result.observing(value.clone());
            }
            result
        }
        // §23.5: a timeout is never a pass. Whether it is a failure or an UNKNOWN is the
        // contract's own declaration, and `timeout_status` is where the contract states it.
        Observation::TimedOut => {
            VerificationResult::new(plan.id().clone(), contract, contract.timeout_status(), now)
                .explained(format!(
                    "the check did not answer within {:?} (§23.5)",
                    contract.timeout()
                ))
        }
        Observation::Unobservable(refusal) => VerificationResult::new(
            plan.id().clone(),
            contract,
            VerificationStatus::Unknown,
            now,
        )
        .explained(refusal.message().to_owned()),
    }
}

/// Everything closing a plan needs (§4.9, §37).
#[derive(Debug)]
pub struct CloseRequest<'a> {
    plan: &'a ChangePlan,
    providers: &'a ProviderRegistry,
    assets: &'a [RecoveryAsset],
    remove: bool,
}

impl<'a> CloseRequest<'a> {
    /// Closes `plan`, leaving its assets alone (§4.9's "closing does not necessarily remove").
    #[must_use]
    pub const fn new(
        plan: &'a ChangePlan,
        providers: &'a ProviderRegistry,
        assets: &'a [RecoveryAsset],
    ) -> Self {
        Self {
            plan,
            providers,
            assets,
            remove: false,
        }
    }

    /// Closes the plan and asks the providers to remove its assets (§37.1).
    #[must_use]
    pub const fn removing_assets(mut self) -> Self {
        self.remove = true;
        self
    }
}

/// What closing established (§4.9, §37.4).
#[derive(Debug, Clone, PartialEq)]
pub struct CloseOutcome {
    state: PlanState,
    removed: Vec<RecoveryAssetId>,
    retained: Vec<RecoveryAssetId>,
    failures: Vec<ErrorValue>,
    failure_point: Option<FailurePoint>,
}

impl CloseOutcome {
    /// §4.9's state. Closing is not conditional on the assets going away.
    #[must_use]
    pub const fn state(&self) -> PlanState {
        self.state
    }

    /// The assets that were removed.
    #[must_use]
    pub fn removed(&self) -> &[RecoveryAssetId] {
        &self.removed
    }

    /// The assets that still occupy storage, which §37.4 requires to be surfaced.
    #[must_use]
    pub fn retained(&self) -> &[RecoveryAssetId] {
        &self.retained
    }

    /// The refusals cleanup raised.
    #[must_use]
    pub fn failures(&self) -> &[ErrorValue] {
        &self.failures
    }

    /// Appendix F's `CLOSED_WITH_ASSETS`: the plan is closed and an asset it meant to remove is
    /// still there.
    ///
    /// §4.1 has one closed state, because §4.9 makes retention independent of closing; the fact
    /// Appendix F's last row needs is this predicate rather than a fourteenth state word.
    #[must_use]
    pub fn is_closed_with_assets(&self) -> bool {
        self.state == PlanState::Closed && !self.retained.is_empty()
    }

    /// Which of Appendix F's rows this is, where cleanup did not complete.
    #[must_use]
    pub const fn failure_point(&self) -> Option<FailurePoint> {
        self.failure_point
    }
}

/// Closes a plan, and removes its assets only where the request asked for it (§4.9, §37.4).
///
/// Appendix F's last row is the one this exists for: a cleanup that fails produces no new target
/// mutation, the plan is `CLOSED`, and the asset that could not be removed is surfaced rather than
/// forgotten.
#[must_use]
pub fn close(request: &mut CloseRequest<'_>) -> CloseOutcome {
    let mut removed = Vec::new();
    let mut retained = Vec::new();
    let mut failures = Vec::new();
    if request.remove {
        for asset in request.assets {
            if request.plan.state().retains_assets_indefinitely() || asset.retention().is_held() {
                // §37.2: the assets of a plan that did not succeed are not removed by ordinary
                // success retention.
                retained.push(asset.id().clone());
                continue;
            }
            let outcome = request.providers.get(asset.provider()).map_or_else(
                || {
                    Err(error::provider_unavailable(
                        asset.provider(),
                        "the provider that created the asset is not registered here",
                    ))
                },
                |provider| provider.cleanup(asset),
            );
            match outcome {
                Ok(()) => removed.push(asset.id().clone()),
                Err(failure) => {
                    failures.push(failure);
                    retained.push(asset.id().clone());
                }
            }
        }
    } else {
        retained.extend(request.assets.iter().map(|asset| asset.id().clone()));
    }
    let failure_point = (!failures.is_empty()).then_some(FailurePoint::Cleanup);
    CloseOutcome {
        state: request
            .plan
            .clone()
            .advance(LifecycleEvent::Close)
            .map_or(PlanState::Closed, |plan| plan.state()),
        removed,
        retained,
        failures,
        failure_point,
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
    fn should_treat_every_failure_point_from_the_first_mutation_onwards_as_having_touched_the_system()
     {
        for point in [
            FailurePoint::Claim,
            FailurePoint::PlanState,
            FailurePoint::TargetRevalidation,
            FailurePoint::PrivilegeCheck,
            FailurePoint::RecoveryDiscovery,
            FailurePoint::Gate,
            FailurePoint::RecoveryAssetCreation,
            FailurePoint::RecoveryValidation,
            FailurePoint::ApplicationQuiesce,
            FailurePoint::ActionNotStarted,
            FailurePoint::Cleanup,
        ] {
            assert!(
                !point.may_have_mutated(),
                "Appendix F's second column says no mutation occurred at {point}"
            );
        }
        for point in [
            FailurePoint::FirstMutateAction,
            FailurePoint::MiddleMutateAction,
            FailurePoint::RemoteDisconnect,
            FailurePoint::UnknownOutcome,
            FailurePoint::RequiredVerification,
            FailurePoint::VerificationTimeout,
            FailurePoint::RecoveryAction,
            FailurePoint::RecoveryVerification,
        ] {
            assert!(
                point.may_have_mutated(),
                "Appendix F's second column says yes or unknown at {point}"
            );
        }
    }

    #[test]
    fn should_never_record_an_unestablished_outcome_as_a_failure() {
        let refusal = error::remote_state_unknown("api-04", "restart nginx");
        assert_eq!(
            ExecutionOutcome::Unknown(refusal).status(),
            ActionStatus::Unknown,
            "Appendix F.2: the state MUST be UNKNOWN, not guessed"
        );
        assert_eq!(
            ExecutionOutcome::Succeeded.status(),
            ActionStatus::Succeeded
        );
    }

    #[test]
    fn should_carry_the_refusal_out_of_an_outcome_that_has_one() {
        let refusal = error::tool_failed("/usr/bin/systemctl", "the unit refused");
        let outcome = ExecutionOutcome::Failed(refusal);
        assert_eq!(
            outcome.error().map(|error| error.code().name()),
            Some("recovery.provider_unavailable")
        );
        assert!(ExecutionOutcome::Succeeded.error().is_none());
    }

    #[test]
    fn should_hold_no_capability_a_planning_only_session_was_not_given() {
        let authority = Authority::planning_only();
        assert!(
            !authority.has_change(ChangeCapability::ActionExecute),
            "§43.2: planning MAY be available without mutation capability"
        );
        assert!(!authority.is_elevated());
        assert!(
            Authority::full().has_recovery(RecoveryCapability::Prepare),
            "a full session holds §12.2's five"
        );
    }

    #[test]
    fn should_stop_everything_downstream_of_a_failed_action() {
        let plan = ono_change_core::PlanId::derive(&["p"]);
        let first = PlanAction::new(
            &plan,
            1,
            ono_change_core::ActionRole::Mutate,
            "a",
            ono_change_core::Execution::Program {
                program: Arc::from("/bin/true"),
                argv: Vec::new(),
            },
        );
        let second = PlanAction::new(
            &plan,
            2,
            ono_change_core::ActionRole::Mutate,
            "b",
            ono_change_core::Execution::Program {
                program: Arc::from("/bin/true"),
                argv: Vec::new(),
            },
        )
        .after(first.id().clone());
        let third = PlanAction::new(
            &plan,
            3,
            ono_change_core::ActionRole::Mutate,
            "c",
            ono_change_core::Execution::Program {
                program: Arc::from("/bin/true"),
                argv: Vec::new(),
            },
        )
        .after(second.id().clone());
        let actions = vec![first.clone(), second, third];

        let blocked = dependents_of(&actions, first.id());

        assert_eq!(
            blocked.len(),
            2,
            "Appendix F: stop the dependency chain, transitively"
        );
        assert!(
            !blocked.contains(first.id()),
            "the failed action is not its own dependent"
        );
    }

    #[test]
    fn should_name_every_row_of_the_matrix_in_the_word_the_appendix_uses() {
        assert_eq!(
            FailurePoint::TargetRevalidation.to_string(),
            "target-revalidation"
        );
        assert_eq!(FailurePoint::RemoteDisconnect.as_str(), "remote-disconnect");
    }
}
