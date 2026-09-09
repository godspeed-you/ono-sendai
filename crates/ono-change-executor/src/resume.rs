//! Reconstructing an interrupted apply and deciding what may continue (spec v0.6 §41.2, §41.3).
//!
//! §41.2: *"On shell crash/restart, plan state MUST be reconstructable from persisted action
//! records and provider evidence."* [`PlanStore::record_action_status`](ono_change_plan::PlanStore)
//! wrote those records as each action settled, and this module lays them back over the plan and
//! asks §41's question of each action separately.
//!
//! `PlanAction::may_resume` already encodes the rule — an idempotent action may be rerun whatever
//! is known about it, a failed one may be rerun where its contract accepts a request token, and an
//! unknown or non-idempotent one may not be rerun blindly. What this module owns is the answer for
//! the *plan*: §41.3 says `resume` "MAY continue only actions whose prior status and idempotency
//! permit it. Otherwise a new recovery or rebase decision is required", and a resume that quietly
//! skipped the actions it could not rerun would leave a half-applied plan calling itself resumed.
//!
//! Drift is the other refusal. §7.3 stops an apply against state that moved, and a crash is not an
//! exemption: the world had longer than usual to change while nobody was looking, so
//! [`resume_with`] takes what revalidation found and refuses on anything that blocks.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ActionId, ActionStatus, ChangePlan, DriftFinding, Idempotency, PlanAction, PlanState, error,
};
use ono_change_plan::PlanStore;
use ono_value::{ErrorValue, Value};

/// What §41.3 permits after reading the persisted records back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeDecision {
    /// Every unfinished action may be rerun, so the apply may go on (§41.3).
    Continue,
    /// Every action already settled successfully; there is nothing left to do.
    AlreadyComplete,
    /// §41.3's other branch: a new recovery or rebase decision is required instead.
    RequiresRecoveryOrRebase,
    /// The world moved while the plan was interrupted, so §7.3 stops it (§62.8).
    Drifted,
}

impl ResumeDecision {
    /// Whether the apply may go on from here.
    #[must_use]
    pub const fn may_continue(self) -> bool {
        matches!(self, ResumeDecision::Continue)
    }

    /// The word an outcome renders.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ResumeDecision::Continue => "continue",
            ResumeDecision::AlreadyComplete => "already-complete",
            ResumeDecision::RequiresRecoveryOrRebase => "requires-recovery-or-rebase",
            ResumeDecision::Drifted => "drifted",
        }
    }
}

/// One action resume may not rerun, and the sentence §41.2 owes the operator for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedAction {
    action: ActionId,
    summary: Arc<str>,
    status: ActionStatus,
    idempotency: Idempotency,
    reason: Arc<str>,
}

impl BlockedAction {
    /// The action.
    #[must_use]
    pub const fn action(&self) -> &ActionId {
        &self.action
    }

    /// The line a person reads.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// What the persisted record says happened to it.
    #[must_use]
    pub const fn status(&self) -> ActionStatus {
        self.status
    }

    /// The contract's idempotency class (§41.1).
    #[must_use]
    pub const fn idempotency(&self) -> Idempotency {
        self.idempotency
    }

    /// Why §41.2 refuses to rerun it.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// What resume established (§41.2, §41.3).
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeOutcome {
    state: PlanState,
    decision: ResumeDecision,
    resumable: Vec<ActionId>,
    completed: Vec<ActionId>,
    blocked: Vec<BlockedAction>,
    uncertain: Vec<ActionId>,
    refusal: Option<ErrorValue>,
}

impl ResumeOutcome {
    /// The state the persisted records reconstruct to (§41.2).
    #[must_use]
    pub const fn state(&self) -> PlanState {
        self.state
    }

    /// What §41.3 permits.
    #[must_use]
    pub const fn decision(&self) -> ResumeDecision {
        self.decision
    }

    /// Whether the apply may go on.
    #[must_use]
    pub const fn may_continue(&self) -> bool {
        self.decision.may_continue()
    }

    /// The actions resume may rerun, in plan order.
    #[must_use]
    pub fn resumable(&self) -> &[ActionId] {
        &self.resumable
    }

    /// The actions that already succeeded and will not be rerun.
    #[must_use]
    pub fn completed(&self) -> &[ActionId] {
        &self.completed
    }

    /// The actions §41.2 refuses to rerun, each carrying why (§55.9 case 41).
    #[must_use]
    pub fn blocked(&self) -> &[BlockedAction] {
        &self.blocked
    }

    /// The actions whose outcome was never established (Appendix F.2).
    #[must_use]
    pub fn uncertainty_boundary(&self) -> &[ActionId] {
        &self.uncertain
    }

