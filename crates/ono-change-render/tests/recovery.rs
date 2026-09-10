//! The recovery plan view: §24.4, §24.5, Appendix E.6 and §62.8.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::{Charset, NEWER_STATE_AT_RISK, RECOVERY_NOT_EXECUTED, recovery_view};
use ono_value::Value;

mod support;
use support::{
    contains, index_of, list, map, rewritten, rollback_recovery, s, selective_recovery,
    unanalysed_recovery,
};

/// The lines of one block of the view, up to the blank line that ends it.
fn block(lines: &[String], heading: &str) -> Vec<String> {
    lines
        .iter()
        .skip_while(|line| line.as_str() != heading)
        .skip(1)
        .take_while(|line| !line.is_empty())
        .map(|line| line.trim().to_owned())
        .collect()
}

#[test]
fn should_title_a_recovery_plan_distinctly_from_a_change_plan() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    let title = lines.first().expect("a title");
    assert!(
        title.starts_with("RECOVERY PLAN / "),
        "Appendix E.6: recovery uses a visually distinct title, because it is a different act"
    );
}

#[test]
fn should_name_the_objects_it_would_restore_under_the_restore_target() {
    // Appendix E.6's `RESTORE TARGET` is the objects being restored, which is what an operator
    // scans the block for. The state they would be restored *from* is a fact about the source,
    // and it sits with the plan and the asset that hold it.
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    let target = index_of(&lines, "RESTORE TARGET").expect("the restore target exists");
    let named = lines[target + 1..]
        .iter()
        .take_while(|line| line.starts_with("  "))
        .any(|line| line.contains('/'));
    assert!(
        named,
        "the objects the recovery would put back are under `RESTORE TARGET`. Got {lines:?}"
    );
    let source = index_of(&lines, "source").expect("the source block exists");
    assert!(
        newer_first(&lines) && target < source,
        "Appendix E.6: newer-state impact, then the target, then where it comes from. \
         Got {lines:?}"
    );
}

/// Whether Appendix E.6's newer-state block is above everything else it orders.
fn newer_first(lines: &[String]) -> bool {
    let newer = index_of(lines, NEWER_STATE_AT_RISK).unwrap_or(usize::MAX);
    let target = index_of(lines, "RESTORE TARGET").unwrap_or(0);
    newer < target
}

#[test]
fn should_put_the_newer_state_block_above_the_restore_target() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    let newer = index_of(&lines, NEWER_STATE_AT_RISK).expect("the newer-state block exists");
    let target = index_of(&lines, "RESTORE TARGET").expect("the restore target exists");
    assert!(
        newer < target,
        "Appendix E.6's layout is explicit: newer-state impact, then the target"
    );
}

#[test]
fn should_say_the_analysis_did_not_run_rather_than_that_nothing_is_at_risk() {
    let lines = recovery_view(&unanalysed_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "the newer-state analysis did not run"),
        "§62.8: recovering without drift analysis is a failure mode, and the view names it"
    );
    assert!(
        contains(&lines, "unknown, not nothing"),
        "§2.4 and §10.5: an absent analysis is not an absence of risk"
    );
}

#[test]
fn should_gate_an_unanalysed_recovery_on_explicit_acceptance() {
    let lines = recovery_view(&unanalysed_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "explicit accept-newer-state-loss"),
        "§24.5 and §56.3: no recovery execution occurs without the explicit gate, and an unanalysed one is gated"
    );
}

#[test]
fn should_name_the_source_plan_and_the_asset_recovery_would_use() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "plan ") && contains(&lines, "recovery asset recovery/"),
        "§24.4's `source` block names the plan and the asset a reader can go and look at"
    );
}

#[test]
fn should_list_the_objects_recovery_would_restore() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "/etc/nginx/nginx.conf"),
        "§24.4's `restore` block is the answer to what recovery is actually for"
    );
}

#[test]
fn should_name_the_method_recovery_would_use() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "selective-file-restore"),
        "Appendix C.1: the least-destructive method that satisfies the goal, named as itself"
    );
}

#[test]
fn should_list_the_newer_state_the_method_leaves_alone() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "newer state preserved"),
        "§24.4's block exists so an operator can see what survives"
    );
    assert!(
        contains(&lines, "/etc/ssh/sshd_config") && contains(&lines, "/etc/hosts"),
        "§24.2: files changed after the plan are the reason recovery is a planner and not a button"
    );
}

#[test]
fn should_name_the_effects_recovery_cannot_reverse() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "TCP sessions"),
        "§24.3: what recovery cannot reverse is part of the recovery plan"
    );
}

