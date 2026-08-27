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

/// A scratch plugin directory holding the SDK's declarative adapter package.
fn adapter_home() -> ono_testkit::Scratch {
    let scratch = ono_testkit::scratch();
    let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ono-kuang-sdk/examples/adapter-package/dev.example.users");
    let mut stack = vec![source.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .expect("the example package")
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path.strip_prefix(&source).expect("under the package");
            let target = scratch.path().join("dev.example.users").join(relative);
            std::fs::create_dir_all(target.parent().expect("a parent")).expect("dirs");
            std::fs::copy(&path, &target).expect("copied");
        }
    }
    scratch
}

#[test]
fn should_load_a_declarative_adapter_package_disabled_under_default_deny() {
    // Spec v0.3 §1.22, §2.3: the pack is known but cannot influence structured output until
    // process.exec is granted; the program itself keeps running raw.
    let home = adapter_home();
    let run = ono(
        &home,
        "load plugin dev.example.users; getent passwd root | where uid == 0 | count | to text",
    );
    assert_ne!(run.status().code(), 0);
    assert!(
        run.stdout().contains("loaded dev.example.users"),
        "got {:?}",
        run.stdout()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0902"),
        "adapter.disabled, not a silent downgrade, got {:?}",
        run.stderr()
    );
    let raw = ono(
        &home,
        "load plugin dev.example.users; getent passwd root | grep -c root",
    );
    raw.assert_success();
    assert_eq!(
        raw.stdout().lines().last().unwrap_or(""),
        "1",
        "bytes downstream stay raw, got {:?}",
        raw.stdout()
    );
}

#[test]
fn should_adapt_through_a_third_party_pack_once_its_grant_is_explicit() {
    let home = adapter_home();
    let run = ono(
        &home,
        "load plugin dev.example.users --grant process.exec; getent passwd root | select uid name home | to json",
    );
    run.assert_success();
    assert!(
        run.stdout().contains("\"uid\":0") && run.stdout().contains("\"home\":\"/root\""),
        "spec v0.3 §1.26: a KUANG/11 package adapts a tool, got {:?}",
        run.stdout()
    );
    let provenance = ono(
        &home,
        "load plugin dev.example.users --grant process.exec; getent passwd root | inspect | to json",
    );
    provenance.assert_success();
    assert!(
        provenance
            .stdout()
            .contains("adapter:dev.example.users.getent-passwd"),
        "got {:?}",
        provenance.stdout()
    );
}

#[test]
fn should_refuse_a_package_whose_adapter_runs_something_its_grant_does_not_name() {
    let home = adapter_home();
    // The pack stays as shipped; the manifest's grant no longer covers what it runs.
    let manifest = home.path().join("dev.example.users/manifest.yaml");
    let text = std::fs::read_to_string(&manifest)
        .expect("the manifest")
        .replace("executables: [getent]", "executables: [id]");
    std::fs::write(&manifest, text).expect("rewritten");
    let run = ono(&home, "load plugin dev.example.users --grant process.exec");
    assert_ne!(run.status().code(), 0);
    assert!(
        run.stderr().contains("Ono-Sendai-E0909"),
        "adapter.capability_denied: no adapter can spawn outside its declared set (spec v0.3 §1.22), got {:?}",
        run.stderr()
    );
}

#[test]
fn should_keep_an_experimental_pack_out_of_structured_output_unless_allowed() {
    let home = adapter_home();
    let pack = home.path().join("dev.example.users/adapters.yaml");
    let text = std::fs::read_to_string(&pack)
        .expect("the pack")
        .replace("tier: community", "tier: experimental");
    std::fs::write(&pack, text).expect("rewritten");
    let held = ono(
        &home,
        "load plugin dev.example.users --grant process.exec; getent passwd root | where uid == 0",
    );
    assert!(
        held.stderr().contains("Ono-Sendai-E0902"),
        "spec v0.3 §1.56: experimental needs an explicit allowance, got {:?}",
        held.stderr()
    );
    let allowed = ono(
        &home,
        "load plugin dev.example.users --grant process.exec --allow-experimental; getent passwd root | where uid == 0 | count | to text",
    );
    allowed.assert_success();
    assert_eq!(allowed.stdout().lines().last().unwrap_or(""), "1");
}
