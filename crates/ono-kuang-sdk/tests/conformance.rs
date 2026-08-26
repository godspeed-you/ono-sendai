//! The conformance suite of spec §31.74, run against the example plugin binary under the
//! deterministic test host of spec §31.73.
//!
//! Every test asserts an outcome at the protocol boundary: what the stream delivered, how the
//! invocation ended, what the audit trail recorded, which state the package landed in. None of
//! them knows how the supervisor is built.
//!
//! Coverage map (spec §31.74 → test):
//! - manifest validation → `ono-kuang-protocol/tests/manifest_validation.rs`, plus
//!   `should_refuse_a_hello_that_contradicts_the_manifest`
//! - schema validation / output schema conformance →
//!   `should_close_the_stream_when_output_leaves_the_declared_schema`
//! - command metadata parse → `should_surface_contract_shaped_contribution_tables`
//! - capability declaration completeness →
//!   `should_deny_and_audit_a_host_call_the_command_never_declared`
//! - denial paths required/optional →
//!   `should_refuse_to_load_when_a_required_capability_is_denied`,
//!   `should_load_degraded_when_an_optional_capability_is_denied`, and
//!   `should_refuse_and_audit_a_path_outside_the_granted_scope`
//! - cancellation behaviour → `should_stop_cleanly_when_the_host_cancels_a_stream`
//! - backpressure behaviour → `should_deliver_everything_under_a_small_credit_window`,
//!   `should_quarantine_a_plugin_that_emits_beyond_credit`
//! - resource quota behaviour →
//!   `should_refuse_a_state_write_beyond_quota_and_keep_state_intact`
//! - protocol violations → `should_quarantine_a_plugin_that_breaks_framing`,
//!   `should_quarantine_a_plugin_that_declares_an_oversized_frame`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_kuang_protocol::{AuditResult, Capability, InvokeStatus, KuangErrorCode, PluginState};
use ono_kuang_supervisor::{HostLimits, StreamEvent};
use ono_kuang_testhost::{TestHost, VIRTUAL_NOW};
use ono_value::Value;
use serde_json::{Map as JsonMap, Value as Json, json};

const PLUGIN: &str = env!("CARGO_BIN_EXE_kuang-example-plugin");

fn manifest() -> String {
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
    - filesystem.read: {paths: ["/tmp/**"]}
    - state.persist
    - process.signal
network:
  outbound: none
"#
    .to_owned()
}

fn manifest_requiring_clock() -> String {
    manifest().replace(
        "capabilities:\n  optional:",
        "capabilities:\n  required:\n    - clock.read\n  optional:",
    )
}

fn args(pairs: &[(&str, Json)]) -> JsonMap<String, Json> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

/// Grants every optional capability the fixture manifest declares, so the package loads
/// undegraded where a test asserts `Loaded`.
fn fully_granted(host: TestHost) -> TestHost {
    host.grant(Capability::ClockRead)
        .grant(Capability::FilesystemRead)
        .grant(Capability::StatePersist)
        .grant(Capability::ProcessSignal)
}

fn values_of(events: &[StreamEvent]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Value(value) => Some(value.clone()),
            StreamEvent::Failed(_) => None,
        })
        .collect()
}

// --- contribution surface ----------------------------------------------------------------------

#[tokio::test]
async fn should_surface_contract_shaped_contribution_tables() {
    let plugin = fully_granted(TestHost::new(PLUGIN, &manifest()))
        .load()
        .await
        .expect("the plugin loads");
    assert_eq!(plugin.state(), PluginState::Loaded);
    let commands = plugin.commands();
    let emit = commands
        .iter()
        .find(|command| command.contribution.id == "dev.example.echo.command.emit")
        .expect("the emit command is contributed");
    assert_eq!(emit.provider, "plugin:dev.example.echo");
    assert_eq!(emit.contribution.output, "stream<int>");
    assert!(!emit.contribution.examples.is_empty());
    let targets = plugin.targets();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].contribution.name, "echo-item");
    assert_eq!(targets[0].contribution.schema, "dev.example.echo.item/1");
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_stream_typed_values_for_a_contributed_command() {
    let plugin = fully_granted(TestHost::new(PLUGIN, &manifest()))
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.emit",
            args(&[("count", json!(3))]),
        )
        .await
        .expect("the command starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    assert_eq!(
        values_of(&events),
        vec![Value::Int(1), Value::Int(2), Value::Int(3)],
        "typed values cross the boundary as themselves"
    );
    assert_eq!(plugin.state(), PluginState::Loaded, "the invocation ended");
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- denial paths ------------------------------------------------------------------------------

#[tokio::test]
async fn should_refuse_to_load_when_a_required_capability_is_denied() {
    let error = TestHost::new(PLUGIN, &manifest_requiring_clock())
        .load()
        .await
        .expect_err("a denied required capability fails the load");
    assert_eq!(error.code(), KuangErrorCode::LoadCapabilityDenied);
}

