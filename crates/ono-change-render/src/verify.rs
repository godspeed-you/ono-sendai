//! Verification views: §23.4 for a plan, §25.2 for a recovery.
//!
//! §2.14 is the invariant both views serve: verification is separate from execution success. A
//! command that returned zero proved that it returned zero, and §62.9 names answering the other
//! question from an exit code as a failure mode. So every line here is an observation an
//! `ono.change-verification/1` recorded, and the closing verdict is composed from §23.2's classes
//! rather than asserted.
//!
//! §25.3 governs the recovery half and it is a language rule with teeth: *"User-visible language
//! MUST describe the verified scope."* [`recovery_verification`] therefore reports §25.1's three
//! equivalence domains separately and closes with two sentences, one of which is
//! [`NO_FULL_EQUIVALENCE`]. There is no code path here that emits a global claim about a
//! rollback, and the crate's tests read every string it can emit to keep it that way.
//!
//! §25.2's per-subject words are finer than §23.3's four outcomes: `DIFFERENT / EXPECTED` is a
//! statement that a difference was anticipated, which no status carries. A producer that has
//! established it puts it in `equivalence_state`, and where none is present the status is read
//! instead — a difference nobody classified stays a plain observation rather than becoming an
//! expected one, because §2.4 forbids promoting an unknown.

use ono_value::RecordValue;

use crate::{display_width, fit, heading, text};

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
/// `plan` is an `ono.change-plan/1` and `results` are its `ono.change-verification/1` records.
/// The three blocks are §23.2's classes, and they are kept apart because they mean different
/// things about the plan: a failed required postcondition makes the plan `FAILED`, a failed
/// advisory one may make it `DEGRADED`, and an observation never changes the state at all.
/// Folding them into one list would make the reader do the classification §23.2 already did.
#[must_use]
pub fn verification_view(
    plan: &RecordValue,
    results: &[RecordValue],
    width: usize,
) -> Vec<String> {
    let mut lines = vec![fit(
        &format!("VERIFY / plan {}", crate::plan::short(plan, "id")),
        width,
    )];
    for (class, title) in [
        ("required", "required"),
        ("advisory", "advisory"),
        ("observational", "observed"),
    ] {
        let of_class: Vec<&RecordValue> = results
            .iter()
            .filter(|result| text(*result, "class").as_deref() == Some(class))
            .collect();
        if of_class.is_empty() {
            continue;
        }
        heading(&mut lines, title);
        for result in of_class {
            let subject = subject_of(result);
            let status = text(result, "status").unwrap_or_else(|| "unknown".to_owned());
            lines.push(fit(&row(&subject, status_word(&status)), width));
        }
    }
    if results.is_empty() {
        heading(&mut lines, "observed");
        lines.push(fit("  no contract was answered", width));
    }
    heading(&mut lines, "status");
    lines.push(fit(&format!("  {}", verdict(results)), width));
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
pub fn recovery_verification(results: &[RecordValue], width: usize) -> Vec<String> {
    let mut lines = vec![fit("RECOVERY VERIFICATION", width)];
    let mut persistent_states: Vec<&'static str> = Vec::new();
    let mut unrecoverable = false;
    for (domain, title) in [
        ("persistent-state", "persistent state"),
        ("runtime-state", "runtime"),
        ("external-side-effect", "external side effects"),
    ] {
        heading(&mut lines, title);
        let of_domain: Vec<&RecordValue> = results
            .iter()
            .filter(|result| text(*result, "equivalence_domain").as_deref() == Some(domain))
            .collect();
        if of_domain.is_empty() {
            lines.push(fit("  nothing was observed in this domain", width));
        }
        for result in of_domain {
            let word = equivalence_word(result);
            if domain == "persistent-state" {
                persistent_states.push(word);
            }
            unrecoverable |= word == "NOT RECOVERABLE";
            lines.push(fit(&row(&subject_of(result), word), width));
        }
    }
    heading(&mut lines, "result");
    // §25.2's first sentence names the one domain a local asset can actually settle. The negative
    // form is printed when it does not hold, because §10.5 makes an absent claim and a denied
    // claim different facts and the operator is entitled to the difference.
    if !persistent_states.is_empty() && persistent_states.iter().all(|word| *word == "RESTORED") {
        lines.push(fit("  PERSISTENT STATE VERIFIED", width));
    } else {
        lines.push(fit("  PERSISTENT STATE NOT VERIFIED", width));
    }
    if unrecoverable {
        lines.push(fit("  SOME EFFECTS ARE OUTSIDE ANY RECOVERY", width));
    }
    lines.push(fit(&format!("  {NO_FULL_EQUIVALENCE}"), width));
    lines
}

/// `nginx.service == running` — what the check was about, as §23.4's example writes it.
fn subject_of(result: &RecordValue) -> String {
    let subject = text(result, "subject").unwrap_or_else(|| "unnamed".to_owned());
    match text(result, "expression") {
        Some(expression) => format!("{subject} {expression}"),
        None => subject,
    }
}

/// `  nginx.conf             RESTORED` — a subject and what was observed about it.
fn row(subject: &str, verdict: &str) -> String {
    let padding = " ".repeat(SUBJECT.saturating_sub(display_width(subject)).max(1));
    format!("  {subject}{padding}{verdict}")
}

/// §23.3's outcomes, in the words §23.4's example prints.
///
/// `unknown` keeps its own word: §23.5 forbids treating a check that could not be answered as
/// success, and a blank cell is exactly how that happens.
const fn status_word(status: &str) -> &'static str {
    match status.as_bytes() {
        b"passed" => "PASS",
        b"failed" => "FAIL",
        b"skipped" => "SKIPPED",
        _ => "UNKNOWN",
    }
}

