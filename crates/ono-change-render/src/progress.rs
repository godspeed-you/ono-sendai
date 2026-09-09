//! Apply progress and the failure display (Appendix E.4, E.5).
//!
//! Appendix E.4 forbids one thing in particular: *"Do not show one generic progress bar that
//! hides whether protection is complete."* The lifecycle boundaries of §4 are what the operator
//! actually needs — a plan that is 70% through has told them nothing, and a plan whose PREPARE
//! finished has told them the recovery assets exist. [`apply_progress`] therefore prints one line
//! per phase, always all of them, whatever state the plan is in.
//!
//! Appendix E.5 governs the other half. A failed apply is the moment an operator is most likely
//! to do something irreversible, and the appendix is explicit that Ono *"MUST NOT immediately
//! suggest recovery as the only correct next step"*. [`next_steps`] therefore offers every step
//! that is genuinely available, and recovery is one of them rather than the whole list.
//!
//! Appendix F.2 shapes the failure display too: an action whose outcome could not be established
//! is neither completed nor failed, and it gets its own block. Folding it into either one is the
//! reading that loses data.

use ono_change_core::{
    ActionRole, ActionStatus, ChangePlan, PlanAction, PlanState, ProtectionLevel, RecoveryAsset,
    VerificationResult,
};

use crate::symbols::{Charset, Symbol};
use crate::{column_pair, counted, fit, heading, safe};

/// How wide the phase label column is.
const LABEL: usize = 9;

/// One boundary of the plan lifecycle, as Appendix E.4 shows it.
///
/// The three of [`Phase::LIFECYCLE`] are the ones §4 makes load bearing and Appendix E.4 requires
/// to stay separately visible. Cleanup is a fourth because §37 gives it its own actions, and a
/// collapsed plan that silently dropped them would under-count what the plan does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// §4.5: revalidation, capability checks and recovery asset creation, before any mutation.
    Prepare,
    /// §4.7: the mutating actions, and §24's recovery actions, which mutate as well.
    Apply,
    /// §4.8: the postconditions that decide whether the plan achieved what it intended.
    Verify,
    /// §37: removal of temporary resources once retention permits. Never a lifecycle gate.
    Cleanup,
}

impl Phase {
    /// The three phases Appendix E.4 requires progress to keep apart, in lifecycle order.
    pub const LIFECYCLE: &'static [Phase] = &[Phase::Prepare, Phase::Apply, Phase::Verify];

    /// Every phase, in the order a plan runs them.
    pub const ORDER: &'static [Phase] =
        &[Phase::Prepare, Phase::Apply, Phase::Verify, Phase::Cleanup];

    /// The heading Appendix E.2 and E.4 print for the phase.
    #[must_use]
    pub const fn heading(self) -> &'static str {
        match self {
            Phase::Prepare => "PREPARE",
            Phase::Apply => "APPLY",
            Phase::Verify => "VERIFY",
            Phase::Cleanup => "CLEANUP",
        }
    }

    /// The phase an action of `role` belongs to (§3.3, §4.5, §4.7, §4.8).
    #[must_use]
    pub const fn of(role: ActionRole) -> Self {
        match role {
            ActionRole::Prepare => Phase::Prepare,
            ActionRole::Mutate | ActionRole::Recover => Phase::Apply,
            ActionRole::Verify => Phase::Verify,
            ActionRole::Cleanup => Phase::Cleanup,
        }
    }
}

/// Appendix E.4's progress: one line per lifecycle phase, always all three.
///
/// `results` are the verification results observed so far. §23's contracts and §3.3's verify
/// actions are two ways of expressing the same phase, so the VERIFY line counts whichever the
/// plan actually carries — the actions when it has them, the contracts otherwise. Neither is
/// invented: a plan with neither renders `none`, which is a different fact from `0/0`.
#[must_use]
pub fn apply_progress(
    plan: &ChangePlan,
    results: &[VerificationResult],
    width: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    for phase in Phase::LIFECYCLE {
        lines.push(fit(
            &column_pair(phase.heading(), &tally(plan, *phase, results), LABEL),
            width,
        ));
    }
    let cleanup = actions_of(plan, Phase::Cleanup);
    if !cleanup.is_empty() {
        lines.push(fit(
            &column_pair(
                Phase::Cleanup.heading(),
                &tally(plan, Phase::Cleanup, results),
                LABEL,
            ),
            width,
        ));
    }
    lines
}

/// Appendix E.5's failure display, in Appendix E.5's block order.
///
/// The `protection` block is not decoration: §37.2 keeps a failed plan's recovery assets past the
/// ordinary retention window, and an operator deciding what to do next needs to know the way back
/// still exists. The `next` block is [`next_steps`], which never offers recovery alone.
#[must_use]
pub fn apply_failure(
    plan: &ChangePlan,
    assets: &[RecoveryAsset],
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let mut lines = vec![fit(failure_title(plan.state()), width)];

    heading(&mut lines, "completed");
    let completed = completed_by_role(plan);
    if completed.is_empty() {
        lines.push(fit("  no action completed", width));
    }
    for line in completed {
        lines.push(fit(&format!("  {line}"), width));
    }

    heading(&mut lines, "failed");
    let failed: Vec<&PlanAction> = with_status(plan, ActionStatus::Failed);
    if failed.is_empty() {
        lines.push(fit("  no action reported failure", width));
    }
    for action in failed {
        lines.push(fit(
            &format!(
                "  {} {}",
                Symbol::Risk.glyph(charset),
                action_reference(action)
            ),
            width,
        ));
    }

    // Appendix F.2: an outcome nobody could establish is not a failure and not a success, and
    // recovery planning has to see it as its own uncertainty boundary.
    let unknown: Vec<&PlanAction> = with_status(plan, ActionStatus::Unknown);
    if !unknown.is_empty() {
        heading(&mut lines, "outcome unknown");
        for action in unknown {
            lines.push(fit(
                &format!(
                    "  {} {}",
                    Symbol::Unknown.glyph(charset),
                    action_reference(action)
                ),
                width,
            ));
        }
    }

    heading(&mut lines, "not executed");
    let pending = plan
        .actions()
        .iter()
        .filter(|action| {
            matches!(
                action.status(),
                ActionStatus::Pending | ActionStatus::Skipped
            )
        })
        .count();
    lines.push(fit(
        &format!("  {}", counted(pending, "action", "actions")),
        width,
    ));

    heading(&mut lines, "protection");
    lines.push(fit(
        &format!("  {}", retention(plan.state(), assets)),
        width,
    ));

    heading(&mut lines, "next");
    for step in next_steps(plan) {
        lines.push(fit(&format!("  {step}"), width));
    }
    lines
}

