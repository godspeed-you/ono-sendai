//! `why` at the shell's boundary (spec v0.5 §16.1, §16.2, §16.4, §38.1, §55.8).
//!
//! Two sentences are on trial here, and both are about the *command* rather than about the causal
//! engine — `crates/ono-temporal-query/tests/why.rs` already holds the engine to its answers.
//!
//! - **§16.1:** "`why` is the primary human-facing causal query. It is not an LLM prompt and MUST
//!   NOT require an AI model." §55.8 names the failure it prevents: "If `why` cannot explain known
//!   systemd/Ono action causality without a model configured, the core design has failed." So the
//!   shell these tests drive has no model configured, and it is asked to explain anyway.
//! - **§16.2:** "v0.5 defines exactly three forms" — `why <target> <selector>`, `why event <ref>`
//!   and `why field <name>`. All three are asked here, through the real binary, and the shell's
//!   own refusal is asked to name them when it is given none.
//!
//! Every shell runs with its XDG roots inside a scratch directory, so the store it opens is the
//! test's own and the developer's history is neither read nor written.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "temporal")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::time::Duration;

use ono_testkit::{Run, Scratch};
use serde_yaml_ng::Value;

use support::{field, last_json_rows, recording_shell, rows, search, text};

/// How long a fixture process may take to notice the signal Ono sent it.
const REAPING: Duration = Duration::from_secs(5);

/// The one `ono.causal-explanation/1` a `why` prints, or a panic carrying what it printed instead.
///
/// §16.4 fixes the answer as one `CausalExplanation` value, so a `why` that printed two of them —
/// or none — has already broken the contract before any field is read.
fn explanation(run: &Run) -> Value {
    run.assert_success();
    let mut printed = rows(run);
    assert_eq!(
        printed.len(),
        1,
        "v0.5 §16.4: `why` returns one `CausalExplanation`, got {:?}",
        run.output()
    );
    let value = printed.remove(0);
    assert_eq!(
        field(&value, "provenance.schema"),
        Value::String("ono.causal-explanation/1".to_owned()),
        "v0.5 §16.4, §35.5: the answer is the canonical typed explanation, got {value:?}"
    );
    value
}

/// Stops a process this test owns in `home`'s ledger, and answers with the `action.executed`
/// event the shell recorded for it.
///
/// §17.2's lifecycle is the one causal anchor a shell can always produce on a machine with no
/// service manager, which is what makes it the subject `why event` is asked about here.
fn recorded_action(home: &Scratch) -> Value {
    let mut child = support::SleepChild::spawn();
    let run = recording_shell(
        home,
        &format!(
            "stop process {}\nfind event 'kind == \"action.executed\"' | to json",
            child.pid()
        ),
    );
    child.signal_within(REAPING);
    let mut events = last_json_rows(&run);
    assert!(
        !events.is_empty(),
        "v0.5 §17.2: stopping a process records `action.executed`, got {:?}",
        run.output()
    );
    events.remove(0)
}

#[test]
fn should_explain_without_a_model_when_none_is_configured() {
    let home = ono_testkit::scratch();

    let catalogue = recording_shell(&home, "get model | to json");
    assert!(
        rows(&catalogue).is_empty(),
        "the shell under test must have no model provider configured, or §55.8 is untested; got \
         {:?}",
        catalogue.output()
    );

    // `process/1` is on every Linux host, so the target resolves wherever the test runs; a
    // target that did not would be refused before any explanation was attempted (ADR-1066).
    let answer = explanation(&recording_shell(&home, "why process 1 | to json"));
    assert!(
        !field(&answer, "state_or_change").is_null(),
        "v0.5 §16.1: `why` \"is not an LLM prompt and MUST NOT require an AI model\" — with no \
         model configured it still says what it is explaining, got {answer:?}"
    );
    for reached in ["model", "inference", "hypothesis", "confidence"] {
        assert_eq!(
            search(&answer, reached),
            None,
            "v0.5 §38.1: AI is a consumer of temporal truth and never a source of it, so no \
             explanation carries `{reached}`; got {answer:?}"
        );
    }
}

#[test]
fn should_explain_a_state_when_the_first_form_names_a_target_and_a_selector() {
    let home = ono_testkit::scratch();
    let child = support::SleepChild::spawn();

    let answer = explanation(&recording_shell(
        &home,
        &format!("why process {} | to json", child.pid()),
    ));

    // The fixture is a `sleep`, and the explanation names the place the selector resolved to.
    assert!(
        text(&answer, "state_or_change").contains("process/sleep"),
        "v0.5 §16.2, §16.3: `why <target> <selector>` explains the subject the selector named, \
         got {answer:?}"
    );
    assert!(
        !field(&answer, "coverage").is_null(),
        "v0.5 §16.4: an explanation carries the coverage that qualifies it, whatever it concludes"
    );
}

#[test]
fn should_explain_an_event_when_the_second_form_names_a_reference() {
    let home = ono_testkit::scratch();
    let event = recorded_action(&home);
    let reference = text(&event, "reference");

    let answer = explanation(&recording_shell(
        &home,
        &format!("why event @{reference} | to json"),
    ));

    assert_eq!(
        text(&answer, "explained_event"),
        text(&event, "event_id"),
        "v0.5 §16.2, §11.6: `why event @{reference}` explains that event and no other, got \
         {answer:?}"
    );
}

#[test]
fn should_explain_a_field_when_the_third_form_names_one() {
    let home = ono_testkit::scratch();

    let answer = explanation(&recording_shell(&home, "why field state | to json"));

    assert!(
        text(&answer, "state_or_change").contains("state"),
        "v0.5 §16.2: `why field <name>` asks about the most recent supported change to that \
         field on the current spatial object, got {answer:?}"
    );
    assert!(
        !field(&answer, "subject").is_null(),
        "v0.5 §16.2: the third form operates on the current spatial object, so the explanation \
         names one, got {answer:?}"
    );
}