#[tokio::test]
async fn should_load_degraded_when_an_optional_capability_is_denied() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .load()
        .await
        .expect("loads");
    assert_eq!(
        plugin.state(),
        PluginState::Degraded,
        "denied optional capabilities degrade rather than fail (spec §31.8, §31.17)"
    );
    assert!(plugin.contract().degraded);
    assert!(
        plugin
            .contract()
            .denied
            .iter()
            .any(|denied| denied.capability == "clock.read"),
        "the contract names what was denied (spec §31.63)"
    );
    assert_eq!(
        plugin.disabled_features(),
        &["tell-time".to_owned()],
        "the plugin adapts and names the feature it switched off (spec §31.63)"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_deny_and_audit_a_host_call_the_command_never_declared() {
    // The command declares no capabilities, then calls clock.now anyway. The broker refuses
    // with the structured denial, and the denial is audited (spec §31.16, §31.37).
    let plugin = TestHost::new(PLUGIN, &manifest())
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.sneaky-clock", args(&[]))
        .await
        .expect("the invocation starts; the denial happens at the call");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    let error = result.error.expect("the failure is structured");
    assert_eq!(error.name, "capability.denied");
    let audit = plugin.audit();
    let denial = audit
        .iter()
        .find(|event| event.result == AuditResult::Denied)
        .expect("the denial is in the trail — denials are audited as loudly as successes");
    assert_eq!(denial.capability, "clock.read");
    assert_eq!(denial.action, "clock.now");
    assert_eq!(denial.at, VIRTUAL_NOW, "the host's clock stamps the trail");
    assert_eq!(denial.plugin, "dev.example.echo");
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_refuse_and_audit_a_path_outside_the_granted_scope() {
    let outside = tempfile::tempdir().expect("tempdir");
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, b"not for the plugin").expect("fixture");
    let allowed = tempfile::tempdir().expect("tempdir");
    let readable = allowed.path().join("readable.txt");
    std::fs::write(&readable, b"fixture-bytes").expect("fixture");

    let mut scope = JsonMap::new();
    scope.insert(
        "paths".to_owned(),
        json!([format!("{}/**", allowed.path().display())]),
    );
    let plugin = TestHost::new(PLUGIN, &manifest())
        .grant_scoped(Capability::FilesystemRead, scope)
        .load()
        .await
        .expect("loads");

    // Inside the scope: the host performs the read and the plugin sees the bytes.
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.read-file",
            args(&[("path", json!(readable.to_string_lossy()))]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    assert_eq!(values_of(&events), vec![Value::Int(13)]);

    // Outside the scope: the structured scope violation, with the attempt in the trail.
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.read-file",
            args(&[("path", json!(secret.to_string_lossy()))]),
        )
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    let error = result.error.expect("structured");
    assert_eq!(error.name, "capability.scope_violation");
    assert!(
        error.metadata.contains_key("attempted") && error.metadata.contains_key("granted"),
        "the error carries the attempted value beside the granted scope"
    );
    let audit = plugin.audit();
    let violation = audit
        .iter()
        .find(|event| event.result == AuditResult::Denied)
        .expect("audited");
    assert_eq!(violation.capability, "filesystem.read");
    let success = audit
        .iter()
        .find(|event| event.result == AuditResult::Success)
        .expect("the successful read is audited too");
    assert_eq!(success.action, "filesystem.read");
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- cancellation ------------------------------------------------------------------------------

#[tokio::test]
async fn should_stop_cleanly_when_the_host_cancels_a_stream() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .load()
        .await
        .expect("loads");
    let mut invocation = plugin
        .invoke("dev.example.echo.command.count-forever", args(&[]))
        .await
        .expect("starts");
    for expected in 1..=3 {
        let event = invocation.next().await.expect("a value");
        assert_eq!(event, StreamEvent::Value(Value::Int(expected)));
    }
    invocation.cancel().await;
    let result = invocation.finish().await;
    assert_eq!(
        result.status,
        InvokeStatus::Cancelled,
        "cancellation is observable, not a stream that simply stops (spec §31.14)"
    );
    // The instance survived its cancellation and keeps serving.
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.emit",
            args(&[("count", json!(1))]),
        )
        .await
        .expect("a later invocation still works");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    assert_eq!(values_of(&events), vec![Value::Int(1)]);
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- backpressure ------------------------------------------------------------------------------

#[tokio::test]
async fn should_deliver_everything_under_a_small_credit_window() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .limits(HostLimits {
            queue_depth: 2,
            ..HostLimits::default()
        })
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.emit",
            args(&[("count", json!(6))]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    assert_eq!(
        values_of(&events),
        (1..=6).map(Value::Int).collect::<Vec<_>>(),
        "a producer faster than its window still delivers everything, in order, by waiting"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_quarantine_a_plugin_that_emits_beyond_credit() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .args(&["--misbehave=flood"])
        .load()
        .await
        .expect("the flood plugin loads; it misbehaves only when invoked");
    let invocation = plugin
        .invoke("dev.example.echo.command.flood", args(&[]))
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::Failed(error)
                if error.name == "runtime.protocol_violation")),
        "emitting beyond credit is a protocol violation, not a queue (spec §31.15)"
    );
    assert_eq!(plugin.state(), PluginState::Quarantined);
    assert!(plugin.quarantine_reason().is_some());
}