/// The steps an operator may take after a failed apply (Appendix E.5).
///
/// Appendix E.5 forbids presenting recovery as the only correct next step, so every step that is
/// actually available is offered and each one is gated on a fact of the plan rather than on
/// habit: `resume` on §41.3's per-action answer, `recover` on §24.1's recoverable states and on
/// there being something to recover with, `rebase` on there being work left to re-plan.
/// `inspect` is unconditional, because reading is always available and §40.2 would rather the
/// operator read than type a flag.
#[must_use]
pub fn next_steps(plan: &ChangePlan) -> Vec<String> {
    let reference = format!("plan/{}", plan.id().short());
    let mut steps = vec![format!("inspect {reference}")];
    if plan.actions().iter().any(PlanAction::may_resume) {
        steps.push(format!("resume {reference}"));
    }
    if plan.state().is_recoverable() && plan.protection().level() != ProtectionLevel::Unprotected {
        steps.push(format!("recover {reference}"));
    }
    if plan
        .actions()
        .iter()
        .any(|action| !action.status().is_settled())
    {
        steps.push(format!("rebase {reference}"));
    }
    steps
}

/// `PLAN APPLY FAILED` and the three other headlines §4 distinguishes.
///
/// §4.5 makes `PREPARE_FAILED` reachable without passing through `APPLYING`, and Appendix F turns
/// on the operator being told which of the two happened: one means nothing was changed, the other
/// means something was.
const fn failure_title(state: PlanState) -> &'static str {
    match state {
        PlanState::PrepareFailed => "PLAN PREPARE FAILED",
        PlanState::Failed | PlanState::Degraded => "PLAN VERIFICATION FAILED",
        PlanState::RecoveryFailed => "RECOVERY FAILED",
        _ => "PLAN APPLY FAILED",
    }
}

/// `4/4`, `pending`, or `none` — how far one phase has come (Appendix E.4).
fn tally(plan: &ChangePlan, phase: Phase, results: &[VerificationResult]) -> String {
    let actions = actions_of(plan, phase);
    let (done, total) = if phase == Phase::Verify && actions.is_empty() {
        (results.len(), plan.verification().contracts().len())
    } else {
        (
            actions
                .iter()
                .filter(|action| action.status().is_settled())
                .count(),
            actions.len(),
        )
    };
    if total == 0 {
        return "none".to_owned();
    }
    let running = actions
        .iter()
        .any(|action| action.status() == ActionStatus::Running);
    if done == 0 && !running {
        return "pending".to_owned();
    }
    format!("{done}/{total}")
}

/// The plan's actions in one phase, in plan order.
fn actions_of(plan: &ChangePlan, phase: Phase) -> Vec<&PlanAction> {
    plan.actions()
        .iter()
        .filter(|action| Phase::of(action.role()) == phase)
        .collect()
}

/// The plan's actions in one status, in plan order.
fn with_status(plan: &ChangePlan, status: ActionStatus) -> Vec<&PlanAction> {
    plan.actions()
        .iter()
        .filter(|action| action.status() == status)
        .collect()
}

/// `6 mutate actions` — what completed, counted per role (Appendix E.5).
fn completed_by_role(plan: &ChangePlan) -> Vec<String> {
    ActionRole::ALL
        .iter()
        .filter_map(|role| {
            let count = plan
                .actions()
                .iter()
                .filter(|action| {
                    action.role() == *role && action.status() == ActionStatus::Succeeded
                })
                .count();
            (count > 0).then(|| {
                format!(
                    "{count} {} {}",
                    role.as_str(),
                    if count == 1 { "action" } else { "actions" }
                )
            })
        })
        .collect()
}

/// `action 7: restart service api-04` (Appendix E.5).
fn action_reference(action: &PlanAction) -> String {
    let summary = safe(action.summary());
    let mut line = format!("action {}: {summary}", action.ordinal());
    // A summary that already names its target — `restart nginx.service` — is not improved by
    // the target after it, and repeating it reads as two different objects.
    if let Some(target) = action.target().map(safe)
        && !summary.contains(&target)
    {
        line.push_str(&format!(" {target}"));
    }
    line
}

/// What happens to the assets the failed apply created (§37.2).
fn retention(state: PlanState, assets: &[RecoveryAsset]) -> String {
    let created = assets
        .iter()
        .filter(|asset| asset.state().occupies_storage())
        .count();
    if created == 0 {
        return "no recovery asset was created".to_owned();
    }
    if state.retains_assets_indefinitely() {
        format!(
            "{} retained until an operator releases them",
            counted(created, "created asset", "created assets")
        )
    } else {
        format!(
            "{} retained by policy",
            counted(created, "created asset", "created assets")
        )
    }
}
