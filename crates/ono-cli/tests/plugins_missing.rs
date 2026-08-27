//! RED outcome tests for the KUANG/11 surface the contract declares and the shell does not yet
//! deliver: `ono.plugin/1` records that compose (spec §31.8), `find`/`inspect`/`install`/`remove`/
//! `unload`/`set`/`verify plugin` (spec §31.8, §31.9, §31.36, §31.81, `lifecycle.v1.yaml`), the
//! capability commands (spec §31.16–§31.19), the audit trail (spec §31.37), hot reload
//! (spec §31.72), and the assistant, model, finding and audit streams (spec §31.41–§31.52) —
//! `docs/spec/commands/kuang.yaml` throughout.
//!
//! Everything runs the real binary against a scratch plugin home holding the SDK's example
//! package `dev.example.echo` (the fixture of `plugins.rs`), offline and unprivileged. What
//! `plugins.rs` already proves — discovery in the table render, `load plugin`, contributed
//! commands, adapter packs — is not repeated here.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;
use std::time::Duration;

use ono_testkit::Shell;
use serde_yaml_ng::Value;

const ECHO: &str = "dev.example.echo";

/// The example package's manifest, as `plugins.rs` writes it, with the identity and the host
/// API range parameterised so a second package and a tampered one can be laid out the same way.
fn manifest(id: &str, name: &str, version: &str, kuang_api: &str) -> String {
    format!(
        r#"
format: kuang-package/1
package:
  id: {id}
  name: {name}
  version: {version}
  description: Emits what it is asked to emit.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: "{kuang_api}"
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
    )
}

/// Lays out one package directory — manifest plus the built example plugin binary — under
/// `root/<id>`, the installed layout of ADR-0051.
fn lay_out_package(root: &Path, id: &str, name: &str, version: &str, kuang_api: &str) {
    let package = root.join(id);
    std::fs::create_dir_all(package.join("runtime")).expect("the runtime directory");
    std::fs::write(
        package.join("manifest.yaml"),
        manifest(id, name, version, kuang_api),
    )
    .expect("the manifest");
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    std::fs::copy(&binary, package.join("runtime/echo"))
        .expect("the example plugin binary is built");
}

/// A scratch root: `plugins/` is the plugin home (`ONO_PLUGIN_PATH`) holding the example
/// package as installed; `state/`, `config/` and `home/` keep every persisted byte inside the
/// scratch so two runs sharing the root share exactly the state a user's machine would.
fn plugin_home() -> ono_testkit::Scratch {
    let scratch = ono_testkit::scratch();
    lay_out_package(
        &scratch.path().join("plugins"),
        ECHO,
        "echo",
        "0.1.0",
        ">=11.1 <12",
    );
    scratch
}

fn ono(home: &ono_testkit::Scratch, script: &str) -> ono_testkit::Run {
    let root = home.path();
    Shell::new()
        .args(["-c", script])
        .env(
            "ONO_PLUGIN_PATH",
            root.join("plugins").display().to_string(),
        )
        .env("HOME", root.join("home").display().to_string())
        .env("XDG_STATE_HOME", root.join("state").display().to_string())
        .env("XDG_CONFIG_HOME", root.join("config").display().to_string())
        .env(
            "ONO_CONFIG_DIR",
            root.join("config/ono").display().to_string(),
        )
        .timeout(Duration::from_secs(30))
        .run()
}

/// Parses one line of `to json` output. JSON is YAML, so the workspace's YAML parser reads it.
fn json(text: &str) -> Value {
    serde_yaml_ng::from_str(text).unwrap_or_else(|error| {
        panic!("`to json` emits a JSON document (spec §33.5): {error}\n{text}")
    })
}

/// The last `to json` line on stdout as a JSON document — a script's final `| to json`.
fn last_json(run: &ono_testkit::Run) -> Value {
    let line = run
        .stdout()
        .lines()
        .rfind(|line| line.starts_with('['))
        .unwrap_or_else(|| panic!("a `to json` document on stdout, got {:?}", run.output()));
    json(line)
}

/// The last `to json` line on stdout verbatim — for exact comparisons such as `[0]` and `[]`.
fn last_line(run: &ono_testkit::Run) -> &str {
    run.stdout()
        .lines()
        .rfind(|line| line.starts_with('['))
        .unwrap_or_else(|| panic!("a `to json` document on stdout, got {:?}", run.output()))
}

/// A value rendered as text, for substring checks on nested structure.
fn text(value: &Value) -> String {
    serde_yaml_ng::to_string(value).expect("a value renders")
}

fn items(value: &Value) -> &[Value] {
    value
        .as_sequence()
        .unwrap_or_else(|| {
            panic!("`to json` emits an array of the stream's values (spec §33.5), got {value:?}")
        })
        .as_slice()
}

