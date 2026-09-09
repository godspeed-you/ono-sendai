//! Ono actions as causal anchors, at the shell's boundary (spec v0.5 §17.1, §17.2, §17.3, §17.4,
//! §17.6, §2's invariants 13 and 14).
//!
//! §17.1: "A shell has one source of causal information that external monitoring systems often
//! lack: it knows exactly which actions the operator requested through the shell." These tests ask
//! the real binary to prove it keeps that advantage, and — just as importantly — that it does not
//! overspend it.
//!
//! Two invariants sit either side of that line:
//!
//! - **Ono actions are traceable.** Every native mutation emits §17.2's lifecycle under one
//!   `ActionId` minted before execution (§17.3), carrying the actor, the session and a redacted
//!   summary of what was asked (§17.4). The identity exists whether or not the mutation succeeds,
//!   because §17.3 mints it *before* the provider is called.
//! - **External commands remain honest.** §17.6: "Ono MUST NOT claim arbitrary downstream effects
//!   of P unless an adapter, provider or other evidence source reports them." So a session that
//!   ran an external program records nothing about what that program went on to do, and a process
//!   the shell did not create is explained with `cause: unknown` rather than attributed to the
//!   shell that happened to be running.
//!
//! The mapping §17.3 requires from an `ActionId` to an external authority's own transaction id is
//! the provider's half to supply; `crates/ono-provider-systemd/tests/jobs.rs` holds systemd to it.
//! What this file asserts about that mapping is the other direction, and it is the one a machine
//! with no service manager can prove: where no authority returned a transaction, the ledger
//! records none.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use ono_testkit::Scratch;
use serde_yaml_ng::Value;

use support::{SleepChild, field, last_json_rows, recording_shell, text};

/// How long a fixture process may take to notice the signal Ono sent it.
const REAPING: Duration = Duration::from_secs(5);

/// The four stages §17.2 requires a native mutation's lifecycle to represent.
const LIFECYCLE: [&str; 4] = [
    "action.requested",
    "action.authorized",
    "action.executed",
    "action.completed",
];

/// Every event the shell recorded while running `script` in `home`.
fn ledger_of(home: &Scratch, script: &str) -> Vec<Value> {
    let run = recording_shell(home, &format!("{script}\nfind event | to json"));
    let events = last_json_rows(&run);
    assert!(
        !events.is_empty(),
        "the script under test must leave something in the ledger, or the assertion below holds \
         of nothing; got {:?}",
        run.output()
    );
    events
}

/// The events `script` recorded after it stopped a process this test owns.
///
/// A `sleep` started outside the shell is a target Ono did not create, so stopping it is a
/// mutation against the world rather than against the shell's own children — which is the case
/// §17.1's action lifecycle is about.
fn after_stopping_a_process(home: &Scratch, script: &str) -> Vec<Value> {
    let mut child = SleepChild::spawn();
    let recorded = ledger_of(home, &format!("stop process {}\n{script}", child.pid()));
    child.signal_within(REAPING);
    recorded
}

/// The distinct `ActionId`s the recorded events carry.
fn action_ids(events: &[Value]) -> BTreeSet<String> {
    events
        .iter()
        .filter_map(|event| match field(event, "payload.action_id") {
            Value::String(id) => Some(id),
            _ => None,
        })
        .collect()
}

/// One text property of an action event's §17.4 payload, or a panic naming the event that did
/// not carry it as text.
fn property(event: &Value, name: &str) -> String {
    match field(event, &format!("payload.{name}")) {
        Value::String(text) => text,
        other => panic!("v0.5 §17.4: `{name}` is text on an action event, got {other:?}"),
    }
}

/// The kinds the recorded events carry.
fn kinds(events: &[Value]) -> BTreeSet<String> {
    events.iter().map(|event| text(event, "kind")).collect()
}

#[test]
fn should_record_the_whole_lifecycle_under_one_action_id_when_a_process_is_stopped() {
    let home = ono_testkit::scratch();

    let events = after_stopping_a_process(&home, "");

    let seen = kinds(&events);
    for stage in LIFECYCLE {
        assert!(
            seen.contains(stage),
            "v0.5 §17.2: an Ono-native mutation emits `{stage}` as part of its causally linked \
             lifecycle, got {seen:?}"
        );
    }
    assert_eq!(
        action_ids(&events).len(),
        1,
        "v0.5 §17.3: one mutation is one `ActionId`, and every stage of its lifecycle carries \
         that one identity, got {events:?}"
    );
}