#[test]
fn should_keep_a_compensation_beside_the_effect_it_does_not_undo() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "compensated by clients reconnect"),
        "§35.3: a compensation is COMPENSATABLE and the original effect stays in the list"
    );
}

#[test]
fn should_say_which_file_metadata_a_restore_does_not_return() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "SELinux labels"),
        "Appendix C.7: a restore that returns the bytes and loses the label has not returned the file"
    );
}

#[test]
fn should_show_what_a_full_rollback_would_discard() {
    let lines = recovery_view(&rollback_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "will discard"),
        "§24.5's worked example names what the rollback throws away"
    );
    assert!(
        contains(&lines, "GiB of changed blocks"),
        "§24.5: `18 GiB changed blocks since snapshot` is the number that changes an operator's mind"
    );
}

#[test]
fn should_name_every_newer_snapshot_a_rollback_would_destroy() {
    let lines = recovery_view(&rollback_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "will destroy"),
        "§24.5 and §62.5: destroying newer snapshots to make rollback work is never a convenience"
    );
    assert!(
        contains(&lines, "tank/data@later-1") && contains(&lines, "tank/data@later-2"),
        "§13.6 and Appendix D.5: the objects are named, not counted away"
    );
}

#[test]
fn should_require_the_explicit_gate_before_a_destructive_rollback() {
    let lines = recovery_view(&rollback_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "explicit accept-newer-state-loss"),
        "§24.5: no recovery execution occurs without the explicit gate"
    );
}

#[test]
fn should_count_the_newer_objects_at_risk_before_naming_them() {
    let lines = recovery_view(&rollback_recovery(), 80, Charset::Ascii);
    let block = index_of(&lines, NEWER_STATE_AT_RISK).expect("the block exists");
    let summary = &lines[block + 1..block + 5].join("\n");
    assert!(
        summary.contains("would be discarded") && summary.contains("would be destroyed"),
        "Appendix E.6's block is a summary of the loss, above the detail that explains it"
    );
}

#[test]
fn should_close_with_recovery_not_executed() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    assert_eq!(
        lines.last().map(String::as_str),
        Some(RECOVERY_NOT_EXECUTED),
        "§5.8 and §24.1: `recover` produces a plan and changes nothing, and the operator learns it here"
    );
}

#[test]
fn should_lay_the_recovery_view_out_at_the_width_it_was_given() {
    for width in [40usize, 80, 120] {
        for line in recovery_view(&rollback_recovery(), width, Charset::Ascii) {
            assert!(
                line.chars().count() <= width,
                "v0.4 §39.3: a recovery view is read under pressure and has to fit the terminal"
            );
        }
    }
}

#[test]
fn should_drop_recovery_not_executed_once_the_recovery_has_begun() {
    let mut fields: Vec<(&str, ono_value::Value)> = Vec::new();
    let planned = rollback_recovery();
    for name in [
        "id",
        "method",
        "target_state",
        "newer_state_analysed",
        "risk",
    ] {
        if let Some(value) = planned.get(name) {
            fields.push((name, value.clone()));
        }
    }
    fields.push(("state", support::s("recovering")));
    let running = support::record("ono.recovery-plan", &fields);
    let lines = recovery_view(&running, 80, Charset::Ascii);
    assert!(
        !contains(&lines, RECOVERY_NOT_EXECUTED),
        "§4.1: once recovery is executing, saying it has not been executed is false"
    );
}

#[test]
fn should_render_the_same_bytes_for_the_same_recovery_plan() {
    let recovery = rollback_recovery();
    assert_eq!(
        recovery_view(&recovery, 80, Charset::Ascii),
        recovery_view(&recovery, 80, Charset::Ascii),
        "§50: rendering is deterministic"
    );
}

#[test]
fn should_answer_the_reboot_question_even_when_the_answer_is_no() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    assert_eq!(
        block(&lines, "reboot"),
        vec!["no".to_owned()],
        "§24.3: the recovery plan shows whether a reboot is required, and `no` is an answer"
    );
    let rebooting = rewritten(&selective_recovery(), "requires_reboot", Value::Bool(true));
    assert_eq!(
        block(&recovery_view(&rebooting, 80, Charset::Ascii), "reboot"),
        vec!["yes".to_owned()],
        "§24.3 and §13.7: a recovery that takes effect at the next boot says so"
    );
}

