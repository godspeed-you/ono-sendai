//! The default plan view: §20.1's fixed question order, §20.2's shape, §20.4's prominence.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::{Charset, PLAN_NOT_EXECUTED, QUESTIONS, Section, plan_view};
use ono_value::RecordValue;

mod support;
use support::{
    contains, draft_nginx_plan, empty_plan, failed_plan, headings, index_of, nginx_plan_in,
    sealed_nginx_plan, zfs_asset,
};

fn view(plan: &RecordValue) -> Vec<String> {
    plan_view(plan, &[zfs_asset()], 80, Charset::Ascii)
}

/// The §4.1 states before the first mutation, which Appendix F makes one class of answer.
const BEFORE_APPLYING: [&str; 7] = [
    "draft",
    "resolved",
    "sealed",
    "expired",
    "preparing",
    "prepare-failed",
    "protected",
];

#[test]
fn should_answer_the_nine_questions_of_section_twenty_one_in_order() {
    let asked: Vec<&str> = Section::ORDER
        .iter()
        .flat_map(|section| section.questions().iter().copied())
        .collect();
    assert_eq!(
        asked,
        QUESTIONS.to_vec(),
        "§20.1 fixes the order a plan answers the operator's questions in, and it is not negotiable"
    );
}

#[test]
fn should_render_the_sections_in_the_order_section_twenty_two_writes_them() {
    let lines = view(&sealed_nginx_plan());
    let found = headings(&lines[1..]);
    let expected: Vec<String> = Section::ORDER
        .iter()
        .map(|section| section.heading().to_owned())
        .chain(std::iter::once(PLAN_NOT_EXECUTED.to_owned()))
        .collect();
    assert_eq!(
        found, expected,
        "§20.1 and §20.2 fix the block order; reordering the view reorders what the reader learns first"
    );
}

#[test]
fn should_render_every_section_even_when_the_plan_has_nothing_to_say_in_it() {
    let lines = plan_view(&empty_plan(), &[], 80, Charset::Ascii);
    for section in Section::ORDER {
        assert!(
            contains(&lines, section.heading()),
            "§10.5: a missing `{}` block reads as an answer, and it is not one",
            section.heading()
        );
    }
}

#[test]
fn should_name_the_intent_in_the_words_the_operator_used() {
    let lines = view(&sealed_nginx_plan());
    assert!(
        contains(&lines, "replace nginx configuration and restart service"),
        "§20.1 question 1 is answered from the operator's own sentence"
    );
}

#[test]
fn should_list_the_concrete_objects_the_selectors_resolved_to() {
    let lines = view(&sealed_nginx_plan());
    assert!(
        contains(&lines, "/etc/nginx/nginx.conf") && contains(&lines, "nginx.service"),
        "§20.1 question 2 and §7.1: the view names objects, never selectors"
    );
}

#[test]
fn should_number_the_planned_actions_in_the_order_the_plan_holds_them() {
    let lines = view(&sealed_nginx_plan());
    let first = index_of(&lines, "snapshot rpool/ROOT/debian").expect("the prepare action shows");
    let last = index_of(&lines, "verify service running").expect("the verify action shows");
    assert!(
        first < last,
        "§20.1 question 3: what Ono expects to happen is a sequence, and the order is the answer"
    );
}

#[test]
fn should_show_the_impact_rows_section_twenty_two_writes() {
    let lines = view(&sealed_nginx_plan());
    for row in ["direct", "related", "possible", "unknown"] {
        assert!(
            lines.iter().any(|line| line.trim_start().starts_with(row)),
            "§20.2's impact block answers questions 4 and 5 with the `{row}` row"
        );
    }
}

#[test]
fn should_put_protection_above_risk_and_verification() {
    let lines = view(&sealed_nginx_plan());
    let protection = index_of(&lines, "protection").expect("§20.4 requires a protection block");
    let risk = index_of(&lines, "\nrisk").or_else(|| lines.iter().position(|line| line == "risk"));
    let verification = lines.iter().position(|line| line == "verification");
    assert!(
        protection < risk.expect("a risk block")
            && protection < verification.expect("a verification block"),
        "§20.4: protection MUST be visible near the top-level plan summary"
    );
}

