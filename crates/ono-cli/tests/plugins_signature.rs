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
    ono_with_plugins(
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

    let before = ono_with_plugins(&home, &format!("verify plugin {PACKAGE} | to json"));
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

    let after = ono_with_plugins(&home, &format!("verify plugin {PACKAGE} | to json"));
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
        let run = ono_with_plugins(&home, &format!("verify plugin {PACKAGE} | to json"));
        assert_eq!(
            str_field(only(&last_json(&run)), "integrity"),
            "valid",
            "the same artifact hashes the same way twice"
        );
    }
}

// --- signing, and what a bad answer does (spec §31.36, ADR-0311, ADR-0312) --------------------

use ono_kuang_protocol::{Manifest, PublicKey, SIGNATURE_FILE, SecretKey, SignedPackage};

mod support;
use support::ono_with_plugins;

/// A key whose bytes are fixed, so a test never depends on the machine's entropy.
fn key(seed: u8) -> SecretKey {
    SecretKey::from_bytes(&[seed; 32])
}

/// Signs the package in `directory` with `key`, as a package author would.
fn sign(directory: &Path, key: &SecretKey) {
    let text = std::fs::read_to_string(directory.join("manifest.yaml")).expect("the manifest");
    let manifest = Manifest::parse(&text).expect("the fixture manifest is valid");
    let described = SignedPackage::new(
        &manifest.package.id,
        &manifest.package.version,
        &manifest.package.publisher,
        ono_kuang_protocol::artifact_files(directory),
    )
    .expect("the fixture package is describable");
    std::fs::write(
        directory.join(SIGNATURE_FILE),
        key.sign(&described).to_yaml(),
    )
    .expect("the signature is written");
}

/// Writes a trust store enrolling `key` for `publisher` with `standing`.
fn enrol(home: &ono_testkit::Scratch, publisher: &str, key: &PublicKey, standing: &str) {
    let directory = home.path().join("config/ono/kuang");
    std::fs::create_dir_all(&directory).expect("the configuration directory");
    std::fs::write(
        directory.join("trust.yaml"),
        format!(
            "format: kuang-trust/1\nkeys:\n  - publisher: {publisher}\n    key: {key}\n    \
             trust: {standing}\n"
        ),
    )
    .expect("the trust store");
}

/// Verifies the installed package and answers its `ono.verification-result/1` record.
fn verified(home: &ono_testkit::Scratch) -> (ono_testkit::Run, Value) {
    let run = ono_with_plugins(home, &format!("verify plugin {PACKAGE} | to json"));
    let record = last_json(&run);
    let record = only(&record).clone();
    (run, record)
}

#[test]
fn should_answer_signature_valid_and_trust_unknown_when_no_store_names_the_key() {
    let home = scratch();
    let source = lay_out(&home.path().join("source"), PACKAGE);
    sign(&source, &key(1));
    install(&home, &source).assert_success();

    let (run, record) = verified(&home);
    assert_eq!(
        str_field(&record, "signature"),
        "valid",
        "spec §31.36: a signature that verifies is `valid`, got {record:?}"
    );
    assert_eq!(
        str_field(&record, "publisher"),
        "dev.example",
        "the publisher the signature attests to"
    );
    assert_eq!(
        str_field(&record, "key"),
        key(1).public_key().to_string(),
        "spec §31.36 prints the key that signed"
    );
    assert_eq!(
        str_field(&record, "trust"),
        "unknown",
        "spec §31.36: a valid signature from a key this system does not know is `unknown`, \
         never trusted"
    );
    assert!(
        field(&record, "blocking_failures")
            .as_sequence()
            .is_some_and(Vec::is_empty),
        "a signature nobody vouches for is not a failure, got {record:?}"
    );
    run.assert_success();
}

#[test]
fn should_answer_trust_user_trusted_when_the_operator_enrolled_the_key() {
    let home = scratch();
    let source = lay_out(&home.path().join("source"), PACKAGE);
    sign(&source, &key(1));
    install(&home, &source).assert_success();
    enrol(&home, "dev.example", &key(1).public_key(), "trusted");

    let (run, record) = verified(&home);
    run.assert_success();
    assert_eq!(
        str_field(&record, "trust"),
        "user-trusted",
        "ADR-0312: a key in the operator's store is `user-trusted`, got {record:?}"
    );
    let table = ono_with_plugins(
        &home,
        &format!("get plugin {PACKAGE} | select trust | to json"),
    );
    assert_eq!(
        str_field(only(&last_json(&table)), "trust"),
        "verified",
        "plugin.v1: `verified` is a valid signature from a trusted publisher"
    );
}

#[test]
fn should_not_let_one_enrolled_key_vouch_for_another_publisher() {
    let home = scratch();
    let source = lay_out(&home.path().join("source"), PACKAGE);
    sign(&source, &key(1));
    install(&home, &source).assert_success();
    enrol(&home, "dev.elsewhere", &key(1).public_key(), "trusted");

    let (_, record) = verified(&home);
    assert_eq!(
        str_field(&record, "trust"),
        "unknown",
        "ADR-0312: an entry matches on the key and the publisher together, got {record:?}"
    );
}