// --- resource quotas ---------------------------------------------------------------------------

#[tokio::test]
async fn should_refuse_a_state_write_beyond_quota_and_keep_state_intact() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .grant(Capability::StatePersist)
        .limits(HostLimits {
            state_quota: 64,
            ..HostLimits::default()
        })
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.state-write",
            args(&[("key", json!("small")), ("size", json!(8))]),
        )
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(
        result.status,
        InvokeStatus::Completed,
        "a write inside quota lands"
    );

    let invocation = plugin
        .invoke(
            "dev.example.echo.command.state-write",
            args(&[("key", json!("big")), ("size", json!(500))]),
        )
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    assert_eq!(
        result.error.expect("structured").name,
        "state.quota_exceeded",
        "exceeding the quota fails the write (spec §31.15, §31.31)"
    );

    // Nothing was evicted: the small key can be rewritten inside the same quota.
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.state-write",
            args(&[("key", json!("small")), ("size", json!(8))]),
        )
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- protocol violations -----------------------------------------------------------------------

#[tokio::test]
async fn should_quarantine_a_plugin_that_breaks_framing() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .args(&["--misbehave=garbage"])
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.flood", args(&[]))
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    assert_eq!(plugin.state(), PluginState::Quarantined);
    let failure = plugin.last_failure().expect("the violation is recorded");
    assert_eq!(failure.code(), KuangErrorCode::RuntimeProtocolViolation);
}

#[tokio::test]
async fn should_quarantine_a_plugin_that_declares_an_oversized_frame() {
    // A 1 MiB+1 length claim fails on the claim, before any allocation (ADR-0015 T7).
    let plugin = TestHost::new(PLUGIN, &manifest())
        .args(&["--misbehave=huge-frame"])
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.flood", args(&[]))
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    assert_eq!(plugin.state(), PluginState::Quarantined);
}

#[tokio::test]
async fn should_refuse_a_hello_that_contradicts_the_manifest() {
    let error = TestHost::new(PLUGIN, &manifest())
        .args(&["--misbehave=bad-hello"])
        .load()
        .await
        .expect_err("an instance claiming another identity does not load");
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
}

// --- output schema conformance -----------------------------------------------------------------

#[tokio::test]
async fn should_close_the_stream_when_output_leaves_the_declared_schema() {
    let plugin = fully_granted(TestHost::new(PLUGIN, &manifest()))
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.wrong-schema", args(&[]))
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::Failed(error)
                if error.name == "runtime.schema_violation")),
        "contributed output is validated, not trusted (spec §31.34)"
    );
    assert_eq!(result.status, InvokeStatus::Failed);
    assert_eq!(
        plugin.state(),
        PluginState::Loaded,
        "a schema violation degrades the stream, not the shell — and not the whole plugin"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- provider contributions --------------------------------------------------------------------

#[tokio::test]
async fn should_answer_a_provider_query_with_host_stamped_records() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .query("echo-item", args(&[]))
        .await
        .expect("the query starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    let values = values_of(&events);
    assert_eq!(values.len(), 3);
    for value in &values {
        let Value::Record(record) = value else {
            panic!("a provider answers records, not text");
        };
        assert_eq!(record.schema_id().to_string(), "dev.example.echo.item/1");
        assert_eq!(
            record.provenance().provider(),
            "plugin:dev.example.echo",
            "provenance is stamped by the host; a plugin cannot claim another source (spec §31.80)"
        );
    }
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- runtime capability requests and leases ----------------------------------------------------

#[tokio::test]
async fn should_answer_a_runtime_request_with_a_bounded_lease_or_a_denial() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .grant(Capability::ProcessSignal)
        .load()
        .await
        .expect("loads");
    // With a user action behind it and policy allowing: a lease with an expiry.
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.request-capability",
            args(&[(
                "action_context",
                json!("operator selected unit staging-api"),
            )]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    assert_eq!(
        values_of(&events),
        vec![Value::String("lease:2026-08-26T12:05:00Z".into())],
        "the lease expires five minutes after virtual now — bounded, never open-ended (spec §31.49)"
    );

    // With no user action behind it: denied without prompting (spec §31.17).
    let invocation = plugin
        .invoke("dev.example.echo.command.request-capability", args(&[]))
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    assert_eq!(result.error.expect("structured").name, "capability.denied");
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- audit of granted calls --------------------------------------------------------------------

#[tokio::test]
async fn should_audit_a_granted_call_with_the_virtual_clock() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .grant(Capability::ClockRead)
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.clock", args(&[]))
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    assert_eq!(
        values_of(&events),
        vec![Value::String(VIRTUAL_NOW.into())],
        "virtual time is what makes plugin tests deterministic (spec §31.73)"
    );
    let audit = plugin.audit();
    let success = audit
        .iter()
        .find(|event| event.result == AuditResult::Success && event.action == "clock.now")
        .expect("the granted call is audited");
    assert_eq!(success.capability, "clock.read");
    assert_eq!(success.invocation, "command:dev.example.echo.command.clock");
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}
