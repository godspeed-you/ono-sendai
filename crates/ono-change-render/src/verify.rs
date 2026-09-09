//! Verification views: §23.4 for a plan, §25.2 for a recovery.
//!
//! §2.14 is the invariant both views serve: verification is separate from execution success. A
//! command that returned zero proved that it returned zero, and §62.9 names answering the other
//! question from an exit code as a failure mode. So every line here is an observation against
//! the world, and the closing verdict is [`ono_change_core::VerificationSet::verdict`]'s rather
//! than this module's.
//!
//! §25.3 governs the recovery half and it is a language rule with teeth: *"User-visible language
//! MUST describe the verified scope."* [`recovery_verification`] therefore reports §25.1's three
//! equivalence domains separately and closes with two sentences, one of which is
//! `FULL WORLD EQUIVALENCE NOT CLAIMED`. There is no code path here that emits a global claim
//! about a rollback, and the crate's tests read every string it can emit to keep it that way.

use ono_change_core::{
    EquivalenceState, PlanId, RecoveryOutcome, Verdict, VerificationClass, VerificationResult,
    VerificationSet, VerificationStatus,
};

use crate::{display_width, fit, heading, safe};

/// §25.2's second closing sentence, which every recovery verification carries.
///
/// It is a constant because it is a promise about what Ono did *not* establish, and §25.1 makes
/// that promise the same in every recovery: a service restart may restore configuration while
/// legitimately creating new PIDs, and no local asset touches a webhook that already left.
pub const NO_FULL_EQUIVALENCE: &str = "FULL WORLD EQUIVALENCE NOT CLAIMED";

/// How wide the subject column of a verification block is.
const SUBJECT: usize = 24;

/// §23.4's plan verification: required, advisory and observed, then the status.
///
/// The three blocks are §23.2's classes, and they are kept apart because they mean different
/// things about the plan: a failed required postcondition makes the plan `FAILED`, a failed
/// advisory one may make it `DEGRADED`, and an observation never changes the state at all.
/// Folding them into one list would make the reader do the classification §23.2 already did.
#[must_use]
pub fn verification_view(
    plan: &PlanId,
    results: &[VerificationResult],
    width: usize,
) -> Vec<String> {
    let mut lines = vec![fit(&format!("VERIFY / plan {}", plan.short()), width)];
    for (class, title) in [
        (VerificationClass::Required, "required"),
        (VerificationClass::Advisory, "advisory"),
        (VerificationClass::Observational, "observed"),
    ] {
        let of_class: Vec<&VerificationResult> = results
            .iter()
            .filter(|result| result.class() == class)
            .collect();
        if of_class.is_empty() {
            continue;
        }
        heading(&mut lines, title);
        for result in of_class {
            lines.push(fit(
                &row(&safe(result.subject()), status_word(result.status())),
                width,
            ));
        }
    }
    if results.is_empty() {
        heading(&mut lines, "observed");
        lines.push(fit("  no contract was answered", width));
    }
    heading(&mut lines, "status");
    lines.push(fit(
        &format!("  {}", verdict_word(VerificationSet::verdict(results))),
        width,
    ));
    lines
}

/// §25.2's recovery verification: one block per equivalence domain, then a scoped result.
///
/// §25.1 requires the three domains to be distinguished, and the shape of the answer is what
/// makes the distinction useful: `worker PIDs  DIFFERENT / EXPECTED` says the restart worked and
/// the identity changed, which is a success in the runtime domain and would be a failure in the
/// persistent one. §25.3 forbids collapsing all of that into one word, and the `result` block
/// names the scope it verified and the scope it did not.
#[must_use]
pub fn recovery_verification(outcome: &RecoveryOutcome, width: usize) -> Vec<String> {
    let mut lines = vec![fit("RECOVERY VERIFICATION", width)];
    for (title, entries) in [
        ("persistent state", outcome.persistent()),
        ("runtime", outcome.runtime()),
        ("external side effects", outcome.external()),
    ] {
        heading(&mut lines, title);
        if entries.is_empty() {
            lines.push(fit("  nothing was observed in this domain", width));
        }
        for (subject, state) in entries {
            lines.push(fit(&row(&safe(subject), equivalence_word(*state)), width));
        }
    }
    heading(&mut lines, "result");
    // §25.2's first sentence names the one domain a local asset can actually settle. The
    // negative form is printed when it does not hold, because §10.5 makes an absent claim and a
    // denied claim different facts and the operator is entitled to the difference.
    if outcome.persistent_state_verified() {
        lines.push(fit("  PERSISTENT STATE VERIFIED", width));
    } else {
        lines.push(fit("  PERSISTENT STATE NOT VERIFIED", width));
    }
    if outcome.has_unrecoverable() {
        lines.push(fit("  SOME EFFECTS ARE OUTSIDE ANY RECOVERY", width));
    }
    lines.push(fit(&format!("  {NO_FULL_EQUIVALENCE}"), width));
    lines
}

/// `  nginx.conf             RESTORED` — a subject and what was observed about it.
fn row(subject: &str, verdict: &str) -> String {
    let padding = " ".repeat(SUBJECT.saturating_sub(display_width(subject)).max(1));
    format!("  {subject}{padding}{verdict}")
}

/// §23.3's outcomes, in the words §23.4's example prints.
const fn status_word(status: VerificationStatus) -> &'static str {
    match status {
        VerificationStatus::Passed => "PASS",
        VerificationStatus::Failed => "FAIL",
        // §23.5 forbids treating an unanswerable check as success, so it keeps its own word.
        VerificationStatus::Unknown => "UNKNOWN",
        VerificationStatus::Skipped => "SKIPPED",
    }
}

/// §4.8's plan-level outcome.
const fn verdict_word(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Verified => "VERIFIED",
        Verdict::Degraded => "DEGRADED",
        Verdict::Failed => "FAILED",
    }
}

/// §25.2's per-subject words, including the one that says a difference was expected.
const fn equivalence_word(state: EquivalenceState) -> &'static str {
    match state {
        EquivalenceState::Restored => "RESTORED",
        EquivalenceState::DifferentAsExpected => "DIFFERENT / EXPECTED",
        EquivalenceState::NotRecoverable => "NOT RECOVERABLE",
        EquivalenceState::NotRestored => "NOT RESTORED",
        EquivalenceState::Unknown => "UNKNOWN",
    }
}
