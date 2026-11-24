//! `trace link` draws the streams a link multiplexes, each as an object of its own (spec §21.4,
//! §22.4; issue #219 (b), ADR-1099).
//!
//! ADR-0105 left them undrawn: the protocol did not expose its open streams, and a count would be
//! a number without an identity. Each stream has one — the id both ends route its frames by — and
//! the link record lists them while they are open.

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

const LINK: &str = "link host testbox --transport local";

fn last_json(run: &ono_testkit::Run) -> Value {
    serde_json::from_str(&support::last_line(run))
        .unwrap_or_else(|_| panic!("JSON on the last line, got {:?}", run.stdout()))
}

#[test]
fn should_draw_a_stream_that_is_open_while_the_link_is_traced() {
    // `get process` across the link holds a query stream open while `each` traces the link for
    // its first record: the trace draws that stream, by id, with what opened it and about what.
    let run = support::ono_at_home(
        &scratch(),
        &format!(
            "{LINK}; enter link testbox; get process | each {{ trace link testbox }} | take 1 | \
             to json"
        ),
    );
    run.assert_success();
    let graphs = last_json(&run);
    let graph = graphs.as_array().map_or(&graphs, |all| &all[0]);
    let edges = graph["edges"].as_array().cloned().unwrap_or_default();
    let carries: Vec<&Value> = edges
        .iter()
        .filter(|edge| edge["relation"] == "carries")
        .collect();
    assert_eq!(carries.len(), 1, "one stream is open, got {edges:#?}");
    assert_eq!(carries[0]["confidence"], "exact");
    let nodes = graph["nodes"].as_array().cloned().unwrap_or_default();
    let stream = nodes
        .iter()
        .find(|node| node["kind"] == "ono.link-stream/1")
        .unwrap_or_else(|| panic!("a stream node, got {nodes:#?}"));
    let shown = stream.to_string();
    assert!(
        shown.contains("\"kind\":\"query\"") && shown.contains("\"target\":\"process\""),
        "the stream says what opened it and about what, got {stream}"
    );
}

#[test]
fn should_list_no_streams_on_an_idle_link_and_none_at_all_without_an_agent() {
    let idle = support::ono_at_home(
        &scratch(),
        &format!("{LINK}; get link | select streams | to json"),
    );
    idle.assert_success();
    assert_eq!(
        last_json(&idle),
        serde_json::json!([{ "streams": [] }]),
        "an agent link that carries nothing right now lists nothing"
    );

    let reduced = support::ono_at_home(
        &scratch(),
        &format!("{LINK} --agentless; get link | select streams | to json"),
    );
    reduced.assert_success();
    assert_eq!(
        last_json(&reduced),
        serde_json::json!([{ "streams": null }]),
        "the agentless fallback multiplexes nothing, which is not an empty list"
    );
}
