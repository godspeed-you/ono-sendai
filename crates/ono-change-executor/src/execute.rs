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

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ActionId, ActionRole, ActionStatus, ChangeCapability, ChangePlan, DriftFinding, DriftVerdict,
    LifecycleEvent, PlanAction, PlanKind, PlanState, ProtectionAction, ProtectionLevel,
    RecoveryAsset, RecoveryAssetId, RecoveryCapability, RecoveryValidation, Verdict,
    VerificationClass, VerificationContract, VerificationResult, VerificationStatus, error,
    topological_order,
};
use ono_change_plan::PlanStore;
use ono_change_protection::ProviderRegistry;
use ono_value::{ErrorValue, Value};

use crate::strategy::{StrategyRun, TargetResult, run_waves};

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
    quiesce: Option<QuiesceReport>,
}

impl Prepared {
    /// The validated assets protection produced (§4.6, §11.4).
    #[must_use]
    pub fn assets(&self) -> &[RecoveryAsset] {
        &self.assets
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
    for action in request.protection {
        match create_and_validate(request.providers, action, request.now) {
            Ok(asset) => {
                let usable = asset.is_usable();
                let invalid = asset.state();
                request.created.push(asset);
                if !usable && action.is_required() {
                    let asset_id = request
                        .created
                        .last()
                        .map(|asset| asset.id().clone())
                        .unwrap_or_else(|| RecoveryAssetId::of(action.provider(), None, "", ""));
                    let failures = request
                        .created
                        .last()
                        .and_then(RecoveryAsset::validation)
                        .map(RecoveryValidation::failures)
                        .unwrap_or_default();
                    let _ = invalid;
                    failure = Some((
                        FailurePoint::RecoveryValidation,
                        error::asset_invalid(&asset_id, &failures),
                    ));
                    break;
                }
            }
            Err(refusal) => {
                if action.is_required() {
                    failure = Some((FailurePoint::RecoveryAssetCreation, refusal));
                    break;
                }
            }
        }
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
            assets: request.created.clone(),
            quiesce: request.quiesce_report.clone(),
        }),
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
        Err(refusal) => Ok(asset.validated(
            RecoveryValidation::none(now, refusal.message().to_owned()).existing(true),
        )),
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
                report.retained.push((asset.id().clone(), Arc::from(reason)));
                continue;
            }
            let removal = request
                .providers
                .get(asset.provider())
                .map_or_else(
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
        }
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
}

impl ApplyOutcome {
    /// Where §4.1 leaves the plan.
    #[must_use]
    pub const fn state(&self) -> PlanState {
        self.state
    }

