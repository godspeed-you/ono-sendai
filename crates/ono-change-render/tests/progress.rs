//! Apply progress and the failure display: Appendix E.4, E.5 and Appendix F.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::{Charset, apply_failure, apply_progress, next_steps};
use ono_value::Value;

mod support;
use support::{
    contains, empty_plan, failed_plan, nginx_plan_in, nginx_results, ready_asset, record, s,
    sealed_nginx_plan, unprotected_rows,
};

#[test]
fn should_keep_the_three_lifecycle_phases_separately_visible() {
    let lines = apply_progress(&sealed_nginx_plan(), &[], 80);
    for phase in ["PREPARE", "APPLY", "VERIFY"] {
        assert!(
            lines.iter().any(|line| line.starts_with(phase)),
            "Appendix E.4: do not show one generic progress bar that hides whether protection is complete ({phase})"
        );
    }
}

#[test]
fn should_keep_the_phases_visible_even_for_a_plan_that_has_no_actions_in_one() {
    let lines = apply_progress(&empty_plan(), &[], 80);
    assert_eq!(
        lines.len(),
        3,
        "Appendix E.4: the lifecycle boundaries are the point, and an absent phase is still one"
    );
    assert!(
        lines.iter().all(|line| line.ends_with("none")),
        "§10.5: a phase with no actions says `none`, which is not the same as `0/0` in progress"
    );
}

#[test]
fn should_count_a_phase_that_has_begun() {
    let plan = nginx_plan_in(
        "applying",
        &["succeeded", "succeeded", "pending", "pending", "pending"],
    );
    let lines = apply_progress(&plan, &[], 80);
    assert!(
        contains(&lines, "PREPARE  1/1"),
        "Appendix E.4: the operator learns from this line that protection is complete"
    );
    assert!(
        contains(&lines, "APPLY    1/3"),
        "Appendix E.4's `APPLY 7/9` is a count of actions, not a percentage"
    );
}

#[test]
fn should_call_a_phase_that_has_not_started_pending() {
    let lines = apply_progress(&sealed_nginx_plan(), &[], 80);
    assert!(
        contains(&lines, "VERIFY   pending"),
        "Appendix E.4 prints `VERIFY pending` for a phase nothing has entered"
    );
}

#[test]
fn should_count_verification_by_the_contracts_when_the_plan_has_no_verify_action() {
    let plan = record(
        "ono.change-plan",
        &[
            ("id", s("a82f1c0d9e4b7a63")),
            ("state", s("verifying")),
            ("actions", Value::list([])),
            (
                "verification_contracts",
                Value::list([
                    support::contract("required", "nginx.service", "== running"),
                    support::contract("required", "socket :443", "exists"),
                    support::contract("advisory", "worker count", "== 4"),
                ]),
            ),
        ],
    );
    let lines = apply_progress(&plan, &nginx_results(), 80);
    assert!(
        contains(&lines, "VERIFY   3/3"),
        "§23.1: a plan that expresses verification as contracts still shows the phase completing"
    );
}

#[test]
fn should_distinguish_a_prepare_failure_from_an_apply_failure() {
    let plan = nginx_plan_in(
        "prepare-failed",
        &["failed", "pending", "pending", "pending", "pending"],
    );
    let lines = apply_failure(&plan, &[], 80, Charset::Ascii);
    assert_eq!(
        lines.first().map(String::as_str),
        Some("PLAN PREPARE FAILED"),
        "§4.5 and Appendix F: a prepare failure means no mutating action ran, and the reader has \
         to be told which of the two happened"
    );
}

#[test]
fn should_title_a_failed_apply_as_an_apply_failure() {
    let lines = apply_failure(&failed_plan(), &[ready_asset()], 80, Charset::Ascii);
    assert_eq!(
        lines.first().map(String::as_str),
        Some("PLAN APPLY FAILED"),
        "Appendix E.5's headline, and §4.5 makes it different from a prepare failure"
    );
}

#[test]
fn should_render_the_blocks_appendix_e_five_writes_in_its_order() {
    let lines = apply_failure(&failed_plan(), &[ready_asset()], 80, Charset::Ascii);
    let order: Vec<usize> = ["completed", "failed", "not executed", "protection", "next"]
        .iter()
        .map(|block| {
            lines
                .iter()
                .position(|line| line == block)
                .unwrap_or_else(|| panic!("Appendix E.5's `{block}` block is missing"))
        })
        .collect();
    assert!(
        order.windows(2).all(|pair| pair[0] < pair[1]),
        "Appendix E.5's block order is what an operator reads under pressure"
    );
}