fn field<'a>(record: &'a Value, name: &str) -> &'a Value {
    record
        .get(name)
        .unwrap_or_else(|| panic!("the record declares a field `{name}`, got {record:?}"))
}

fn str_field<'a>(record: &'a Value, name: &str) -> &'a str {
    field(record, name)
        .as_str()
        .unwrap_or_else(|| panic!("`{name}` is a string, got {record:?}"))
}

/// The single record a one-value stream renders as.
fn only<'a>(run: &'a ono_testkit::Run, value: &'a Value) -> &'a Value {
    let records = items(value);
    assert_eq!(
        records.len(),
        1,
        "exactly one value was expected, got {:?}",
        run.output()
    );
    &records[0]
}

fn assert_refused_with(run: &ono_testkit::Run, code: &str, why: &str) {
    assert!(
        !run.status().is_success(),
        "{why}: the run must not succeed, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains(code),
        "{why}: expected {code}, got {:?}",
        run.stderr()
    );
}

// ---------------------------------------------------------------------------------------------
// `get plugin` as records — spec §31.8, `ono.plugin/1`
// ---------------------------------------------------------------------------------------------

#[test]
fn should_emit_plugin_records_when_get_plugin_is_piped() {
    let home = plugin_home();
    let run = ono(&home, "get plugin | to json");
    run.assert_success();
    let value = last_json(&run);
    let record = only(&run, &value);
    assert_eq!(
        str_field(record, "id"),
        ECHO,
        "plugin.v1: `id` is the publisher-namespaced package id (spec §31.5)"
    );
    assert_eq!(
        str_field(record, "version"),
        "0.1.0",
        "plugin.v1: `version`"
    );
    assert_eq!(
        str_field(record, "publisher"),
        "dev.example",
        "plugin.v1: `publisher` is the namespace the id begins with"
    );
    assert_eq!(
        str_field(record, "state"),
        "installed",
        "spec §31.8: an untouched package is `installed`, nothing of it has run"
    );
    let trust = str_field(record, "trust");
    assert!(
        ["local", "unknown", "untrusted"].contains(&trust),
        "spec §31.36: an unsigned local package is visibly untrusted, got {trust}"
    );
    assert_eq!(
        str_field(record, "isolation"),
        "trusted-native",
        "lifecycle.v1 isolation_tiers: `runtime.kind: native-process` is T1 trusted-native"
    );
    assert_eq!(
        str_field(record, "kuang_api"),
        ">=11.1 <12",
        "plugin.v1: `kuang_api` is the range the manifest declares"
    );
    assert!(
        field(record, "enabled").as_bool().is_some(),
        "plugin.v1: `enabled` is a required bool, got {record:?}"
    );
    assert_eq!(
        field(record, "jobs").as_i64(),
        Some(0),
        "plugin.v1: `jobs` counts running jobs; an unloaded package has none"
    );
    let source = str_field(record, "source");
    assert!(
        source.contains(&home.path().join("plugins").display().to_string()),
        "plugin.v1: `source` names where the artifact came from (spec §31.9); a directory in the plugin home is a path source, got {source}"
    );
    assert!(
        field(record, "loaded_at").is_null(),
        "plugin.v1: `loaded_at` is null while the package has never been loaded"
    );
}

#[test]
fn should_count_loaded_packages_before_and_after_load() {
    let home = plugin_home();
    let run = ono(
        &home,
        &format!(
            "get plugin | where state == \"loaded\" | count | to json; load plugin {ECHO} --grant clock.read; get plugin | where state == \"loaded\" | count | to json"
        ),
    );
    run.assert_success();
    let counts: Vec<String> = run
        .stdout()
        .lines()
        .filter(|line| line.starts_with('['))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        counts,
        vec!["[0]".to_owned(), "[1]".to_owned()],
        "spec §31.8: `get plugin | where state == loaded` composes (kuang.yaml example) and sees the session's runtime states, got {:?}",
        run.output()
    );
}

#[test]
fn should_report_degraded_when_an_optional_capability_was_denied_at_load() {
    let home = plugin_home();
    let run = ono(
        &home,
        &format!("load plugin {ECHO}; get plugin {ECHO} | select state degraded_reason | to json"),
    );
    run.assert_success();
    let value = last_json(&run);
    let record = only(&run, &value);
    assert_eq!(
        str_field(record, "state"),
        "degraded",
        "spec §31.8, §31.17: a denied optional capability degrades, it does not fail"
    );
    let reason = str_field(record, "degraded_reason");
    assert!(
        reason.contains("clock.read"),
        "plugin.v1: `degraded_reason` names the unavailable capability, got {reason:?}"
    );
}

