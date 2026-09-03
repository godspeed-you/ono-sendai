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
//!   `should_quarantine_a_plugin_that_emits_beyond_credit`,
//!   `should_end_the_stream_and_keep_the_instance_when_the_negotiated_overflow_fails_the_stream`,
//!   `should_keep_the_oldest_values_and_drop_the_rest_when_the_overflow_drops_the_newest`
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

/// The same manifest with a declared overflow preference (spec §31.15).
fn manifest_with_overflow(policy: &str) -> String {
    manifest().replace(
        "  startup: lazy",
        &format!("  startup: lazy\n  overflow: {policy}"),
    )
}

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
  model_broker: ono-model/1
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
    - model.infer: {providers: ["*"]}
    - context.read
    - schema.read
    - object.read
    - relation.read
    - relation.write
    - history.read
    - history.write
    - secret.use: {secrets: ["api-token"]}
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
        .grant(Capability::ModelInfer)
        .grant(Capability::ContextRead)
        .grant(Capability::SchemaRead)
        .grant(Capability::ObjectRead)
        .grant(Capability::RelationRead)
        .grant(Capability::RelationWrite)
        .grant(Capability::HistoryRead)
        .grant(Capability::HistoryWrite)
        .grant(Capability::SecretUse)
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

#[tokio::test]
async fn should_end_the_stream_and_keep_the_instance_when_the_negotiated_overflow_fails_the_stream()
{
    // §31.15: `fail-stream` is "required for correctness-sensitive analyses" — it terminates the
    // stream with `runtime.backpressure_failure` rather than lose data. Terminating the *package*
    // instead is a different answer, and §31.34 says plugin failure degrades the plugin, not
    // more than the plugin.
    let plugin = TestHost::new(PLUGIN, &manifest_with_overflow("fail-stream"))
        .args(&["--misbehave=flood"])
        .load()
        .await
        .expect("the flood plugin loads; it misbehaves only when invoked");
    let invocation = plugin
        .invoke("dev.example.echo.command.flood", args(&[]))
        .await
        .expect("starts");
    // The fixture speaks the wire directly and reports its own invocation status, so the host's
    // answer to the overrun is the stream event and the instance's state — not a status the
    // producer chose for itself.
    let (events, _) = invocation.collect().await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::Failed(error)
                if error.name == "runtime.backpressure_failure")),
        "§31.15: a `fail-stream` overrun ends the stream with its own condition, got {events:?}"
    );
    assert_ne!(
        plugin.state(),
        PluginState::Quarantined,
        "§31.34: the overrun the package declared a policy for degrades the stream, not the package"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_keep_the_oldest_values_and_drop_the_rest_when_the_overflow_drops_the_newest() {
    // §31.15: the five policies are what "policy can choose" when a plugin cannot keep up, and a
    // negotiated policy that decides nothing is not a choice. `drop-newest` is explicit only, so
    // it can come from host policy and never from a manifest preference.
    let plugin = TestHost::new(PLUGIN, &manifest())
        .args(&["--misbehave=flood"])
        .limits(HostLimits {
            queue_depth: 4,
            overflow: ono_kuang_protocol::OverflowPolicy::DropNewest,
            ..HostLimits::default()
        })
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.flood", args(&[]))
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(
        values_of(&events),
        vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)],
        "§31.15: `drop-newest` keeps what fitted and drops what did not, got {events:?}"
    );
    assert_eq!(
        result.status,
        InvokeStatus::Completed,
        "the stream is not failed by a policy that says values may be dropped"
    );
    assert_ne!(plugin.state(), PluginState::Quarantined);
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
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

// --- the model broker (spec §31.43, §31.44, §31.52, §31.82; ADR-0566) ---------------------------

/// A model that echoes the first context segment it was sent, speaking `ono-model/1`.
fn echo_model(directory: &std::path::Path) -> String {
    let script = directory.join("echo-model");
    std::fs::write(
        &script,
        "#!/bin/sh\ndoc=$(cat)\ntext=$(printf '%s' \"$doc\" | grep -o '\"content\":\"[^\"]*\"' | head -1 | cut -d'\"' -f4)\nprintf '{\"protocol\":\"ono-model/1\",\"parts\":[{\"kind\":\"text\",\"text\":\"echo: %s\"},{\"kind\":\"citation\",\"object\":\"ono.process/1[1]\"}]}' \"$text\"\n",
    )
    .expect("write the model");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    script.to_string_lossy().into_owned()
}

