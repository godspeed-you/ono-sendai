//! A link's latency, measured while it is open (v0.4 §19.1, issue #308, ADR-1095).
//!
//! The handshake's `round_trip` is measured once, when the link is made. The link map also shows
//! a `latency` the shell measures with the link protocol's probe when the map is drawn — on
//! demand, bounded, and reused for a few seconds — and states when it measured it. A link that
//! cannot be probed says why, with a null rather than an old number.
// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "remote")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::scratch;
use serde_json::Value;

mod support;
use support::peers::{Peer, pin};

/// A shell in a scratch home of its own running `script` (no developer home is read or written).
fn ono(script: &str) -> ono_testkit::Run {
    support::ono_at_home(&scratch(), script)
}

/// The `kill` program; `kill` is a native verb of the shell, the program is reached by its path.
fn kill_program() -> &'static str {
    ["/usr/bin/kill", "/bin/kill"]
        .into_iter()
        .find(|path| std::path::Path::new(path).exists())
        .expect("a kill program")
}

const LINK: &str = "link host testbox --transport local";

/// The link-map entry for `name` in one `look --json` document.
fn link_in(view: &Value, name: &str) -> Value {
    view.get("links")
        .and_then(Value::as_array)
        .and_then(|links| {
            links
                .iter()
                .find(|link| link.get("name").and_then(Value::as_str) == Some(name))
        })
        .cloned()
        .unwrap_or_else(|| panic!("§19.1: the link map lists `{name}`, got {view}"))
}

fn field<'a>(link: &'a Value, name: &str) -> &'a Value {
    link.get(name)
        .unwrap_or_else(|| panic!("ono.link-place/1 declares `{name}`, got {link}"))
}

#[test]
fn should_measure_a_live_latency_beside_the_handshake_round_trip_when_the_map_is_drawn() {
    let run = ono(&format!("{LINK}; look --json"));
    run.assert_success();
    let views = support::json_lines(&run);
    let link = link_in(&views[0], "testbox");

    assert_eq!(
        field(&link, "latency_status"),
        "measured",
        "the agent offers the probe, so drawing the map measured the link, got {link}"
    );
    assert!(
        !field(&link, "latency").is_null() && !field(&link, "latency_measured_at").is_null(),
        "a measured latency states its figure and when it was taken, got {link}"
    );
    assert!(
        !field(&link, "round_trip").is_null(),
        "the handshake's round trip is kept beside it, as what it is, got {link}"
    );
}

#[test]
fn should_reuse_one_probe_for_every_view_drawn_within_the_reuse_window() {
    // ADR-1095: drawing many maps sends at most one probe per link per five seconds — no probe
    // storm. Three views drawn back to back carry one measurement, taken once.
    let run = ono(&format!("{LINK}; look --json; look --json; look --json"));
    run.assert_success();
    let views = support::json_lines(&run);
    assert_eq!(views.len(), 3, "three views, got {:?}", run.stdout());
    let taken: Vec<Value> = views
        .iter()
        .map(|view| field(&link_in(view, "testbox"), "latency_measured_at").clone())
        .collect();
    assert!(
        !taken[0].is_null() && taken.iter().all(|at| *at == taken[0]),
        "one probe served every view inside the window, got {taken:?}"
    );
}

#[test]
fn should_state_no_latency_for_a_link_this_session_no_longer_follows() {
    let run = ono(&format!("{LINK}; detach link testbox; look --json"));
    run.assert_success();
    let link = link_in(&support::json_lines(&run)[0], "testbox");
    assert!(
        field(&link, "latency").is_null(),
        "a detached link is not probed, and no earlier figure stands in for one, got {link}"
    );
    assert_eq!(field(&link, "latency_status"), "unavailable");
}

#[test]
fn should_not_carry_a_measurement_over_to_a_link_made_again() {
    // A link torn down and made again is another link: what was measured on the first is not
    // a measurement of the second (ADR-1095).
    let run = ono(&format!(
        "{LINK}; look --json; remove link testbox; add link nas01 --host nas01.invalid \
         --transport local; look --json"
    ));
    run.assert_success();
    let views = support::json_lines(&run);
    let nas = link_in(&views[1], "nas01");
    assert!(
        field(&nas, "latency").is_null() && field(&nas, "latency_measured_at").is_null(),
        "a definition nobody connected was never probed, got {nas}"
    );
    assert_eq!(field(&nas, "latency_status"), "unavailable");
}