#[test]
fn should_resolve_one_package_by_its_id_selector() {
    let home = plugin_home();
    let run = ono(&home, &format!("get plugin {ECHO} | to json"));
    run.assert_success();
    let value = last_json(&run);
    let record = only(&run, &value);
    assert_eq!(str_field(record, "id"), ECHO, "kuang.yaml selector `id`");

    let miss = ono(&home, "get plugin dev.example.nosuch | to json");
    assert!(
        !miss.stdout().contains(ECHO),
        "kuang.yaml: the `id` selector resolves one package, not the whole set, got {:?}",
        miss.output()
    );
}

// ---------------------------------------------------------------------------------------------
// `inspect plugin` — spec §31.33, `ono.plugin-inspection/1`
// ---------------------------------------------------------------------------------------------

#[test]
fn should_show_manifest_contributions_and_capability_requests_when_inspected() {
    let home = plugin_home();
    let run = ono(&home, &format!("inspect plugin {ECHO} | to json"));
    run.assert_success();
    let value = last_json(&run);
    let record = only(&run, &value);
    assert_eq!(
        str_field(record, "origin"),
        "plugin",
        "plugin-inspection.v1: a package under the plugin home has origin `plugin`"
    );
    let manifest = field(record, "manifest");
    assert!(
        manifest.get("package").is_some() || manifest.get("format").is_some(),
        "plugin-inspection.v1: `manifest` is the parsed manifest record (spec §31.33), got {manifest:?}"
    );
    let contributions = text(field(record, "contributions"));
    assert!(
        contributions.contains("emit") && contributions.contains("clock"),
        "spec §31.33: `inspect plugin` shows registered contributions — the echo commands, got {contributions}"
    );
    let requests = items(field(record, "capability_requests"));
    let clock = requests
        .iter()
        .find(|request| request.get("capability").and_then(Value::as_str) == Some("clock.read"))
        .unwrap_or_else(|| {
            panic!("spec §31.17: the manifest's optional `clock.read` is a capability request, got {requests:?}")
        });
    assert_eq!(
        clock.get("class").and_then(Value::as_str),
        Some("optional"),
        "spec §31.17: the request carries its class"
    );
    assert!(
        field(record, "runtime").is_null(),
        "plugin-inspection.v1: `runtime` is null while the package is not loaded"
    );
    assert!(
        field(record, "memory_current").is_null(),
        "plugin-inspection.v1: resource use of an unloaded package is null, never zero (spec §35.3)"
    );
    assert_eq!(
        field(record, "restart_count").as_i64(),
        Some(0),
        "plugin-inspection.v1: `restart_count`"
    );
    assert!(
        field(record, "last_error").is_null(),
        "plugin-inspection.v1: no error has been recorded"
    );
    let verification = field(record, "verification");
    assert_eq!(
        verification.get("manifest").and_then(Value::as_str),
        Some("valid"),
        "plugin-inspection.v1: `verification` embeds the verification result"
    );
}

// ---------------------------------------------------------------------------------------------
// `find plugin` — spec §31.9, `ono.plugin-package/1`
// ---------------------------------------------------------------------------------------------

#[test]
fn should_find_an_installed_package_without_loading_it() {
    let home = plugin_home();
    let run = ono(
        &home,
        "find plugin echo | to json; get plugin | where state == \"loaded\" | count | to json",
    );
    run.assert_success();
    let lines: Vec<&str> = run
        .stdout()
        .lines()
        .filter(|line| line.starts_with('['))
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "two `to json` documents, got {:?}",
        run.output()
    );
    let found = json(lines[0]);
    let package = only(&run, &found);
    assert_eq!(
        str_field(package, "id"),
        ECHO,
        "plugin-package.v1: the search hit is the package in the configured source"
    );
    assert_eq!(
        str_field(package, "version"),
        "0.1.0",
        "plugin-package.v1: `version`"
    );
    assert_eq!(
        field(package, "installed").as_bool(),
        Some(true),
        "plugin-package.v1: `installed` says whether this version is already on disk"
    );
    assert_eq!(
        str_field(package, "signature"),
        "absent",
        "lifecycle.v1 verification: an unsigned package says so everywhere it appears"
    );
    assert_eq!(
        lines[1], "[0]",
        "kuang.yaml: `find plugin` executes no package (spec §31.9)"
    );
}

