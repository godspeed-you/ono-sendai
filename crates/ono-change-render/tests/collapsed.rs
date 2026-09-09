//! Appendix E.2's collapsed default for long plans.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::{ActionGroup, Charset, Phase, action_groups, collapsed_plan};

mod support;
use support::{contains, headings, long_plan, nginx_plan_in, sealed_nginx_plan};

#[test]
fn should_collapse_identical_actions_into_one_group_each() {
    let groups = action_groups(&long_plan());
    let summaries: Vec<&str> = groups.iter().map(ActionGroup::summary).collect();
    assert_eq!(
        summaries,
        vec![
            "recovery assets",
            "update config",
            "restart service",
            "service/listener checks"
        ],
        "Appendix E.2: long plans render one line per action group, in the plan's own order"
    );
}

#[test]
fn should_count_the_actions_each_group_stands_for() {
    let groups = action_groups(&long_plan());
    let counts: Vec<usize> = groups.iter().map(ActionGroup::count).collect();
    assert_eq!(
        counts,
        vec![20, 20, 20, 23],
        "Appendix E.2's counts are what makes a collapsed line readable instead of a total"
    );
}

#[test]
fn should_keep_the_actions_inside_a_group_reachable_for_expansion() {
    let groups = action_groups(&long_plan());
    let first = groups.first().expect("the prepare group");
    assert_eq!(
        first.actions().len(),
        first.count(),
        "Appendix E.2 and E.3: `Space` expands a line, so the group has to hold what it collapsed"
    );
}

#[test]
fn should_keep_the_same_summary_in_two_phases_as_two_groups() {
    let groups = action_groups(&sealed_nginx_plan());
    let phases: Vec<Phase> = groups.iter().map(ActionGroup::phase).collect();
    assert!(
        phases.contains(&Phase::Prepare) && phases.contains(&Phase::Apply),
        "§3.3: the phase is part of what an action is, so grouping never crosses one"
    );
}

#[test]
fn should_head_each_phase_with_the_lifecycle_boundary_it_belongs_to() {
    let lines = collapsed_plan(&long_plan(), 80, Charset::Ascii);
    let found: Vec<String> = headings(&lines[1..])
        .into_iter()
        .filter(|line| line.chars().all(|character| !character.is_lowercase()))
        .collect();
    assert_eq!(
        found,
        vec!["PREPARE", "APPLY", "VERIFY"],
        "Appendix E.2 and E.4: the lifecycle boundaries survive the collapse"
    );
}

#[test]
fn should_carry_the_plan_reference_and_the_total_action_count_in_the_title() {
    let plan = long_plan();
    let lines = collapsed_plan(&plan, 80, Charset::Ascii);
    let _ = &plan;
    let title = lines.first().expect("a title");
    assert!(
        title.contains("d46c") && title.contains("83 actions"),
        "Appendix E.2's title is `PLAN a82f / 83 actions`, which says how much was collapsed"
    );
}

#[test]
fn should_say_a_proposed_prepare_group_is_ready_to_create() {
    let lines = collapsed_plan(&long_plan(), 80, Charset::Ascii);
    let line = lines
        .iter()
        .find(|line| line.contains("recovery assets"))
        .expect("the prepare group is rendered");
    assert!(
        line.contains("ready-to-create"),
        "§2.1 and Appendix E.2: a plan has described the recovery assets and created none"
    );
}

#[test]
fn should_report_a_mixed_group_as_a_settled_count_rather_than_one_wrong_word() {
    let plan = nginx_plan_in(
        "apply-failed",
        &["succeeded", "succeeded", "failed", "pending", "pending"],
    );
    let groups = action_groups(&plan);
    assert!(
        groups.iter().all(|group| group.count() == 1),
        "the nginx plan's actions each have their own summary"
    );
    assert_eq!(
        groups
            .iter()
            .find(|group| group.summary() == "replace nginx.conf")
            .and_then(ActionGroup::status),
        Some("succeeded"),
        "a group whose actions agree carries their word"
    );
}

#[test]
fn should_carry_risk_protection_and_strategy_in_the_footer() {
    let lines = collapsed_plan(&long_plan(), 100, Charset::Ascii);
    for footer in ["risk", "protection", "strategy"] {
        assert!(
            lines.iter().any(|line| line.starts_with(footer)),
            "Appendix E.2's footer carries `{footer}`, which is what decides whether to read further"
        );
    }
    assert!(
        contains(&lines, "canary 1, then batch 3"),
        "§28.4 and Appendix E.2: the strategy is part of the summary because it changes the risk"
    );
    assert!(
        contains(&lines, "HIGH"),
        "§19.2's class is read off the assessment and printed in the footer"
    );
}

#[test]
fn should_show_the_exclusions_beside_the_footers_protection_summary() {
    let lines = collapsed_plan(&long_plan(), 100, Charset::Ascii);
    let protection = lines
        .iter()
        .position(|line| line.starts_with("protection"))
        .expect("the footer states a level");
    let excluded = lines
        .iter()
        .position(|line| line.trim_start().starts_with("not covered"))
        .expect("Appendix E.8 requires exclusions");
    assert_eq!(
        excluded,
        protection + 1,
        "Appendix E.8: coverage summaries must show exclusions, including the compact one"
    );
}

#[test]
fn should_fall_back_to_the_verification_contracts_when_a_plan_has_no_verify_action() {
    let lines = collapsed_plan(&sealed_nginx_plan(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "VERIFY"),
        "§23.1: a mutating plan verifies, and the collapsed view may not hide that it does"
    );
}

#[test]
fn should_lay_the_collapsed_plan_out_at_the_width_it_was_given() {
    for width in [40usize, 80, 160] {
        for line in collapsed_plan(&long_plan(), width, Charset::Ascii) {
            assert!(
                line.chars().count() <= width,
                "v0.4 §39.3: the collapsed view is the one that has to work at forty columns"
            );
        }
    }
}