    /// Whether the target system may already have been changed (Appendix F's second column).
    #[must_use]
    pub const fn has_mutated(&self) -> bool {
        self.state.has_mutated()
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
        matches!(
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
}

/// Runs §5.6's commitment point: claim, revalidate, check, gate, prepare, mutate, verify.
///
/// Every step before preparation refuses with the plan untouched and unprepared, which is what
/// Appendix F's first three rows require; preparation may leave assets behind and never a changed
/// target (§2.3); and from the first mutating action onwards the outcome says what ran.
#[must_use]
pub fn apply(request: &mut ApplyRequest<'_>) -> ApplyOutcome {
    let plan = request.prepare.plan;
    let now = request.prepare.now;

    if let Some(refusal) = appliability(plan, now) {
        return refused(plan, FailurePoint::PlanState, refusal);
    }

    // 1. §42.4: two sessions MUST NOT apply one sealed plan at once. The claim is held for the
    //    whole of this function and released on the way out, including on an early return (§42.3).
    let claim = match request.store.claim(plan.id(), &request.session, now) {
        Ok(claim) => claim,
        Err(refusal) => return refused(plan, FailurePoint::Claim, refusal),
    };

    // 2. §7.3: revalidate before anything is prepared. Material drift and an unanswerable check
    //    both stop the apply, because §2.4 forbids promoting unknown to expected.
    if let Some(refusal) = revalidate_plan(plan, request.revalidate) {
        drop(claim);
        return refused(plan, FailurePoint::TargetRevalidation, refusal);
    }

    // 3. §43.2, §43.3: capability and privilege, before prepare.
    if let Some(refusal) = check_authority(plan, &request.prepare.authority, request.prepare.protection) {
        drop(claim);
        return refused(plan, FailurePoint::PrivilegeCheck, refusal);
    }
    if let Some(refusal) = check_discovery(plan, request.prepare.protection) {
        drop(claim);
        return refused(plan, FailurePoint::RecoveryDiscovery, refusal);
    }

    // 4. §19.4: an outstanding acknowledgement refuses before prepare. §40.3 makes this the whole
    //    of a script's gate: nothing here prompts, so a missing flag is a refusal.
    if let Some(refusal) = check_gates(plan) {
        drop(claim);
        return refused(plan, FailurePoint::Gate, refusal);
    }

    // 5. §4.5: create and validate the recovery assets the policy requires.
    let prepared = if request.prepare.protection.is_empty() {
        Ok(Prepared {
            assets: Vec::new(),
            quiesce: None,
        })
    } else {
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
            outcome.untouched = target_labels(plan);
            drop(claim);
            return outcome;
        }
    };

    // 6. §4.7: the mutating actions, in dependency order, by the plan's strategy.
    let mutation = mutate(request, &prepared);
    drop(claim);
    mutation
}

/// §5.6: `apply` MUST refuse drafts and expired plans.
fn appliability(plan: &ChangePlan, now: Timestamp) -> Option<ErrorValue> {
    if plan.is_expired_at(now) {
        return Some(error::plan_expired(plan.id()));
    }
    if !plan.state().is_appliable() {
        return Some(error::plan_not_sealed(plan.id(), plan.state()));
    }
    None
}

/// §7.3: material drift, or a precondition nobody could check, stops the apply before prepare.
fn revalidate_plan(plan: &ChangePlan, revalidate: &Revalidate<'_>) -> Option<ErrorValue> {
    let mut blocking: Vec<(String, String, String)> = Vec::new();
    for action in plan.actions() {
        match revalidate(action) {
            Ok(findings) => {
                for finding in &findings {
                    if finding.verdict().blocks_apply() {
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
        return Some(error::privilege_required(
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
    if protection.iter().any(ProtectionAction::is_required)
        && !authority.has_recovery(RecoveryCapability::Prepare)
    {
        return Some(error::privilege_required(
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
    if !protection.is_empty() && shortfall.is_empty() {
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
    }
}

/// §4.7: the mutating actions, in dependency order, by the plan's strategy, then §4.8.
fn mutate(request: &mut ApplyRequest<'_>, prepared: &Prepared) -> ApplyOutcome {
    let plan = request.prepare.plan;
    let now = request.prepare.now;
    let order = match topological_order(plan.actions()) {
        Ok(order) => order,
        Err(cycle) => {
            let mut outcome = refused(
                plan,
                FailurePoint::PlanState,
                error::action_graph_cyclic(plan.id(), &cycle),
            );
            outcome.assets = prepared.assets.to_vec();
            return outcome;
        }
    };

    let statuses: RefCell<Vec<(ActionId, ActionStatus)>> = RefCell::new(
        plan.actions()
            .iter()
            .map(|action| (action.id().clone(), ActionStatus::Pending))
            .collect(),
    );
    let stop: RefCell<Option<(FailurePoint, ErrorValue, ActionId)>> = RefCell::new(None);
    let store_failures: RefCell<Vec<ErrorValue>> = RefCell::new(Vec::new());
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
            now,
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
            let mut failure = None;
            for action in mutating
                .iter()
                .filter(|action| action_covers(action, target, plan))
            {
                if stop.borrow().is_some() {
                    break;
                }
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
                        failure = Some(refusal.clone());
                        *stop.borrow_mut() = Some((point, refusal.clone(), action.id().clone()));
                    }
                    ExecutionOutcome::Unknown(refusal) => {
                        let point = if is_remote(action, plan) {
                            FailurePoint::RemoteDisconnect
                        } else {
                            FailurePoint::UnknownOutcome
                        };
                        failure = Some(refusal.clone());
                        *stop.borrow_mut() = Some((point, refusal.clone(), action.id().clone()));
                    }
                }
            }
            results.push(TargetResult::new(Arc::clone(target), status, failure));
        }
        results
    };

    let gate = |_wave: &ono_change_core::Wave, _targets: &[Arc<str>]| -> Verdict {
        // §28.6: for a canary strategy the plan's *required* verification must pass before the
        // remaining batches continue. Advisory checks say nothing about whether to go on.
        let required: Vec<&VerificationContract> = plan
            .verification()
            .contracts()
            .iter()
            .filter(|contract| contract.class() == VerificationClass::Required)
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
    let assets = prepared.assets.to_vec();
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
    };

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
        return outcome;
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
        return outcome;
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
    outcome
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