#[test]
fn should_find_an_uninstalled_package_in_a_path_source() {
    let home = plugin_home();
    let elsewhere = ono_testkit::scratch();
    lay_out_package(
        elsewhere.path(),
        "dev.example.other",
        "other",
        "0.3.0",
        ">=11.1 <12",
    );
    let run = ono(
        &home,
        &format!(
            "find plugin other --source path:{} | to json",
            elsewhere.path().display()
        ),
    );
    run.assert_success();
    let value = last_json(&run);
    let package = only(&run, &value);
    assert_eq!(
        str_field(package, "id"),
        "dev.example.other",
        "spec §31.9: `path:` is a core source scheme (lifecycle.v1 sources)"
    );
    assert_eq!(
        field(package, "installed").as_bool(),
        Some(false),
        "plugin-package.v1: a package only in a source is not installed"
    );
    assert!(
        str_field(package, "source").starts_with("path:"),
        "plugin-package.v1: `source` is the resolved reference"
    );
}

// ---------------------------------------------------------------------------------------------
// `verify plugin` — spec §31.36, `ono.verification-result/1`
// ---------------------------------------------------------------------------------------------

#[test]
fn should_report_an_unsigned_local_package_as_compatible_when_verified() {
    let home = plugin_home();
    let run = ono(&home, &format!("verify plugin {ECHO} | to json"));
    run.assert_success();
    let value = last_json(&run);
    let result = only(&run, &value);
    assert_eq!(
        str_field(result, "package"),
        ECHO,
        "verification-result.v1: `package`"
    );
    assert_eq!(
        str_field(result, "manifest"),
        "valid",
        "spec §31.36: the manifest satisfies manifest.v1's rules"
    );
    assert_eq!(
        str_field(result, "compatibility"),
        "compatible",
        "spec §31.62: `>=11.1 <12` includes this host's API"
    );
    assert_eq!(
        str_field(result, "signature"),
        "absent",
        "lifecycle.v1 verification: absent is not a failure"
    );
    assert!(
        ["valid", "unknown"].contains(&str_field(result, "integrity")),
        "spec §31.36: integrity answers whether the bytes are the referenced ones; a local directory is valid or unknown, never invalid, got {result:?}"
    );
    assert_eq!(
        str_field(result, "runtime"),
        "trusted-native",
        "spec §31.36: the tier is reported, never judged"
    );
    assert_eq!(
        items(field(result, "blocking_failures")).len(),
        0,
        "verification-result.v1: nothing blocks a valid local package"
    );
}

#[test]
fn should_report_incompatibility_when_the_kuang_api_range_excludes_the_host() {
    let home = plugin_home();
    home.write(
        format!("plugins/{ECHO}/manifest.yaml"),
        manifest(ECHO, "echo", "0.1.0", ">=99"),
    );
    let run = ono(&home, &format!("verify plugin {ECHO} | to json"));
    assert!(
        !run.status().is_success(),
        "lifecycle.v1 verification: a blocking check that fails is a failed verification, got {:?}",
        run.output()
    );
    let value = last_json(&run);
    let result = only(&run, &value);
    assert_eq!(
        str_field(result, "compatibility"),
        "incompatible",
        "spec §31.7, §31.62: `kuang_api: >=99` excludes every host that exists"
    );
    let blocking = text(field(result, "blocking_failures"));
    assert!(
        blocking.contains("compatib"),
        "verification-result.v1: `blocking_failures` names the check, got {blocking}"
    );
}

#[test]
fn should_refuse_to_load_an_incompatible_package() {
    let home = plugin_home();
    home.write(
        format!("plugins/{ECHO}/manifest.yaml"),
        manifest(ECHO, "echo", "0.1.0", ">=99"),
    );
    let run = ono(&home, &format!("load plugin {ECHO}"));
    assert!(
        !run.status().is_success(),
        "lifecycle.v1: a blocking check prevents load, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-K11002"),
        "spec §31.79: package.incompatible is rendered `Ono-Sendai-K11002` (ADR-0022), folded into the global error model (ADR-0040 §3) rather than carried inside a provider.unsupported message, got {:?}",
        run.stderr()
    );
}

// ---------------------------------------------------------------------------------------------
// `install plugin` — spec §31.9, lifecycle.v1 `install`
// ---------------------------------------------------------------------------------------------

