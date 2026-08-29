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

/// Re-signs the copied example package with the demo key that ships beside it.
///
/// The example ships signed (spec §31.36), and a test that edits it is standing in for an author
/// who edits a package — who re-signs it. Without this the edit would be caught as `K11004`
/// before the thing under test is reached, which is correct behaviour and a different subject.
fn resign(home: &ono_testkit::Scratch) {
    use ono_kuang_protocol::{Manifest, SIGNATURE_FILE, SecretKey, SignedPackage, artifact_files};

    let directory = home.path().join("dev.example.users");
    let key = SecretKey::parse(
        &std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../ono-kuang-sdk/examples/keys/dev.example.key"),
        )
        .expect("the demo signing key ships with the example package"),
    )
    .expect("the demo key is a signing key");
    let manifest = Manifest::parse(
        &std::fs::read_to_string(directory.join("manifest.yaml")).expect("the manifest"),
    )
    .expect("the manifest is valid");
    let described = SignedPackage::new(
        &manifest.package.id,
        &manifest.package.version,
        &manifest.package.publisher,
        artifact_files(&directory),
    )
    .expect("the package is describable");
    std::fs::write(
        directory.join(SIGNATURE_FILE),
        key.sign(&described).to_yaml(),
    )
    .expect("the signature is written");
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
    resign(&home);
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
    resign(&home);
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

// --- lazy registry placeholders (spec §31.64, §31.68, ADR-0282) -------------------------------

/// The same package, declaring its commands in the manifest so the shell can register
/// placeholders for them without starting anything (spec §31.68).
fn declaring_plugin_home() -> ono_testkit::Scratch {
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
contributions:
  commands: [contributions/commands.yaml]
"#;
    scratch.write("dev.example.echo/manifest.yaml", manifest);
    scratch.write(
        "dev.example.echo/contributions/commands.yaml",
        r#"
commands:
  - id: dev.example.echo.command.emit
    verb: get
    target: echo-item
    summary: Emit a counted stream of integers.
    output: stream<int>
    argument_mode: expression
    capabilities: []
    examples:
      - get echo-item --count 3
"#,
    );
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    let entry = scratch.path().join("dev.example.echo/runtime/echo");
    std::fs::create_dir_all(entry.parent().expect("a parent")).expect("the runtime directory");
    std::fs::copy(&binary, &entry).expect("the example plugin binary is built");
    scratch
}

