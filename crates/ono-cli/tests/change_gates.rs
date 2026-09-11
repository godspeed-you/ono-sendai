//! The acknowledgement gates, and the normal path they leave alone (v0.6 §19.4, §40).
//!
//! §40.1 keeps `apply` usable: a LOW or MODERATE plan with no irreversible action applies on
//! `apply` alone, because a gate on every plan is a gate nobody reads. §40.2 fixes what a gate
//! says when there is one — the rule's own reason — and §40.3 fixes who may be asked: a script
//! never waits, so every gate is a flag and every refusal is structured.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{build_disk_home, home, one, ono_at, requires_a_service, text};
use ono_testkit::{SkipReason, skipped};

/// What a host that serves no systemd unit cannot present (v0.4.1 §38.1).
const NO_SERVICE: &str = "this host serves no systemd unit, so there is no bulk restart to plan";

/// A plan §19.4 gates: fifty targets reaches `change.bulk.high_risk_targets` and is HIGH (§53).
fn high_risk_plan(home: &std::path::Path) -> Option<String> {
    requires_a_service(home)?;
    let run = ono_at(
        home,
        "get service | take 50 | plan restart service | to json",
    );
    run.assert_success();
    let record = one(&run);
    Some(text(&record, "id")[..4].to_owned())
}