#[test]
fn should_install_a_package_from_a_path_reference_when_confirmed() {
    let home = plugin_home();
    let elsewhere = ono_testkit::scratch();
    lay_out_package(
        elsewhere.path(),
        "dev.example.other",
        "other",
        "0.3.0",
        ">=11.1 <12",
    );
    let reference = format!(
        "path:{}",
        elsewhere.path().join("dev.example.other").display()
    );
    let run = ono(
        &home,
        &format!("install plugin {reference} --confirm | to json"),
    );
    run.assert_success();
    let value = last_json(&run);
    let result = only(&run, &value);
    assert_eq!(
        str_field(result, "status"),
        "success",
        "kuang.yaml: `install plugin` answers an ono.action-result/1"
    );
    assert_eq!(
        field(result, "changed").as_bool(),
        Some(true),
        "action-result: something was installed"
    );

    // A second session sees the package: installation placed it in the plugin home
    // (ADR-0051: installed is a directory layout), not in the first session's memory.
    let listed = ono(
        &home,
        "get plugin | where id == \"dev.example.other\" | select id version state | to json",
    );
    listed.assert_success();
    let value = last_json(&listed);
    let record = only(&listed, &value);
    assert_eq!(
        str_field(record, "version"),
        "0.3.0",
        "the installed version"
    );
    assert_eq!(
        str_field(record, "state"),
        "installed",
        "spec §31.8: install runs no package code and grants nothing"
    );
}

#[test]
fn should_refuse_to_install_without_confirmation_in_a_script() {
    let home = plugin_home();
    let elsewhere = ono_testkit::scratch();
    lay_out_package(
        elsewhere.path(),
        "dev.example.other",
        "other",
        "0.3.0",
        ">=11.1 <12",
    );
    let reference = format!(
        "path:{}",
        elsewhere.path().join("dev.example.other").display()
    );
    let run = ono(&home, &format!("install plugin {reference}"));
    assert_refused_with(
        &run,
        "Ono-Sendai-E0701",
        "spec §17.4, §31.9: a script never waits for the install plan's prompt; without `--confirm` the answer is safety.confirmation_required",
    );
    assert!(
        !home.exists("plugins/dev.example.other/manifest.yaml"),
        "lifecycle.v1 install: the plan comes before any mutation; nothing was written"
    );
}

#[test]
fn should_refuse_to_install_a_version_that_is_already_installed() {
    let home = plugin_home();
    let elsewhere = ono_testkit::scratch();
    lay_out_package(elsewhere.path(), ECHO, "echo", "0.1.0", ">=11.1 <12");
    let reference = format!("path:{}", elsewhere.path().join(ECHO).display());
    let before = home.read(format!("plugins/{ECHO}/manifest.yaml"));
    let run = ono(&home, &format!("install plugin {reference} --confirm"));
    assert_refused_with(
        &run,
        "Ono-Sendai-E0303",
        "plugin.v1 identity is [id, version]; installing the same version again would overwrite it — io.already_exists, not a silent replace",
    );
    assert_eq!(
        home.read(format!("plugins/{ECHO}/manifest.yaml")),
        before,
        "the installed artifact is untouched"
    );
}

// ---------------------------------------------------------------------------------------------
// `unload plugin` — lifecycle.v1 `unload`
// ---------------------------------------------------------------------------------------------

#[test]
fn should_withdraw_contributions_when_a_package_is_unloaded() {
    let home = plugin_home();
    let run = ono(
        &home,
        &format!(
            "load plugin {ECHO}; unload plugin {ECHO} | to json; get plugin | where state == \"loaded\" | count | to json; echo:emit --count 3 | to json"
        ),
    );
    let lines: Vec<&str> = run
        .stdout()
        .lines()
        .filter(|line| line.starts_with('['))
        .collect();
    assert!(
        lines.len() >= 2,
        "the unload result and the count, got {:?}",
        run.output()
    );
    let unload = json(lines[0]);
    let result = only(&run, &unload);
    assert_eq!(
        str_field(result, "status"),
        "success",
        "kuang.yaml: `unload plugin` streams ono.action-result/1"
    );
    assert_eq!(
        lines[1], "[0]",
        "lifecycle.v1 unload: the package returns to `enabled`, no runtime instance remains"
    );
    assert!(
        !run.stdout().contains("[1,2,3]"),
        "lifecycle.v1 unload: contributions are withdrawn, the command no longer runs, got {:?}",
        run.stdout()
    );
    assert!(
        !run.status().is_success() && run.stderr().contains("Ono-Sendai-E"),
        "ADR-0051: an unloaded package's namespace is a structured refusal, got {:?}",
        run.output()
    );
}

// ---------------------------------------------------------------------------------------------
// `set plugin` — spec §31.3, lifecycle.v1 `enable`/`disable`, on-disk state (spec §31.31)
// ---------------------------------------------------------------------------------------------

