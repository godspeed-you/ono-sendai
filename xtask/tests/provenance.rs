//! The release input manifest of Appendix H (spec §43.2, §57 H10).
//!
//! One question, asked by a maintainer two years from now: what exactly did we trust to produce
//! these bytes? The manifest is the answer, and it is only an answer if it is written by the
//! build itself, from the same files the pin scanners read, rather than typed out afterwards.
//!
//! These tests drive the real `xtask build-manifest`, because the thing under test is what the
//! release workflow runs.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use ono_testkit::{Scratch, scratch};

fn this_repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
}

/// Runs `xtask build-manifest` into a scratch file and returns what it wrote.
fn emit(environment: &[(&str, &str)]) -> (Scratch, PathBuf, serde_json::Value) {
    let output = scratch();
    let path = output.path().join("build-inputs.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .arg("build-manifest")
        .arg("--output")
        .arg(&path)
        .current_dir(this_repository());
    // A manifest must not inherit the identity of whatever run happens to be around it.
    for name in [
        "GITHUB_REF",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "GITHUB_WORKFLOW",
        "GITHUB_REPOSITORY",
        "GITHUB_SHA",
        "RUNNER_OS",
        "RUNNER_ARCH",
        "SOURCE_DATE_EPOCH",
    ] {
        command.env_remove(name);
    }
    for (name, value) in environment {
        command.env(name, value);
    }
    let result = command
        .output()
        .unwrap_or_else(|error| panic!("xtask must be runnable in the gate: {error}"));
    assert!(
        result.status.success(),
        "`xtask build-manifest` failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("nothing was written to {}: {error}", path.display()));
    let value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("the manifest is not JSON: {error}\n{text}"));
    (output, path, value)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(this_repository())
        .output()
        .expect("git must be runnable in the gate");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn should_emit_a_build_input_manifest_carrying_every_field_appendix_h_requires() {
    let (_scratch, _path, manifest) = emit(&[]);

    // Appendix H, field by field. A key that is absent is a question nobody answered; a key
    // whose value is null is an answer of "unknown", which spec §35.3 allows and fabrication
    // does not.
    for pointer in [
        "/source/commit",
        "/source/tag",
        "/source/version",
        "/toolchain/channel",
        "/lockfile/sha256",
        "/containers/build",
        "/containers/package_test",
        "/actions",
        "/tools",
        "/source_date_epoch",
        "/run/workflow",
        "/run/id",
    ] {
        assert!(
            manifest.pointer(pointer).is_some(),
            "the manifest carries no `{pointer}`, so Appendix H is not answered:\n{manifest:#}"
        );
    }

    for (pointer, what) in [
        ("/source/commit", "the commit it was built from"),
        ("/toolchain/channel", "the toolchain that compiled it"),
        ("/lockfile/sha256", "the dependency graph it resolved"),
        ("/source_date_epoch", "the timestamp its artifacts carry"),
    ] {
        let value = manifest.pointer(pointer).expect("the field");
        assert!(
            value.as_str().is_some_and(|text| !text.is_empty()),
            "the manifest does not name {what} (`{pointer}` is {value})"
        );
    }

    // Spec §62.5 in miniature, and the exit test of #103: what the manifest says the release
    // pulls is what the pin scanners see, because both read the same files.
    let images: Vec<String> = manifest
        .pointer("/containers/build")
        .and_then(serde_json::Value::as_array)
        .expect("the build containers")
        .iter()
        .chain(
            manifest
                .pointer("/containers/package_test")
                .and_then(serde_json::Value::as_array)
                .expect("the package-test containers"),
        )
        .map(|entry| entry["reference"].as_str().expect("a reference").to_owned())
        .collect();
    assert!(
        !images.is_empty() && images.iter().all(|image| image.contains("@sha256:")),
        "a container the release uses is recorded without its digest: {images:?}"
    );
    let actions: Vec<String> = manifest["actions"]
        .as_array()
        .expect("the actions")
        .iter()
        .map(|entry| entry["uses"].as_str().expect("a reference").to_owned())
        .collect();
    assert!(
        !actions.is_empty()
            && actions.iter().all(|uses| {
                uses.rsplit_once('@').is_some_and(|(_, sha)| {
                    sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())
                })
            }),
        "an action the release runs is recorded without its commit: {actions:?}"
    );

    let tools = manifest["tools"].as_object().expect("the tool versions");
    assert!(
        tools.contains_key("cargo-deb") && tools.contains_key("cargo-generate-rpm"),
        "the manifest does not say which packaging tools built the artifacts: {tools:?}"
    );
}

#[test]
fn should_bind_the_build_input_manifest_to_the_release_it_describes() {
    let (_scratch, _path, manifest) = emit(&[
        ("GITHUB_REF", "refs/tags/v9.9.9"),
        ("GITHUB_RUN_ID", "424242"),
        ("GITHUB_RUN_ATTEMPT", "2"),
        ("GITHUB_WORKFLOW", "release"),
        ("GITHUB_REPOSITORY", "godspeed-you/ono-sendai"),
    ]);

    assert_eq!(
        manifest["source"]["commit"].as_str(),
        Some(git(&["rev-parse", "HEAD"]).as_str()),
        "the manifest describes a commit other than the one it was generated from"
    );
    assert_eq!(
        manifest["source"]["tag"].as_str(),
        Some("v9.9.9"),
        "the manifest does not carry the tag the release is being published under"
    );
    assert_eq!(manifest["run"]["id"].as_str(), Some("424242"));
    assert_eq!(manifest["run"]["attempt"].as_str(), Some("2"));
    assert_eq!(manifest["run"]["workflow"].as_str(), Some("release"));

    let checksum = Command::new("sha256sum")
        .arg("Cargo.lock")
        .current_dir(this_repository())
        .output()
        .expect("sha256sum must be available in the gate");
    let expected = String::from_utf8_lossy(&checksum.stdout)
        .split_whitespace()
        .next()
        .expect("a checksum")
        .to_owned();
    assert_eq!(
        manifest["lockfile"]["sha256"].as_str(),
        Some(expected.as_str()),
        "the manifest's lockfile hash is not the hash of the committed Cargo.lock"
    );

    // Without a run around it the manifest says so rather than inventing one (spec §35.3).
    let (_scratch, _path, local) = emit(&[]);
    assert!(
        local["run"]["id"].is_null() && local["source"]["tag"].is_null(),
        "a manifest generated outside a tagged release run claims to be one:\n{local:#}"
    );

    // And the release workflow is what emits it, rather than a step somebody runs by hand.
    let workflow = std::fs::read_to_string(this_repository().join(".github/workflows/release.yml"))
        .expect("the release workflow");
    assert!(
        workflow.contains("build-manifest"),
        "the release workflow does not emit the build input manifest, so the release cannot \
         state its own inputs (spec §43.2, Appendix H)"
    );
}