/// Two providers over the echo model: one local, one remote with the external-ok policy.
fn two_providers(directory: &std::path::Path) -> std::sync::Arc<dyn ono_model_broker::ModelBroker> {
    let command = echo_model(directory);
    let catalogue = ono_model_broker::Catalogue::parse(&format!(
        "providers:\n  - id: local-echo\n    name: Local echo\n    kind: local\n    location: workstation\n    command: [{command}]\n    data_policy: local-only\n  - id: remote-echo\n    name: Remote echo\n    kind: remote\n    location: configured\n    command: [{command}]\n    data_policy: external-ok\n"
    ))
    .expect("a catalogue");
    std::sync::Arc::new(ono_model_broker::CommandBroker::new(catalogue, None))
}

fn providers_scope(names: &[&str]) -> JsonMap<String, Json> {
    let mut scope = JsonMap::new();
    scope.insert("providers".to_owned(), json!(names));
    scope
}

#[tokio::test]
async fn should_list_only_the_model_providers_within_the_grants_scope() {
    let models = tempfile::tempdir().expect("tempdir");
    let plugin = TestHost::new(PLUGIN, &manifest())
        .models(two_providers(models.path()))
        .grant_scoped(Capability::ModelInfer, providers_scope(&["local-echo"]))
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.models", args(&[]))
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    assert_eq!(
        values_of(&events),
        vec![Value::String("local-echo".into())],
        "a package sees the providers it may use, not the operator's whole configuration"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_answer_an_inference_through_the_operator_configured_command() {
    let models = tempfile::tempdir().expect("tempdir");
    let plugin = TestHost::new(PLUGIN, &manifest())
        .models(two_providers(models.path()))
        .grant_scoped(Capability::ModelInfer, providers_scope(&["*"]))
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.infer",
            args(&[("prompt", json!("hello"))]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    let answered = values_of(&events);
    assert_eq!(answered[0], Value::String("echo: hello".into()));
    assert!(
        matches!(&answered[1], Value::String(text) if text.starts_with("citation: ")),
        "every part comes back as data; got {answered:?}"
    );
    let audit = plugin.audit();
    let request = audit
        .iter()
        .find(|event| event.action == "models.infer" && event.result == AuditResult::Success)
        .expect("every model request is audited");
    assert_eq!(request.capability, "model.infer");
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_refuse_a_provider_outside_the_granted_scope_and_audit_the_attempt() {
    let models = tempfile::tempdir().expect("tempdir");
    let plugin = TestHost::new(PLUGIN, &manifest())
        .models(two_providers(models.path()))
        .grant_scoped(Capability::ModelInfer, providers_scope(&["local-echo"]))
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.infer",
            args(&[
                ("prompt", json!("hello")),
                ("provider", json!("remote-echo")),
            ]),
        )
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    let error = result.error.expect("structured");
    assert_eq!(error.name, "capability.scope_violation");
    let audit = plugin.audit();
    assert!(
        audit
            .iter()
            .any(|event| event.result == AuditResult::Denied && event.capability == "model.infer"),
        "the denied request is in the trail (spec §31.37)"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_refuse_a_request_carrying_a_class_the_provider_may_not_receive() {
    let models = tempfile::tempdir().expect("tempdir");
    let plugin = TestHost::new(PLUGIN, &manifest())
        .models(two_providers(models.path()))
        .grant_scoped(Capability::ModelInfer, providers_scope(&["*"]))
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.infer",
            args(&[
                ("prompt", json!("AKIA...")),
                ("class", json!("credentials")),
                ("provider", json!("remote-echo")),
            ]),
        )
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    let error = result.error.expect("structured");
    assert_eq!(error.name, "model.policy_denied");
    assert_eq!(
        error.metadata.get("denied_classes"),
        Some(&json!(["credentials"])),
        "the refusal names the class, so the boundary is visible (spec §31.44)"
    );
    let audit = plugin.audit();
    assert!(
        audit
            .iter()
            .any(|event| event.action == "models.infer" && event.result == AuditResult::Failed),
        "the refused request is in the trail"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_send_a_transformed_class_redacted_and_disclose_the_plan_before_the_first_remote_call()
 {
    let models = tempfile::tempdir().expect("tempdir");
    let plugin = TestHost::new(PLUGIN, &manifest())
        .models(two_providers(models.path()))
        .grant_scoped(Capability::ModelInfer, providers_scope(&["*"]))
        .load()
        .await
        .expect("loads");
    for _ in 0..2 {
        let invocation = plugin
            .invoke(
                "dev.example.echo.command.infer",
                args(&[
                    ("prompt", json!("Sep 03 sshd: password=hunter2")),
                    ("class", json!("logs")),
                    ("provider", json!("remote-echo")),
                ]),
            )
            .await
            .expect("starts");
        let (events, result) = invocation.collect().await;
        assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
        assert_eq!(
            values_of(&events)[0],
            Value::String("echo: [redacted: logs]".into()),
            "the log line left redacted, as `external-ok` transforms logs (spec §31.44)"
        );
    }
    let audit = plugin.audit();
    let disclosures: Vec<_> = audit
        .iter()
        .filter(|event| event.action == "model.disclosure")
        .collect();
    assert_eq!(
        disclosures.len(),
        1,
        "the data-boundary plan is disclosed before the first remote inference, once (spec §31.82)"
    );
    let plan = disclosures[0]
        .target
        .clone()
        .expect("the plan is the target");
    assert_eq!(plan["provider"], json!("remote-echo"));
    assert_eq!(plan["kind"], json!("remote"));
    assert_eq!(plan["redacted"]["logs"], json!(1));
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_change_no_grant_when_untrusted_text_asks_for_a_capability() {
    // The prompt-injection fixture of spec §31.52 and §31.74: `assistants.v1.yaml`'s
    // `no-model-in-privileged-path` and `untrusted-text-cannot-instruct`.
    let models = tempfile::tempdir().expect("tempdir");
    let plugin = TestHost::new(PLUGIN, &manifest())
        .models(two_providers(models.path()))
        .grant_scoped(Capability::ModelInfer, providers_scope(&["*"]))
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.inject", args(&[]))
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    let answered = values_of(&events);
    assert!(
        answered.contains(&Value::String("filesystem.read:denied".into())),
        "the text asked for a capability and the grant is what it was; got {answered:?}"
    );
    let audit = plugin.audit();
    assert!(
        !audit
            .iter()
            .any(|event| event.capability == "filesystem.read"
                && event.result == AuditResult::Success),
        "nothing was granted and nothing was read"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_answer_provider_unavailable_when_no_model_is_configured() {
    // `degrades-without-a-model`: the package loads, and the turn says why it cannot answer.
    let plugin = TestHost::new(PLUGIN, &manifest())
        .grant_scoped(Capability::ModelInfer, providers_scope(&["*"]))
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.infer",
            args(&[("prompt", json!("hello"))]),
        )
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    assert_eq!(
        result.error.expect("structured").name,
        "model.provider_unavailable"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- context and schemas, pulled through host streams (spec §31.12, §31.15, §31.64; ADR-0567) --

#[tokio::test]
async fn should_answer_the_context_the_host_published_and_nothing_beyond_it() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .grant(Capability::ContextRead)
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.context", args(&[]))
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    let Some(Value::String(text)) = values_of(&events).into_iter().next() else {
        panic!("the context is one string");
    };
    let context: Json = serde_json::from_str(&text).expect("JSON");
    assert_eq!(
        context["cwd"],
        json!("/"),
        "the test host's fixed context (spec §31.73)"
    );
    assert_eq!(context["host"], json!("test-host"));
    assert_eq!(context["interactive"], json!(false));
    assert!(
        context.get("environment").is_none() && context.get("history").is_none(),
        "the context stack, and nothing beyond it: {context}"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_stream_the_registered_schemas_under_a_prefix_as_the_plugin_pulls_them() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .grant(Capability::SchemaRead)
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.schemas",
            args(&[("prefix", json!("ono.proc"))]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    let ids: Vec<String> = values_of(&events)
        .into_iter()
        .filter_map(|value| match value {
            Value::String(text) => Some(text.to_string()),
            _ => None,
        })
        .collect();
    assert!(
        ids.contains(&"ono.process/1".to_owned())
            && ids.contains(&"ono.process-detail/1".to_owned()),
        "the core schemas under the prefix, pulled two at a time; got {ids:?}"
    );
    assert!(
        ids.iter().all(|id| id.starts_with("ono.proc")),
        "nothing outside the prefix; got {ids:?}"
    );
    let audit = plugin.audit();
    assert!(
        audit
            .iter()
            .any(|event| event.action == "schemas.list" && event.result == AuditResult::Success),
        "the read is audited under schema.read"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_describe_one_schema_with_its_fields_and_origin() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .grant(Capability::SchemaRead)
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.schema",
            args(&[("id", json!("ono.process/1"))]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    let fields = values_of(&events);
    assert!(
        fields.contains(&Value::String("pid".into())),
        "the process schema's fields, by name; got {fields:?}"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_refuse_the_context_and_the_schemas_to_a_package_without_the_grant() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .load()
        .await
        .expect("loads degraded");
    for command in ["context", "schemas", "schema"] {
        // The command declares the capability it costs, so the refusal comes before it starts.
        let refused = match plugin
            .invoke(&format!("dev.example.echo.command.{command}"), args(&[]))
            .await
        {
            Err(error) => error.name,
            Ok(invocation) => {
                let (_, result) = invocation.collect().await;
                assert_eq!(
                    result.status,
                    InvokeStatus::Failed,
                    "{command} without its grant"
                );
                result.error.expect("structured").name
            }
        };
        assert_eq!(
            refused, "capability.denied",
            "{command}: deny by default (spec §31.19)"
        );
    }
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- the object, relation, history, process and secret domains (spec §31.12; ADR-0568) --------

/// A host with two items, one edge, two history entries, a process that takes signals and one
/// secret — deterministic, so the domains are proven without a machine behind them.
#[derive(Debug, Default)]
struct FakeHost {
    signals: std::sync::Mutex<Vec<(Json, String)>>,
    appended: std::sync::Mutex<Vec<(String, Json)>>,
    contributed: std::sync::Mutex<Vec<(String, Json)>>,
}

fn item(seq: i64, label: &str) -> Json {
    json!({"schema": "dev.example.echo.item/1", "seq": seq, "label": label})
}

#[async_trait::async_trait]
impl ono_kuang_supervisor::HostServices for FakeHost {
    async fn object_get(&self, id: Json) -> Result<Json, ono_kuang_supervisor::HostError> {
        match id
            .get("values")
            .and_then(Json::as_array)
            .and_then(|v| v.first())
            .and_then(Json::as_i64)
        {
            Some(1) => Ok(item(1, "first")),
            Some(2) => Ok(item(2, "second")),
            _ => Err(ono_kuang_supervisor::HostError::not_found(format!(
                "no item {id}"
            ))),
        }
    }
    async fn object_query(
        &self,
        query: Json,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        if query.get("target").and_then(Json::as_str) != Some("item") {
            return Err(ono_kuang_supervisor::HostError::not_found(
                "the fake host has only `item`",
            ));
        }
        let limit = query
            .get("limit")
            .and_then(Json::as_u64)
            .unwrap_or(u64::MAX);
        let mut items = vec![
            item(1, "first"),
            item(2, "second"),
            item(3, "third"),
            item(4, "fourth"),
        ];
        items.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(ono_kuang_supervisor::ready_stream(items))
    }
    async fn object_resolve(
        &self,
        target: String,
        selector: Json,
    ) -> Result<Vec<Json>, ono_kuang_supervisor::HostError> {
        let wanted = selector
            .get("field")
            .and_then(|f| f.get("value"))
            .and_then(Json::as_str);
        Ok(match (target.as_str(), wanted) {
            ("item", Some("second")) => vec![
                json!({"id": {"schema": "dev.example.echo.item/1", "values": [2]}, "label": "second"}),
            ],
            _ => Vec::new(),
        })
    }
    async fn object_snapshot(
        &self,
        query: Json,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        self.object_query(query).await
    }
    async fn object_subscribe(
        &self,
        _query: Json,
        _overflow: Option<String>,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            for seq in 5..7 {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                let _ = tx
                    .send(Ok(json!({"kind": "added", "object": item(seq, "late")})))
                    .await;
            }
        });
        Ok(rx)
    }
    async fn object_watch(
        &self,
        query: Json,
        _policy: Json,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        self.object_query(query).await
    }
    async fn relations_query(
        &self,
        from: Option<Json>,
        _to: Option<Json>,
        _relations: Option<Vec<String>>,
        _depth: Option<u64>,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        let from = from.ok_or_else(|| {
            ono_kuang_supervisor::HostError::malformed("the fake host walks from an object")
        })?;
        Ok(ono_kuang_supervisor::ready_stream(vec![json!({
            "from": from, "to": {"schema": "dev.example.echo.item/1", "values": [2]},
            "relation": "follows", "direction": "directed", "confidence": "exact",
            "provider": "fake", "metadata": {}
        })]))
    }
    async fn relations_contribute(
        &self,
        package: &str,
        edges: Vec<Json>,
    ) -> Result<u64, ono_kuang_supervisor::HostError> {
        let count = edges.len() as u64;
        self.contributed
            .lock()
            .expect("lock")
            .extend(edges.into_iter().map(|edge| (package.to_owned(), edge)));
        Ok(count)
    }
    async fn history_query(
        &self,
        _window: Option<String>,
        _filter: Option<Json>,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        Ok(ono_kuang_supervisor::ready_stream(vec![
            json!({"command": "get process", "cwd": "/", "status": 0}),
            json!({"command": "curl -H 'Authorization: [redacted]'", "cwd": "/", "status": 0}),
        ]))
    }
    async fn history_append(
        &self,
        package: &str,
        entry: Json,
    ) -> Result<(), ono_kuang_supervisor::HostError> {
        self.appended
            .lock()
            .expect("lock")
            .push((package.to_owned(), entry));
        Ok(())
    }
    async fn process_signal(
        &self,
        object: Json,
        signal: String,
    ) -> Result<Json, ono_kuang_supervisor::HostError> {
        self.signals
            .lock()
            .expect("lock")
            .push((object.clone(), signal.clone()));
        Ok(
            json!({"target": object, "operation": "process.signal", "status": "success", "changed": true, "message": format!("sent {signal}")}),
        )
    }
    async fn secret_request(
        &self,
        _package: &str,
        name: &str,
        _purpose: &str,
    ) -> Result<(), ono_kuang_supervisor::HostError> {
        if name == "api-token" {
            Ok(())
        } else {
            Err(ono_kuang_supervisor::HostError::not_found(format!(
                "no secret `{name}`"
            )))
        }
    }
}

fn strings(events: &[StreamEvent]) -> Vec<String> {
    values_of(events)
        .into_iter()
        .filter_map(|value| match value {
            Value::String(text) => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn should_stream_the_hosts_objects_to_a_granted_package_and_honour_its_limit() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .host(std::sync::Arc::new(FakeHost::default()))
        .grant(Capability::ObjectRead)
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.objects",
            args(&[("target", json!("item")), ("limit", json!(3))]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    assert_eq!(
        strings(&events),
        ["first", "second", "third"],
        "three, pulled three at a time"
    );
    let audit = plugin.audit();
    assert!(
        audit
            .iter()
            .any(|event| event.action == "objects.query" && event.result == AuditResult::Success),
        "the query is audited under object.read with its target"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_fetch_one_object_by_identity_and_refuse_one_that_is_not_there() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .host(std::sync::Arc::new(FakeHost::default()))
        .grant(Capability::ObjectRead)
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.object",
            args(&[(
                "id",
                json!({"schema": "dev.example.echo.item/1", "values": [2]}),
            )]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    assert!(strings(&events)[0].contains("\"label\":\"second\""));

    let invocation = plugin
        .invoke(
            "dev.example.echo.command.object",
            args(&[(
                "id",
                json!({"schema": "dev.example.echo.item/1", "values": [9]}),
            )]),
        )
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    assert_eq!(
        result.error.expect("structured").name,
        "resolve.target_not_found"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_walk_the_edges_and_the_history_the_host_shares() {
    let host = std::sync::Arc::new(FakeHost::default());
    let plugin = TestHost::new(PLUGIN, &manifest())
        .host(host.clone())
        .grant(Capability::RelationRead)
        .grant(Capability::HistoryRead)
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.edges",
            args(&[(
                "from",
                json!({"schema": "dev.example.echo.item/1", "values": [1]}),
            )]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    assert!(
        strings(&events)[0].contains("-[follows]->"),
        "got {:?}",
        strings(&events)
    );

    let invocation = plugin
        .invoke("dev.example.echo.command.history", args(&[]))
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    let shown = strings(&events);
    assert_eq!(shown.len(), 2);
    assert!(
        shown[1].contains("[redacted]"),
        "secret-bearing values reach a package redacted (ADR-0015 T8); got {shown:?}"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_signal_a_process_within_the_granted_signals_and_refuse_one_outside() {
    let host = std::sync::Arc::new(FakeHost::default());
    let mut scope = JsonMap::new();
    scope.insert("signals".to_owned(), json!(["SIGTERM"]));
    let plugin = TestHost::new(PLUGIN, &manifest())
        .host(host.clone())
        .grant_scoped(Capability::ProcessSignal, scope)
        .load()
        .await
        .expect("loads");
    let object = json!({"schema": "ono.process/1", "values": [4242, "2026-08-26T12:00:00Z"]});
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.signal",
            args(&[("object", object.clone()), ("signal", json!("SIGTERM"))]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    assert!(strings(&events)[0].contains("\"status\":\"success\""));
    assert_eq!(
        host.signals.lock().expect("lock").len(),
        1,
        "one signal reached the host"
    );

    let invocation = plugin
        .invoke(
            "dev.example.echo.command.signal",
            args(&[("object", object), ("signal", json!("SIGKILL"))]),
        )
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    assert_eq!(
        result.error.expect("structured").name,
        "capability.scope_violation"
    );
    assert_eq!(
        host.signals.lock().expect("lock").len(),
        1,
        "the refused signal never reached the host"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_issue_a_secret_handle_within_the_scope_and_never_the_material() {
    let mut scope = JsonMap::new();
    scope.insert("secrets".to_owned(), json!(["api-token"]));
    let plugin = TestHost::new(PLUGIN, &manifest())
        .host(std::sync::Arc::new(FakeHost::default()))
        .grant_scoped(Capability::SecretUse, scope)
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.secret",
            args(&[("name", json!("api-token"))]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    let shown = strings(&events);
    assert!(
        shown[0].starts_with("handle:") && shown[0] != "handle:none",
        "got {shown:?}"
    );
    assert_eq!(shown.get(1).map(String::as_str), Some("released"));

    let invocation = plugin
        .invoke(
            "dev.example.echo.command.secret",
            args(&[("name", json!("db-password"))]),
        )
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    assert_eq!(
        result.error.expect("structured").name,
        "capability.scope_violation"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_say_a_domain_is_unavailable_when_the_host_serves_none() {
    let plugin = TestHost::new(PLUGIN, &manifest())
        .grant(Capability::ObjectRead)
        .grant(Capability::HistoryRead)
        .load()
        .await
        .expect("loads");
    for command in ["objects", "history"] {
        let invocation = plugin
            .invoke(&format!("dev.example.echo.command.{command}"), args(&[]))
            .await
            .expect("starts");
        let (_, result) = invocation.collect().await;
        assert_eq!(result.status, InvokeStatus::Failed, "{command}");
        assert_eq!(
            result.error.expect("structured").name,
            "provider.unavailable",
            "{command}: the honest answer of a host that serves none (spec §35.3)"
        );
    }
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}