#[test]
fn should_show_protection_without_asking_for_a_verbose_inspector() {
    let lines = view(&sealed_nginx_plan());
    assert!(
        contains(&lines, "PROTECTED"),
        "§20.4: protection MUST NOT be hidden under a verbose inspector, and the default view has no flag to hide it behind"
    );
    assert!(
        contains(&lines, "rpool/ROOT/debian@ono-a82f"),
        "§64: the planned recovery asset is part of the default plan view"
    );
}

#[test]
fn should_name_what_remains_unrecoverable() {
    let lines = view(&sealed_nginx_plan());
    let heading = lines
        .iter()
        .position(|line| line == "not recoverable")
        .expect("§20.1 question 7 has a block");
    let body = &lines[heading + 1..heading + 4];
    assert!(
        body.iter().any(|line| line.contains("active TCP sessions")),
        "§20.1 question 7 and §2.13: what nothing brings back is stated, never implied"
    );
}

#[test]
fn should_not_repeat_a_subject_named_by_both_an_exclusion_and_an_effect() {
    let lines = view(&sealed_nginx_plan());
    let mentions = lines
        .iter()
        .skip(
            lines
                .iter()
                .position(|line| line == "not recoverable")
                .expect("the block exists"),
        )
        .take_while(|line| !line.is_empty())
        .filter(|line| line.contains("active TCP sessions"))
        .count();
    assert_eq!(
        mentions, 1,
        "a reader counting the irreversible list must not read one loss as two"
    );
}

#[test]
fn should_print_the_risk_class_the_assessment_computed() {
    let lines = view(&sealed_nginx_plan());
    assert!(
        contains(&lines, "MODERATE"),
        "§19.2 and §50.1: the class is read off the assessment, never decided here"
    );
    assert!(
        contains(
            &lines,
            "restarting nginx interrupts the connections it is serving"
        ),
        "§40.2: a gate that cannot say why it is gating teaches the operator to ignore it"
    );
}

#[test]
fn should_answer_the_reboot_question_even_when_the_answer_is_no() {
    let lines = view(&sealed_nginx_plan());
    let index = lines
        .iter()
        .position(|line| line == "reboot")
        .expect("§13.7's question is answered");
    assert_eq!(
        lines[index + 1].trim(),
        "no",
        "§10.4: a protected plan whose way back is a reboot is not a cheap plan, so the view always says"
    );
}

#[test]
fn should_list_the_verification_contracts_the_plan_carries() {
    let lines = view(&sealed_nginx_plan());
    assert!(
        contains(&lines, "nginx.service == running"),
        "§20.1 question 8 and §23.1: a mutating plan carries contracts and the view shows them"
    );
    assert!(
        contains(&lines, "(advisory)"),
        "§23.2: an advisory expectation means something different from a required one"
    );
}

/// The nginx plan with §19.4's known irreversible action: a rule found the irreversibility.
fn irreversible_nginx_plan() -> RecordValue {
    support::rewritten(
        &sealed_nginx_plan(),
        "risk_findings",
        ono_value::Value::list([support::map(&[
            ("dimension", support::s("irreversibility")),
            ("class", support::s("moderate")),
            ("rule", support::s("rule.webhook.emit")),
            ("reason", support::s("the reload webhook cannot be un-sent")),
        ])]),
    )
}

/// The line after `approval`, which is §20.1's answer to question 9.
fn approval(lines: &[String]) -> Vec<String> {
    block(lines, "approval")
        .into_iter()
        .map(|line| line.trim().to_owned())
        .collect()
}

#[test]
fn should_name_the_acknowledgement_an_irreversible_plan_still_needs() {
    let lines = view(&irreversible_nginx_plan());
    let index = lines
        .iter()
        .position(|line| line == "approval")
        .expect("§20.1 question 9 is answered");
    assert!(
        lines[index + 1].contains("--accept-irreversible"),
        "§19.4: a plan with an irreversible effect is gated, and §40.3 names the flag that opens it"
    );
}

