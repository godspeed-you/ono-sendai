//! What `get plugin` answers about one package version: `ono.plugin/2` (spec §31.8, §31.33;
//! v0.4.1 §17.2; ADR-0448, ADR-0996, ADR-0997).
//!
//! Issue #199: one field answers one question — the record names the tier an instance runs in
//! once, as `execution_tier`, and no longer carries the manifest's `isolation` beside it. Issue
//! #222: an instance's memory and the CPU time it used are measured figures where the host can
//! measure them, and null — never zero — where it cannot.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0913).
#![cfg(feature = "kuang")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use ono_testkit::{Scratch, Shell};

const MANIFEST: &str = r#"
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

/// A plugin home holding the example package as a native process.
fn plugin_home() -> Scratch {
    let scratch = ono_testkit::scratch();
    scratch.write("plugins/dev.example.echo/manifest.yaml", MANIFEST);
    let entry = scratch.path().join("plugins/dev.example.echo/runtime/echo");
    std::fs::create_dir_all(entry.parent().expect("a parent")).expect("the runtime directory");
    ono_testkit::executable_copy(sibling("kuang-example-plugin"), &entry);
    scratch
}

/// A binary the workspace builds beside `ono`.
fn sibling(name: &str) -> std::path::PathBuf {
    ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join(name)
}

fn ono(home: &Scratch, script: &str) -> ono_testkit::Run {
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
        .env("XDG_CACHE_HOME", root.join("cache").display().to_string())
        .timeout(Duration::from_secs(60))
        .run()
}

/// The one record `get plugin` answers for the example package, after `script` ran before it.
fn plugin_record(home: &Scratch, script: &str) -> serde_yaml_ng::Value {
    let run = ono(
        home,
        &format!("{script}get plugin dev.example.echo | to json"),
    );
    run.assert_success();
    let rows = support::last_json_rows(&run);
    assert_eq!(rows.len(), 1, "one installed version: {:?}", run.output());
    rows[0].clone()
}

/// The fields `docs/contracts/schemas/plugin.v2.yaml` declares, in the repository the binary was
/// built from.
fn declared_fields() -> BTreeSet<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/contracts/schemas/plugin.v2.yaml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the contract at {}: {error}", path.display()));
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).expect("YAML");
    assert_eq!(
        document["id"].as_str(),
        Some("ono.plugin/2"),
        "the contract is the second version"
    );
    document["fields"]
        .as_mapping()
        .expect("a field mapping")
        .keys()
        .filter_map(|key| key.as_str().map(str::to_owned))
        .collect()
}

#[test]
fn should_name_the_tier_an_instance_runs_in_with_one_field_when_plugins_are_listed() {
    // Issue #199's exit test: one field answers one question. `execution_tier` says what an
    // instance runs in (v0.4.1 §17.2); the manifest's declared kind is read from the manifest
    // (`inspect plugin`), not repeated as a second tier-shaped field beside it.
    let home = plugin_home();
    let record = plugin_record(&home, "");

    assert_eq!(
        record["execution_tier"].as_str(),
        Some("native-confined"),
        "the tier is a name (ADR-0448): {record:?}"
    );
    assert!(
        record.get("isolation").is_none(),
        "ono.plugin/2 carries no second answer to the tier question: {record:?}"
    );
    let shown: BTreeSet<String> = record
        .as_mapping()
        .expect("a record")
        .keys()
        .filter_map(|key| key.as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        shown,
        declared_fields(),
        "the record is exactly the shape ono.plugin/2 declares"
    );
}

#[test]
fn should_say_get_plugin_answers_the_second_version_of_the_plugin_record() {
    let home = plugin_home();
    let run = ono(&home, "help get plugin");

    run.assert_success();
    assert!(
        run.stdout().contains("stream<ono.plugin/2>"),
        "spec §10.4: removing a field is a breaking change, and the help page names the version \
         the command answers with: {:?}",
        run.stdout()
    );
}

