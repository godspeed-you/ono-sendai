//! Clock uncertainty in a rendered row (spec v0.5 §24.3, §24.4, §26.3).
//!
//! §24.4: Ono "may display them in timestamp order but MUST NOT claim the first caused the
//! second unless the connection identity or other evidence links them." So the uncertainty is
//! printed, and the ordering says nothing.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_temporal_render::{RenderOptions, timeline};
use ono_value::{Duration, Value};

mod support;
use support::{event, subject};

/// §24.4's own example: two hosts, two uncertain clocks, one connection.
fn two_hosts() -> Vec<Value> {
    vec![
        event(
            "e14031210000000000000001",
            "provider.event",
            "14:03:12.100",
            "connection opened",
            &[
                ("subject", subject("connection opened", "socket")),
                ("host", Value::string("web01")),
                (
                    "clock_uncertainty",
                    Value::Duration(Duration::parse("40ms").expect("a duration")),
                ),
            ],
        ),
        event(
            "e14031211800000000000002",
            "provider.event",
            "14:03:12.118",
            "connection accepted",
            &[
                ("subject", subject("connection accepted", "socket")),
                ("host", Value::string("db01")),
                (
                    "clock_uncertainty",
                    Value::Duration(Duration::parse("55ms").expect("a duration")),
                ),
            ],
        ),
    ]
}

#[test]
fn should_show_the_uncertainty_when_the_event_carries_one() {
    // §24.4: the renderer shows the uncertainty rather than implying a precision the clock does
    // not have.
    let view = support::timeline_record(two_hosts(), Vec::new());
    let lines = timeline(&view, 100, &RenderOptions::default());
    let rendered = lines.join("\n");
    let first = lines
        .iter()
        .find(|line| line.contains("14:03:12.100"))
        .unwrap_or_else(|| panic!("the first row is drawn, got {rendered}"));
    assert!(
        first.contains("+/- 40ms"),
        "§24.4: the row states how far the clock may be out, got {first:?}"
    );
    assert!(
        first.contains("web01"),
        "§25.5: the row names the clock domain it was timed in, got {first:?}"
    );
    assert!(
        rendered.contains("+/- 55ms") && rendered.contains("db01"),
        "§24.4: the second row states its own uncertainty, got {rendered}"
    );
}

#[test]
fn should_claim_no_causation_between_two_rows_of_different_clock_domains() {
    // §24.3 and §24.4: timestamp order is a display order and nothing more (§26.3).
    let view = support::timeline_record(two_hosts(), Vec::new());
    let rendered = timeline(&view, 100, &RenderOptions::default())
        .join("\n")
        .to_ascii_lowercase();
    for word in ["because", "therefore", "led to", "caused"] {
        assert!(
            !rendered.contains(word),
            "§24.4: two rows in timestamp order assert nothing about cause, `{word}` in {rendered}"
        );
    }
}

#[test]
fn should_show_no_uncertainty_when_none_was_measured() {
    // §35.3 of v0.2: unknown is never a fabricated zero. An event with no measured offset says
    // nothing about its offset.
    let view = support::timeline_record(
        vec![event(
            "e14031212000000000000003",
            "object.changed",
            "14:03:12.200",
            "nginx.service",
            &[],
        )],
        Vec::new(),
    );
    let rendered = timeline(&view, 100, &RenderOptions::default()).join("\n");
    assert!(
        !rendered.contains("+/-"),
        "an unmeasured offset is not a measured zero, got {rendered}"
    );
}