#[test]
fn should_call_an_unsigned_package_local_and_let_it_install_and_load() {
    let home = scratch();
    let source = lay_out(&home.path().join("source"), PACKAGE);
    install(&home, &source).assert_success();

    let (run, record) = verified(&home);
    run.assert_success();
    assert_eq!(
        str_field(&record, "signature"),
        "absent",
        "spec §31.36: an unsigned local development package says so"
    );
    assert!(
        field(&record, "publisher").is_null() && field(&record, "key").is_null(),
        "there is no publisher to attest to and no key to name, got {record:?}"
    );
    assert!(
        field(&record, "blocking_failures")
            .as_sequence()
            .is_some_and(Vec::is_empty),
        "spec §31.36: `absent` is not a failure, got {record:?}"
    );
    let table = ono_with_plugins(
        &home,
        &format!("get plugin {PACKAGE} | select trust | to json"),
    );
    assert_eq!(
        str_field(only(&last_json(&table)), "trust"),
        "local",
        "plugin.v1: `local` is an unsigned development package"
    );
}

#[test]
fn should_refuse_to_install_a_package_whose_signature_does_not_verify() {
    let home = scratch();
    let source = lay_out(&home.path().join("source"), PACKAGE);
    sign(&source, &key(1));
    // Signed, then changed: the artifact is no longer the one the key attested to.
    std::fs::write(
        source.join("adapters.yaml"),
        "format: ono-adapter-pack/1\nadapters: [ ]\n",
    )
    .expect("the adapter pack is rewritten");

    let run = install(&home, &source);
    assert!(
        run.stderr().contains("Ono-Sendai-K11004"),
        "spec §31.79: a signature that does not verify is `package.signature_invalid`, got {:?}",
        run.output()
    );
    assert!(
        !home.path().join("plugins").join(PACKAGE).exists(),
        "a blocking check prevents the install rather than warning about it"
    );
}

#[test]
fn should_refuse_to_load_a_package_whose_signature_broke_after_it_was_installed() {
    let home = scratch();
    let source = lay_out(&home.path().join("source"), PACKAGE);
    sign(&source, &key(1));
    install(&home, &source).assert_success();
    std::fs::write(
        home.path()
            .join("plugins")
            .join(PACKAGE)
            .join("fixtures/rows.yaml"),
        "rows:\n  - tampered\n",
    )
    .expect("the fixture is rewritten in the plugin home");

    let (_, record) = verified(&home);
    assert_eq!(
        str_field(&record, "signature"),
        "invalid",
        "the signature no longer covers what is on disk, got {record:?}"
    );
    let run = ono_with_plugins(&home, &format!("load plugin {PACKAGE}"));
    assert!(
        run.stderr().contains("Ono-Sendai-K11004"),
        "lifecycle.v1: verification is re-run at load, not only at install, got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_package_signed_by_a_revoked_key() {
    let home = scratch();
    let source = lay_out(&home.path().join("source"), PACKAGE);
    sign(&source, &key(1));
    install(&home, &source).assert_success();
    enrol(&home, "dev.example", &key(1).public_key(), "revoked");

    let (run, record) = verified(&home);
    assert_eq!(
        str_field(&record, "signature"),
        "valid",
        "the signature still verifies; it is the key that is no longer accepted"
    );
    assert_eq!(
        str_field(&record, "trust"),
        "untrusted",
        "ADR-0312: a revoked key is a positive statement, got {record:?}"
    );
    assert!(
        run.stderr().contains("Ono-Sendai-K11005"),
        "spec §31.79: a key this system does not trust is `publisher.untrusted`, got {:?}",
        run.output()
    );
    let load = ono_with_plugins(&home, &format!("load plugin {PACKAGE}"));
    assert!(
        load.stderr().contains("Ono-Sendai-K11005"),
        "a revoked key prevents loading too, got {:?}",
        load.output()
    );
}

#[test]
fn should_show_the_signature_state_and_its_publisher_in_the_install_plan() {
    let home = scratch();
    let signed_source = lay_out(&home.path().join("source"), PACKAGE);
    sign(&signed_source, &key(1));
    // Without `--confirm` a script gets the plan back rather than an installation.
    let refused = ono_with_plugins(
        &home,
        &format!(
            "try {{ install plugin path:{} }} catch e {{ $e | to json }}",
            signed_source.display()
        ),
    );
    let text = serde_yaml_ng::to_string(&last_json(&refused)).expect("the error renders");
    assert!(
        text.contains("valid / dev.example"),
        "spec §31.9's plan prints `signature      valid / dev.ono-labs`, got {text}"
    );

    let unsigned_source = lay_out(&home.path().join("other"), "dev.example.unsigned");
    let plain = ono_with_plugins(
        &home,
        &format!(
            "try {{ install plugin path:{} }} catch e {{ $e | to json }}",
            unsigned_source.display()
        ),
    );
    let text = serde_yaml_ng::to_string(&last_json(&plain)).expect("the error renders");
    assert!(
        text.contains("unsigned"),
        "lifecycle.v1 install_plan: `signature` is the state, or `unsigned`, got {text}"
    );
}

#[test]
fn should_report_a_trust_store_it_cannot_read_rather_than_treating_its_keys_as_absent() {
    let home = scratch();
    let source = lay_out(&home.path().join("source"), PACKAGE);
    sign(&source, &key(1));
    install(&home, &source).assert_success();
    let directory = home.path().join("config/ono/kuang");
    std::fs::create_dir_all(&directory).expect("the configuration directory");
    std::fs::write(
        directory.join("trust.yaml"),
        "format: kuang-trust/1\nkeys: [oops]\n",
    )
    .expect("the trust store");

    let run = ono_with_plugins(&home, &format!("verify plugin {PACKAGE} | to json"));
    assert!(
        run.stderr().contains("trust.yaml"),
        "a store that does not parse is named, not silently skipped, got {:?}",
        run.output()
    );
}
