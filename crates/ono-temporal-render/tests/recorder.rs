//! What `get recorder` reads as (spec v0.5 §10.3, §10.4, §30.1, §43.2).
//!
//! §30.1: "The user must know when Ono is retaining system history and how much it retains", and
//! persistent history must be "impossible to confuse with hidden surveillance". So every number
//! that bounds the retention is on the screen, and §43.2's dropped count is beside them rather
//! than in a log nobody reads.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_temporal_render::{RenderOptions, recorder_status};
use ono_value::Value;

mod support;
use support::recorder;

#[test]
fn should_say_where_history_is_kept_and_how_much_of_it_when_the_recorder_runs() {
    let status = recorder(true, Some("/home/case/.local/state/ono/temporal.db"), &[]);
    let rendered = recorder_status(&status, 100, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("running") && rendered.contains("healthy"),
        "§10.8: the status says whether it is collecting and how it is, got {rendered}"
    );
    assert!(
        rendered.contains("/home/case/.local/state/ono/temporal.db"),
        "§30.2: the store is named so its permissions are checkable, got {rendered}"
    );
    assert!(
        rendered.contains("1d 00h") && rendered.contains("512.00 MiB"),
        "§10.4: the retention limits are visible, got {rendered}"
    );
    assert!(
        rendered.contains("4812") && rendered.contains("12:00:00 - 12:25:00"),
        "§30.1: how much is retained, and over which window, got {rendered}"
    );
}

#[test]
fn should_say_that_nothing_is_retained_when_the_recorder_is_off() {
    // §10.2: persistent recording is disabled by default, and the shell still answers temporal
    // queries from §10.7's session ledger. A missing store is stated rather than left blank.
    let status = recorder(
        false,
        None,
        &[
            ("events", Value::Int(0)),
            ("size", Value::Null),
            ("earliest", Value::Null),
            ("latest", Value::Null),
            ("health", Value::string("stopped")),
        ],
    );
    let rendered = recorder_status(&status, 100, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("stopped") && rendered.contains("disabled"),
        "§10.2: an opt-in recorder that was not opted into says so, got {rendered}"
    );
    assert!(
        rendered.contains("session only"),
        "§10.7: nothing on disk is a fact about retention, got {rendered}"
    );
}

#[test]
fn should_count_what_the_bounded_queues_discarded_when_events_were_dropped() {
    // §43.2 forbids silent loss, and §43.4 makes the recorder's own health a visible fact.
    let status = recorder(
        true,
        Some("/home/case/.local/state/ono/temporal.db"),
        &[
            ("dropped", Value::Int(1_204)),
            ("health", Value::string("degraded")),
        ],
    );
    let rendered = recorder_status(&status, 100, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("dropped") && rendered.contains("1204"),
        "§43.2: what the queues discarded is part of the status, got {rendered}"
    );
    assert!(
        rendered.contains("degraded"),
        "§43.4: the recorder's own health is stated, got {rendered}"
    );
}
