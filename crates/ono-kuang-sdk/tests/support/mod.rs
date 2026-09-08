//! Helpers the SDK's outcome suites share (v0.4.1 §39.1): one definition per job.

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use ono_kuang_supervisor::StreamEvent;
use ono_value::Value;

/// The values an invocation streamed, with its failures left out.
pub fn values_of(events: &[StreamEvent]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Value(value) => Some(value.clone()),
            StreamEvent::Failed(_) => None,
        })
        .collect()
}
