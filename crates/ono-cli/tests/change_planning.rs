//! Planning changes nothing, and refuses what it cannot describe (v0.6 §2.1, §5, §6, §62.3).
//!
//! §2.1 is the invariant the whole tranche rests on — *creating or inspecting a change plan MUST
//! NOT mutate the target system* — and §62.3 names silent mutation during planning as the failure
//! mode that would make the feature worthless. The first test here is the one that proves it
//! against a real object: a service this host actually serves is planned against, and its
//! generation is read before and after.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "change")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;
#[path = "support/service_manager.rs"]
mod service_manager;

use change_support::{home, in_the_past, one, ono_at, ono_with, plan, rows, text};
use ono_testkit::{Run, Scratch, SkipReason, skipped};
use service_manager::{FixtureServiceManager, Requests};

/// What a host without `dbus-daemon` cannot present (v0.4.1 §38.1).
const NO_BUS: &str = "this needs `dbus-daemon`, so the fixture can serve the service manager \
                      whose units the plan freezes";

/// A scratch home and a service manager holding units of the test's own (ADR-1080).
///
/// The host's unit list is not a fixture: a unit that comes or goes while a suite runs makes a
/// plan over `get service | take 3` measure the machine (issue #300, review finding TST-17).
struct Services {
    home: Scratch,
    manager: FixtureServiceManager,
}

impl Services {
    /// Five units, `fixture-0.service` … `fixture-4.service`, or `None` without `dbus-daemon`.
    fn start() -> Option<Self> {
        Some(Self {
            manager: FixtureServiceManager::try_start(5)?,
            home: home(),
        })
    }

    /// The unit a single-target plan names.
    const UNIT: &'static str = "fixture-0.service";

    /// `script` in the home, against the fixture's service manager.
    fn ono(&self, script: &str) -> Run {
        change_support::ono_on_bus(self.home.path(), &self.manager.address(), script)
    }

    /// The `state`, `substate` and `since` of `name`, the generation §7.2 freezes.
    fn generation(&self, name: &str) -> String {
        let run = self.ono(&format!("get service {name} | to json"));
        run.assert_success();
        let record = one(&run);
        format!(
            "{}|{}|{}",
            record["state"].as_str().unwrap_or("unknown"),
            record["substate"].as_str().unwrap_or("unknown"),
            record["since"].as_str().unwrap_or("unknown"),
        )
    }
}

#[test]
fn should_leave_the_service_untouched_when_a_restart_is_planned() {
    let Some(services) = Services::start() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let service = Services::UNIT;
    let before = services.generation(service);
    let run = services.ono(&format!("plan restart service {service}"));
    run.assert_success();
    let after = services.generation(service);
    assert_eq!(
        before, after,
        "v0.6 §2.1 and §55.1 case 1: creating a plan MUST NOT mutate the target system, so the \
         unit's state, sub-state and the instant it entered them are the same after planning as \
         before. Got {before:?} then {after:?}"
    );
}

