//! The four ways a plugin can fail, told apart (v0.4.1 §18, issue #62).
//!
//! §18 splits one word — "the plugin failed" — into four outcomes that call for different things
//! from the operator, and forbids one confusion in particular:
//!
//! - **§18.1 pre-exec failure.** The confinement could not be installed, so the package never
//!   safely started. It gets a *launch* failure, and it MUST NOT be quarantined: quarantine is a
//!   judgement about how a package behaved, and this one never got to behave.
//! - **§18.2 protocol violation.** The package started correctly and then sent something
//!   malformed, oversized or beyond its credit. Quarantine is exactly right.
//! - **§18.3 resource-limit termination.** The kernel ended it over a limit the host configured.
//!   Classified apart from a crash, naming the resource class rather than "plugin exited".
//! - **§18.4 crash containment.** The package died without breaking the protocol. The shell keeps
//!   running, the other packages keep running, and nothing it contributed is left visible as
//!   healthy.
//!
//! Each case is driven through the deterministic test host of spec §31.73 against the real
//! example plugin binary, so what is asserted is the outcome a shell would see (ADR-0446).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::sync::Arc;

use ono_kuang_protocol::{Capability, Control, KuangErrorCode, PluginState};
use ono_kuang_supervisor::{ConfinementPlan, ConfinementPlatform, NativePlatform, StreamEvent};
use ono_kuang_testhost::TestHost;
use serde_json::{Map as JsonMap, Value as Json};

const PLUGIN: &str = env!("CARGO_BIN_EXE_kuang-example-plugin");
const PACKAGE: &str = "dev.example.echo";

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
network:
  outbound: none
"#
    .to_owned()
}

/// The injectable platform layer of §59.7: everything the host installs, except one refusal.
struct Refuses(Control);

impl ConfinementPlatform for Refuses {
    fn install(&self, control: Control, plan: &ConfinementPlan) -> std::io::Result<()> {
        if control == self.0 {
            return Err(std::io::Error::from_raw_os_error(libc::EPERM));
        }
        NativePlatform.install(control, plan)
    }
}

fn refusing(control: Control) -> Arc<dyn ConfinementPlatform> {
    Arc::new(Refuses(control))
}

fn host() -> TestHost {
    TestHost::new(PLUGIN, &manifest()).grant(Capability::ClockRead)
}