#[test]
fn should_count_what_completed_by_the_role_it_completed_in() {
    let lines = apply_failure(&failed_plan(), &[ready_asset()], 80, Charset::Ascii);
    assert!(
        contains(&lines, "mutate actions"),
        "Appendix E.5's `6 mutate actions` says which kind of work already changed the system"
    );
}

#[test]
fn should_name_the_action_that_failed() {
    let lines = apply_failure(&failed_plan(), &[ready_asset()], 80, Charset::Ascii);
    assert!(
        contains(&lines, "action 4: restart nginx.service"),
        "Appendix E.5: `action 7: restart service api-04` names it, because a count cannot be investigated"
    );
}

#[test]
fn should_count_the_actions_that_never_ran() {
    let lines = apply_failure(&failed_plan(), &[ready_asset()], 80, Charset::Ascii);
    let index = lines
        .iter()
        .position(|line| line == "not executed")
        .expect("the block exists");
    assert_eq!(
        lines[index + 1].trim(),
        "1 action",
        "Appendix E.5: what did not run is a different fact from what failed"
    );
}

#[test]
fn should_give_an_unestablished_outcome_its_own_block() {
    let plan = nginx_plan_in(
        "apply-failed",
        &["succeeded", "succeeded", "unknown", "pending", "pending"],
    );
    let lines = apply_failure(&plan, &[], 80, Charset::Ascii);
    assert!(
        contains(&lines, "outcome unknown"),
        "Appendix F.2: an outcome nobody could establish is not a failure and not a success"
    );
}

#[test]
fn should_say_that_the_created_recovery_assets_are_retained() {
    let lines = apply_failure(&failed_plan(), &[ready_asset()], 80, Charset::Ascii);
    assert!(
        contains(&lines, "retained until an operator releases them"),
        "§37.2: a plan that failed is a plan somebody may still need to recover from"
    );
}

#[test]
fn should_say_when_no_recovery_asset_was_created_at_all() {
    let lines = apply_failure(&failed_plan(), &[], 80, Charset::Ascii);
    assert!(
        contains(&lines, "no recovery asset was created"),
        "§62.1: silence where protection goes is exactly the snapshot theatre the spec names"
    );
}

#[test]
fn should_offer_more_than_one_next_step_when_more_than_one_exists() {
    let steps = next_steps(&failed_plan());
    assert!(
        steps.len() > 1,
        "Appendix E.5: Ono MUST NOT immediately suggest recovery as the only correct next step"
    );
    assert!(
        steps.iter().any(|step| step.starts_with("inspect")),
        "reading is always available and §40.2 would rather the operator read"
    );
}

#[test]
fn should_offer_recovery_as_one_step_among_others_rather_than_as_the_list() {
    let steps = next_steps(&failed_plan());
    let recover = steps
        .iter()
        .position(|step| step.starts_with("recover"))
        .expect("a recoverable plan offers recovery");
    assert!(
        steps.len() > recover + 1 || recover > 0,
        "Appendix E.5: recovery is one of the steps, never the whole of them"
    );
}

#[test]
fn should_not_offer_recovery_for_a_plan_nothing_covers() {
    let plan = record(
        "ono.change-plan",
        &[
            ("id", s("a82f1c0d9e4b7a63")),
            ("state", s("apply-failed")),
            (
                "actions",
                Value::list([support::action(
                    1,
                    "mutate",
                    "replace nginx.conf",
                    None,
                    "failed",
                    Value::list([]),
                )]),
            ),
            ("protection", unprotected_rows()),
            ("protection_level", s("unprotected")),
        ],
    );
    let steps = next_steps(&plan);
    assert!(
        !steps.iter().any(|step| step.starts_with("recover")),
        "§62.1: offering recovery where nothing covers the change is snapshot theatre"
    );
}

#[test]
fn should_render_the_steps_in_the_failure_display() {
    let lines = apply_failure(&failed_plan(), &[ready_asset()], 80, Charset::Ascii);
    let index = lines
        .iter()
        .position(|line| line == "next")
        .expect("the block exists");
    let listed = lines[index + 1..].len();
    assert!(
        listed > 1,
        "Appendix E.5's `next` block lists every step that is actually available"
    );
}

#[test]
fn should_lay_the_failure_display_out_at_the_width_it_was_given() {
    for width in [40usize, 80] {
        for line in apply_failure(&failed_plan(), &[ready_asset()], width, Charset::Ascii) {
            assert!(
                line.chars().count() <= width,
                "v0.4 §39.3: the failure display is read on whatever terminal was open"
            );
        }
    }
}