/// A plan §40.1 leaves alone: one file, no irreversible action.
fn ordinary_plan(home: &std::path::Path) -> String {
    let source = home.join("source.txt");
    let destination = home.join("destination.txt");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let run = ono_at(
        home,
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    let record = one(&run);
    text(&record, "id")[..4].to_owned()
}

#[test]
fn should_refuse_a_high_risk_plan_when_the_risk_was_not_acknowledged() {
    let home = home();
    let Some(reference) = high_risk_plan(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let run = ono_at(home.path(), &format!("apply {reference}"));
    assert!(
        run.output().contains("change.risk_not_accepted"),
        "v0.6 §19.4: a HIGH plan requires an explicit acknowledgement before it may be applied. \
         Got {:?}",
        run.output()
    );
}

#[test]
fn should_name_the_rule_that_fired_when_a_risk_gate_refuses() {
    let home = home();
    let Some(reference) = high_risk_plan(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let run = ono_at(home.path(), &format!("apply {reference}"));
    assert!(
        run.output().contains("risk.bulk.high-threshold"),
        "v0.6 §40.2: the refusal carries the rule's own sentence rather than a generic question. \
         Got {:?}",
        run.output()
    );
}

#[test]
fn should_change_nothing_when_a_risk_gate_refuses() {
    let home = home();
    let Some(reference) = high_risk_plan(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let run = ono_at(home.path(), &format!("apply {reference}"));
    assert!(
        run.output().contains("Nothing was changed"),
        "v0.6 §40.2: a refused gate has changed nothing, and the refusal says so rather than \
         leaving the operator to infer it. Got {:?}",
        run.output()
    );
    let state = ono_at(home.path(), &format!("get plan {reference} | to json"));
    state.assert_success();
    assert_eq!(
        text(&one(&state), "state"),
        "sealed",
        "v0.6 §4.1: a plan refused at the gate is still the sealed plan it was; nothing moved it \
         along the lifecycle"
    );
}

#[test]
fn should_name_the_flag_that_answers_the_gate() {
    let home = home();
    let Some(reference) = high_risk_plan(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let run = ono_at(home.path(), &format!("apply {reference}"));
    assert!(
        run.output().contains("--accept-risk"),
        "v0.6 §40.3: every acknowledgement has a flag, and the refusal names it so a script can \
         supply it as policy rather than waiting for a prompt. Got {:?}",
        run.output()
    );
}

#[test]
fn should_get_past_the_gate_when_the_risk_is_acknowledged() {
    let home = home();
    let Some(reference) = high_risk_plan(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let run = ono_at(
        home.path(),
        &format!("apply {reference} --accept-risk --confirm"),
    );
    assert!(
        !run.output().contains("change.risk_not_accepted"),
        "v0.6 §19.4: `--accept-risk` is the acknowledgement, and a plan carrying it is not \
         refused for the class it acknowledged. Got {:?}",
        run.output()
    );
}

#[test]
fn should_seal_the_acknowledgement_into_a_new_revision() {
    let home = home();
    let Some(reference) = high_risk_plan(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    ono_at(
        home.path(),
        &format!("apply {reference} --accept-risk --confirm"),
    );
    let run = ono_at(home.path(), &format!("get plan {reference} | to json"));
    run.assert_success();
    let record = one(&run);
    assert!(
        record["revision"].as_u64().unwrap_or(1) > 1,
        "v0.6 §19.4: the acknowledgement is stored in the sealed revision, so acknowledging a \
         plan produces the next revision of it rather than editing the one that was refused. \
         Got {record:?}"
    );
}

#[test]
fn should_require_the_commitment_to_be_confirmed_outside_a_terminal() {
    let home = home();
    let Some(reference) = high_risk_plan(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let run = ono_at(home.path(), &format!("apply {reference} --accept-risk"));
    assert!(
        run.output().contains("safety.confirmation_required"),
        "v0.6 §40.3: a plan carrying any gate needs the commitment stated outside a terminal, and \
         a script never waits for a prompt. Got {:?}",
        run.output()
    );
}

#[test]
fn should_apply_an_ordinary_plan_without_any_acknowledgement() {
    let home = home();
    let reference = ordinary_plan(home.path());
    let run = ono_at(home.path(), &format!("apply {reference} | to json"));
    assert!(
        !run.output().contains("safety.confirmation_required")
            && !run.output().contains("change.risk_not_accepted"),
        "v0.6 §40.1: for a LOW or MODERATE plan with no irreversible action, invoking `apply` is \
         sufficient intent — the normal path stays usable. Got {:?}",
        run.output()
    );
    assert_eq!(
        std::fs::read_to_string(home.path().join("destination.txt"))
            .expect("the destination is readable"),
        "new\n",
        "v0.6 §5.6: an applied plan carried out its mutation"
    );
}

#[test]
fn should_refuse_to_apply_a_plan_nothing_names() {
    let home = home();
    let run = ono_at(home.path(), "apply ffffffff");
    assert!(
        run.output().contains("change.plan_not_found"),
        "v0.6 §36.4: a reference that matches no plan is refused by name rather than treated as \
         an empty selection. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_to_apply_an_expired_plan() {
    let home = home();
    let source = home.path().join("source.txt");
    let destination = home.path().join("destination.txt");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let run = ono_at(
        home.path(),
        &format!(
            "plan --expires 1ns copy file {} {} --overwrite | to json",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    let reference = text(&one(&run), "id")[..4].to_owned();
    let applied = ono_at(home.path(), &format!("apply {reference} --confirm"));
    assert!(
        applied.output().contains("change.plan_expired"),
        "v0.6 §5.6 and §4.1: `apply` refuses an expired plan. Got {:?}",
        applied.output()
    );
    assert_eq!(
        std::fs::read_to_string(&destination).expect("the destination is readable"),
        "old\n",
        "v0.6 §5.6: a plan refused for expiry changed nothing"
    );
}

#[test]
fn should_stream_an_action_result_for_every_action_it_ran() {
    // On the build disk: §11.2 protects the overwritten destination only on a persistent
    // filesystem, so under a tmpfs temporary directory the executor settles the copy alone, and
    // on any other disk the copy and its protection.
    let home = build_disk_home("change-gates-results");
    let reference = ordinary_plan(home.path());
    let run = ono_at(home.path(), &format!("apply {reference} | to json"));
    let rows = change_support::rows(&run);
    assert!(
        rows.len() == 2
            && rows.iter().any(|row| {
                row["operation"]
                    .as_str()
                    .is_some_and(|operation| operation.starts_with("copy file "))
            }),
        "v0.6 §5.6: `apply` answers with a stream of `ono.action-result/1`, one per action the \
         executor settled — the copy, and the protection §11.2 takes of the destination it \
         overwrites. Got {rows:?}"
    );
    assert!(
        rows.iter()
            .all(|row| row["status"].as_str() == Some("success")),
        "v0.6 §4.7: every action result is recorded independently. Got {rows:?}"
    );
}

#[test]
fn should_keep_the_progress_display_out_of_the_value_stream() {
    let home = home();
    let reference = ordinary_plan(home.path());
    let run = ono_at(home.path(), &format!("apply {reference} | to json"));
    assert!(
        !run.stdout().contains("PREPARE"),
        "v0.2 §13.1 and Appendix E.4: the progress display is a presentation of the stream and \
         goes to the diagnostic stream, never into the values. Got stdout {:?}",
        run.stdout()
    );
    assert!(
        run.stderr().contains("PREPARE"),
        "Appendix E.4: PREPARE, APPLY and VERIFY stay separately visible, so no single bar hides \
         whether protection completed. Got stderr {:?}",
        run.stderr()
    );
}