#[test]
fn should_say_no_approval_is_required_when_none_is() {
    let lines = plan_view(&empty_plan(), &[], 80, Charset::Ascii);
    let index = lines
        .iter()
        .position(|line| line == "approval")
        .expect("§20.1 question 9 is answered");
    assert_eq!(
        lines[index + 1].trim(),
        "none required",
        "§40.1: a gate on every plan is a gate nobody reads, and the view says so plainly"
    );
}

#[test]
fn should_stop_asking_for_an_acknowledgement_the_plan_already_carries() {
    let approval = |lines: &[String]| -> String {
        let index = lines
            .iter()
            .position(|line| line == "approval")
            .expect("§20.1 question 9 is answered");
        lines[index + 1].trim().to_owned()
    };
    // The control: the same sealed plan, with nothing acknowledged, is gated.
    let unacknowledged = plan_view(&irreversible_nginx_plan(), &[], 80, Charset::Ascii);
    assert!(
        approval(&unacknowledged).contains("--accept-irreversible"),
        "precondition: a rule found the plan irreversible, so it asks"
    );
    let accepted = support::rewritten(
        &irreversible_nginx_plan(),
        "accepted_risk_overrides",
        support::list(&["--accept-irreversible"]),
    );
    let lines = plan_view(&accepted, &[], 80, Charset::Ascii);
    let index = lines
        .iter()
        .position(|line| line == "approval")
        .expect("§20.1 question 9 is answered");
    assert_eq!(
        lines[index + 1].trim(),
        "none required",
        "§19.4: an acknowledgement recorded in the sealed revision is not asked for twice"
    );
}

#[test]
fn should_close_with_plan_not_executed_for_a_draft_plan() {
    let lines = plan_view(&draft_nginx_plan(), &[], 80, Charset::Ascii);
    assert_eq!(
        lines.last().map(String::as_str),
        Some(PLAN_NOT_EXECUTED),
        "§2.1 and §62.3: planning is side-effect free, and the operator learns it from this line"
    );
}

#[test]
fn should_close_with_plan_not_executed_for_every_state_before_applying() {
    for state in BEFORE_APPLYING {
        let lines = plan_view(&nginx_plan_in(state, &[]), &[], 80, Charset::Ascii);
        assert!(
            contains(&lines, PLAN_NOT_EXECUTED),
            "Appendix F: everything before the first mutation is told as nothing having happened ({state})"
        );
    }
}

#[test]
fn should_drop_plan_not_executed_for_every_state_from_applying_onwards() {
    for state in [
        "applying",
        "apply-failed",
        "verifying",
        "verified",
        "degraded",
        "failed",
        "closed",
        "recovery-planned",
        "recovering",
        "recovered",
        "recovery-failed",
        "recovery-verified",
    ] {
        let lines = plan_view(&nginx_plan_in(state, &[]), &[], 80, Charset::Ascii);
        assert!(
            !contains(&lines, PLAN_NOT_EXECUTED),
            "Appendix F: at or after APPLYING the operator must be told something may have happened ({state})"
        );
    }
}

#[test]
fn should_drop_plan_not_executed_from_a_failed_plan() {
    let lines = plan_view(&failed_plan(), &[], 80, Charset::Ascii);
    assert!(
        !contains(&lines, PLAN_NOT_EXECUTED),
        "§4.7: what ran, ran, and the view may not say otherwise"
    );
}

#[test]
fn should_carry_the_plan_identity_and_revision_in_the_title() {
    let plan = sealed_nginx_plan();
    let lines = view(&plan);
    let title = lines.first().expect("a title");
    assert!(
        title.starts_with("PLAN / ") && title.contains("a82f"),
        "§20.2 and §36.4: the title carries the reference an operator types back"
    );
    assert!(
        title.contains("rev 3"),
        "§7.5: the revision is part of the identity a reader cites"
    );
}

#[test]
fn should_lay_out_at_the_width_the_caller_states() {
    for width in [40usize, 80, 200] {
        let lines = plan_view(&sealed_nginx_plan(), &[zfs_asset()], width, Charset::Ascii);
        for line in &lines {
            assert!(
                line.chars().count() <= width,
                "§50 and v0.4 §39.3: the layout honours the width it was given, at {width} columns"
            );
        }
    }
}

