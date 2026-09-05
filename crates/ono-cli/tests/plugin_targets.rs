//! A KUANG/11 package with `roles: [provider]` answers `get <target>` through the same registry
//! the built-in providers answer through.
//!
//! Spec §31.23 (target and schema contribution), §31.64 (contributed ids enter the real
//! registries with origin `plugin(...)`), §31.80 (the host stamps provenance). The supervisor
//! already speaks `provider.query` and the SDK already lets a package answer it; what these
//! tests hold is the shell's integration step, which is what turns a contributed target from a
//! protocol capability into something a user can type.
//!
//! The example package `dev.example.echo` contributes the target `echo-item` and answers a
//! provider query for it with three records.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;

mod support;
use support::{echo_package_manifest, json, ono_with_plugins};

const ECHO: &str = "dev.example.echo";

/// The package's declaration of the target it answers for, readable before any of its code runs
/// (spec §31.64, §31.68).
///
/// A target, not a `get`-shaped command. The difference is the whole point: a command is invoked
/// and answers whatever it likes, while a target is a noun the provider machinery answers for —
/// with the schema it declared, provenance the host stamps, and a route into the spatial model.
const TARGETS: &str = r#"
targets:
  - name: echo-item
    schema: dev.example.echo.item/1
    summary: Items the example package provides.
    identity_doc: Two observations are the same item when their `seq` matches.
"#;

fn lay_out_package(root: &Path, id: &str) {
    let package = root.join(id);
    std::fs::create_dir_all(package.join("runtime")).expect("the runtime directory");
    std::fs::write(package.join("manifest.yaml"), echo_package_manifest(id)).expect("the manifest");
    std::fs::create_dir_all(package.join("contributions")).expect("the contributions directory");
    std::fs::write(package.join("contributions/targets.yaml"), TARGETS).expect("the document");
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    std::fs::copy(&binary, package.join("runtime/echo"))
        .expect("the example plugin binary is built");
}

fn plugin_home() -> ono_testkit::Scratch {
    let scratch = ono_testkit::scratch();
    lay_out_package(&scratch.path().join("plugins"), ECHO);
    scratch
}

/// The last `to json` document on stdout.
fn last_json(run: &ono_testkit::Run) -> serde_yaml_ng::Value {
    let line = run
        .stdout()
        .lines()
        .rfind(|line| line.starts_with('['))
        .unwrap_or_else(|| panic!("a `to json` document on stdout, got {:?}", run.output()));
    json(line)
}

#[test]
fn should_answer_get_for_a_contributed_target() {
    // The whole point of a provider package: the target it contributes is a noun the user types,
    // not a command namespace they have to learn (spec §31.23, §35.1 of the Kubernetes spec).
    let home = plugin_home();
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item | to json"),
    );
    run.assert_success();
    let items = last_json(&run);
    let items = items.as_sequence().expect("a sequence of records");
    assert_eq!(items.len(), 3, "the example provider answers three items");
}

#[test]
fn should_stamp_a_contributed_records_provenance_with_the_package() {
    // §31.80: the host stamps provenance; a package cannot claim another source. A record that
    // arrived from a plugin must say so wherever it is inspected.
    let home = plugin_home();
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item | take 1 | inspect | to json"),
    );
    run.assert_success();
    assert!(
        run.stdout().contains(&format!("plugin:{ECHO}")),
        "inspect must attribute the record to the package that produced it, got {:?}",
        run.output()
    );
}

#[test]
fn should_not_answer_a_target_the_package_does_not_contribute() {
    // A package answers for what it declared and nothing else. An undeclared target is a
    // resolution failure, never an empty success.
    let home = plugin_home();
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-nonexistent | to json"),
    );
    assert_ne!(
        run.status().code(),
        0,
        "an undeclared target must fail rather than answer emptily, got {:?}",
        run.output()
    );
}

#[test]
fn should_answer_a_contributed_target_without_an_explicit_load() {
    // §31.68: `installed manifest -> registry placeholders -> first invocation -> runtime load`.
    // The declaration is what makes the noun typeable; loading is what the first use pays for.
    // A user should not have to know which package answers `get echo-item` in order to ask.
    let home = plugin_home();
    let run = ono_with_plugins(&home, "get echo-item | to json");
    run.assert_success();
    let items = last_json(&run);
    assert_eq!(
        items.as_sequence().expect("a sequence").len(),
        3,
        "the package loaded on first use and answered, got {:?}",
        run.output()
    );
}

#[test]
fn should_carry_the_schema_the_target_declared() {
    // A target names its schema in the declaration, and the records that arrive must be of it.
    // This is what separates a provider answer from a command that happens to be spelled `get`.
    let home = plugin_home();
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item | take 1 | inspect | to json"),
    );
    run.assert_success();
    assert!(
        run.stdout().contains("dev.example.echo.item/1"),
        "the record must carry the declared schema, got {:?}",
        run.output()
    );
}

#[test]
fn should_compose_a_contributed_target_with_the_pipeline() {
    // A contributed target is not a special case: it is a stream of typed records, so `where`
    // filters it and `select` projects it exactly as for a built-in provider.
    let home = plugin_home();
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item | where seq > 1 | select label | to json"),
    );
    run.assert_success();
    let items = last_json(&run);
    let items = items.as_sequence().expect("a sequence");
    assert_eq!(
        items.len(),
        2,
        "two of three items have seq > 1, got {:?}",
        run.output()
    );
}

#[test]
fn should_carry_a_targets_options_into_the_provider_query() {
    // A contributed target is invoked with words like any other stage, and those words are the
    // only way a user can say *which* of something they want — a context, a namespace, a kind.
    // The first version of this route passed an empty option map, so every target answered as if
    // it had been asked with no arguments at all: not visibly broken, just permanently unfiltered.
    let home = plugin_home();
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item --count 1 | to json"),
    );
    run.assert_success();
    let items = last_json(&run);
    assert_eq!(
        items.as_sequence().expect("a sequence").len(),
        1,
        "`--count 1` must reach the provider handler, got {:?}",
        run.output()
    );
}