#[test]
fn should_say_a_link_without_an_agent_cannot_be_probed() {
    // The agentless fallback of spec §21.3 has no agent to answer a probe: the latency is
    // unknown, and the reason is stated rather than a zero or the handshake figure.
    let run = ono("link host testbox --transport local --agentless; look --json");
    run.assert_success();
    let link = link_in(&support::json_lines(&run)[0], "testbox");
    assert!(field(&link, "latency").is_null(), "got {link}");
    assert_eq!(
        field(&link, "latency_status"),
        "not_offered",
        "a link nobody can probe says so, got {link}"
    );
}

#[test]
fn should_print_the_measured_latency_in_the_link_map() {
    let run = ono(&format!("{LINK}; look"));
    run.assert_success();
    let line = run
        .stdout()
        .lines()
        .find(|line| line.contains("testbox") && line.contains("connected"))
        .unwrap_or_else(|| panic!("§19.1: the link map lists testbox, got {}", run.stdout()))
        .to_owned();
    assert!(
        !line.contains("handshake")
            && line
                .split_whitespace()
                .any(|word| word.ends_with("ms") || word.ends_with("us") || word.ends_with("ns")),
        "a measured latency is shown as §19.1's figure, got {line:?}"
    );
}

#[test]
fn should_measure_the_latency_when_the_view_is_drawn_not_when_the_link_was_made() {
    // The figure is a probe made when the map is drawn (ADR-1095), not the handshake's: a far
    // agent frozen after the link was made holds its connection and answers nothing, so the
    // view drawn then says `timed_out` with no figure — the handshake's number, or any earlier
    // one, standing in would be the carried-forward figure #308 forbids. Thawed, and past the
    // reuse window, the next view measures again, later.
    let client = scratch();
    let far = Peer::start("127.0.0.40", &[]);
    far.authorize(&client);
    pin(&client, &far);
    let pid = far.pid().expect("the agent runs");
    let kill = kill_program();
    // The script waits past the probe reuse window (five seconds, ADR-1095): a measurement
    // younger than that is reused by design, so only a view drawn after it probes again.
    let wait = "6s";
    let started = jiff::Timestamp::now();
    let run = support::ono_at_home(
        &client,
        &format!(
            "link host {address} --transport tcp; {kill} -STOP {pid}; look --json; \
             {kill} -CONT {pid}; sleep {wait}; look --json",
            address = far.address
        ),
    );
    far.thaw();
    run.assert_success();
    let views = support::json_lines(&run);
    assert_eq!(views.len(), 2, "two views, got {:?}", run.stdout());
    let frozen = link_in(&views[0], &far.address);
    let thawed = link_in(&views[1], &far.address);

    assert_eq!(
        field(&frozen, "latency_status"),
        "timed_out",
        "a far side that answers nothing when the view is drawn is not measured, got {frozen}"
    );
    assert!(
        field(&frozen, "latency").is_null(),
        "an unanswered probe has no figure — never the handshake's, never an earlier one, got \
         {frozen}"
    );
    assert!(
        !field(&frozen, "round_trip").is_null(),
        "the handshake's figure stays what it is, beside it, got {frozen}"
    );
    assert_eq!(
        field(&thawed, "latency_status"),
        "measured",
        "the thawed agent answers the next view's probe, got {thawed}"
    );
    assert!(!field(&thawed, "latency").is_null(), "got {thawed}");
    let at = |link: &Value| -> jiff::Timestamp {
        field(link, "latency_measured_at")
            .as_str()
            .and_then(|text| text.parse().ok())
            .unwrap_or_else(|| panic!("a probed link states when, got {link}"))
    };
    assert!(
        at(&frozen) > started,
        "the probe was made while the view was drawn, after the shell started (and so after \
         nothing but the link existed), got {frozen}"
    );
    assert!(
        at(&thawed) > at(&frozen),
        "the later view measured again, later, got {frozen} then {thawed}"
    );
}
