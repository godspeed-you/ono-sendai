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

/// The example package built as a component, or the reason it could not be.
///
/// The build goes to its own target directory, so it never contends for the lock of the one
/// this test runs from, and it is cached there between runs.
pub fn component_fixture() -> Result<std::path::PathBuf, String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the workspace root")
        .to_path_buf();
    let installed = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map_err(|error| format!("rustup could not be run: {error}"))?;
    if !String::from_utf8_lossy(&installed.stdout).contains("wasm32-wasip2") {
        return Err("the wasm32-wasip2 target is not installed".to_owned());
    }
    let target_dir = root.join("target").join("wasm-fixture");
    let status = std::process::Command::new("cargo")
        .args([
            "build",
            "--quiet",
            "--target",
            "wasm32-wasip2",
            "-p",
            "ono-kuang-sdk",
            "--bin",
            "kuang-example-plugin",
        ])
        .env("CARGO_TARGET_DIR", &target_dir)
        .current_dir(&root)
        .status()
        .map_err(|error| format!("cargo could not be run: {error}"))?;
    if !status.success() {
        return Err(format!("the component build ended with {status}"));
    }
    Ok(target_dir
        .join("wasm32-wasip2")
        .join("debug")
        .join("kuang-example-plugin.wasm"))
}

/// Compiles `component` with the SDK's `kuang-compile` into a fresh store, as `install plugin`
/// does into the operator's (ADR-0870). Returns the scratch directory, which has to outlive the
/// load, and the store inside it.
pub fn compile_into_scratch(
    component: &std::path::Path,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let store = scratch.path().join("compiled");
    let run = std::process::Command::new(env!("CARGO_BIN_EXE_kuang-compile"))
        .arg("--store")
        .arg(&store)
        .arg(component)
        .output()
        .expect("kuang-compile runs");
    assert!(
        run.status.success(),
        "kuang-compile compiles the component: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    (scratch, store)
}
