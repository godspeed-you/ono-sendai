//! `watch host` probes reachability rather than reporting a snapshot (issue #219 (a), ADR-1096).
//!
//! ADR-0105 rejected a watch that dials every known host at each interval and left probing "a
//! later decision, taken when a cheap liveness check exists on the link protocol". ADR-1095 built
//! that check. A host's record now states its reachability as the latest probe of the link held
//! to it found, the watch reads it at every poll, and a transition is a `changed` event. Nothing
//! is dialled: a host with no held link is `unknown`, with the reason.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "remote")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_testkit::scratch;
use serde_json::Value;

mod support;
use support::peers::{Peer, Streaming, pin};

fn host_named<'a>(events: &'a [Value], name: &str) -> &'a Value {
    events
        .iter()
        .find(|event| event["host"]["name"] == name)
        .unwrap_or_else(|| panic!("an event about `{name}`, got {events:#?}"))
}

#[test]
fn should_report_a_host_reachable_when_the_probe_of_its_link_is_answered() {
    let home = scratch();
    let run = support::ono_at_home(
        &home,
        "link host testbox --transport local; watch host --every 1s | take 1 | to jsonl",
    );
    run.assert_success();
    let found = support::json_lines(&run);
    let host = &host_named(&found, "testbox")["host"];
    assert_eq!(
        host["reachability"], "reachable",
        "a probe of the held link was answered, got {host}"
    );
    assert!(
        host["reachability_since"].is_string(),
        "a probed reachability says since when, got {host}"
    );
    assert_eq!(host_named(&found, "testbox")["source"], "poll");
}

#[test]
fn should_say_unknown_and_dial_nothing_for_a_host_without_a_held_link() {
    let home = scratch();
    let run = support::ono_at_home(
        &home,
        "add host lone --address 192.0.2.1; watch host --every 1s | take 1 | to jsonl",
    );
    run.assert_success();
    let found = support::json_lines(&run);
    let host = &host_named(&found, "lone")["host"];
    assert_eq!(
        host["reachability"], "unknown",
        "no evidence is not `unreachable`, got {host}"
    );
    assert!(
        host["reachability_reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("test host")),
        "the reason says how to get evidence, got {host}"
    );
    assert!(host["reachability_since"].is_null());
}

#[test]
fn should_say_unknown_for_a_link_whose_far_side_has_no_agent_to_probe() {
    let home = scratch();
    let run = support::ono_at_home(
        &home,
        "link host testbox --transport local --agentless; watch host --every 1s | take 1 | \
         to jsonl",
    );
    run.assert_success();
    let found = support::json_lines(&run);
    let host = &host_named(&found, "testbox")["host"];
    assert_eq!(host["reachability"], "unknown", "got {host}");
}

#[test]
fn should_emit_a_change_when_the_far_side_of_a_held_link_goes_away() {
    // A real second agent on its own loopback address, with its own identity. The watch reports
    // it reachable; when the process dies, the next poll reports the transition — once, as a
    // `changed` event, and the probes before it changed nothing (no event per probe).
    let client = scratch();
    let mut far = Peer::start("127.0.0.2", &[]);
    far.authorize(&client);
    pin(&client, &far);

    let watch = Streaming::start(
        &client,
        &format!(
            "link host {} --transport tcp; watch host --every 1s | to jsonl",
            far.address
        ),
    );
    let (first, _) = watch.until(Duration::from_secs(30), |line| {
        line.contains("\"reachability\":\"reachable\"")
    });
    assert!(
        first.contains("\"kind\":\"snapshot\""),
        "the watch begins with the state now, got {first}"
    );

    far.kill();
    let (changed, before) = watch.until(Duration::from_secs(30), |line| {
        line.contains("\"reachability\":\"unreachable\"")
    });
    assert!(
        changed.contains("\"kind\":\"changed\""),
        "a transition is a `changed` event, got {changed}"
    );
    assert!(
        before.is_empty(),
        "nothing changed while the host stayed reachable, so nothing was emitted: {before:#?}"
    );
}

#[test]
fn should_report_a_host_reachable_over_the_authenticated_transport_without_reading_anything() {
    // Transport reachability is not authorization: a client the far side authorizes to connect
    // but not to observe still learns the host answers, because a probe reads nothing (ADR-1096).
    let client = scratch();
    let far = Peer::start("127.0.0.3", &[]);
    far.authorize_without_observe(&client);
    pin(&client, &far);

    let run = support::ono_at_home(
        &client,
        &format!(
            "link host {} --transport tcp; watch host --every 1s | take 1 | to jsonl",
            far.address
        ),
    );
    run.assert_success();
    let found = support::json_lines(&run);
    let host = &found
        .iter()
        .find(|event| event["host"]["link"].is_string())
        .unwrap_or_else(|| panic!("an event about the linked host, got {found:#?}"))["host"];
    assert_eq!(host["reachability"], "reachable", "got {host}");
}

#[test]
fn should_emit_one_change_to_unreachable_when_a_held_link_stops_answering_and_back_when_it_does() {
    // The far agent is frozen, not killed: it holds its connection open and answers nothing, so
    // the link has not failed and only a probe can tell (ADR-1096). The watch says so once, as a
    // `changed` event to `unreachable` — the reachable measurement is reused for at most the
    // reuse window, then the probe waits its timeout, then one poll reads it — and once more,
    // back to `reachable`, when the agent runs again.
    let client = scratch();
    let far = Peer::start("127.0.0.41", &[]);
    far.authorize(&client);
    pin(&client, &far);

    let watch = Streaming::start(
        &client,
        &format!(
            "link host {} --transport tcp; watch host --every 1s | to jsonl",
            far.address
        ),
    );
    watch.until(Duration::from_secs(30), |line| {
        line.contains("\"reachability\":\"reachable\"")
    });

    far.freeze();
    let (changed, before) = watch.until(Duration::from_secs(30), |line| {
        line.contains("\"reachability\":\"unreachable\"")
    });
    assert!(
        changed.contains("\"kind\":\"changed\"") && changed.contains("not answered within"),
        "an unanswered probe over an open connection is a transition to unreachable, got \
         {changed}"
    );
    assert!(
        before.is_empty(),
        "nothing was emitted before the transition: {before:#?}"
    );

    far.thaw();
    let (back, between) = watch.until(Duration::from_secs(30), |line| {
        line.contains("\"reachability\":\"reachable\"")
    });
    assert!(back.contains("\"kind\":\"changed\""), "got {back}");
    assert!(
        between.is_empty(),
        "exactly one event to unreachable, then the one back: {between:#?}"
    );
}
