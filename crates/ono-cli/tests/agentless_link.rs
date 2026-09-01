//! What a user sees on a link with no Ono agent on the far side (spec §21.3).
//!
//! Spec §21.3: "If no Ono-Sendai agent exists remotely, the link MAY fall back to SSH and a
//! limited provider set implemented through standard commands/procfs reads. Fallback MUST be
//! visible because semantics and performance may differ."
//!
//! `crates/ono-remote/tests/agentless.rs` proves the reduced set itself. These are the outcomes
//! at the shell: that a reduced link really answers, that it refuses what it cannot see instead
//! of showing an empty table, that `get link` and `test host` describe the link that was made
//! rather than the one that was asked for, and that an ordinary agent link is unchanged.
//!
//! Every test is offline: the far side is `--transport local`, which is this machine.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::ono;
use serde_yaml_ng::Value;

mod support;
use support::last_line;

const AGENTLESS: &str = "link host testbox --transport local --agentless";
const AGENT: &str = "link host testbox --transport local";

/// The last non-empty line of stdout: what `to json` wrote for the final statement.
fn parsed(run: &ono_testkit::Run) -> Value {
    serde_yaml_ng::from_str(&last_line(run)).unwrap_or_else(|error| {
        panic!(
            "the last line is not JSON: {error}, got {:?}",
            last_line(run)
        )
    })
}

fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    let row = match value {
        Value::Sequence(rows) => rows
            .first()
            .unwrap_or_else(|| panic!("an empty answer: {value:?}")),
        other => other,
    };
    row.get(name)
        .unwrap_or_else(|| panic!("no `{name}` in {value:?}"))
}

#[test]
fn should_answer_processes_over_a_link_whose_far_side_has_no_agent() {
    // The reduced set is not a label: inside the link frame, `get process` is answered by
    // running a standard command on the far side and decoding it (spec §21.3).
    let run = ono(&format!(
        "{AGENTLESS}; enter link testbox; get process | count | to json"
    ));
    run.assert_success();

    let counted = parsed(&run);
    let Value::Sequence(rows) = &counted else {
        panic!("`to json` answers a list, got {counted:?}");
    };
    let count = rows
        .first()
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("`count` answers a number, got {counted:?}"));
    assert!(
        count > 0,
        "spec §21.3: a reduced link really reads the far side, got {counted:?}"
    );
}

#[test]
fn should_refuse_a_target_the_reduced_set_cannot_answer_rather_than_show_an_empty_table() {
    // Spec §35.3: an empty answer means "there are none", and must never stand in for "I cannot
    // see this". On a reduced link the second is the truth, so it is a structured refusal.
    let run = ono(&format!(
        "{AGENTLESS}; enter link testbox; try {{ get service }} catch e {{ $e | to json }}"
    ));
    run.assert_success();

    let error = parsed(&run);
    assert_eq!(
        field(&error, "name"),
        &Value::String("provider.unavailable".to_owned()),
        "the target exists and this link cannot reach it — which is not `target not found`, and \
         not an empty list, got {error:?}"
    );
    let message = field(&error, "message")
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        message.contains("agentless"),
        "spec §21.3: the refusal names the mode that reduced the answer, got {message:?}"
    );
}

#[test]
fn should_list_only_what_a_reduced_link_can_answer_in_the_link_table() {
    // Spec §21.3's visibility, in the table: `mode` says agentless, and `targets` — the field
    // `ono.link/1` documents as "what its context can answer" — is the reduced set, so the two
    // link kinds can be compared without connecting to anything.
    let run = ono(&format!(
        "{AGENTLESS}; get link | select mode targets | to json"
    ));
    run.assert_success();

    let row = parsed(&run);
    assert_eq!(
        field(&row, "mode"),
        &Value::String("agentless".to_owned()),
        "got {row:?}"
    );
    let targets: Vec<String> = match field(&row, "targets") {
        Value::Sequence(items) => items
            .iter()
            .map(|item| item.as_str().unwrap_or_default().to_owned())
            .collect(),
        other => panic!("`targets` is a list, got {other:?}"),
    };
    assert_eq!(
        targets,
        vec!["process".to_owned(), "filesystem".to_owned()],
        "a reduced link lists what it can answer, and that is visibly less than an agent's"
    );
}

#[test]
fn should_name_the_far_side_as_agentless_when_a_host_is_tested() {
    // `ono.probe-result/1`'s own contract: "the agentless fallback of §21.3 names itself too, so
    // the fallback is visible". Nothing negotiated a protocol version, so it is null rather than
    // a number nobody agreed on (spec §35.3).
    let run = ono(&format!(
        "{AGENTLESS}; test host testbox | select agent protocol_version providers | to json"
    ));
    run.assert_success();

    let row = parsed(&run);
    let agent = field(&row, "agent").as_str().unwrap_or_default().to_owned();
    assert!(
        agent.starts_with("agentless"),
        "the far side names itself as what it is, got {agent:?}"
    );
    assert_eq!(
        field(&row, "protocol_version"),
        &Value::Null,
        "no handshake settled a version, so none is claimed, got {row:?}"
    );
}

#[test]
fn should_still_name_the_agent_when_the_far_side_has_one() {
    // The reduced path must not become the ordinary one: a far side that does answer the agent
    // protocol is described exactly as before.
    let run = ono(&format!(
        "{AGENT}; test host testbox | select agent protocol_version | to json"
    ));
    run.assert_success();

    let row = parsed(&run);
    let agent = field(&row, "agent").as_str().unwrap_or_default().to_owned();
    assert!(
        agent.starts_with("ono/"),
        "an agent link still names the agent of spec §21.4, got {agent:?}"
    );
    assert_ne!(
        field(&row, "protocol_version"),
        &Value::Null,
        "an agent link settled a protocol version, got {row:?}"
    );
}

#[test]
fn should_say_which_mode_a_link_was_made_in_when_it_is_made() {
    let reduced = ono(AGENTLESS);
    reduced.assert_success();
    assert!(
        reduced.stdout().contains("agentless"),
        "spec §21.3: the fallback is visible where the link is made, got {:?}",
        reduced.stdout()
    );

    let full = ono(AGENT);
    full.assert_success();
    assert!(
        !full.stdout().contains("agentless"),
        "an agent link says nothing about a fallback it did not take, got {:?}",
        full.stdout()
    );
}

#[test]
fn should_say_what_a_reduced_link_can_answer_before_a_command_runs() {
    // Spec §42.2 makes the execution context part of the plan, and §21.3 makes the reduction
    // part of what a user must be able to see. `explain` reads the link that was established,
    // so it names the mode and the targets the reduced set really answers — before a command is
    // refused rather than after.
    let run = ono(&format!(
        "{AGENTLESS}; enter link testbox; explain get process"
    ));
    run.assert_success();

    let shown = run.stdout();
    assert!(
        shown.contains("mode") && shown.contains("agentless"),
        "the plan names the mode the link is in, got {shown:?}"
    );
    assert!(
        shown.contains("answers") && shown.contains("process filesystem"),
        "the plan names what this link can answer, which is the visible half of what it cannot, \
         got {shown:?}"
    );

    let full = ono(&format!("{AGENT}; enter link testbox; explain get process"));
    full.assert_success();
    assert!(
        !full.stdout().contains("agentless"),
        "an agent link's plan says nothing about a fallback it did not take, got {:?}",
        full.stdout()
    );
}