#[test]
fn should_disable_a_package_and_refuse_to_load_it() {
    let home = plugin_home();
    let run = ono(
        &home,
        &format!(
            "set plugin {ECHO} --enabled false | to json; get plugin {ECHO} | select enabled | to json"
        ),
    );
    run.assert_success();
    let lines: Vec<&str> = run
        .stdout()
        .lines()
        .filter(|line| line.starts_with('['))
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "two `to json` documents, got {:?}",
        run.output()
    );
    let set = json(lines[0]);
    assert_eq!(
        str_field(only(&run, &set), "status"),
        "success",
        "kuang.yaml: `set plugin` streams ono.action-result/1"
    );
    let plugin = json(lines[1]);
    assert_eq!(
        field(only(&run, &plugin), "enabled").as_bool(),
        Some(false),
        "plugin.v1: `enabled` reflects the setting (spec §31.3)"
    );

    let load = ono(
        &home,
        &format!(
            "set plugin {ECHO} --enabled false; load plugin {ECHO}; get plugin | where state == \"loaded\" | count | to json"
        ),
    );
    assert!(
        load.stderr().contains("Ono-Sendai-"),
        "lifecycle.v1: `load` is a transition from `enabled`; a disabled package refuses with a structured error, got {:?}",
        load.output()
    );
    assert_eq!(
        last_line(&load),
        "[0]",
        "a disabled package did not load, got {:?}",
        load.output()
    );
}

#[test]
fn should_persist_enablement_across_sessions() {
    let home = plugin_home();
    ono(&home, &format!("set plugin {ECHO} --enabled false")).assert_success();
    let later = ono(
        &home,
        &format!("get plugin {ECHO} | select enabled | to json"),
    );
    later.assert_success();
    let value = last_json(&later);
    assert_eq!(
        field(only(&later, &value), "enabled").as_bool(),
        Some(false),
        "spec §31.31, §31.8: enablement is management state on disk, not a session's memory"
    );
}

// ---------------------------------------------------------------------------------------------
// `remove plugin` — spec §31.81, lifecycle.v1 `remove`
// ---------------------------------------------------------------------------------------------

#[test]
fn should_remove_the_package_directory_when_removed() {
    let home = plugin_home();
    let run = ono(&home, &format!("remove plugin {ECHO} | to json"));
    run.assert_success();
    let value = last_json(&run);
    let result = only(&run, &value);
    assert_eq!(
        str_field(result, "status"),
        "success",
        "kuang.yaml: `remove plugin` streams ono.action-result/1"
    );
    assert_eq!(
        field(result, "changed").as_bool(),
        Some(true),
        "something was removed"
    );
    assert!(
        !home.exists(format!("plugins/{ECHO}/manifest.yaml")),
        "lifecycle.v1 remove: package versions are removed from the plugin home"
    );
    let listed = ono(&home, "get plugin | count | to json");
    listed.assert_success();
    assert_eq!(
        last_line(&listed),
        "[0]",
        "spec §31.81: the package is absent afterwards, got {:?}",
        listed.output()
    );
}

#[test]
fn should_unload_a_loaded_package_before_removing_it() {
    let home = plugin_home();
    let run = ono(
        &home,
        &format!(
            "load plugin {ECHO}; remove plugin {ECHO} | to json; echo:emit --count 3 | to json"
        ),
    );
    assert!(
        run.stdout().contains("\"status\":\"success\""),
        "lifecycle.v1 remove: a loaded instance is unloaded first and the removal succeeds, got {:?}",
        run.output()
    );
    assert!(
        !run.stdout().contains("[1,2,3]"),
        "a removed package contributes nothing, got {:?}",
        run.stdout()
    );
    assert!(
        !home.exists(format!("plugins/{ECHO}/manifest.yaml")),
        "the package directory is gone"
    );
}

// ---------------------------------------------------------------------------------------------
// Capabilities — spec §31.16–§31.19, `ono.capability-grant/1`
// ---------------------------------------------------------------------------------------------

#[test]
fn should_list_capability_definitions() {
    let home = plugin_home();
    let run = ono(&home, "get capability | to json");
    run.assert_success();
    let text = text(&last_json(&run));
    assert!(
        text.contains("clock.read") && text.contains("process.exec"),
        "kuang.yaml: `get capability` shows capability definitions (docs/spec/capabilities.yaml → kuang_capabilities), got {text}"
    );
}

#[test]
fn should_show_a_grant_made_at_load_for_the_package() {
    let home = plugin_home();
    let run = ono(
        &home,
        &format!(
            "load plugin {ECHO} --grant clock.read; get capability --plugin {ECHO} | where capability == \"clock.read\" | to json"
        ),
    );
    run.assert_success();
    let value = last_json(&run);
    let grant = only(&run, &value);
    assert_eq!(
        str_field(grant, "decision"),
        "allow",
        "capability-grant.v1: `--grant` on the command line is an allow decision (ADR-0065)"
    );
    assert_eq!(
        str_field(grant, "class"),
        "optional",
        "spec §31.17: the manifest declares clock.read optional"
    );
    assert!(
        text(field(grant, "plugin")).contains(ECHO),
        "capability-grant.v1: `plugin` references the package, got {grant:?}"
    );
    assert_eq!(
        str_field(grant, "enforcement"),
        "broker",
        "spec §31.19: clock.read is broker-enforced"
    );
    assert!(
        ["session", "command", "once", "always"].contains(&str_field(grant, "duration")),
        "capability-grant.v1: `duration` is one of its enum values, got {grant:?}"
    );
}