#[test]
fn should_name_all_three_forms_when_why_is_asked_to_explain_nothing() {
    let home = ono_testkit::scratch();

    let run = recording_shell(&home, "why");

    let output = run.output();
    assert!(
        output.contains("type.mismatch"),
        "v0.5 §16.2: `why` with no subject is a structured refusal rather than a guess, got \
         {output:?}"
    );
    for form in ["why <target> <selector>", "why event", "why field"] {
        assert!(
            output.contains(form),
            "v0.5 §16.2 defines exactly three forms, and the refusal names them all; `{form}` is \
             missing from {output:?}"
        );
    }
}

#[test]
fn should_explain_the_named_target_when_two_targets_are_asked_about() {
    // Issue #148: `why process 1` and `why process 9` answered about one and the same subject —
    // the place the session stood in. §16.3 makes the explanation about the target the selector
    // names, so two targets are two subjects.
    let home = ono_testkit::scratch();
    let child = support::SleepChild::spawn();

    let init = explanation(&recording_shell(&home, "why process 1 | to json"));
    let sleeper = explanation(&recording_shell(
        &home,
        &format!("why process {} | to json", child.pid()),
    ));

    let subject = |answer: &Value| field(answer, "subject");
    assert!(
        !subject(&init).is_null() && !subject(&sleeper).is_null(),
        "v0.5 §16.4: an explanation names its subject. Got {init:?} and {sleeper:?}"
    );
    // Review finding COR-T8: each subject is the spatial identity of the target that was named —
    // the one `enter` reaches — not merely a different one.
    for (selector, answer) in [
        ("process 1".to_owned(), &init),
        (format!("process {}", child.pid()), &sleeper),
    ] {
        let place = recording_shell(&home, &format!("enter {selector}\nlook | to json"));
        let entered = last_json_rows(&place);
        assert_eq!(
            subject(answer),
            field(&entered[0], "id"),
            "v0.5 §16.3: `why {selector}` explains the place `enter {selector}` reaches. Got \
             {answer:?}"
        );
    }
    assert_ne!(
        subject(&init),
        subject(&sleeper),
        "v0.5 §16.3: `why process 1` and `why process {}` are about two different processes. Got \
         {init:?} and {sleeper:?}",
        child.pid()
    );
    let root = explanation(&recording_shell(&home, "why field state | to json"));
    assert_ne!(
        subject(&init),
        subject(&root),
        "issue #148: the target is not the place the session stands in. Got {init:?}"
    );
}

#[test]
fn should_refuse_a_target_that_does_not_resolve_rather_than_explain_the_current_place() {
    // ADR-1066: a selector nothing answers to is refused by v0.4 §40's resolution refusal, and
    // nothing is explained — explaining the current place instead would answer a question that
    // was not asked.
    let home = ono_testkit::scratch();

    let run = recording_shell(&home, "why process 4294967294 | to json");

    assert!(
        !run.status().is_success() && run.output().contains("spatial.not_found"),
        "issue #148: a target that does not resolve is refused as such. Got {:?}",
        run.output()
    );
    assert!(
        !run.output().contains("ono.causal-explanation"),
        "nothing is explained for a target that does not exist. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_target_the_past_did_not_record_as_not_recorded_rather_than_not_found() {
    // Review finding COR-T3: at a historical coordinate `why <target>` looks for the target in the
    // reconstruction (§14.1), and a place that is not in it was not recorded there — which is
    // §12.3's `temporal.not_recorded`, with what each source can reach — not v0.4 §40's
    // `spatial.not_found`, which says there is no such place. `process 1` exists; a child this
    // test owns was observed alive and is then gone.
    let home = ono_testkit::scratch();
    let child = support::SleepChild::spawn();
    let pid = child.pid();
    let run = recording_shell(
        &home,
        &format!(
            "enter process {pid}\nup\nstop process {pid}\nsleep 2s\nat -1s\n\
             why process 1 | to json\nwhy process {pid} | to json"
        ),
    );
    drop(child);
    assert!(
        !run.output().contains("spatial.not_found"),
        "COR-T3: a target the past did not record is not a place that does not exist. Got {:?}",
        run.output()
    );
    assert!(
        run.output().contains("temporal.not_recorded")
            || run.output().contains("ono.causal-explanation"),
        "COR-T3: either the reconstruction held the target and it is explained, or it is \
         refused as not recorded. Got {:?}",
        run.output()
    );
}

#[test]
fn should_answer_the_same_when_the_coordinate_is_the_stages_own_or_the_sessions() {
    // Review finding COR-T4: `why` took its coordinate from the session and ignored the stage's
    // own `--at`, which v0.5 §4.5 makes the same engine as `at` context: `why process 1 --at -1s`
    // answered about the present while `at -1s; why process 1` answered about one second ago.
    let home = ono_testkit::scratch();
    let staged = recording_shell(&home, "sleep 2s\nwhy process 1 --at -1s | to json");
    let session = recording_shell(&home, "sleep 2s\nat -1s\nwhy process 1 | to json");
    let outcome = |run: &Run| {
        if run.status().is_success() {
            format!("{:?}", field(&rows(run)[0], "subject"))
        } else {
            run.stderr()
                .split_whitespace()
                .find(|word| word.starts_with("Ono-Sendai-E"))
                .unwrap_or("no code")
                .to_owned()
        }
    };
    assert_eq!(
        outcome(&staged),
        outcome(&session),
        "v0.5 §4.5: `--at` and `at` are one coordinate. `--at` said {:?}; `at` said {:?}",
        staged.output(),
        session.output()
    );
}