    /// The structured refusal, where resume refuses.
    #[must_use]
    pub const fn refusal(&self) -> Option<&ErrorValue> {
        self.refusal.as_ref()
    }
}

/// Reads the persisted action records back and decides what may continue (§41.2, §41.3).
///
/// # Errors
///
/// A store that cannot be read produces a [`ResumeOutcome`] whose decision is
/// [`ResumeDecision::RequiresRecoveryOrRebase`] and whose refusal is the store's own: §41.2 makes
/// the records the evidence, and a resume that cannot read them has no basis to rerun anything.
#[must_use]
pub fn resume(plan: &ChangePlan, store: &PlanStore, now: Timestamp) -> ResumeOutcome {
    resume_with(plan, store, now, &[])
}

/// [`resume`], told what revalidation found since the interruption (§7.3, §41.3).
#[must_use]
pub fn resume_with(
    plan: &ChangePlan,
    store: &PlanStore,
    now: Timestamp,
    drift: &[DriftFinding],
) -> ResumeOutcome {
    let _ = now;
    let statuses = match store.action_statuses(plan.id(), plan.revision()) {
        Ok(statuses) => statuses,
        Err(refusal) => {
            return ResumeOutcome {
                state: plan.state(),
                decision: ResumeDecision::RequiresRecoveryOrRebase,
                resumable: Vec::new(),
                completed: Vec::new(),
                blocked: Vec::new(),
                uncertain: Vec::new(),
                refusal: Some(refusal),
            };
        }
    };

    let mut resumable = Vec::new();
    let mut completed = Vec::new();
    let mut blocked = Vec::new();
    let mut uncertain = Vec::new();
    for action in plan.actions() {
        let status = statuses
            .get(action.id().as_str())
            .copied()
            .unwrap_or_else(|| action.status());
        let settled = action.clone().with_status(status);
        if status == ActionStatus::Unknown {
            uncertain.push(action.id().clone());
        }
        match status {
            ActionStatus::Succeeded => completed.push(action.id().clone()),
            _ if settled.may_resume() => resumable.push(action.id().clone()),
            _ => blocked.push(BlockedAction {
                action: action.id().clone(),
                summary: Arc::from(action.summary()),
                status,
                idempotency: action.idempotency(),
                reason: Arc::from(refusal_reason(status, action.idempotency())),
            }),
        }
    }

    let state = reconstructed_state(plan, &statuses);
    let blocking_drift: Vec<&DriftFinding> = drift
        .iter()
        .filter(|finding| finding.verdict().blocks_apply())
        .collect();
    if !blocking_drift.is_empty() {
        let rows: Vec<(String, String, String)> = blocking_drift
            .iter()
            .map(|finding| {
                (
                    finding.subject().to_owned(),
                    finding.field().to_owned(),
                    "changed while the plan was interrupted (§7.3, §41.3)".to_owned(),
                )
            })
            .collect();
        return ResumeOutcome {
            state,
            decision: ResumeDecision::Drifted,
            resumable,
            completed,
            blocked,
            uncertain,
            refusal: Some(error::drift_detected(plan.id(), &rows)),
        };
    }

    let decision = if !blocked.is_empty() {
        ResumeDecision::RequiresRecoveryOrRebase
    } else if resumable.is_empty() {
        ResumeDecision::AlreadyComplete
    } else {
        ResumeDecision::Continue
    };
    let refusal = (decision == ResumeDecision::RequiresRecoveryOrRebase)
        .then(|| blocked_refusal(plan, state, &blocked));
    ResumeOutcome {
        state,
        decision,
        resumable,
        completed,
        blocked,
        uncertain,
        refusal,
    }
}

/// The state §41.2 reconstructs from the persisted records alone.
///
/// An action that settled at all means mutation began, so the plan is at least `APPLYING`; a plan
/// whose every action succeeded is past mutation and awaiting §4.8. Nothing here promotes an
/// unknown to a failure, because Appendix F.2 forbids exactly that.
fn reconstructed_state(
    plan: &ChangePlan,
    statuses: &std::collections::BTreeMap<String, ActionStatus>,
) -> PlanState {
    let mutating: Vec<&PlanAction> = plan
        .actions()
        .iter()
        .filter(|action| action.role().mutates_target())
        .collect();
    let observed: Vec<ActionStatus> = mutating
        .iter()
        .filter_map(|action| statuses.get(action.id().as_str()).copied())
        .collect();
    if observed.is_empty() {
        return plan.state();
    }
    if observed.contains(&ActionStatus::Failed) {
        return PlanState::ApplyFailed;
    }
    if observed.len() == mutating.len()
        && observed
            .iter()
            .all(|status| *status == ActionStatus::Succeeded)
    {
        return PlanState::Verifying;
    }
    PlanState::Applying
}

/// Why §41.2 will not rerun an action in this state.
const fn refusal_reason(status: ActionStatus, idempotency: Idempotency) -> &'static str {
    match (status, idempotency) {
        (ActionStatus::Unknown, _) => {
            "its outcome was never established and its contract does not permit a blind rerun \
             (§41.2, Appendix F.2)"
        }
        (ActionStatus::Running, _) => {
            "it was in flight when the shell stopped and its contract does not permit a blind \
             rerun (§41.2)"
        }
        (ActionStatus::Failed, _) => {
            "it failed and its contract does not accept a retry, with or without a request token \
             (§41.1)"
        }
        _ => "its prior status and idempotency class do not permit continuing it (§41.3)",
    }
}

/// §41.3's refusal: a new recovery or rebase decision is required instead of a resume.
///
/// `change.resume_refused` is its own code because the answer an operator needs is not "the plan
/// is in the wrong state" — the plan is in exactly the state the interruption left it in. What is
/// refused is continuing *these* actions, and the reasons travel one per action so a script can
/// tell "nothing may be rerun" from "one action may not". The state travels too, because §41.2
/// keeps the plan inspectable either way.
fn blocked_refusal(plan: &ChangePlan, state: PlanState, blocked: &[BlockedAction]) -> ErrorValue {
    let reasons: Vec<(String, String)> = blocked
        .iter()
        .map(|action| {
            (
                action.action().as_str().to_owned(),
                format!("{}: {}", action.summary(), action.reason()),
            )
        })
        .collect();
    error::resume_refused(plan.id(), &reasons)
        .with_help(
            "v0.6 §41.3: resume may continue only actions whose prior status and idempotency \
             permit it. Otherwise a new recovery or rebase decision is required, and §41.2 \
             forbids blindly rerunning an unknown or non-idempotent action"
                .to_owned(),
        )
        .with_metadata("state", Value::string(state.as_str()))
        .with_metadata(
            "blocked_actions",
            Value::list(
                blocked
                    .iter()
                    .map(|action| Value::string(action.action().as_str())),
            ),
        )
}