#[test]
fn should_grant_and_revoke_a_capability_at_runtime() {
    let home = plugin_home();
    let granted = ono(
        &home,
        &format!(
            "load plugin {ECHO}; grant capability clock.read --plugin {ECHO} | to json; echo:clock | to json"
        ),
    );
    granted.assert_success();
    let lines: Vec<&str> = granted
        .stdout()
        .lines()
        .filter(|line| line.starts_with('['))
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "the grant and the clock, got {:?}",
        granted.output()
    );
    let grant = json(lines[0]);
    let record = only(&granted, &grant);
    assert_eq!(
        str_field(record, "capability"),
        "clock.read",
        "capability-grant.v1"
    );
    assert_eq!(
        str_field(record, "decision"),
        "allow",
        "kuang.yaml: `grant capability` creates a scoped grant"
    );
    assert!(
        lines[1].contains("20"),
        "spec §31.16: the granted clock is readable — a timestamp was emitted, got {}",
        lines[1]
    );

    let revoked = ono(
        &home,
        &format!(
            "load plugin {ECHO}; grant capability clock.read --plugin {ECHO}; revoke capability clock.read --plugin {ECHO} | to json; echo:clock | to json"
        ),
    );
    assert!(
        revoked.stdout().contains("\"status\":\"success\""),
        "kuang.yaml: `revoke capability` streams ono.action-result/1, got {:?}",
        revoked.output()
    );
    assert!(
        !revoked.status().is_success()
            && (revoked.stderr().contains("capability.denied")
                || revoked.stderr().contains("K11301")),
        "spec §31.19: after revocation the broker denies the call (capability.denied), got {:?}",
        revoked.output()
    );
}

#[test]
fn should_refuse_to_grant_an_unknown_capability() {
    let home = plugin_home();
    let run = ono(
        &home,
        &format!("grant capability nosuch.thing --plugin {ECHO}"),
    );
    assert_refused_with(
        &run,
        "Ono-Sendai-E0102",
        "spec §31.16: a capability outside capabilities.yaml's kuang_capabilities is not a target the broker knows",
    );
}

// ---------------------------------------------------------------------------------------------
// Audit — spec §31.37, `ono.plugin-audit-event/1`
// ---------------------------------------------------------------------------------------------

#[test]
fn should_record_a_capability_use_in_the_audit_trail() {
    let home = plugin_home();
    let run = ono(
        &home,
        &format!(
            "load plugin {ECHO} --grant clock.read; echo:clock; get audit --plugin {ECHO} | where capability == \"clock.read\" | to json"
        ),
    );
    run.assert_success();
    let value = last_json(&run);
    let events = items(&value);
    let used = events
        .iter()
        .find(|event| event.get("result").and_then(Value::as_str) == Some("success"))
        .unwrap_or_else(|| {
            panic!("spec §31.37: a capability-sensitive host call is audited with its result, got {events:?}")
        });
    assert!(
        text(field(used, "plugin")).contains(ECHO),
        "plugin-audit-event.v1: `plugin` names the package"
    );
    assert!(
        !str_field(used, "action").is_empty(),
        "plugin-audit-event.v1: `action` is the command or host call"
    );
    assert!(
        !str_field(used, "at").is_empty(),
        "plugin-audit-event.v1: `at` is a timestamp"
    );
    assert_eq!(
        str_field(used, "enforcement"),
        "broker",
        "plugin-audit-event.v1: the broker enforced clock.read"
    );
}

#[test]
fn should_record_a_denied_capability_use_in_the_audit_trail() {
    let home = plugin_home();
    let run = ono(
        &home,
        &format!(
            "load plugin {ECHO}; echo:clock; get audit --plugin {ECHO} | where result == \"denied\" | to json"
        ),
    );
    let value = last_json(&run);
    let events = items(&value);
    assert!(
        events
            .iter()
            .any(|event| event.get("capability").and_then(Value::as_str) == Some("clock.read")),
        "spec §31.37: a denial is audited as `denied` with the capability that was refused, got {events:?}"
    );
}

