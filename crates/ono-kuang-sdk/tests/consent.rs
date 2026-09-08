//! Just-in-time consent at the broker (K11P §14, §20.3, §33 Gates H–J, §34.7; ADR-0603), run
//! against the example plugin under the deterministic test host with a scripted consent source.
//!
//! Every test asserts an outcome at the protocol boundary: what the call answered, what the
//! source was asked, what the audit trail recorded. None of them knows how the broker asks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_kuang_protocol::{AuditResult, Capability, InvokeStatus};
use ono_kuang_supervisor::{ConsentAnswer, ConsentDuration, ScriptedConsent};
use ono_kuang_testhost::TestHost;
use ono_value::Value;
use serde_json::{Map as JsonMap, Value as Json, json};

mod support;
use support::values_of;

const PLUGIN: &str = env!("CARGO_BIN_EXE_kuang-example-plugin");

/// A `kuang-package/2` manifest whose helper execution is a just-in-time permission, as the
/// Kubernetes reference provider declares it (K11P §8.2).
fn declared_manifest() -> String {
    r#"
format: kuang-package/2
package:
  id: dev.example.echo
  name: echo
  version: 0.1.0
  description: Emits what it is asked to emit.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.2 <12"
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
    - process.exec
network:
  outbound: none
permissions:
  profiles:
    minimal: {title: Minimal, permissions: []}
    recommended: {title: Recommended, permissions: [clock]}
  requests:
    - id: clock
      kind: local-contribution
      title: Read the clock
      phase: automatic
      recommended: true
      grants:
        - capability: clock.read
    - id: login-helper
      kind: execute-helper
      title: Run an external login helper when a context requires it
      purpose: authenticate to the selected context
      phase: jit
      recommended: false
      grants:
        - capability: process.exec
          scope: runtime-derived
"#
    .to_owned()
}

/// A `kuang-package/1` manifest that declares nothing about permissions: `process.exec` is
/// derived as a just-in-time permission (K11P §8.1, ADR-0600 §4).
fn derived_manifest() -> String {
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
    - process.exec
network:
  outbound: none
"#
    .to_owned()
}

/// Two helper programs the tests may name: files that exist, so the broker resolves them to
/// themselves, in a directory the test owns rather than a system path that may be a symlink.
struct Helpers {
    _dir: tempfile::TempDir,
    a: String,
    b: String,
}

fn helpers() -> Helpers {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut made = Vec::new();
    for name in ["login-helper", "other-helper"] {
        let path = dir.path().join(name);
        std::fs::write(&path, "#!/bin/sh\necho through the broker\n").expect("written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("executable");
        made.push(
            std::fs::canonicalize(&path)
                .expect("canonical")
                .to_string_lossy()
                .into_owned(),
        );
    }
    Helpers {
        _dir: dir,
        b: made.pop().expect("two"),
        a: made.pop().expect("two"),
    }
}

/// A host that runs nothing and answers a `process.exec` as a program that printed one line
/// and exited cleanly — the outside world, faked, so the broker in front of it is real.
#[derive(Debug)]
struct ExecHost;

#[async_trait::async_trait]
impl ono_kuang_supervisor::HostServices for ExecHost {
    async fn object_get(&self, id: Json) -> Result<Json, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost.object_get(id).await
    }
    async fn object_query(
        &self,
        query: Json,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost.object_query(query).await
    }
    async fn object_resolve(
        &self,
        target: String,
        selector: Json,
    ) -> Result<Vec<Json>, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost
            .object_resolve(target, selector)
            .await
    }
    async fn object_snapshot(
        &self,
        query: Json,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost.object_snapshot(query).await
    }
    async fn object_subscribe(
        &self,
        query: Json,
        overflow: Option<String>,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost
            .object_subscribe(query, overflow)
            .await
    }
    async fn object_watch(
        &self,
        query: Json,
        policy: Json,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost
            .object_watch(query, policy)
            .await
    }
    async fn relations_query(
        &self,
        from: Option<Json>,
        to: Option<Json>,
        relations: Option<Vec<String>>,
        depth: Option<u64>,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost
            .relations_query(from, to, relations, depth)
            .await
    }
    async fn relations_contribute(
        &self,
        package: &str,
        edges: Vec<Json>,
    ) -> Result<u64, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost
            .relations_contribute(package, edges)
            .await
    }
    async fn history_query(
        &self,
        window: Option<String>,
        filter: Option<Json>,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost
            .history_query(window, filter)
            .await
    }
    async fn history_append(
        &self,
        package: &str,
        entry: Json,
    ) -> Result<(), ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost
            .history_append(package, entry)
            .await
    }
    async fn process_signal(
        &self,
        object: Json,
        signal: String,
    ) -> Result<Json, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost
            .process_signal(object, signal)
            .await
    }
    async fn process_exec(
        &self,
        _package: &str,
        _program: String,
        _arguments: Vec<String>,
        _environment: Vec<(String, String)>,
    ) -> Result<ono_kuang_supervisor::LiveStream, ono_kuang_supervisor::HostError> {
        Ok(ono_kuang_supervisor::ready_stream(vec![
            json!({"stream": "stdout", "line": "through the broker"}),
            json!({"exited": 0}),
        ]))
    }
    async fn network_connect(
        &self,
        host: String,
        port: u16,
        protocol: String,
    ) -> Result<ono_kuang_supervisor::Connection, ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost
            .network_connect(host, port, protocol)
            .await
    }
    async fn network_listen(
        &self,
        port: u16,
        protocol: String,
    ) -> Result<
        tokio::sync::mpsc::Receiver<(String, ono_kuang_supervisor::Connection)>,
        ono_kuang_supervisor::HostError,
    > {
        ono_kuang_supervisor::NoHost
            .network_listen(port, protocol)
            .await
    }
    async fn secret_request(
        &self,
        package: &str,
        name: &str,
        purpose: &str,
    ) -> Result<(), ono_kuang_supervisor::HostError> {
        ono_kuang_supervisor::NoHost
            .secret_request(package, name, purpose)
            .await
    }
}

