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
//! Appendix F.2 shapes the failure display too: an action whose `status` is `unknown` is neither
//! completed nor failed, and it gets its own block. Folding it into either one is the reading
//! that loses data.

use ono_value::RecordValue;

use crate::symbols::{Charset, Symbol};
use crate::{Item, column_pair, count, counted, fit, flag, heading, items, list_len, text};

/// How wide the phase label column is.
const LABEL: usize = 9;

/// The §3.3 roles, in the order §46.2 lists them.
const ROLES: [&str; 5] = ["prepare", "mutate", "verify", "recover", "cleanup"];

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
    ///
    /// A role this build does not know is an applying one. §4.7's reading of an unrecognised
    /// action is the one that assumes it may change the system, which is the safe direction.
    #[must_use]
    pub fn of(role: &str) -> Self {
        match role {
            "prepare" => Phase::Prepare,
            "verify" => Phase::Verify,
            "cleanup" => Phase::Cleanup,
            _ => Phase::Apply,
        }
    }
}

/// Whether an action `status` is finished, whatever the outcome was (§4.7, Appendix F.2).
pub(crate) fn is_settled(status: &str) -> bool {
    matches!(status, "succeeded" | "failed" | "skipped" | "unknown")
}

/// Appendix E.4's progress: one line per lifecycle phase, always all three.
///
/// `plan` is an `ono.change-plan/1`; `results` are the `ono.change-verification/1` records
/// observed so far. §23's contracts and §3.3's verify actions are two ways of expressing the same
/// phase, so the VERIFY line counts whichever the plan actually carries — the actions when it has
/// them, the contracts otherwise. Neither is invented: a plan with neither renders `none`, which
/// is a different fact from `0/0`.
#[must_use]
pub fn apply_progress(plan: &RecordValue, results: &[RecordValue], width: usize) -> Vec<String> {
    let actions = items(plan, "actions");
    let mut lines = Vec::new();
    for phase in Phase::LIFECYCLE {
        lines.push(fit(
            &column_pair(
                phase.heading(),
                &tally(plan, &actions, *phase, results),
                LABEL,
            ),
            width,
        ));
    }
    if !of_phase(&actions, Phase::Cleanup).is_empty() {
        lines.push(fit(
            &column_pair(
                Phase::Cleanup.heading(),
                &tally(plan, &actions, Phase::Cleanup, results),
                LABEL,
            ),
            width,
        ));
    }
    lines
}

/// Appendix E.5's failure display for a run whose verification was not observed.
///
/// [`failure_display`] with no verification results. A caller that ran verification passes its
/// `ono.change-verification/1` records there instead, and never in `assets`.
#[must_use]
pub fn apply_failure(
    plan: &RecordValue,
    assets: &[RecordValue],
    width: usize,
    charset: Charset,
) -> Vec<String> {
    failure_display(plan, assets, &[], width, charset)
}

