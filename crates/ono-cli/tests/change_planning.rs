//! Planning changes nothing, and refuses what it cannot describe (v0.6 §2.1, §5, §6, §62.3).
//!
//! §2.1 is the invariant the whole tranche rests on — *creating or inspecting a change plan MUST
//! NOT mutate the target system* — and §62.3 names silent mutation during planning as the failure
//! mode that would make the feature worthless. The first test here is the one that proves it
//! against a real object: a service this host actually serves is planned against, and its
//! generation is read before and after.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{
    home, in_the_past, one, ono_at, plan, requires_a_service, rows, service_generation, text,
};
use ono_testkit::{SkipReason, skipped};

/// What a host that serves no systemd unit cannot present (v0.4.1 §38.1).
const NO_SERVICE: &str = "this host serves no systemd unit, so there is nothing to plan against";

#[test]
fn should_leave_the_service_untouched_when_a_restart_is_planned() {
    let home = home();
    let Some(service) = requires_a_service(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let before = service_generation(home.path(), &service);
    let run = ono_at(home.path(), &format!("plan restart service {service}"));
    run.assert_success();
    let after = service_generation(home.path(), &service);
    assert_eq!(
        before, after,
        "v0.6 §2.1 and §55.1 case 1: creating a plan MUST NOT mutate the target system, so the \
         unit's state, sub-state and the instant it entered them are the same after planning as \
         before. Got {before:?} then {after:?}"
    );
}

#[test]
fn should_say_that_nothing_ran_when_a_plan_is_shown() {
    let home = home();
    let Some(service) = requires_a_service(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let run = ono_at(home.path(), &format!("plan restart service {service}"));
    run.assert_success();
    assert!(
        run.stdout().contains("PLAN NOT EXECUTED"),
        "v0.6 §62.3 and §20.2: an operator who reads a plan and walks away has to know that \
         nothing has happened, and this line is the whole of the answer. Got {:?}",
        run.output()
    );
}

#[test]
fn should_answer_with_a_sealed_plan_when_a_mutation_is_planned() {
    let home = home();
    let Some(service) = requires_a_service(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let run = ono_at(
        home.path(),
        &format!("plan restart service {service} | to json"),
    );
    run.assert_success();
    let record = one(&run);
    assert_eq!(
        text(&record, "state"),
        "sealed",
        "v0.6 §4.4: `plan` produces a sealed object, not a draft. Got {record:?}"
    );
    assert!(
        !text(&record, "digest").is_empty(),
        "v0.6 §4.4: the seal carries an integrity digest, which is what makes a plan the same \
         plan when it is read back. Got {record:?}"
    );
}

#[test]
fn should_refuse_to_plan_when_the_session_stands_in_the_past() {
    let home = home();
    let run = in_the_past(home.path(), "plan remove file /etc/hostname");
    assert!(
        run.output().contains("[PAST"),
        "v0.5 §4.6: this test is about what happens in historical context, and without the \
         marker the session never entered it. Got {:?}",
        run.output()
    );
    assert!(
        run.output().contains("change.historical_context_read_only"),
        "v0.6 §6.4 and §2.5: a plan intended for application is resolved against present state, \
         so historical context refuses. Got {:?}",
        run.output()
    );
}

#[test]
fn should_name_the_way_back_when_planning_in_the_past_is_refused() {
    let home = home();
    let run = in_the_past(home.path(), "plan remove file /etc/hostname");
    assert!(
        run.output().contains("now"),
        "v0.6 §6.4: the refusal names `now` as the way back rather than leaving the operator to \
         guess how to leave historical context. Got {:?}",
        run.output()
    );
}

#[test]
fn should_leave_the_coordinate_in_the_past_when_planning_is_refused() {
    let home = home();
    let run = in_the_past(
        home.path(),
        "plan remove file /etc/hostname\ntimeline --all",
    );
    assert!(
        run.output().contains("[PAST"),
        "v0.6 §6.4 and §2.5: the refusal is a refusal and not a transition — the session is \
         still standing where it was. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_an_opaque_external_command_by_default() {
    let home = home();
    let run = ono_at(home.path(), "plan sh -c 'rm -rf /somewhere'");
    assert!(
        run.output().contains("change.opaque_action_forbidden"),
        "v0.6 §6.2 and §55.1 case 5: an arbitrary command has no declared target scope or side \
         effects, so planning it fails by default. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_an_opaque_command_when_the_host_does_not_permit_the_escape() {
    let home = home();
    let run = ono_at(home.path(), "plan --opaque sh -c 'rm -rf /somewhere'");
    assert!(
        run.output().contains("change.opaque_action_forbidden"),
        "v0.6 §6.3 and §53: the escape is an explicit request *and* a host that permits it. \
         `change.allow_opaque_actions` is false by default, so `--opaque` alone is not enough. \
         Got {:?}",
        run.output()
    );
}

#[test]
fn should_admit_an_opaque_command_when_both_the_flag_and_the_setting_are_given() {
    let home = home();
    let run = change_support::ono_with(
        home.path(),
        "ONO_CHANGE_ALLOW_OPAQUE_ACTIONS",
        "true",
        "plan --opaque sh -c 'true' | to json",
    );
    run.assert_success();
    let record = one(&run);
    let actions = record["actions"]
        .as_sequence()
        .expect("v0.6 §46.1: a plan carries its actions")
        .clone();
    assert!(
        actions
            .iter()
            .any(|action| action["execution"]["method"].as_str() == Some("opaque")),
        "v0.6 §6.3: the acknowledged escape produces an opaque action, whose impact and \
         reversibility are unknown. Got {actions:?}"
    );
}

#[test]
fn should_cap_protection_when_the_plan_holds_an_opaque_action() {
    let home = home();
    let run = change_support::ono_with(
        home.path(),
        "ONO_CHANGE_ALLOW_OPAQUE_ACTIONS",
        "true",
        "plan --opaque sh -c 'true' | to json",
    );
    run.assert_success();
    let record = one(&run);
    let level = record["protection"]["level"]
        .as_str()
        .unwrap_or("missing")
        .to_owned();
    assert_ne!(
        level, "protected",
        "v0.6 §6.3 and Appendix A.7: an opaque action MUST NOT receive a PROTECTED status merely \
         because a filesystem snapshot exists somewhere on the host. Got {record:?}"
    );
}

#[test]
fn should_refuse_an_operation_no_contract_declares() {
    let home = home();
    let run = ono_at(home.path(), "plan validate config nginx");
    assert!(
        run.output().contains("change.action_not_plannable"),
        "v0.6 §6.1 and ADR-0813: an operation is plannable only where a contract declares its \
         semantics, and `validate config` is no contract this shell has. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_query_as_a_mutation() {
    let home = home();
    let run = ono_at(home.path(), "plan get service nginx");
    assert!(
        run.output().contains("change.action_not_plannable"),
        "v0.6 §6.1: `get service` is a command and not a mutation this shell can describe the \
         effects of, so it is refused rather than planned as a change that changes nothing. \
         Got {:?}",
        run.output()
    );
}

#[test]
fn should_produce_one_plan_when_a_pipeline_supplies_the_targets() {
    let home = home();
    if requires_a_service(home.path()).is_none() {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    }
    let run = ono_at(
        home.path(),
        "get service | take 3 | plan restart service | to json",
    );
    run.assert_success();
    let rows = rows(&run);
    assert_eq!(
        rows.len(),
        1,
        "v0.6 §5.3: a pipeline produces one plan over the frozen set unless one plan per object \
         is asked for. Got {rows:?}"
    );
}

#[test]
fn should_freeze_exactly_the_objects_the_pipeline_resolved() {
    let home = home();
    if requires_a_service(home.path()).is_none() {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    }
    let run = ono_at(
        home.path(),
        "get service | take 3 | plan restart service | to json",
    );
    run.assert_success();
    let record = one(&run);
    let targets = record["targets"]
        .as_sequence()
        .expect("v0.6 §46.1: a plan carries its frozen targets")
        .len();
    assert_eq!(
        targets, 3,
        "v0.6 §2.6 and §55.1 case 2: sealed bulk target membership is fixed at resolution — the \
         plan holds exactly what the pipeline resolved, and nothing joins it afterwards. \
         Got {record:?}"
    );
}

#[test]
fn should_produce_one_plan_per_object_when_it_is_asked_for() {
    let home = home();
    if requires_a_service(home.path()).is_none() {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    }
    let run = ono_at(
        home.path(),
        "get service | take 3 | plan --one-per-object restart service | to json",
    );
    run.assert_success();
    let rows = rows(&run);
    assert_eq!(
        rows.len(),
        3,
        "v0.6 §5.3: one plan per input object is the explicit request the default is not. \
         Got {rows:?}"
    );
}

#[test]
fn should_refuse_a_block_that_holds_a_loop() {
    let home = home();
    let run = ono_at(
        home.path(),
        "plan {\n    for name in [1, 2] {\n        remove file /etc/hostname\n    }\n}",
    );
    assert!(
        run.output().contains("change.action_not_plannable"),
        "v0.6 §5.2: the plan block is a bounded list of action descriptions and MUST NOT \
         introduce loops. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_block_that_declares_a_function() {
    let home = home();
    let run = ono_at(
        home.path(),
        "plan {\n    fn wipe() {\n        remove file /etc/hostname\n    }\n}",
    );
    assert!(
        run.output().contains("change.action_not_plannable"),
        "v0.6 §5.2: the plan block MUST NOT introduce arbitrary functions. Got {:?}",
        run.output()
    );
}

#[test]
fn should_split_a_block_into_actions_and_verification_contracts() {
    let home = home();
    let Some(service) = requires_a_service(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let run = ono_at(
        home.path(),
        &format!(
            "plan {{\n    restart service {service}\n    verify service {service} substate == \
             running\n}} | to json"
        ),
    );
    run.assert_success();
    let record = one(&run);
    let actions = record["actions"].as_sequence().map_or(0, Vec::len);
    let checks = record["verification_contracts"]
        .as_sequence()
        .map_or(0, Vec::len);
    assert_eq!(
        actions, 1,
        "ADR-0813 and v0.6 §4.7: a `verify` line is a contract rather than an action, so a \
         two-line block has one action. Got {record:?}"
    );
    assert!(
        checks >= 2,
        "ADR-0813 and v0.6 §23.1: the `verify` line joins the plan's verification set beside the \
         contract the operation itself declares. Got {record:?}"
    );
}

#[test]
fn should_bind_the_operations_own_options_to_the_operation() {
    let home = home();
    let source = home.path().join("source.txt");
    let destination = home.path().join("destination.txt");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let run = ono_at(
        home.path(),
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    assert!(
        run.stdout().contains("overwrite"),
        "ADR-0814: `plan`'s own options end at the first word that is not one, and everything \
         after it belongs to the operation — `--overwrite` is `copy file`'s and must not be \
         bound against `plan`'s contract. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_protection_mode_that_is_not_one_of_the_four() {
    let home = home();
    let run = ono_at(
        home.path(),
        "plan --protection paranoid remove file /etc/hostname",
    );
    assert!(
        run.output().contains("type.mismatch"),
        "v0.6 §17.2: the modes are off, prefer, require and maximize, and a word outside them is \
         refused by name rather than silently defaulted. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_unbounded_parallelism() {
    let home = home();
    let run = ono_at(
        home.path(),
        "plan --strategy parallel remove file /etc/hostname",
    );
    assert!(
        run.output().contains("type.mismatch"),
        "v0.6 §28.4: unlimited parallel mutation is not offered, so `parallel` without a bound \
         is refused. Got {:?}",
        run.output()
    );
}

#[test]
fn should_record_the_strategy_in_the_sealed_plan() {
    let home = home();
    if requires_a_service(home.path()).is_none() {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    }
    let run = ono_at(
        home.path(),
        "get service | take 3 | plan --strategy 'batch 2' restart service | to json",
    );
    run.assert_success();
    let record = one(&run);
    assert_eq!(
        record["strategy"].as_str(),
        Some("batch 2"),
        "v0.6 §28.5: the strategy is part of the seal, because it changes operational risk. \
         Got {record:?}"
    );
}

#[test]
fn should_answer_with_a_plan_that_serialises_through_to_json() {
    let home = home();
    let reference = plan(home.path(), "remove file /etc/hostname");
    assert_eq!(
        reference.len(),
        4,
        "v0.6 §36.4: a plan is referenced by an unambiguous prefix of its identity, and the \
         record `to json` produced carries the identity that prefix comes from"
    );
}