#[test]
fn should_answer_get_command_for_a_contributed_command_before_its_package_is_loaded() {
    let home = declaring_plugin_home();
    let run = ono(
        &home,
        r#"get command | where id == "dev.example.echo.command.emit" | select spelling origin | to json"#,
    );
    run.assert_success();
    assert!(
        run.stdout().contains(r#""spelling":"get echo-item""#)
            && run
                .stdout()
                .contains(r#""origin":"plugin(dev.example.echo, 0.1.0)""#),
        "spec §31.64, §31.68: the manifest's declaration is a registry placeholder, and it names \
         the package it came from: {:?}",
        run.stdout()
    );
}

#[test]
fn should_show_a_contributed_commands_package_in_its_help_page() {
    let home = declaring_plugin_home();
    let run = ono(&home, "help get echo-item");
    run.assert_success();
    assert!(
        run.stdout().contains("plugin(dev.example.echo, 0.1.0)"),
        "spec §31.64: help says who told the shell about the command: {:?}",
        run.stdout()
    );
}

#[test]
fn should_name_the_contributing_package_when_a_contributed_stage_is_explained() {
    let home = declaring_plugin_home();
    let run = ono(&home, "explain \"get echo-item\"");
    run.assert_success();
    assert!(
        run.stdout().contains("plugin(dev.example.echo, 0.1.0)"),
        "spec §31.64: `explain` exposes origin: {:?}",
        run.stdout()
    );
}

#[test]
fn should_load_the_package_when_a_declared_contribution_is_first_invoked() {
    let home = declaring_plugin_home();
    let run = ono(&home, "get echo-item --count 3 | to json");
    run.assert_success();
    assert!(
        run.stdout().contains("[1,2,3]"),
        "spec §31.68: invoking the command triggers the load that answers it: {:?}",
        run.stdout()
    );
}

#[test]
fn should_refuse_a_contribution_that_would_shadow_a_core_command() {
    let scratch = ono_testkit::scratch();
    scratch.write(
        "dev.example.thief/manifest.yaml",
        r#"
format: kuang-package/1
package:
  id: dev.example.thief
  name: thief
  version: 0.1.0
  description: Tries to answer for a name Ono already answers to.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: native-process
  entry: runtime/thief
  memory_max: 8MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
network:
  outbound: none
contributions:
  commands: [contributions/commands.yaml]
"#,
    );
    scratch.write(
        "dev.example.thief/contributions/commands.yaml",
        r#"
commands:
  - id: dev.example.thief.command.process
    verb: get
    target: process
    summary: Answer for processes instead of Ono.
    output: stream<int>
    argument_mode: expression
    capabilities: []
    examples: []
"#,
    );

    let run = ono(
        &scratch,
        r#"get command --verb get --target process | select id origin | to json"#,
    );
    run.assert_success();
    assert!(
        run.stdout().contains(r#""id":"ono.process.get""#)
            && !run.stdout().contains("dev.example.thief"),
        "spec §31.65: a contribution never replaces a name the shell already answers to: {:?}",
        run.stdout()
    );

    let reported = ono(&scratch, "get plugin");
    assert!(
        reported.stderr().contains("shadow"),
        "the refusal is reported rather than silently dropped: {:?}",
        reported.stderr()
    );
}

// --- runtime isolation (spec §31.10, §31.15, §31.34, ADR-0283) --------------------------------

/// The shell with a plugin home *and* a state root, so an instance gets the private directory
/// spec §31.31 names.
fn ono_with_state(
    home: &ono_testkit::Scratch,
    state: &ono_testkit::Scratch,
    script: &str,
) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .env("ONO_PLUGIN_PATH", home.path().display().to_string())
        .env("XDG_STATE_HOME", state.path().display().to_string())
        .env("ONO_TEST_SECRET", "hunter2")
        .run()
}

#[test]
fn should_end_the_instance_and_not_the_shell_when_a_package_exceeds_its_memory_ceiling() {
    let home = plugin_home();
    let state = ono_testkit::scratch();
    // The manifest declares 64 MiB; the package asks for 512 MiB and touches every page of it.
    let run = ono_with_state(
        &home,
        &state,
        "load plugin dev.example.echo; \
         try { echo:hog --mib 512 | count } catch e { $e.code | to json }; \
         get plugin | select state | to json",
    );

    run.assert_success();
    assert!(
        run.stdout().contains("Ono-Sendai-K11203"),
        "spec §31.34: reaching the declared ceiling is a resource-limit failure with its own \
         code, not an anonymous crash: {:?}",
        run.stdout()
    );
    assert!(
        run.stdout().contains(r#"{"state":"enabled"}"#),
        "spec §31.34: the failure degrades the plugin — the instance is gone and the package is \
         back to enabled — and the shell went on to answer the next stage: {:?}",
        run.stdout()
    );
}

#[test]
fn should_start_a_package_with_an_environment_it_did_not_inherit() {
    let home = plugin_home();
    let state = ono_testkit::scratch();
    let run = ono_with_state(
        &home,
        &state,
        "load plugin dev.example.echo; echo:environment | to json",
    );

    run.assert_success();
    let seen = run.stdout();
    assert!(
        !seen.contains("ONO_TEST_SECRET"),
        "spec §31.80: the shell's environment is not a side channel into a package: {seen:?}"
    );
    assert!(
        !seen.contains("ONO_PLUGIN_PATH"),
        "not even the shell's own configuration crosses the boundary: {seen:?}"
    );
    assert!(
        seen.contains("PATH") && seen.contains("HOME") && seen.contains("LC_ALL"),
        "the instance receives the environment the host built for it: {seen:?}"
    );
}

#[test]
fn should_run_a_package_in_a_private_directory_rather_than_the_users() {
    let home = plugin_home();
    let state = ono_testkit::scratch();
    let run = ono_with_state(
        &home,
        &state,
        "load plugin dev.example.echo; \
         inspect plugin dev.example.echo | select runtime | to json",
    );

    run.assert_success();
    assert!(
        run.stdout().contains("kuang/dev.example.echo/work"),
        "spec §31.10, §31.31: the instance starts in its own directory under the state root, \
         not in whatever directory the user happened to be in: {:?}",
        run.stdout()
    );
    assert!(
        run.stdout().contains(r#""filesystem":"broker""#),
        "spec §31.16: a scope the host can only check when the package asks it is reported as \
         the broker's, never as a kernel boundary: {:?}",
        run.stdout()
    );
}

#[test]
fn should_report_what_a_running_instance_has_allocated_and_used() {
    let home = plugin_home();
    let state = ono_testkit::scratch();
    let run = ono_with_state(
        &home,
        &state,
        "load plugin dev.example.echo; echo:emit --count 3 | count; \
         inspect plugin dev.example.echo | select memory_current memory_limit | to json",
    );

    run.assert_success();
    assert!(
        run.stdout().contains(r#""memory_limit":67108864"#),
        "spec §31.33: `inspect plugin` shows the ceiling the instance is actually under: {:?}",
        run.stdout()
    );
    assert!(
        !run.stdout().contains(r#""memory_current":null"#),
        "spec §31.33: an instance that has run has a measured memory figure, from the kernel's \
         own accounting: {:?}",
        run.stdout()
    );
}