/// Appendix E.5's failure display, in Appendix E.5's block order.
///
/// `plan` is the `ono.change-plan/1` with each action's status as the run left it, `assets` the
/// `ono.recovery-asset/1` records the run created, and `results` the `ono.change-verification/1`
/// records verification observed. Only a record of the asset schema counts as an asset, so a
/// result handed over in the wrong slice cannot be counted as protection.
///
/// The title follows what actually failed (§4.8): every action ran and a check did not hold is
/// `PLAN VERIFICATION FAILED`, whatever lifecycle state the record was sealed in. The `protection`
/// block is not decoration: §37.2 keeps a failed plan's recovery assets past the ordinary
/// retention window, and an operator deciding what to do next needs to know the way back still
/// exists. The `next` block offers `recover` when the run may have changed something and a ready
/// asset exists to recover with, and never offers it alone.
#[must_use]
pub fn failure_display(
    plan: &RecordValue,
    assets: &[RecordValue],
    results: &[RecordValue],
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let assets: Vec<&RecordValue> = assets
        .iter()
        .filter(|record| record.schema_id().to_string() == "ono.recovery-asset/1")
        .collect();
    let actions = items(plan, "actions");
    let state = text(plan, "state").unwrap_or_else(|| "apply-failed".to_owned());
    let mut lines = vec![fit(title(&state, &actions, results), width)];

    heading(&mut lines, "completed");
    let completed = completed_by_role(&actions);
    if completed.is_empty() {
        lines.push(fit("  no action completed", width));
    }
    for line in completed {
        lines.push(fit(&format!("  {line}"), width));
    }

    heading(&mut lines, "failed");
    let failed = with_status(&actions, "failed");
    let failed_checks: Vec<&RecordValue> = results
        .iter()
        .filter(|result| text(*result, "status").as_deref() == Some("failed"))
        .collect();
    if failed.is_empty() && failed_checks.is_empty() {
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
    for check in failed_checks {
        lines.push(fit(
            &format!(
                "  {} check {}",
                Symbol::Risk.glyph(charset),
                check_line(check)
            ),
            width,
        ));
    }

    // Appendix F.2: an outcome nobody could establish is not a failure and not a success, and
    // recovery planning has to see it as its own uncertainty boundary.
    let unknown = with_status(&actions, "unknown");
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
    let pending = actions
        .iter()
        .filter(|action| {
            matches!(
                text(*action, "status").as_deref(),
                Some("pending" | "skipped") | None
            )
        })
        .count();
    lines.push(fit(
        &format!("  {}", counted(pending, "action", "actions")),
        width,
    ));

    heading(&mut lines, "protection");
    lines.push(fit(&format!("  {}", retention(&state, &assets)), width));

    heading(&mut lines, "next");
    for step in steps(plan, Some(&assets)) {
        lines.push(fit(&format!("  {step}"), width));
    }
    lines
}

/// The title of the failure display: what failed, not merely which state the record is in.
fn title(state: &str, actions: &[Item], results: &[RecordValue]) -> &'static str {
    let action_failed = actions.iter().any(|action| {
        matches!(
            text(action, "status").as_deref(),
            Some("failed" | "unknown")
        )
    });
    let verification_failed = !results.is_empty() && crate::verify::verdict(results) != "VERIFIED";
    if verification_failed
        && !action_failed
        && !matches!(state, "prepare-failed" | "recovery-failed")
    {
        return "PLAN VERIFICATION FAILED";
    }
    failure_title(state)
}

/// `nginx.service == running` — the check as the contract states it (§23.4).
fn check_line(check: &RecordValue) -> String {
    let subject = text(check, "subject").unwrap_or_else(|| "unnamed".to_owned());
    match text(check, "expression").filter(|expression| !expression.is_empty()) {
        Some(expression) => format!("{subject} {expression}"),
        None => subject,
    }
}

/// The steps an operator may take after a failed apply (Appendix E.5).
///
/// Appendix E.5 forbids presenting recovery as the only correct next step, so every step that is
/// actually available is offered and each one is gated on a fact of the record rather than on
/// habit: `resume` on there being an action §41.3 permits rerunning, `recover` on §24.1's
/// recoverable states and on there being something to recover with, `rebase` on there being work
/// left to re-plan. `inspect` is unconditional, because reading is always available and §40.2
/// would rather the operator read than type a flag.
#[must_use]
pub fn next_steps(plan: &RecordValue) -> Vec<String> {
    steps(plan, None)
}

/// [`next_steps`], answering "is there something to recover with" from the created assets where
/// the caller has them, and from the plan's protection level where it does not.
fn steps(plan: &RecordValue, assets: Option<&[&RecordValue]>) -> Vec<String> {
    let reference = format!("plan/{}", crate::plan::short(plan, "id"));
    let actions = items(plan, "actions");
    let mut steps = vec![format!("inspect {reference}")];
    if actions.iter().any(may_resume) {
        steps.push(format!("resume {reference}"));
    }
    let recoverable = matches!(
        text(plan, "state").as_deref(),
        Some(
            "verified"
                | "degraded"
                | "failed"
                | "apply-failed"
                | "closed"
                | "recovery-planned"
                | "recovery-failed"
        )
    );
    let offer = match assets {
        // Appendix F: whatever state the record was sealed in, an action that ran may have
        // changed the system, and a ready asset is what a recovery would restore from.
        Some(assets) => {
            (recoverable || actions.iter().any(may_have_mutated))
                && assets
                    .iter()
                    .any(|asset| text(*asset, "state").as_deref() == Some("ready"))
        }
        None => {
            recoverable
                && !matches!(
                    text(plan, "protection_level").as_deref(),
                    Some("unprotected") | None
                )
        }
    };
    if offer {
        steps.push(format!("recover {reference}"));
    }
    if actions
        .iter()
        .any(|action| !is_settled(text(action, "status").as_deref().unwrap_or("pending")))
    {
        steps.push(format!("rebase {reference}"));
    }
    steps
}

/// Whether an action may have changed the system: it mutates or recovers, and it ran (§4.7).
fn may_have_mutated(action: &Item) -> bool {
    matches!(
        text(action, "role").as_deref(),
        Some("mutate" | "recover") | None
    ) && matches!(
        text(action, "status").as_deref(),
        Some("succeeded" | "failed" | "unknown" | "running")
    )
}