#[test]
fn should_render_the_same_bytes_for_the_same_plan() {
    let plan = sealed_nginx_plan();
    assert_eq!(
        plan_view(&plan, &[zfs_asset()], 80, Charset::Ascii),
        plan_view(&plan, &[zfs_asset()], 80, Charset::Ascii),
        "§50: rendering is deterministic, and nothing here reads a clock or a terminal"
    );
}

#[test]
fn should_neutralise_control_characters_a_target_label_carries() {
    let plan = support::record(
        "ono.change-plan",
        &[
            ("id", support::s("0102030405060708")),
            ("state", support::s("draft")),
            ("intent", support::s("replace \u{1b}[2Jthe file")),
        ],
    );
    let lines = plan_view(&plan, &[], 80, Charset::Ascii);
    assert!(
        !lines.iter().any(|line| line.contains('\u{1b}')),
        "v0.2 §49: an escape in a plan's own text must not reach the terminal"
    );
}

/// §6.3's opaque escape as `ono.change-plan/1` carries it: one effect whose domain, kind and
/// confidence are all unknown, which is not marked irreversible because §19.4 gates only *known*
/// irreversibility, and a graph that ends at the boundary the command is.
fn opaque_plan() -> RecordValue {
    let explanation = "opaque action `touch work/opq`: the operator acknowledged that Ono cannot \
                       reason about what this command touches (§6.3), so its domain, its scope \
                       and its reversibility are all unknown";
    let effect = support::nested(
        "ono.proposed-effect",
        &[
            ("id", support::s("effect/opaque")),
            ("object", ono_value::Value::Null),
            ("domain", support::s("unknown")),
            ("kind", support::s("unknown")),
            ("confidence", support::s("unknown")),
            ("explanation", support::s(explanation)),
            ("irreversible", ono_value::Value::Bool(false)),
        ],
    );
    support::record(
        "ono.change-plan",
        &[
            ("id", support::s("97d5c0de12345678")),
            ("revision", support::i(1)),
            ("state", support::s("sealed")),
            ("intent", support::s("touch work/opq")),
            (
                "targets",
                ono_value::Value::list([support::map(&[
                    ("schema", support::s("ono.host/1")),
                    ("identity", support::s("localhost")),
                    ("label", support::s("localhost")),
                ])]),
            ),
            (
                "actions",
                ono_value::Value::list([support::action(
                    1,
                    "mutate",
                    "opaque action: touch work/opq",
                    Some("localhost"),
                    "pending",
                    ono_value::Value::list([effect.clone()]),
                )]),
            ),
            ("effects", ono_value::Value::list([effect])),
            (
                "impact_summary",
                support::map(&[
                    ("direct_targets", support::i(1)),
                    ("boundaries", support::i(1)),
                    ("complete", ono_value::Value::Bool(false)),
                    (
                        "truncated_reason",
                        support::s(
                            "the opaque action `touch work/opq` has no model Ono can traverse",
                        ),
                    ),
                    (
                        "boundary_labels",
                        support::list(&["opaque action: touch work/opq"]),
                    ),
                ]),
            ),
            (
                "protection",
                ono_value::Value::list([support::nested(
                    "ono.protection-coverage",
                    &[
                        ("domain", support::s("unknown")),
                        ("objective", support::s("unknown")),
                        ("protection", support::s("unprotected")),
                        ("required", ono_value::Value::Bool(true)),
                        ("exclusions", ono_value::Value::list([])),
                        ("note", support::s("nothing covers an unknown domain")),
                    ],
                )]),
            ),
            ("protection_level", support::s("unprotected")),
            ("coverage_exclusions", ono_value::Value::list([])),
            ("risk", support::s("unknown")),
            ("risk_findings", ono_value::Value::list([])),
            ("accepted_risk_overrides", ono_value::Value::list([])),
        ],
    )
}

/// The lines of one block of the view, up to the blank line that ends it.
fn block(lines: &[String], heading: &str) -> Vec<String> {
    lines
        .iter()
        .skip_while(|line| line.as_str() != heading)
        .skip(1)
        .take_while(|line| !line.is_empty())
        .cloned()
        .collect()
}