#[test]
fn should_name_the_services_a_recovery_stops_or_restarts() {
    let plan = rewritten(
        &selective_recovery(),
        "services_affected",
        Value::list([map(&[
            ("service", s("nginx.service")),
            ("action", s("restart")),
        ])]),
    );
    let lines = recovery_view(&plan, 80, Charset::Ascii);
    assert_eq!(
        block(&lines, "services"),
        vec!["restart nginx.service".to_owned()],
        "§24.3: the recovery plan shows the services it stops or restarts"
    );
}

#[test]
fn should_say_which_services_are_affected_was_not_established_rather_than_none() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    let services = block(&lines, "services");
    assert!(
        services.iter().any(|line| line.contains("not established")),
        "§10.5: a planner that did not say which services stop is not a planner that said none. \
         Got {services:?}"
    );
    let none = rewritten(&selective_recovery(), "services_affected", Value::list([]));
    assert_eq!(
        block(&recovery_view(&none, 80, Charset::Ascii), "services"),
        vec!["no service is stopped or restarted".to_owned()],
        "an empty list is the planner saying none"
    );
}

#[test]
fn should_list_the_assets_a_recovery_consumes() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    let consumed = block(&lines, "assets consumed");
    assert!(
        consumed.iter().any(|line| line.contains("recovery/b817"))
            && consumed
                .iter()
                .any(|line| line.contains("rpool/ROOT/debian@ono-a82f")),
        "§24.3: the recovery plan shows the assets it consumes, by the reference `get recovery` \
         prints and the state it restores from. Got {consumed:?}"
    );
}

#[test]
fn should_show_what_the_method_not_chosen_would_have_discarded() {
    // Appendix I.5: the 10:00 config is restored selectively, and a full root rollback would
    // have discarded the 12:00 package changes.
    let plan = rewritten(
        &selective_recovery(),
        "rejected_methods",
        Value::list([map(&[
            ("provider", s("ono.recovery.zfs")),
            ("method", s("dataset-rollback")),
            ("reason", s("dominated")),
            ("unmet", Value::list([])),
            (
                "detail",
                s("selective-file-restore reaches the same goal and risks less unrelated state"),
            ),
            (
                "would_discard",
                list(&["/etc/ssh/sshd_config", "/etc/hosts"]),
            ),
        ])]),
    );
    let lines = recovery_view(&plan, 80, Charset::Ascii);
    let rejected = block(&lines, "methods not chosen");
    assert!(
        rejected
            .first()
            .is_some_and(|line| line.starts_with("dataset-rollback") && line.contains("dominated")),
        "Appendix I.5: the view names the method it rejected and why. Got {rejected:?}"
    );
    assert!(
        rejected
            .iter()
            .any(|line| line.contains("would discard") && line.contains("/etc/hosts")),
        "Appendix I.5: the view MUST show what the rejected rollback would discard. Got \
         {rejected:?}"
    );
    let method = index_of(&lines, "method").expect("the method block");
    let not_chosen = lines
        .iter()
        .position(|line| line == "methods not chosen")
        .expect("the block exists");
    assert!(
        method < not_chosen,
        "the method not chosen is read beside the method that was"
    );
}

#[test]
fn should_say_the_rejected_methods_were_not_recorded_rather_than_that_there_were_none() {
    let lines = recovery_view(&selective_recovery(), 80, Charset::Ascii);
    let rejected = block(&lines, "methods not chosen");
    assert!(
        rejected.iter().any(|line| line.contains("not recorded")),
        "§10.5: a record that does not carry the alternatives has not said there were none. Got \
         {rejected:?}"
    );
}

#[test]
fn should_say_what_a_rejected_method_would_discard_was_not_analysed() {
    let plan = rewritten(
        &selective_recovery(),
        "rejected_methods",
        Value::list([map(&[
            ("provider", s("ono.recovery.zfs")),
            ("method", s("clone-and-copy")),
            ("reason", s("metadata-shortfall")),
            ("unmet", list(&["SELinux labels"])),
            (
                "detail",
                s("Appendix C.7: clone-and-copy does not restore SELinux labels"),
            ),
            ("would_discard", Value::Null),
        ])]),
    );
    let rejected = block(
        &recovery_view(&plan, 80, Charset::Ascii),
        "methods not chosen",
    );
    assert!(
        rejected.iter().any(|line| line.contains("SELinux labels")),
        "Appendix C.7: what the method could not put back is why it lost. Got {rejected:?}"
    );
    assert!(
        rejected
            .iter()
            .any(|line| line.contains("what it would discard was not analysed")),
        "§10.5: an unanalysed loss is unknown, not nothing. Got {rejected:?}"
    );
}