#[test]
fn should_filter_the_audit_trail_by_package() {
    let home = plugin_home();
    let run = ono(
        &home,
        &format!(
            "load plugin {ECHO} --grant clock.read; echo:clock; get audit --plugin dev.example.nosuch | to json"
        ),
    );
    run.assert_success();
    assert_eq!(
        last_line(&run),
        "[]",
        "kuang.yaml: `--plugin` restricts to one package, got {:?}",
        run.output()
    );
}

#[test]
fn should_reject_an_unknown_field_on_the_audit_stream() {
    let home = plugin_home();
    let run = ono(&home, "get audit | where plugn == \"x\"");
    assert_refused_with(
        &run,
        "Ono-Sendai-E0202",
        "plugin-audit-event.v1 is the wired output schema, so a misspelt field is type.unknown_field",
    );
    assert!(
        run.stderr().contains("plugin"),
        "spec §43: the error suggests the field that exists, got {:?}",
        run.stderr()
    );
}

// ---------------------------------------------------------------------------------------------
// Hot reload — spec §31.72, lifecycle.v1 `hot_reload`; ADR-0065 §6 "re-loading replaces"
// ---------------------------------------------------------------------------------------------

#[test]
fn should_show_the_new_version_when_a_loaded_package_is_reloaded() {
    let home = plugin_home();
    let manifest_path = format!("plugins/{ECHO}/manifest.yaml");
    let run = ono(
        &home,
        &format!(
            "load plugin {ECHO}; get plugin {ECHO} | select version | to json; cp {stale} {live}; load plugin {ECHO}; get plugin {ECHO} | select version state | to json",
            stale = home
                .write(
                    "staged-manifest.yaml",
                    manifest(ECHO, "echo", "0.2.0", ">=11.1 <12")
                )
                .display(),
            live = home.path().join(&manifest_path).display(),
        ),
    );
    run.assert_success();
    let lines: Vec<&str> = run
        .stdout()
        .lines()
        .filter(|line| line.starts_with('['))
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "two `to json` documents, got {:?}",
        run.output()
    );
    assert!(
        lines[0].contains("0.1.0"),
        "the version before the reload, got {}",
        lines[0]
    );
    let after = json(lines[1]);
    let record = only(&run, &after);
    assert_eq!(
        str_field(record, "version"),
        "0.2.0",
        "spec §31.72: a stateless package with no jobs reloads immediately and its records show the reloaded manifest"
    );
    assert!(
        ["loaded", "degraded"].contains(&str_field(record, "state")),
        "lifecycle.v1 hot_reload: the reloaded instance is running, got {record:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Assistants, models, findings — spec §31.41–§31.52
// ---------------------------------------------------------------------------------------------

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_no_assistants_when_none_is_loaded() {
    let home = plugin_home();
    let run = ono(&home, "get assistant | to json");
    run.assert_success();
    assert_eq!(
        last_line(&run),
        "[]",
        "spec §31.42: an assistant is an object of a loaded assistant package; with none loaded the stream is empty, not unimplemented, got {:?}",
        run.output()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_no_model_providers_when_none_is_configured() {
    let home = plugin_home();
    let run = ono(&home, "get model | to json");
    run.assert_success();
    assert_eq!(
        last_line(&run),
        "[]",
        "spec §31.43: the operator configures providers; none configured is an empty stream, got {:?}",
        run.output()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_a_structured_not_found_when_asking_an_unknown_assistant() {
    let home = plugin_home();
    let run = ono(&home, "ask assistant nobody \"hello\"");
    assert_refused_with(
        &run,
        "Ono-Sendai-E0102",
        "spec §31.42, §7.1: the assistant is selected explicitly and an unknown one is resolve.target_not_found",
    );
    assert!(
        run.stderr().contains("nobody"),
        "the error names the assistant that was asked for, got {:?}",
        run.stderr()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_no_findings_when_nothing_was_analysed() {
    let home = plugin_home();
    let run = ono(
        &home,
        "get finding | to json; get finding | where severity == \"high\" | to json",
    );
    run.assert_success();
    let lines: Vec<&str> = run
        .stdout()
        .lines()
        .filter(|line| line.starts_with('['))
        .collect();
    assert_eq!(
        lines,
        vec!["[]", "[]"],
        "spec §31.24: findings are emitted by analyses; none ran, and `severity` is a finding.v1 field so the filter composes, got {:?}",
        run.output()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_reject_an_unknown_field_on_the_finding_stream() {
    let home = plugin_home();
    let run = ono(&home, "get finding | where sevrity == \"x\"");
    assert_refused_with(
        &run,
        "Ono-Sendai-E0202",
        "finding.v1 is the wired output schema, so a misspelt field is type.unknown_field",
    );
    assert!(
        run.stderr().contains("severity"),
        "spec §43: the error suggests the field that exists, got {:?}",
        run.stderr()
    );
}