fn host(manifest: &str) -> TestHost {
    TestHost::new(PLUGIN, manifest).host(std::sync::Arc::new(ExecHost))
}

fn args(pairs: &[(&str, Json)]) -> JsonMap<String, Json> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

fn exec_args(program: &str) -> JsonMap<String, Json> {
    args(&[
        ("program", json!(program)),
        ("arguments", json!(["through", "the", "broker"])),
    ])
}

fn allow(duration: ConsentDuration) -> ConsentAnswer {
    ConsentAnswer::Allow {
        duration,
        scope: None,
    }
}

#[tokio::test]
async fn should_ask_at_the_call_and_run_the_helper_once_when_allowed_once() {
    let helpers = helpers();
    let consent = ScriptedConsent::answering([allow(ConsentDuration::Once)]);
    let plugin = host(&declared_manifest())
        .consent(consent.clone())
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.exec", exec_args(&helpers.a))
        .await
        .expect("starts: a just-in-time family does not refuse the invocation");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{events:?}");
    assert!(
        values_of(&events).contains(&Value::String("stdout: through the broker".into())),
        "the helper ran through the broker: {events:?}"
    );
    let asked = consent.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].permission.id, "login-helper");
    assert_eq!(asked[0].capability, Capability::ProcessExec);
    assert_eq!(
        asked[0].program(),
        Some(helpers.a.as_str()),
        "the exact helper is named (K11P §14.2)"
    );
    assert_eq!(asked[0].arguments, vec!["through", "the", "broker"]);
    assert_eq!(
        asked[0].permission.purpose.as_deref(),
        Some("authenticate to the selected context")
    );

    // `once` is one evaluation: the next call asks again, and with nothing scripted it is refused.
    let invocation = plugin
        .invoke("dev.example.echo.command.exec", exec_args(&helpers.a))
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    assert_eq!(
        result.error.expect("structured").name,
        "permission.required"
    );
    assert_eq!(consent.asked().len(), 2);
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_keep_a_session_answer_for_that_program_and_ask_again_for_another() {
    let helpers = helpers();
    let consent = ScriptedConsent::answering([
        allow(ConsentDuration::Session),
        allow(ConsentDuration::Session),
    ]);
    let plugin = host(&declared_manifest())
        .consent(consent.clone())
        .load()
        .await
        .expect("loads");
    for _ in 0..2 {
        let invocation = plugin
            .invoke("dev.example.echo.command.exec", exec_args(&helpers.a))
            .await
            .expect("starts");
        let (_, result) = invocation.collect().await;
        assert_eq!(result.status, InvokeStatus::Completed);
    }
    assert_eq!(
        consent.asked().len(),
        1,
        "a session answer covers the same program without asking again"
    );
    // Another program is another question (K11P §14.3, Gate I): the grant was scoped to
    // `/bin/echo`, never to process execution at large.
    let invocation = plugin
        .invoke("dev.example.echo.command.exec", exec_args(&helpers.b))
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    let asked = consent.asked();
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[1].program(), Some(helpers.b.as_str()));
    let scope = asked[1]
        .scope
        .as_ref()
        .expect("the narrowest enforceable scope");
    assert_eq!(scope["programs"], json!([helpers.b.clone()]));
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_never_widen_an_always_answer_beyond_the_program_that_was_asked_about() {
    // A source that answers with a wider scope than it was asked about is narrowed back
    // (K11P §34.7): `always for this program` is never unrestricted `process.exec`.
    let helpers = helpers();
    let mut wide = JsonMap::new();
    wide.insert(
        "programs".to_owned(),
        json!([
            helpers.a.clone(),
            format!(
                "{}/**",
                std::path::Path::new(&helpers.a)
                    .parent()
                    .expect("a parent")
                    .display()
            )
        ]),
    );
    let consent = ScriptedConsent::answering([ConsentAnswer::Allow {
        duration: ConsentDuration::Always,
        scope: Some(wide),
    }]);
    let plugin = host(&declared_manifest())
        .consent(consent.clone())
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.exec", exec_args(&helpers.a))
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    let invocation = plugin
        .invoke("dev.example.echo.command.exec", exec_args(&helpers.b))
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(
        result.status,
        InvokeStatus::Failed,
        "`/bin/true` was never consented to, whatever the source answered"
    );
    assert_eq!(
        consent.asked().len(),
        2,
        "another program is asked about again"
    );
    let allowed = plugin
        .audit()
        .into_iter()
        .find(|event| event.action == "permission.allow")
        .expect("the answer is audited");
    assert_eq!(
        allowed.scope,
        Some(json!({"programs": [helpers.a.clone()], "executables": ["login-helper"]})),
        "the grant is scoped to the resolved path and the executable name, and nothing wider"
    );
    assert_eq!(
        allowed.target,
        Some(json!({"permission": "login-helper", "duration": "always"}))
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_refuse_with_permission_denied_when_the_person_says_no() {
    let helpers = helpers();
    let consent = ScriptedConsent::answering([]);
    // The source is asked once and refuses as a person would.
    let refusing = std::sync::Arc::new(Refusing);
    let plugin = host(&declared_manifest())
        .consent(refusing)
        .load()
        .await
        .expect("loads");
    let _ = consent;
    let invocation = plugin
        .invoke("dev.example.echo.command.exec", exec_args(&helpers.a))
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    let error = result.error.expect("structured");
    assert_eq!(error.name, "permission.denied");
    assert!(
        error.message.contains("login helper") && error.message.contains(&helpers.a),
        "the message leads with the permission and names the helper (K11P §24.4): {}",
        error.message
    );
    assert!(
        values_of(&events).is_empty(),
        "a denial is a failure, never an empty result (K11P §14.5)"
    );
    let audit = plugin.audit();
    let asked = audit
        .iter()
        .find(|event| event.action == "permission.ask")
        .expect("the question is audited");
    let denied = audit
        .iter()
        .find(|event| event.action == "permission.deny")
        .expect("the answer is audited");
    assert_eq!(denied.result, AuditResult::Denied);
    assert!(asked.correlation.is_some());
    assert_eq!(
        asked.correlation, denied.correlation,
        "one question and its answer share a correlation id (K11P §23.2)"
    );
    let call = audit
        .iter()
        .find(|event| event.action == "process.exec")
        .expect("the call itself is audited");
    assert_eq!(call.result, AuditResult::Denied);
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[derive(Debug)]
struct Refusing;

impl ono_kuang_supervisor::ConsentSource for Refusing {
    fn consent(&self, request: &ono_kuang_supervisor::ConsentRequest) -> ConsentAnswer {
        ConsentAnswer::denied(
            request,
            "`set permission echo login-helper --decision allow` allows it",
        )
    }
}

#[tokio::test]
async fn should_answer_permission_required_with_the_remedy_when_nobody_can_be_asked() {
    let helpers = helpers();
    // No consent source: the default answers as a script must be answered (K11P §20.3, Gate J).
    let plugin = host(&declared_manifest()).load().await.expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.exec", exec_args(&helpers.a))
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Failed);
    let error = result.error.expect("structured");
    assert_eq!(error.name, "permission.required");
    assert_eq!(error.metadata["plugin"], json!("dev.example.echo"));
    assert_eq!(error.metadata["permission"], json!("login-helper"));
    assert_eq!(error.metadata["capability"], json!("process.exec"));
    assert_eq!(
        error.metadata["requested_scope"]["program"],
        json!(helpers.a.clone())
    );
    assert_eq!(
        error.metadata["reason"],
        json!("authenticate to the selected context")
    );
    let remedy = error.metadata["remedy"].as_str().expect("a remedy");
    assert!(
        remedy.contains("process.exec") && remedy.contains(&format!("programs={}", helpers.a)),
        "the remedy is a command line for exactly this program: {remedy}"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_answer_ask_to_a_check_of_a_just_in_time_family_nobody_has_decided() {
    let plugin = host(&declared_manifest()).load().await.expect("loads");
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.check",
            args(&[("capability", json!("process.exec"))]),
        )
        .await
        .expect("starts");
    let (events, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    assert_eq!(
        values_of(&events),
        vec![Value::String("process.exec:ask".into())],
        "a check never prompts, and `ask` is not a grant (ADR-0603 §4)"
    );
    // A family nothing decides just in time is still `denied`.
    let invocation = plugin
        .invoke(
            "dev.example.echo.command.check",
            args(&[("capability", json!("filesystem.read"))]),
        )
        .await
        .expect("starts");
    let (events, _) = invocation.collect().await;
    assert_eq!(
        values_of(&events),
        vec![Value::String("filesystem.read:denied".into())]
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_never_ask_over_an_operator_deny() {
    let helpers = helpers();
    let consent = ScriptedConsent::answering([allow(ConsentDuration::Always)]);
    let plugin = host(&declared_manifest())
        .consent(consent.clone())
        .deny(Capability::ProcessExec)
        .load()
        .await
        .expect("loads");
    // The command declares `process.exec`, and an operator deny refuses the invocation itself
    // before any code of the package runs — and before anything could be asked.
    let refused = plugin
        .invoke("dev.example.echo.command.exec", exec_args(&helpers.a))
        .await
        .expect_err("refused at the invocation");
    assert_eq!(refused.name, "capability.denied");
    assert!(
        consent.asked().is_empty(),
        "an operator deny is never re-asked (K11P §14.1, ADR-0603 §1)"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_derive_a_just_in_time_permission_for_a_package_that_declares_none() {
    let helpers = helpers();
    let consent = ScriptedConsent::answering([allow(ConsentDuration::Once)]);
    let plugin = host(&derived_manifest())
        .consent(consent.clone())
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.exec", exec_args(&helpers.a))
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed, "{:?}", result.error);
    let asked = consent.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].permission.id, "process-exec");
    assert_eq!(asked[0].permission.title, "Run external programs");
    assert!(asked[0].permission.derived);
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_not_ask_for_a_family_the_policy_already_grants() {
    let helpers = helpers();
    let consent = ScriptedConsent::answering([allow(ConsentDuration::Once)]);
    let mut scope = JsonMap::new();
    scope.insert(
        "programs".to_owned(),
        json!([format!(
            "{}/**",
            std::path::Path::new(&helpers.a)
                .parent()
                .expect("a parent")
                .display()
        )]),
    );
    let plugin = host(&declared_manifest())
        .consent(consent.clone())
        .grant_scoped(Capability::ProcessExec, scope)
        .load()
        .await
        .expect("loads");
    let invocation = plugin
        .invoke("dev.example.echo.command.exec", exec_args(&helpers.a))
        .await
        .expect("starts");
    let (_, result) = invocation.collect().await;
    assert_eq!(result.status, InvokeStatus::Completed);
    assert!(
        consent.asked().is_empty(),
        "a standing grant is not a question (Gate H)"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}