#[test]
fn should_name_the_actor_the_session_and_what_was_asked_when_an_action_is_recorded() {
    let home = ono_testkit::scratch();

    let events = after_stopping_a_process(&home, "");

    // §17.4 is about what an *action* event carries. The ledger holds more than actions now that
    // the recorder runs — §8.1's coverage markers are events too — and a coverage marker has no
    // actor, because nobody asked for it.
    let actions: Vec<&Value> = events
        .iter()
        .filter(|event| text(event, "kind").starts_with("action."))
        .collect();
    assert!(
        !actions.is_empty(),
        "the mutation must have left its lifecycle on the ledger for this test to be about \
         anything; got {events:?}"
    );
    for event in actions {
        for name in ["action_id", "actor", "session_id", "command", "operation"] {
            assert!(
                !field(event, &format!("payload.{name}")).is_null(),
                "v0.5 §17.4: an `ActionEvent` carries `{name}`, so the record says who asked for \
                 what in which session; got {event:?}"
            );
        }
        let command = property(event, "command");
        assert!(
            command.starts_with("stop process"),
            "v0.5 §17.5: the persisted summary is a semantic summary of the command, not its raw \
             text; got {command:?}"
        );
        assert_eq!(
            property(event, "operation"),
            "ono.process.stop",
            "v0.5 §17.4: the operation is the registered command that ran, got {event:?}"
        );
    }
}

#[test]
fn should_give_two_mutations_two_action_ids() {
    let home = ono_testkit::scratch();
    let mut first = SleepChild::spawn();
    let mut second = SleepChild::spawn();

    let events = ledger_of(
        &home,
        &format!(
            "stop process {}\nstop process {}",
            first.pid(),
            second.pid()
        ),
    );
    first.signal_within(REAPING);
    second.signal_within(REAPING);

    assert_eq!(
        action_ids(&events).len(),
        2,
        "v0.5 §17.3: every mutation receives its own `ActionId`, or two causal chains share one \
         anchor; got {events:?}"
    );
}

#[test]
fn should_mint_an_action_id_before_execution_when_a_service_restart_is_requested() {
    // §17.3: "Every mutation receives an `ActionId` **before** execution." The machine this runs
    // on has no service manager, so the restart cannot succeed — and the identity is required all
    // the same, because a mutation whose identity depended on its outcome could never be the
    // anchor a later effect is joined to.
    let home = ono_testkit::scratch();

    let events = ledger_of(&home, "restart service nginx");

    let requested = events
        .iter()
        .find(|event| text(event, "kind") == "action.requested")
        .unwrap_or_else(|| panic!("v0.5 §17.2: a requested mutation is recorded, got {events:?}"));
    assert!(
        !field(requested, "payload.action_id").is_null(),
        "v0.5 §17.3: the `ActionId` exists before execution, got {requested:?}"
    );
    assert_eq!(property(requested, "operation"), "ono.service.restart");
    assert_eq!(
        action_ids(&events).len(),
        1,
        "v0.5 §17.3: the identity minted before execution is the identity every later stage \
         carries, got {events:?}"
    );
}

#[test]
fn should_record_no_external_transaction_when_no_authority_returned_one() {
    // §17.3 records the mapping to an authority's own job id "if an external authority returns
    // its own transaction/job ID". No service manager answered here, so there is no job — and
    // §35.3's rule applies to a causal token as much as to a metric: unknown stays null rather
    // than becoming a plausible-looking identity nothing can be joined against.
    let home = ono_testkit::scratch();

    let events = ledger_of(&home, "restart service nginx");

    for event in &events {
        assert!(
            field(event, "payload.external_transaction").is_null(),
            "v0.5 §17.3, §17.6: no authority reported a transaction, so the ledger claims none; \
             got {event:?}"
        );
    }
}

#[test]
fn should_record_nothing_about_an_external_command_that_no_source_reported() {
    // §17.6: Ono may claim what it owns — it forked the process — and nothing about what that
    // process then did. The marker is unique to this run, so a ledger entry carrying it could
    // only have come from the external program's own output, which no provider reported.
    let home = ono_testkit::scratch();
    let marker = "ono-external-marker-9f2c";

    let events = after_stopping_a_process(&home, &format!("/bin/echo {marker}"));

    for event in &events {
        let printed = serde_yaml_ng::to_string(event).expect("a recorded event is printable");
        assert!(
            !printed.contains(marker),
            "v0.5 §17.6: Ono must not claim downstream effects of an external command that no \
             adapter, provider or evidence source reported; got {printed}"
        );
    }
}

#[test]
fn should_answer_with_an_unknown_cause_for_a_process_the_shell_did_not_create() {
    // The other half of §17.6, asked through `why`: the shell owns process creation, so a process
    // it did *not* create has no Ono action behind it and none may be invented for it. §15.7
    // makes that a successful typed answer rather than an error.
    let home = ono_testkit::scratch();
    let child = SleepChild::spawn();

    let run = recording_shell(
        &home,
        &format!("/bin/echo unrelated\nwhy process {} | to json", child.pid()),
    );

    run.assert_success();
    let mut printed = last_json_rows(&run);
    assert_eq!(
        printed.len(),
        1,
        "v0.5 §16.4: one explanation, got {:?}",
        run.output()
    );
    let answer = printed.remove(0);
    assert!(
        field(&answer, "cause").is_null(),
        "v0.5 §17.6, §15.7: no evidence source reported why this process exists, and `cause: \
         unknown` is the honest answer; got {answer:?}"
    );
    assert_eq!(
        field(&answer, "causal_chain"),
        Value::Sequence(Vec::new()),
        "v0.5 §16.8: \"Only source-supported links may appear in the causal chain\"; got {answer:?}"
    );
}