#[test]
fn should_say_that_nothing_ran_when_a_plan_is_shown() {
    let Some(services) = Services::start() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let service = Services::UNIT;
    let run = services.ono(&format!("plan restart service {service}"));
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
    let Some(services) = Services::start() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let service = Services::UNIT;
    let run = services.ono(&format!("plan restart service {service} | to json"));
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
    let Some(services) = Services::start() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let run = services.ono("get service | take 3 | plan restart service | to json");
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
    let Some(services) = Services::start() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let run = services.ono("get service | take 3 | plan restart service | to json");
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
    let Some(services) = Services::start() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let run =
        services.ono("get service | take 3 | plan --one-per-object restart service | to json");
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
    let Some(services) = Services::start() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let service = Services::UNIT;
    let run = services.ono(&format!(
        "plan {{\n    restart service {service}\n    verify service {service} substate == \
             running\n}} | to json"
    ));
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
    let Some(services) = Services::start() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let run =
        services.ono("get service | take 3 | plan --strategy 'batch 2' restart service | to json");
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

/// What one plan over every unit of a fixture service manager holding `units` asked it, or `None`
/// where this host has no `dbus-daemon`.
///
/// The pipeline's own `get service` is subtracted: it is the input, read once whatever the plan
/// does with it.
fn asked_while_planning(units: i64) -> Option<Requests> {
    let manager = FixtureServiceManager::try_start(units)?;
    let home = home();
    let listing = ono_with(
        home.path(),
        "DBUS_SYSTEM_BUS_ADDRESS",
        &manager.address(),
        "get service | count",
    );
    listing.assert_success();
    let input = manager.requests();
    let run = ono_with(
        home.path(),
        "DBUS_SYSTEM_BUS_ADDRESS",
        &manager.address(),
        "get service | plan restart service | to json",
    );
    run.assert_success();
    assert_eq!(
        one(&run)["targets"].as_sequence().map(Vec::len),
        Some(usize::try_from(units).expect("a small count")),
        "v0.6 §4.3: the plan froze every unit the pipeline supplied"
    );
    let total = manager.requests();
    Some(Requests {
        list_units: total.list_units - 2 * input.list_units,
        load_unit: total.load_unit - 2 * input.load_unit,
        unit_reads: total.unit_reads - 2 * input.unit_reads,
        jobs: total.jobs - 2 * input.jobs,
    })
}

/// How often one plan may enumerate the service manager's units: once (issue #140, ADR-1081).
const LIST_UNITS_PER_PLAN: u64 = 1;

/// How many reads of its own unit one target may cost: five on this build, measured with no fixed
/// per-plan term (ADR-1081). One more per target turns this red.
const UNIT_READS_PER_TARGET: u64 = 5;

#[test]
fn should_read_the_unit_list_once_per_plan_however_many_targets_it_freezes() {
    // Issue #140: every target read every unit again, so a fifty-target restart cost fifty
    // enumerations of the service manager. One plan is one observation of the machine
    // (ADR-1081): the unit list is read once per plan, and each target costs only the reads of
    // its own unit.
    let (Some(few), Some(many)) = (asked_while_planning(5), asked_while_planning(50)) else {
        skipped(
            SkipReason::ExternalToolUnavailable,
            "this needs `dbus-daemon`, so the fixture can serve a service manager whose \
             requests the test counts",
        );
        return;
    };
    assert_eq!(
        few.list_units, many.list_units,
        "the unit list is enumerated as often for fifty targets as for five — once per plan. \
         Five targets asked {few:?}, fifty asked {many:?}"
    );
    // Equal is not enough on its own — fifty enumerations each would be equal too (TST-14).
    assert!(
        many.list_units <= LIST_UNITS_PER_PLAN,
        "one plan enumerates the unit list at most {LIST_UNITS_PER_PLAN} times, got {many:?}"
    );
    // The forty-five targets fifty has over five cost the difference, so a fixed per-plan term
    // cancels and what is left is the per-target cost, which integer division of each total
    // would blur (PRF-3).
    let extra = many.unit_reads - few.unit_reads;
    assert_eq!(
        extra % 45,
        0,
        "every target costs the same number of unit reads: five asked {few:?}, fifty asked \
         {many:?}"
    );
    let per_target = extra / 45;
    assert!(
        (1..=UNIT_READS_PER_TARGET).contains(&per_target),
        "a target costs between one and {UNIT_READS_PER_TARGET} reads of its own unit, got \
         {per_target} (five asked {few:?}, fifty asked {many:?})"
    );
    assert!(
        few.unit_reads >= 5 * per_target,
        "the fixed per-plan term is not negative: five asked {few:?}"
    );
    assert_eq!(many.jobs, 0, "v0.6 §2.1: planning a restart queued no job");
}

#[test]
fn should_plan_and_apply_a_block_written_in_the_positional_copy_spelling() {
    // ADR-1083: the spelling of ADR-0813's plan block, as this shell has it — `copy file <from>
    // <to>`, no connective — runs, from `plan` through `apply`, in a scratch directory.
    let home = change_support::build_disk_home("positional-copy");
    let source = home.path().join("nginx.conf");
    let destination = home.path().join("nginx.conf.live");
    std::fs::write(&source, "worker_processes 8;\n").expect("the source is written");
    std::fs::write(&destination, "worker_processes 1;\n").expect("the destination is written");

    let reference = plan(
        home.path(),
        &format!(
            "{{ copy file {} {} --overwrite; verify file {} exists }}",
            source.display(),
            destination.display(),
            destination.display()
        ),
    );
    let applied = ono_at(home.path(), &format!("apply {reference}"));

    applied.assert_success();
    assert_eq!(
        std::fs::read_to_string(&destination).expect("the destination is readable"),
        "worker_processes 8;\n",
        "v0.6 §5.6: the applied block copied the source onto the destination"
    );
}
