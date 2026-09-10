//! §6.3's opaque escape keeps what Ono does not know visible (v0.6 §2.4, §6.3, §8.1, §9.6).
//!
//! §6.3 says an opaque action MUST classify impact *and* reversibility as unknown, and §2.4 says
//! an unknown MUST NOT be silently promoted. A plan view that answers "nothing was recorded as
//! unknown" or "nothing was recorded as irreversible" for an opaque command has promoted both, so
//! these tests read the view and the record the way an operator and a script would.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{home, one, ono_with};

const OPAQUE: &str = "ONO_CHANGE_ALLOW_OPAQUE_ACTIONS";

/// The lines of one section of the plan view, up to the next blank line (§20.2).
fn section(view: &str, heading: &str) -> Vec<String> {
    view.lines()
        .skip_while(|line| line.trim_end() != heading)
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// The `unknown` row of the plan view's impact section (§20.2).
fn unknown_row(view: &str) -> String {
    section(view, "impact")
        .into_iter()
        .find(|line| line.trim_start().starts_with("unknown"))
        .unwrap_or_else(|| panic!("§20.2: the impact section has an `unknown` row, got {view}"))
}

#[test]
fn should_not_report_nothing_unknown_for_an_opaque_action_in_the_plan_view() {
    let home = home();
    let run = ono_with(home.path(), OPAQUE, "true", "plan --opaque touch work/opq");
    run.assert_success();
    let view = run.stdout();
    let unknown = unknown_row(view);
    assert!(
        !unknown.contains("nothing was recorded as unknown"),
        "v0.6 §6.3 and §2.4: an opaque action's impact is unknown, so the `unknown` row cannot \
         say nothing was recorded. Got {unknown:?} in {view}"
    );
}

#[test]
fn should_name_an_opaque_action_as_unknown_impact_in_the_plan_view() {
    let home = home();
    let run = ono_with(home.path(), OPAQUE, "true", "plan --opaque touch work/opq");
    run.assert_success();
    let view = run.stdout();
    let unknown = unknown_row(view);
    assert!(
        unknown.contains("touch work/opq"),
        "v0.6 §6.3 and §2.4: an opaque action's impact is unknown, and the `unknown` row names \
         it rather than saying nothing was recorded. Got {unknown:?} in {view}"
    );
}

#[test]
fn should_name_an_opaque_action_as_not_recoverable_in_the_plan_view() {
    let home = home();
    let run = ono_with(home.path(), OPAQUE, "true", "plan --opaque touch work/opq");
    run.assert_success();
    let view = run.stdout();
    let unrecoverable = section(view, "not recoverable");
    assert!(
        unrecoverable.iter().any(|line| {
            line.contains("reversibility unknown") && line.contains("touch work/opq")
        }),
        "v0.6 §6.3: an opaque action's reversibility is unknown, and §2.4 forbids rendering an \
         unknown as recoverable. §19.4 gates only known irreversibility, so the row says \
         `reversibility unknown` and names the command. Got {unrecoverable:?} in {view}"
    );
}

#[test]
fn should_not_report_the_impact_of_an_opaque_action_as_complete() {
    let home = home();
    let run = ono_with(
        home.path(),
        OPAQUE,
        "true",
        "plan --opaque touch work/opq | to json",
    );
    run.assert_success();
    let record = one(&run);
    let summary = &record["impact_summary"];
    assert_eq!(
        summary["complete"].as_bool(),
        Some(false),
        "v0.6 §6.3 and §9.6: Ono cannot see what an opaque action touches, so its impact graph \
         is not complete. Got {summary:?}"
    );
    assert!(
        summary["boundary_count"].as_u64().unwrap_or_default() > 0,
        "v0.6 §9.6: the place the graph ends is visible as a boundary. Got {summary:?}"
    );
}