#[tokio::test]
async fn should_distinguish_a_launch_failure_from_a_quarantine_a_resource_kill_and_a_crash() {
    // --- §18.1 launch failure -----------------------------------------------------------------
    let launch = host()
        .confinement(refusing(Control::NoNewPrivs))
        .load()
        .await
        .expect_err("a package whose required confinement could not be installed does not load");
    assert_eq!(
        launch.code(),
        KuangErrorCode::PluginNoNewPrivsFailed,
        "§18.1: a plugin that never safely started gets a launch failure, in §16.3's own error \
         family. Got: {launch:?}"
    );
    // The consequence §18.1 actually cares about: the package is not barred afterwards, because
    // nothing about it was judged. The same package loads on the next attempt.
    let after = host().load().await.expect("the package itself is fine");
    assert_eq!(
        after.state(),
        PluginState::Loaded,
        "§18.1: a launch failure MUST NOT put the package into quarantine — it never started, so \
         there is nothing to hold against it"
    );

    // --- §18.2 protocol violation -------------------------------------------------------------
    let violator = TestHost::new(PLUGIN, &manifest())
        .args(&["--misbehave=garbage"])
        .load()
        .await
        .expect("the misbehaving fixture starts correctly and misbehaves afterwards");
    let (_, result) = violator
        .invoke(&format!("{PACKAGE}.command.flood"), JsonMap::new())
        .await
        .expect("the invocation reaches a plugin that is running")
        .collect()
        .await;
    let violation = result.error.expect("a malformed frame ends the invocation");
    assert_eq!(
        violation.name,
        KuangErrorCode::RuntimeProtocolViolation.name(),
        "§18.2: a package that started correctly and then sent nonsense is a protocol violation"
    );
    assert_eq!(
        violator.state(),
        PluginState::Quarantined,
        "§18.2: this is the outcome quarantine is for, and the one §18.1 must not share"
    );
    assert!(violator.quarantine_reason().is_some());

    // --- §18.3 resource-limit termination -----------------------------------------------------
    let hog = host().load().await.expect("the package loads");
    let (_, result) = hog
        .invoke(&format!("{PACKAGE}.command.hog"), mib(512))
        .await
        .expect("the invocation reaches a running plugin")
        .collect()
        .await;
    let killed = result
        .error
        .expect("an instance that hits its ceiling ends");
    assert_eq!(
        killed.name,
        KuangErrorCode::RuntimeMemoryLimit.name(),
        "§18.3: the kernel ended it over a configured limit, and the classification says which \
         one rather than 'plugin exited'. Got: {killed:?}"
    );
    assert_eq!(
        killed.metadata.get("resource_class").and_then(Json::as_str),
        Some("memory"),
        "§18.3: the error identifies the enforced resource class, so a script does not have to \
         read the sentence. Got: {killed:?}"
    );
    assert_ne!(
        hog.state(),
        PluginState::Quarantined,
        "§18.3: reaching a declared ceiling is not misbehaviour, so it is not quarantine"
    );

    // --- §18.4 crash --------------------------------------------------------------------------
    let crasher = TestHost::new(PLUGIN, &manifest())
        .args(&["--misbehave=die"])
        .load()
        .await
        .expect("the fixture starts correctly and dies later");
    let (_, result) = crasher
        .invoke(&format!("{PACKAGE}.command.flood"), JsonMap::new())
        .await
        .expect("the invocation reaches a running plugin")
        .collect()
        .await;
    let crash = result
        .error
        .expect("a plugin that exits mid-invocation fails it");
    assert_eq!(
        crash.name,
        KuangErrorCode::RuntimeTrap.name(),
        "§18.4: a package that broke no protocol rule and simply stopped is a crash. Got: {crash:?}"
    );
    assert_eq!(
        crash.metadata.get("resource_class"),
        None,
        "§18.3's resource class belongs to a limit the host enforced, and nothing enforced \
         anything here: {crash:?}"
    );
    assert_ne!(
        crasher.state(),
        PluginState::Quarantined,
        "§31.34 and §18.4: a crash degrades the plugin; quarantine is a judgement about conduct"
    );
}

#[tokio::test]
async fn should_keep_the_shell_and_the_other_plugins_running_when_one_plugin_crashes() {
    // §18.4: "Plugin failure MUST not corrupt the shell's provider registry or leave partially
    // registered capabilities visible as healthy."
    let survivor = host().load().await.expect("the honest package loads");
    let crasher = TestHost::new(PLUGIN, &manifest())
        .args(&["--misbehave=die"])
        .load()
        .await
        .expect("the crashing package loads");

    let (_, result) = crasher
        .invoke(&format!("{PACKAGE}.command.flood"), JsonMap::new())
        .await
        .expect("the invocation reaches a running plugin")
        .collect()
        .await;
    assert!(result.error.is_some(), "the crash fails its own invocation");

    // Nothing the dead instance contributed is left looking healthy.
    assert_ne!(
        crasher.state(),
        PluginState::Loaded,
        "§18.4: a registration from a dead plugin does not stay visible as healthy"
    );
    assert!(
        crasher.last_failure().is_some(),
        "§18.4: and the reason it is unavailable is recorded rather than inferred"
    );
    assert!(
        crasher.probe().await.is_err(),
        "a call into a dead instance is refused rather than hanging"
    );

    // The other package is untouched: same state, and it still answers.
    assert_eq!(
        survivor.state(),
        PluginState::Loaded,
        "§31.34: plugin failure degrades the plugin, not the shell and not its neighbours"
    );
    let (events, result) = survivor
        .invoke(&format!("{PACKAGE}.command.emit"), emit(3))
        .await
        .expect("the surviving instance still takes invocations")
        .collect()
        .await;
    assert!(result.error.is_none(), "the survivor answered: {result:?}");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::Value(_))),
        "the survivor produced values after its neighbour died, got {events:?}"
    );
}

fn mib(count: i64) -> JsonMap<String, Json> {
    let mut args = JsonMap::new();
    args.insert("mib".to_owned(), Json::from(count));
    args
}

fn emit(count: i64) -> JsonMap<String, Json> {
    let mut args = JsonMap::new();
    args.insert("count".to_owned(), Json::from(count));
    args
}