#[test]
fn should_report_the_memory_and_cpu_time_of_an_instance_that_has_run() {
    // Issue #222: a native-process instance is measured from the kernel's own accounting
    // (spec §31.33), so the record shows what it has allocated and the CPU time it has used.
    let home = plugin_home();
    let record = plugin_record(
        &home,
        "load plugin dev.example.echo; echo:emit --count 3 | count; ",
    );

    let memory = record["memory"].as_u64().unwrap_or_else(|| {
        panic!("spec §31.33: a running instance has a measured memory figure: {record:?}")
    });
    assert!(
        memory > 0,
        "a running process has allocated something: {record:?}"
    );
    assert!(
        record["cpu_time"].as_str().is_some(),
        "spec §31.33: a running instance's CPU time is a typed duration: {record:?}"
    );
}

#[test]
fn should_report_no_memory_or_cpu_time_for_a_package_nothing_has_loaded() {
    // Unknown is null, never zero (spec §35.3): an installed package has no instance to measure.
    let home = plugin_home();
    let record = plugin_record(&home, "");

    let null = |field: &str| record.get(field).is_some_and(serde_yaml_ng::Value::is_null);
    assert!(
        null("memory") && null("cpu_time"),
        "spec §10.5: no instance, no figure — null rather than zero: {record:?}"
    );
}

#[test]
fn should_report_a_components_memory_and_no_cpu_time_when_it_is_inspected() {
    // The wasm tier: the runtime accounts every growth of a component's memory, so the figure is
    // exact; it keeps no account of the CPU time one instance used, so that figure is null, as
    // `ono.plugin/2` and `ono.plugin-inspection/1` document — never a zero standing in for it.
    let component = match support::component_fixture() {
        Ok(path) => path,
        Err(why) => {
            ono_testkit::skipped(ono_testkit::SkipReason::ExternalToolUnavailable, &why);
            return;
        }
    };
    let home = ono_testkit::scratch();
    home.write(
        "plugins/dev.example.echo/manifest.yaml",
        MANIFEST
            .replace("kind: native-process", "kind: wasm-component")
            .replace("entry: runtime/echo", "entry: runtime/echo.wasm"),
    );
    let placed = home
        .path()
        .join("plugins/dev.example.echo/runtime/echo.wasm");
    std::fs::create_dir_all(placed.parent().expect("a parent")).expect("the runtime directory");
    std::fs::copy(&component, &placed).expect("the component");
    let compiled = std::process::Command::new(sibling("kuang-compile"))
        .arg(&placed)
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .output()
        .expect("kuang-compile runs");
    assert!(
        compiled.status.success(),
        "kuang-compile compiles the component: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let run = ono(
        &home,
        "load plugin dev.example.echo; echo:emit --count 2 | count; \
         inspect plugin dev.example.echo | select memory_current cpu_time | to json; \
         get plugin dev.example.echo | select execution_tier memory cpu_time | to json",
    );

    run.assert_success();
    let shown = run.stdout();
    let rows: Vec<serde_yaml_ng::Value> = shown
        .lines()
        .filter(|line| line.starts_with("[{"))
        .map(|line| serde_yaml_ng::from_str(line).expect("a JSON document"))
        .collect();
    assert_eq!(rows.len(), 2, "both answers arrived: {:?}", run.output());
    let inspected = &rows[0][0];
    let listed = &rows[1][0];
    assert_eq!(
        listed["execution_tier"].as_str(),
        Some("wasm"),
        "{listed:?}"
    );
    for record in [inspected, listed] {
        let memory = record
            .get("memory_current")
            .or_else(|| record.get("memory"))
            .and_then(serde_yaml_ng::Value::as_u64);
        assert!(
            memory.is_some_and(|bytes| bytes > 0),
            "the runtime's gauge measures a component's memory: {record:?}"
        );
        assert!(
            record
                .get("cpu_time")
                .is_some_and(serde_yaml_ng::Value::is_null),
            "a component's CPU time is not measured, so it is null rather than zero: {record:?}"
        );
    }
}
