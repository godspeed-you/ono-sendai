//! KUANG/11 at the shell boundary (spec §31): a package is discovered, loaded under the
//! capability broker, and its contributed command runs like any other stage.

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::Shell;

/// A scratch plugin directory holding the example package, laid out as installed.
fn plugin_home() -> ono_testkit::Scratch {
    let scratch = ono_testkit::scratch();
    let manifest = r#"
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
"#;
    scratch.write("dev.example.echo/manifest.yaml", manifest);
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    let entry = scratch.path().join("dev.example.echo/runtime/echo");
    std::fs::create_dir_all(entry.parent().expect("a parent")).expect("the runtime directory");
    std::fs::copy(&binary, &entry).expect("the example plugin binary is built");
    scratch
}

fn ono(home: &ono_testkit::Scratch, script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .env("ONO_PLUGIN_PATH", home.path().display().to_string())
        .run()
}

#[test]
fn should_discover_an_installed_package() {
    let home = plugin_home();
    let run = ono(&home, "get plugin");
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("dev.example.echo") && text.contains("installed"),
        "the package directory is the installed set (spec §31.8): {text:?}"
    );
}

#[test]
fn should_load_a_package_and_run_its_contributed_command() {
    let home = plugin_home();
    let run = ono(
        &home,
        "load plugin dev.example.echo; echo:emit --count 3 | to json",
    );
    run.assert_success();
    assert!(
        run.stdout().contains("[1,2,3]"),
        "the contributed command streams typed values (spec §31.23): {:?}",
        run.stdout()
    );
}

#[test]
fn should_refuse_a_contributed_command_before_its_package_was_loaded() {
    let home = plugin_home();
    let run = ono(&home, "echo:emit --count 3");
    assert!(!run.status().is_success());
    assert!(
        run.stderr().contains("Ono-Sendai-E"),
        "an unloaded package's namespace is a structured refusal: {:?}",
        run.stderr()
    );
}