#[test]
fn should_list_an_effect_whose_reversibility_is_unknown_as_not_recoverable() {
    let lines = view(&opaque_plan());
    let body = block(&lines, "not recoverable");
    assert!(
        body.iter()
            .any(|line| line.contains("reversibility unknown: opaque action `touch work/opq`")),
        "v0.6 §6.3 makes an opaque action's reversibility unknown and §2.4 forbids promoting it, \
         so §20.1 question 7 names it. Got {body:?}"
    );
    assert!(
        !body
            .iter()
            .any(|line| line.contains("nothing was recorded")),
        "§2.4: an unknown reversibility is not an absence of loss. Got {body:?}"
    );
}

#[test]
fn should_not_list_an_unknown_effect_a_recovery_asset_covers_as_not_recoverable() {
    let plan = support::rewritten(
        &sealed_nginx_plan(),
        "effects",
        ono_value::Value::list([support::nested(
            "ono.proposed-effect",
            &[
                ("id", support::s("effect/conf")),
                ("object", support::s("/etc/nginx/conf.d")),
                ("domain", support::s("filesystem-persistent")),
                ("kind", support::s("modify")),
                ("confidence", support::s("unknown")),
                ("explanation", support::s("an include nobody resolved")),
                ("irreversible", ono_value::Value::Bool(false)),
            ],
        )]),
    );
    let body = block(&view(&plan), "not recoverable");
    assert!(
        !body
            .iter()
            .any(|line| line.contains("reversibility unknown")),
        "§10.3: an effect in a domain a recovery asset protects is recoverable whatever the \
         provider could not say about it. Got {body:?}"
    );
}

#[test]
fn should_name_the_boundary_an_opaque_action_is_in_the_unknown_row() {
    let lines = view(&opaque_plan());
    let row = block(&lines, "impact")
        .into_iter()
        .find(|line| line.trim_start().starts_with("unknown"))
        .expect("§20.2's impact block has an `unknown` row");
    assert!(
        row.contains("touch work/opq"),
        "§6.3 and §9.6: the row names where Ono's knowledge ends, and for an opaque action that \
         is the command. Got {row:?}"
    );
    assert!(
        !row.contains("external boundar"),
        "§9.6: a boundary in an unknown domain is not an external one. Got {row:?}"
    );
    assert!(
        row.chars().count() <= 80,
        "the row fits the width it was laid out at. Got {row:?}"
    );
}

#[test]
fn should_not_ask_for_an_irreversible_acknowledgement_that_only_an_exclusion_suggests() {
    // The nginx plan excludes TCP sessions nothing can re-establish, and no rule found an
    // irreversible action: §19.4 gates the second and never the first.
    let lines = view(&sealed_nginx_plan());
    assert_eq!(
        approval(&lines),
        vec!["none required".to_owned()],
        "§19.4 requires `accept_irreversible` for known irreversible actions, which the rules \
         find; a coverage exclusion is §10.3's answer about protection, and the shell never asks \
         for a flag because of one"
    );
}

#[test]
fn should_ask_for_the_acknowledgements_the_shell_says_are_outstanding() {
    let high = support::rewritten(&sealed_nginx_plan(), "risk", support::s("high"));
    let mut builder = RecordValue::builder(
        std::sync::Arc::clone(high.schema()),
        high.provenance().clone(),
    );
    for field in high.schema().fields() {
        builder = builder
            .set(
                field.name(),
                high.get(field.name())
                    .cloned()
                    .unwrap_or(ono_value::Value::Null),
            )
            .expect("declared");
    }
    let gated = builder
        .set_extra(
            "ono.change/outstanding-acknowledgements",
            ono_value::Value::list([support::map(&[
                ("flag", support::s("--accept-service-outage")),
                (
                    "reason",
                    support::s("no healthy member of the role remains"),
                ),
            ])]),
        )
        .build();
    let lines = view(&gated);
    let asked = approval(&lines);
    assert!(
        asked
            .iter()
            .any(|line| line.starts_with("--accept-service-outage")
                && line.contains("no healthy member")),
        "§40.2 and §40.3: the view names the flag the shell will ask for, with its reason. Got \
         {asked:?}"
    );
    assert!(
        !asked.iter().any(|line| line.contains("--accept-risk")),
        "the view does not ask for a flag the shell does not name. Got {asked:?}"
    );
}
