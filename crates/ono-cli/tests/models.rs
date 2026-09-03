//! The model broker through the binary (spec §31.43, §31.44, §31.52; ADR-0566): `get model`
//! answers from the operator's catalogue, and a package granted `model.infer` reaches the
//! configured provider through the broker — a shell script speaking `ono-model/1`.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::{Scratch, Shell, scratch};

mod support;
use support::{executable, rows, text};

/// A catalogue under `<scratch>/config/ono/kuang/models.yaml` with an echoing model, a remote
/// twin of it under the `external-ok` policy, and one that is not installed.
fn catalogue(home: &Scratch) {
    let model = home.path().join("bin/echo-model");
    std::fs::create_dir_all(model.parent().expect("a parent")).expect("bin");
    executable(
        &model,
        "#!/bin/sh\ndoc=$(cat)\ntext=$(printf '%s' \"$doc\" | grep -o '\"content\":\"[^\"]*\"' | head -1 | cut -d'\"' -f4)\nprintf '{\"protocol\":\"ono-model/1\",\"parts\":[{\"kind\":\"text\",\"text\":\"echo: %s\"}]}' \"$text\"\n",
    );
    home.write(
        "config/ono/kuang/models.yaml",
        format!(
            "providers:\n  - id: local-echo\n    name: Local echo\n    kind: local\n    location: workstation\n    command: [{model}]\n    context_window: 4096\n    tools: true\n    data_policy: local-only\n  - id: remote-echo\n    name: Remote echo\n    kind: remote\n    location: configured\n    endpoint: https://user:secret@models.example/v1\n    command: [{model}]\n    data_policy: external-ok\n  - id: absent\n    name: Not installed\n    kind: local\n    location: workstation\n    command: [/nonexistent/model]\n    data_policy: local-only\n",
            model = model.display()
        ),
    );
}

/// The example package, installed under `<scratch>/plugins`, requesting `model.infer`.
fn plugin_home(home: &Scratch) {
    home.write(
        "plugins/dev.example.echo/manifest.yaml",
        r#"format: kuang-package/1
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
    - model.infer: {providers: ["*"]}
network:
  outbound: none
"#,
    );
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    let entry = home.path().join("plugins/dev.example.echo/runtime/echo");
    std::fs::create_dir_all(entry.parent().expect("a parent")).expect("the runtime directory");
    std::fs::copy(&binary, &entry).expect("the example plugin binary is built");
}

fn ono(home: &Scratch, script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .env("HOME", home.path().display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            home.path().join("config").display().to_string(),
        )
        .env(
            "XDG_STATE_HOME",
            home.path().join("state").display().to_string(),
        )
        .env(
            "ONO_PLUGIN_PATH",
            home.path().join("plugins").display().to_string(),
        )
        .run()
}

#[test]
fn should_list_the_configured_model_providers_with_their_policy_and_availability() {
    let home = scratch();
    catalogue(&home);

    let run = ono(&home, "get model | to json");
    run.assert_success();
    let listed = rows(&run);
    assert_eq!(
        listed.len(),
        3,
        "got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    let local = &listed[0];
    assert_eq!(text(local, "id"), "local-echo");
    assert_eq!(text(local, "kind"), "local");
    assert_eq!(text(local, "data_policy"), "local-only");
    assert_eq!(local["available"].as_bool(), Some(true));
    assert_eq!(local["context_window"].as_i64(), Some(4096));
    let remote = &listed[1];
    assert_eq!(text(remote, "kind"), "remote");
    assert_eq!(
        text(remote, "endpoint"),
        "[redacted]",
        "an endpoint carrying credentials is never rendered (spec §17.5)"
    );
    assert!(
        remote["denied_classes"]
            .as_sequence()
            .is_some_and(|classes| classes.iter().any(|class| class.as_str() == Some("secret"))),
        "external-ok denies secrets by default; got {remote:?}"
    );
    let absent = &listed[2];
    assert_eq!(absent["available"].as_bool(), Some(false));
    assert!(
        text(absent, "unavailable_reason").contains("/nonexistent/model"),
        "the record says why (spec §35.3); got {absent:?}"
    );
}

#[test]
fn should_answer_an_empty_catalogue_when_no_file_is_configured() {
    let home = scratch();
    let run = ono(&home, "get model | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[]",
        "no file means nothing configured"
    );
}

#[test]
fn should_report_a_catalogue_that_names_a_class_the_spec_does_not_have() {
    let home = scratch();
    home.write(
        "config/ono/kuang/models.yaml",
        "providers:\n  - id: x\n    name: X\n    kind: local\n    location: here\n    data_policy: local-only\n    deny: [top-secret]\n",
    );
    let run = ono(&home, "get model | to json");
    assert!(
        run.stderr().contains("top-secret"),
        "an operator's typo is reported, not turned into a silent allow; stderr {:?}",
        run.stderr()
    );
}

#[test]
fn should_let_a_granted_package_infer_through_the_configured_provider() {
    let home = scratch();
    catalogue(&home);
    plugin_home(&home);

    let run = ono(
        &home,
        r#"grant capability model.infer --plugin dev.example.echo --scope "providers=local-echo" | select capability | to json; load plugin dev.example.echo; echo:infer --prompt hello | to json"#,
    );
    assert!(
        run.stdout().contains("\"echo: hello\""),
        "the model's answer comes back as data; stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_refuse_a_provider_outside_the_grants_scope_and_record_it_in_the_trail() {
    let home = scratch();
    catalogue(&home);
    plugin_home(&home);

    // The refusal is a `capability.scope_violation` inside the package's invocation; the trail
    // is where the shell shows it (spec §31.37). That the invocation's own failure does not
    // reach the shell's exit status is a separate defect, recorded in `docs/STATE.md`.
    let run = ono(
        &home,
        r#"grant capability model.infer --plugin dev.example.echo --scope "providers=local-echo" | count; load plugin dev.example.echo; echo:infer --prompt hello --provider remote-echo | to json; get audit --plugin dev.example.echo | where action == "models.infer" | select action result | to json"#,
    );
    assert!(
        !run.stdout().contains("echo: hello"),
        "nothing was sent to a provider outside the scope; stdout {:?}",
        run.stdout()
    );
    assert!(
        run.stdout()
            .contains(r#"{"action":"models.infer","result":"denied"}"#),
        "the denied request is in the trail; stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}
