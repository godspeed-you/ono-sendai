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

    let answer = explanation(&recording_shell(&home, "why service nginx | to json"));
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

    let answer = explanation(&recording_shell(&home, "why service nginx | to json"));

    assert!(
        text(&answer, "state_or_change").contains("nginx"),
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