/// Whether resume may rerun an action, given its status and its §41.1 idempotency class.
///
/// §41.2 forbids blindly rerunning an unknown or non-idempotent action after a crash, so `unknown`
/// is not a soft `idempotent` and a missing class is read the same way.
fn may_resume(action: &Item) -> bool {
    let idempotency = text(action, "idempotency").unwrap_or_else(|| "unknown".to_owned());
    match text(action, "status").as_deref().unwrap_or("pending") {
        "pending" | "skipped" => true,
        "running" | "unknown" => idempotency == "idempotent",
        "failed" => matches!(idempotency.as_str(), "idempotent" | "retry-safe-with-token"),
        _ => false,
    }
}

/// `PLAN APPLY FAILED` and the three other headlines §4 distinguishes.
///
/// §4.5 makes `prepare-failed` reachable without passing through `applying`, and Appendix F turns
/// on the operator being told which of the two happened: one means nothing was changed, the other
/// means something was.
const fn failure_title(state: &str) -> &'static str {
    match state.as_bytes() {
        b"prepare-failed" => "PLAN PREPARE FAILED",
        b"failed" | b"degraded" => "PLAN VERIFICATION FAILED",
        b"recovery-failed" => "RECOVERY FAILED",
        _ => "PLAN APPLY FAILED",
    }
}

/// `4/4`, `pending`, or `none` — how far one phase has come (Appendix E.4).
fn tally(plan: &RecordValue, actions: &[Item], phase: Phase, results: &[RecordValue]) -> String {
    let of_phase = of_phase(actions, phase);
    let (done, total) = if phase == Phase::Verify && of_phase.is_empty() {
        (results.len(), list_len(plan, "verification_contracts"))
    } else {
        (
            of_phase
                .iter()
                .filter(|action| {
                    is_settled(text(**action, "status").as_deref().unwrap_or("pending"))
                })
                .count(),
            of_phase.len(),
        )
    };
    if total == 0 {
        return "none".to_owned();
    }
    let running = of_phase
        .iter()
        .any(|action| text(*action, "status").as_deref() == Some("running"));
    if done == 0 && !running {
        return "pending".to_owned();
    }
    format!("{done}/{total}")
}

/// The actions in one phase, in the order the record holds them.
fn of_phase(actions: &[Item], phase: Phase) -> Vec<&Item> {
    actions
        .iter()
        .filter(|action| Phase::of(text(*action, "role").as_deref().unwrap_or("mutate")) == phase)
        .collect()
}

/// The actions in one status, in the order the record holds them.
fn with_status<'a>(actions: &'a [Item], status: &str) -> Vec<&'a Item> {
    actions
        .iter()
        .filter(|action| text(*action, "status").as_deref() == Some(status))
        .collect()
}

/// `6 mutate actions` — what completed, counted per role (Appendix E.5).
fn completed_by_role(actions: &[Item]) -> Vec<String> {
    ROLES
        .iter()
        .filter_map(|role| {
            let number = actions
                .iter()
                .filter(|action| {
                    text(*action, "role").as_deref() == Some(*role)
                        && text(*action, "status").as_deref() == Some("succeeded")
                })
                .count();
            (number > 0).then(|| {
                format!(
                    "{number} {role} {}",
                    if number == 1 { "action" } else { "actions" }
                )
            })
        })
        .collect()
}

/// `action 7: restart service api-04` (Appendix E.5).
fn action_reference(action: &Item) -> String {
    let ordinal = count(action, "ordinal").unwrap_or_default();
    let summary = text(action, "summary").unwrap_or_else(|| "unnamed action".to_owned());
    let mut line = format!("action {ordinal}: {summary}");
    // A summary that already names its target — `restart nginx.service` — is not improved by the
    // target after it, and repeating it reads as two different objects.
    if let Some(target) = text(action, "target").filter(|target| !summary.contains(target)) {
        line.push_str(&format!(" {target}"));
    }
    line
}

/// What happens to the assets the failed apply created (§37.2).
fn retention(state: &str, assets: &[&RecordValue]) -> String {
    let created = assets
        .iter()
        .filter(|asset| {
            matches!(
                text(**asset, "state").as_deref(),
                Some("creating" | "ready" | "invalid" | "expired")
            )
        })
        .count();
    if created == 0 {
        return "no recovery asset was created".to_owned();
    }
    let held = assets.iter().any(|asset| flag(*asset, "held"));
    let retains = held
        || matches!(
            state,
            "failed" | "degraded" | "apply-failed" | "prepare-failed" | "recovery-failed"
        );
    if retains {
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