/// §4.8's plan-level outcome, composed from §23.2's classes.
///
/// A failed required postcondition is `FAILED`; a failed or unanswerable advisory one is
/// `DEGRADED`; an observation changes nothing. An unanswerable *required* check is `DEGRADED`
/// rather than `VERIFIED`, because §23.5 forbids reading it as a pass and §2.4 forbids promoting
/// it to one.
fn verdict(results: &[RecordValue]) -> &'static str {
    let mut degraded = false;
    for result in results {
        let class = text(result, "class").unwrap_or_else(|| "observational".to_owned());
        let status = text(result, "status").unwrap_or_else(|| "unknown".to_owned());
        match (class.as_str(), status.as_str()) {
            ("required", "failed") => return "FAILED",
            ("required", "unknown") | ("advisory", "failed" | "unknown") => degraded = true,
            _ => {}
        }
    }
    if degraded { "DEGRADED" } else { "VERIFIED" }
}

/// §25.2's per-subject words, including the one that says a difference was expected.
///
/// The word comes from `equivalence_state` where a producer established one. §46's
/// `ono.change-verification/1` does not yet carry that field, so a result without it is read from
/// its §23.3 status — which never invents `DIFFERENT / EXPECTED`, because "this difference was
/// anticipated" is a fact and §50.1 forbids a renderer from deciding one.
fn equivalence_word(result: &RecordValue) -> &'static str {
    match text(result, "equivalence_state").as_deref() {
        Some("restored") => return "RESTORED",
        Some("different-as-expected") => return "DIFFERENT / EXPECTED",
        Some("not-recoverable") => return "NOT RECOVERABLE",
        Some("not-restored") => return "NOT RESTORED",
        Some("unknown") | None => {}
        Some(_) => return "UNKNOWN",
    }
    match text(result, "status").as_deref() {
        Some("passed") => "RESTORED",
        Some("failed") => "NOT RESTORED",
        Some("skipped") => "NOT RECOVERABLE",
        _ => "UNKNOWN",
    }
}
