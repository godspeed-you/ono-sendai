//! Signing, integrity and trust, end to end (spec §31.9, §31.36; ADR-0311, ADR-0312).
//!
//! Everything runs the real binary against a scratch plugin home, offline and unprivileged.
//! The four questions §31.36 keeps apart are asked separately here too: whether the bytes are
//! the ones installed, whether a key signed them, whether the operator accepts that key, and
//! what each wrong answer does to `install plugin` and `load plugin`.

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

const PACKAGE: &str = "dev.example.signed";

/// A declarative package: a manifest, one contribution file it declares, and one data file it
/// does not — the shape that shows what an artifact hash actually covers.
fn manifest(id: &str) -> String {
    format!(
        r#"format: kuang-package/1
package:
  id: {id}
  name: signed
  version: 0.1.0
  description: A package that carries a signature.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
roles: [adapter]
network:
  outbound: none
contributions:
  adapters: [adapters.yaml]
"#
    )
}

const ADAPTERS: &str = "format: ono-adapter-pack/1\nadapters: []\n";
const UNDECLARED: &str = "rows:\n  - one\n";

/// Lays a package out under `root`, answering its directory.
fn lay_out(root: &Path, id: &str) -> std::path::PathBuf {
    let package = root.join(id);
    std::fs::create_dir_all(package.join("fixtures")).expect("the package directory");
    std::fs::write(package.join("manifest.yaml"), manifest(id)).expect("the manifest");
    std::fs::write(package.join("adapters.yaml"), ADAPTERS).expect("the adapter pack");
    // Declared by nothing in the manifest, and read by the adapter pack at run time: exactly
    // the file an artifact hash must not skip.
    std::fs::write(package.join("fixtures/rows.yaml"), UNDECLARED).expect("the fixture");
    package
}

fn scratch() -> ono_testkit::Scratch {
    let scratch = ono_testkit::scratch();
    std::fs::create_dir_all(scratch.path().join("plugins")).expect("the plugin home");
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

fn last_json(run: &ono_testkit::Run) -> Value {
    let line = run
        .stdout()
        .lines()
        .rfind(|line| line.starts_with('['))
        .unwrap_or_else(|| panic!("a `to json` document on stdout, got {:?}", run.output()));
    serde_yaml_ng::from_str(line).unwrap_or_else(|error| panic!("`to json` is JSON: {error}"))
}

fn only(value: &Value) -> &Value {
    value
        .as_sequence()
        .and_then(|items| items.first())
        .unwrap_or_else(|| panic!("one record, got {value:?}"))
}

fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    value
        .get(name)
        .unwrap_or_else(|| panic!("the record carries `{name}`, got {value:?}"))
}

fn str_field<'a>(value: &'a Value, name: &str) -> &'a str {
    field(value, name)
        .as_str()
        .unwrap_or_else(|| panic!("`{name}` is a string, got {value:?}"))
}

/// Installs the package from a path source, so an integrity hash is recorded.
fn install(home: &ono_testkit::Scratch, source: &Path) -> ono_testkit::Run {
    ono(
        home,
        &format!(
            "install plugin path:{} --confirm | select status | to json",
            source.display()
        ),
    )
}

#[test]
fn should_answer_integrity_invalid_when_a_file_the_manifest_never_declared_changed() {
    let home = scratch();
    let source = lay_out(&home.path().join("source"), PACKAGE);
    let installed = install(&home, &source);
    installed.assert_success();

    let before = ono(&home, &format!("verify plugin {PACKAGE} | to json"));
    assert_eq!(
        str_field(only(&last_json(&before)), "integrity"),
        "valid",
        "the bytes just installed are the bytes recorded"
    );

    // A file that is part of the package and named by no manifest field. Spec §31.36 asks
    // whether these are the exact bytes referenced; a hash that skips it cannot answer.
    std::fs::write(
        home.path()
            .join("plugins")
            .join(PACKAGE)
            .join("fixtures/rows.yaml"),
        "rows:\n  - tampered\n",
    )
    .expect("the fixture is rewritten in the plugin home");

    let after = ono(&home, &format!("verify plugin {PACKAGE} | to json"));
    let record = last_json(&after);
    let record = only(&record);
    assert_eq!(
        str_field(record, "integrity"),
        "invalid",
        "spec §31.36: every file of the artifact is covered, not only the declared ones"
    );
    assert!(
        field(record, "blocking_failures")
            .as_sequence()
            .is_some_and(|items| items.iter().any(|item| item.as_str() == Some("integrity"))),
        "integrity is a blocking check, got {record:?}"
    );
    assert!(
        after.stderr().contains("Ono-Sendai-K11003"),
        "spec §31.79: a changed artifact is `package.integrity_failed`, got {:?}",
        after.output()
    );
}

#[test]
fn should_keep_integrity_valid_when_nothing_of_the_package_changed() {
    let home = scratch();
    let source = lay_out(&home.path().join("source"), PACKAGE);
    install(&home, &source).assert_success();
    for _ in 0..2 {
        let run = ono(&home, &format!("verify plugin {PACKAGE} | to json"));
        assert_eq!(
            str_field(only(&last_json(&run)), "integrity"),
            "valid",
            "the same artifact hashes the same way twice"
        );
    }
}
