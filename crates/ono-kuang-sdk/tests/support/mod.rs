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

/// The example package's manifest, as the two failure-class suites declare it (§31.34).
///
/// One definition per job (v0.4.1 §39.1): `failure_classes.rs` and `memory_ceiling.rs` both need a
/// package whose `memory_max` is the 64 MiB ceiling their subject dies against, and a second copy
/// would be a second ceiling to keep in step.
pub fn example_manifest() -> String {
    r#"
format: kuang-package/1
package:
  id: dev.example.echo
  name: echo
  version: 0.1.0
  description: Emits what it is asked to emit.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: native-process
  entry: runtime/echo
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
capabilities:
  optional:
    - clock.read
network:
  outbound: none
"#
    .to_owned()
}
