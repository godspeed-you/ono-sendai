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

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use ono_testkit::{Scratch, scratch};

mod support;
use support::workflow_job;

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
        local["run"]["id"].is_null(),
        "a manifest generated outside a run claims to have one:\n{local:#}"
    );
    // The tag is the commit's, whatever it is. This read `tag.is_null()` until the first release
    // put a `v*` tag on the commit the gate runs from, and the assertion failed on a manifest
    // that was telling the truth — the test had encoded "no tag exists here" as though it were
    // "no tag is claimed" (AGENTS.md §11, ADR-0579).
    let described = Command::new("git")
        .args(["describe", "--tags", "--exact-match"])
        .current_dir(this_repository())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    assert_eq!(
        local["source"]["tag"].as_str(),
        described.as_deref(),
        "a manifest generated outside a run must report the tag of the commit it was generated \
         from — the one there is, or null — and never one from anywhere else:\n{local:#}"
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

// --- the checksum manifest (spec §47.1, §47.2, ADR-0528) ----------------------------------------

/// Runs `xtask checksums` against a directory and returns its verdict and its output.
fn checksums(arguments: &[&str]) -> (bool, String) {
    let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("checksums")
        .args(arguments)
        .current_dir(this_repository())
        .output()
        .unwrap_or_else(|error| panic!("xtask must be runnable in the gate: {error}"));
    let mut text = String::from_utf8_lossy(&result.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&result.stderr));
    (result.status.success(), text)
}

/// A directory shaped like the assets of one release, written in an awkward order on purpose.
fn release_directory(scratch: &Scratch, name: &str) -> PathBuf {
    let directory = scratch.path().join(name);
    std::fs::create_dir_all(&directory).expect("a scratch release directory");
    // Deliberately not alphabetical, and not the order a release produces them in either: the
    // manifest's ordering has to come from the manifest, not from the filesystem.
    for (file, contents) in [
        ("ono_0.4.1_arm64.deb", "the arm64 package"),
        ("build-inputs.json", "{\"schema\":\"ono.build-inputs.v1\"}"),
        ("ono-0.4.1-1.x86_64.rpm", "the x86_64 rpm"),
        ("ono_0.4.1_amd64.deb", "the amd64 package"),
        ("ono-0.4.1-1.aarch64.rpm", "the aarch64 rpm"),
    ] {
        std::fs::write(directory.join(file), contents).expect("an artifact");
    }
    directory
}

#[test]
fn should_list_every_downloadable_artifact_in_the_checksum_manifest() {
    let scratch = scratch();
    let directory = release_directory(&scratch, "dist");
    let (written, report) = checksums(&["--dir", directory.to_str().expect("a UTF-8 path")]);
    assert!(written, "`xtask checksums` failed:\n{report}");

    let manifest = std::fs::read_to_string(directory.join("SHA256SUMS"))
        .expect("SHA256SUMS is written beside the packages (spec §47.1)");
    for artifact in [
        "ono_0.4.1_amd64.deb",
        "ono_0.4.1_arm64.deb",
        "ono-0.4.1-1.x86_64.rpm",
        "ono-0.4.1-1.aarch64.rpm",
        "build-inputs.json",
    ] {
        assert!(
            manifest.contains(artifact),
            "`{artifact}` is downloadable from the release and absent from SHA256SUMS, so a \
             reader who downloads it has nothing to check it against (spec §47.2):\n{manifest}"
        );
    }
    assert!(
        !manifest.contains("SHA256SUMS"),
        "the manifest lists itself, which no reader can verify:\n{manifest}"
    );

    // §67.7 shows the command a user types. It is the same file and the same format, so the
    // proof is that command succeeding rather than a claim about the format.
    let checked = Command::new("sha256sum")
        .args(["--check", "--strict", "SHA256SUMS"])
        .current_dir(&directory)
        .output()
        .expect("sha256sum must be available in the gate");
    assert!(
        checked.status.success(),
        "`sha256sum -c SHA256SUMS` — the command §67.7 documents — does not verify the manifest \
         this release publishes:\n{}\n{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
}

#[test]
fn should_order_the_checksum_manifest_deterministically() {
    // §46.1 names checksum manifests as a reproducibility target, so the manifest is a
    // deterministic function of the artifacts and nothing else — not of the order they were
    // written in, and not of the locale the release ran under.
    let scratch = scratch();
    let first = release_directory(&scratch, "first");
    let second = scratch.path().join("second");
    std::fs::create_dir_all(&second).expect("a second release directory");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&first)
        .expect("the first directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    names.reverse();
    for path in names {
        std::fs::copy(&path, second.join(path.file_name().expect("a file name")))
            .expect("the artifact is copied");
    }

    for directory in [&first, &second] {
        let (written, report) = checksums(&["--dir", directory.to_str().expect("a UTF-8 path")]);
        assert!(written, "`xtask checksums` failed:\n{report}");
    }
    let left = std::fs::read_to_string(first.join("SHA256SUMS")).expect("the first manifest");
    let right = std::fs::read_to_string(second.join("SHA256SUMS")).expect("the second manifest");
    assert_eq!(
        left, right,
        "two runs over the same artifacts produced two manifests, so the manifest is not itself \
         reproducible (spec §46.1, §47.2)"
    );

    let listed: Vec<&str> = left
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .collect();
    let mut sorted = listed.clone();
    sorted.sort_unstable();
    assert_eq!(
        listed, sorted,
        "the manifest is not in byte order, so its ordering depends on something other than the \
         names it lists:\n{left}"
    );
}

#[test]
fn should_fail_the_release_check_when_an_artifact_is_absent_from_the_manifest() {
    let scratch = scratch();
    let directory = release_directory(&scratch, "dist");
    let path = directory.to_str().expect("a UTF-8 path");
    let (written, report) = checksums(&["--dir", path]);
    assert!(written, "`xtask checksums` failed:\n{report}");

    let (verified, report) = checksums(&["--dir", path, "--verify"]);
    assert!(
        verified,
        "the manifest this repository just wrote does not verify:\n{report}"
    );

    // An artifact that reached the release after the manifest was written. §48.2 asks that
    // package validation check the manifest against the uploaded files, and an asset nobody
    // hashed is exactly what that check exists to catch.
    std::fs::write(
        directory.join("ono-0.4.1-linux-x86_64.tar.gz"),
        "a late arrival",
    )
    .expect("a late artifact");
    let (verified, report) = checksums(&["--dir", path, "--verify"]);
    assert!(
        !verified,
        "an artifact absent from SHA256SUMS passed verification:\n{report}"
    );
    assert!(
        report.contains("ono-0.4.1-linux-x86_64.tar.gz"),
        "the refusal does not name the artifact that is missing from the manifest:\n{report}"
    );

    // And an artifact whose bytes no longer match the digest the manifest carries.
    std::fs::remove_file(directory.join("ono-0.4.1-linux-x86_64.tar.gz")).expect("the tarball");
    std::fs::write(directory.join("ono_0.4.1_amd64.deb"), "different bytes").expect("a tamper");
    let (verified, report) = checksums(&["--dir", path, "--verify"]);
    assert!(
        !verified && report.contains("ono_0.4.1_amd64.deb"),
        "an artifact whose bytes changed after the manifest was written passed \
         verification:\n{report}"
    );
}

// --- the signature over the manifest (spec §47.3, ADR-0529) -------------------------------------

/// A stand-in for `cosign` on `PATH`.
///
/// The gate has no OIDC token and no route to Fulcio or Rekor, so it cannot make or check a real
/// Sigstore signature — that is what the release workflow does, on a tag, and ADR-0529 says so.
/// What these tests own is the *verification path*: that `scripts/verify-release.sh` asks for a
/// keyless verification constrained to this repository's release workflow, and that it reports
/// what the tool reports rather than deciding for itself.
///
/// The stand-in therefore implements the one property the real tool implements — the bundle was
/// made over exactly these bytes — and records every argument it was called with. Faking the
/// outside world is allowed; faking our own layer is not (AGENTS.md §11).
fn cosign_stub(scratch: &Scratch) -> PathBuf {
    let bin = scratch.path().join("bin");
    std::fs::create_dir_all(&bin).expect("a scratch bin directory");
    let stub = bin.join("cosign");
    std::fs::write(
        &stub,
        "#!/usr/bin/env bash\n\
         printf '%s\\n' \"$*\" >> \"$ONO_COSIGN_LOG\"\n\
         [ \"$1\" = verify-blob ] || { echo \"unexpected cosign subcommand $1\" >&2; exit 64; }\n\
         shift\n\
         bundle=\"\"; blob=\"\"\n\
         while [ $# -gt 0 ]; do\n\
         \x20 case \"$1\" in\n\
         \x20   --bundle) bundle=\"$2\"; shift 2 ;;\n\
         \x20   --certificate-identity-regexp|--certificate-oidc-issuer) shift 2 ;;\n\
         \x20   --*) shift ;;\n\
         \x20   *) blob=\"$1\"; shift ;;\n\
         \x20 esac\n\
         done\n\
         want=\"$(sha256sum \"$blob\" | cut -d' ' -f1)\"\n\
         if grep -qx \"$want\" \"$bundle\"; then echo 'Verified OK'; exit 0; fi\n\
         echo 'Error: signature verification failed' >&2\n\
         exit 1\n",
    )
    .expect("the cosign stand-in is written");
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
        .expect("the stand-in is executable");
    bin
}

/// A release directory with its checksum manifest and a signature over it.
fn signed_release(scratch: &Scratch) -> PathBuf {
    let directory = release_directory(scratch, "release");
    let (written, report) = checksums(&["--dir", directory.to_str().expect("a UTF-8 path")]);
    assert!(written, "`xtask checksums` failed:\n{report}");
    let manifest = std::fs::read(directory.join("SHA256SUMS")).expect("the manifest");
    std::fs::write(
        directory.join("SHA256SUMS.sigstore.json"),
        format!("{}\n", xtask::reproducibility::digest(&manifest)),
    )
    .expect("the signature bundle");
    directory
}

/// Runs `scripts/verify-release.sh` with the cosign stand-in ahead of anything on `PATH`.
fn verify_release(scratch: &Scratch, arguments: &[&str]) -> (bool, String, String) {
    let bin = cosign_stub(scratch);
    let log = scratch.path().join("cosign.log");
    let _ = std::fs::write(&log, "");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new("bash")
        .arg(this_repository().join("scripts/verify-release.sh"))
        .args(arguments)
        .current_dir(this_repository())
        .env("PATH", path)
        .env("ONO_COSIGN_LOG", &log)
        .output()
        .unwrap_or_else(|error| panic!("bash must be runnable in the gate: {error}"));
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    (output.status.success(), text, calls)
}

#[test]
fn should_verify_the_published_signature_over_the_checksum_manifest() {
    let scratch = scratch();
    let directory = signed_release(&scratch);
    let (verified, report, calls) = verify_release(
        &scratch,
        &[
            "--dir",
            directory.to_str().expect("a UTF-8 path"),
            // The provenance is a separate promise and a separate test (#108).
            "--without-provenance",
        ],
    );
    assert!(
        verified,
        "a release carrying a manifest and a signature over it did not verify:\n{report}"
    );

    // §47.3: keyless, and bound to an identity rather than to whatever certificate happens to
    // be in the bundle. A verification with no `--certificate-identity*` constraint accepts a
    // signature from anybody Fulcio has ever issued a certificate to.
    assert!(
        calls.contains("verify-blob"),
        "the signature was never checked:\n{calls}"
    );
    assert!(
        calls.contains("--certificate-identity-regexp")
            && calls.contains("--certificate-oidc-issuer"),
        "the verification is not constrained to a signing identity, so it accepts a signature \
         from anyone (spec §47.3):\n{calls}"
    );
    assert!(
        calls.contains("godspeed-you/ono-sendai")
            && calls.contains(r"workflows/release\.yml@refs/tags/v")
            && calls.contains("token.actions.githubusercontent.com"),
        "the identity the verification demands is not this repository's release workflow \
         authenticated by GitHub's OIDC issuer:\n{calls}"
    );
    assert!(
        !calls.contains("--key "),
        "the verification uses a long-lived key rather than a keyless identity (spec \
         §47.3):\n{calls}"
    );

    // And the signing half is the trusted workflow's, needs no repository secret, and holds the
    // OIDC permission on the publishing job alone.
    let workflow = std::fs::read_to_string(this_repository().join(".github/workflows/release.yml"))
        .expect("the release workflow");
    assert!(
        workflow.contains("sign-release.sh") && workflow.contains("id-token: write"),
        "the release workflow does not sign the checksum manifest with an OIDC identity (spec \
         §47.1, §47.3):\n{workflow}"
    );
    assert!(
        !workflow.contains("secrets."),
        "the release workflow reaches for a repository secret. §47.3 asks the reference \
         implementation to sign without a long-lived private key stored as one:\n{workflow}"
    );
    let signing = std::fs::read_to_string(this_repository().join("scripts/sign-release.sh"))
        .expect("the signing script");
    assert!(
        !signing.contains("--key "),
        "signing hands cosign a key file, so the release depends on a secret somebody has to \
         hold, rotate and revoke (spec §47.3):\n{signing}"
    );
    assert!(
        signing.contains("verify-release.sh"),
        "the release signs without checking its own signature, so an unverifiable signature \
         would be published rather than failing the run:\n{signing}"
    );
}

#[test]
fn should_fail_verification_when_the_checksum_manifest_is_altered() {
    let scratch = scratch();
    let directory = signed_release(&scratch);
    let path = directory.to_str().expect("a UTF-8 path").to_owned();

    // The manifest itself, rewritten to bless different bytes. The digests still describe the
    // artifacts beside them, so only the signature can tell.
    let manifest = std::fs::read_to_string(directory.join("SHA256SUMS")).expect("the manifest");
    std::fs::write(
        directory.join("ono_0.4.1_amd64.deb"),
        "an amd64 package somebody else built",
    )
    .expect("the substituted artifact");
    let (written, report) = checksums(&["--dir", &path]);
    assert!(written, "`xtask checksums` failed:\n{report}");
    assert_ne!(
        manifest,
        std::fs::read_to_string(directory.join("SHA256SUMS")).expect("the manifest"),
        "the substitution did not change the manifest, so the test proves nothing"
    );

    let (verified, report, calls) =
        verify_release(&scratch, &["--dir", &path, "--without-provenance"]);
    assert!(
        !verified,
        "a checksum manifest that is not the one that was signed verified anyway:\n{report}\n\
         {calls}"
    );
    assert!(
        report.contains("SHA256SUMS"),
        "the refusal does not say the signature over the manifest is what failed:\n{report}"
    );

    // An artifact whose bytes no longer match a manifest that is still correctly signed.
    let directory = signed_release(&scratch);
    let path = directory.to_str().expect("a UTF-8 path").to_owned();
    std::fs::write(
        directory.join("ono-0.4.1-1.x86_64.rpm"),
        "not what was hashed",
    )
    .expect("the tampered artifact");
    let (verified, report, _) = verify_release(&scratch, &["--dir", &path, "--without-provenance"]);
    assert!(
        !verified && report.contains("ono-0.4.1-1.x86_64.rpm"),
        "an artifact that does not hash to what the signed manifest says verified \
         anyway:\n{report}"
    );

    // And a release with no signature at all is refused rather than reported as checksummed.
    let directory = signed_release(&scratch);
    let path = directory.to_str().expect("a UTF-8 path").to_owned();
    std::fs::remove_file(directory.join("SHA256SUMS.sigstore.json")).expect("the signature");
    let (verified, report, _) = verify_release(&scratch, &["--dir", &path, "--without-provenance"]);
    assert!(
        !verified && report.contains("SHA256SUMS.sigstore.json"),
        "a release with no signature over its checksum manifest was accepted, so verification \
         fails open (spec §2.3, §47.1):\n{report}"
    );
}

// --- provenance (spec §47.4, §62.5, ADR-0530) ---------------------------------------------------

/// The build input manifest a release hands to provenance (ADR-0451), as a fixture.
fn staged_build_inputs(directory: &Path) {
    std::fs::write(
        directory.join("build-inputs.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ono.build-inputs.v1",
            "source": { "commit": "0".repeat(40), "tag": "v9.9.9", "version": "9.9.9" },
            "toolchain": { "file": "rust-toolchain.toml", "channel": "1.94" },
            "lockfile": { "path": "Cargo.lock", "sha256": "a".repeat(64) },
            "source_date_epoch": "1700000000",
            "run": {
                "workflow": "release",
                "repository": "godspeed-you/ono-sendai",
                "id": "424242",
                "attempt": "1",
                "ref": "refs/tags/v9.9.9",
                "runner": { "os": "Linux", "arch": "X64" }
            }
        }))
        .expect("the fixture serialises")
            + "\n",
    )
    .expect("the build input manifest");
}

/// Runs `xtask provenance` against a release directory.
fn provenance(arguments: &[&str]) -> (bool, String) {
    let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("provenance")
        .args(arguments)
        .current_dir(this_repository())
        .env_remove("GITHUB_REPOSITORY")
        .env_remove("GITHUB_SHA")
        .env_remove("GITHUB_REF")
        .env_remove("GITHUB_RUN_ID")
        .env_remove("GITHUB_WORKFLOW")
        .output()
        .unwrap_or_else(|error| panic!("xtask must be runnable in the gate: {error}"));
    let mut text = String::from_utf8_lossy(&result.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&result.stderr));
    (result.status.success(), text)
}

/// A release directory with its checksum manifest, its input manifest and its provenance.
fn attested_release(scratch: &Scratch, name: &str) -> PathBuf {
    let directory = release_directory(scratch, name);
    staged_build_inputs(&directory);
    let path = directory.to_str().expect("a UTF-8 path");
    let (written, report) = checksums(&["--dir", path]);
    assert!(written, "`xtask checksums` failed:\n{report}");
    let (attested, report) = provenance(&["--dir", path]);
    assert!(attested, "`xtask provenance` failed:\n{report}");
    directory
}

#[test]
fn should_bind_all_seven_required_fields_to_every_artifact_digest() {
    let scratch = scratch();
    let directory = attested_release(&scratch, "release");
    let document: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(directory.join("build-provenance.json"))
            .expect("build-provenance.json is published beside the packages (spec §47.1)"),
    )
    .expect("the provenance is JSON");

    // §47.4's list, field by field. None of them may be null: an unbound field is a question the
    // provenance was written to answer.
    for (pointer, what) in [
        ("/predicate/repository", "the repository it came from"),
        ("/predicate/source_commit", "the commit it was built from"),
        ("/predicate/release_tag", "the tag it is published under"),
        ("/predicate/workflow/name", "the workflow that produced it"),
        ("/predicate/workflow/run_id", "the run that produced it"),
        ("/predicate/builder/id", "the builder"),
        ("/predicate/builder/toolchain", "the toolchain version"),
        ("/predicate/build_timestamp", "when it was built"),
    ] {
        let value = document.pointer(pointer);
        assert!(
            value.is_some_and(|value| value.as_str().is_some_and(|text| !text.is_empty())),
            "the provenance does not bind {what} (`{pointer}` is {value:?}), so it attests to \
             less than §47.4 requires:\n{document:#}"
        );
    }

    // §46.2 again: the timestamp is the release's own, not the moment the document was written.
    assert_eq!(
        document
            .pointer("/predicate/build_timestamp")
            .and_then(serde_json::Value::as_str),
        Some("2023-11-14T22:13:20Z"),
        "the build timestamp is not SOURCE_DATE_EPOCH, so a wall clock reached the provenance"
    );

    // §66.7: all published artifacts, not the binary while the packages go unattested. The
    // checksum manifest is a subject too — it is what the signature covers, and a provenance
    // that did not bind it would leave the one file a reader trusts unattested.
    let subjects: BTreeMap<String, String> = document["subject"]
        .as_array()
        .expect("the subjects")
        .iter()
        .map(|subject| {
            (
                subject["name"].as_str().expect("a name").to_owned(),
                subject["digest"]["sha256"]
                    .as_str()
                    .expect("a sha256")
                    .to_owned(),
            )
        })
        .collect();
    for artifact in [
        "ono_0.4.1_amd64.deb",
        "ono_0.4.1_arm64.deb",
        "ono-0.4.1-1.x86_64.rpm",
        "ono-0.4.1-1.aarch64.rpm",
        "build-inputs.json",
        "SHA256SUMS",
    ] {
        let digest = subjects
            .get(artifact)
            .unwrap_or_else(|| panic!("`{artifact}` is published and unattested:\n{document:#}"));
        let bytes = std::fs::read(directory.join(artifact)).expect("the artifact");
        assert_eq!(
            *digest,
            xtask::reproducibility::digest(&bytes),
            "the provenance binds a digest `{artifact}` does not have"
        );
    }
}

#[test]
fn should_verify_every_artifact_digest_against_the_checksum_manifest_and_the_provenance_before_publication()
 {
    let scratch = scratch();
    let directory = attested_release(&scratch, "release");
    let path = directory.to_str().expect("a UTF-8 path").to_owned();

    let (verified, report) = provenance(&["--dir", &path, "--verify"]);
    assert!(
        verified,
        "the provenance this repository just wrote does not verify:\n{report}"
    );

    // §62.5: each artifact digest is present in checksum *and* provenance output. An asset that
    // reaches the release after the provenance was written is in neither.
    std::fs::write(
        directory.join("ono-0.4.1-linux-x86_64.tar.gz"),
        "a late arrival",
    )
    .expect("a late artifact");
    let (verified, report) = provenance(&["--dir", &path, "--verify"]);
    assert!(
        !verified && report.contains("ono-0.4.1-linux-x86_64.tar.gz"),
        "an artifact absent from the provenance passed verification:\n{report}"
    );
    std::fs::remove_file(directory.join("ono-0.4.1-linux-x86_64.tar.gz")).expect("the tarball");

    // A provenance that binds a digest the artifact does not have.
    let directory = attested_release(&scratch, "swapped");
    let path = directory.to_str().expect("a UTF-8 path").to_owned();
    let document = std::fs::read_to_string(directory.join("build-provenance.json"))
        .expect("the provenance")
        .replace(
            &xtask::reproducibility::digest(
                &std::fs::read(directory.join("ono_0.4.1_amd64.deb")).expect("the package"),
            ),
            &"b".repeat(64),
        );
    std::fs::write(directory.join("build-provenance.json"), document).expect("the swap");
    let (verified, report) = provenance(&["--dir", &path, "--verify"]);
    assert!(
        !verified && report.contains("ono_0.4.1_amd64.deb"),
        "a provenance binding a digest no artifact has passed verification:\n{report}"
    );

    // And an unbound field is refused rather than published as an attestation of nothing.
    let directory = attested_release(&scratch, "unbound");
    let path = directory.to_str().expect("a UTF-8 path").to_owned();
    let document = std::fs::read_to_string(directory.join("build-provenance.json"))
        .expect("the provenance")
        .replace("\"v9.9.9\"", "null");
    std::fs::write(directory.join("build-provenance.json"), document).expect("the unbinding");
    let (verified, report) = provenance(&["--dir", &path, "--verify"]);
    assert!(
        !verified && report.contains("release_tag"),
        "a provenance with an unbound field passed verification (spec §47.4):\n{report}"
    );

    // The workflow verifies before it publishes, rather than after. §49.1 puts signing and
    // provenance ahead of publication, and §62.5 says "before publication" in as many words.
    let workflow = std::fs::read_to_string(this_repository().join(".github/workflows/release.yml"))
        .expect("the release workflow");
    let publish = workflow_job(&workflow, "publish");
    let attested = publish
        .find("xtask -- provenance")
        .expect("the publishing job generates provenance (spec §47.4)");
    let verified = publish
        .find("verify-release.sh")
        .expect("the publishing job verifies what it is about to publish (spec §62.5)");
    let published = publish
        .find("publish-release.sh")
        .expect("the publishing job attaches the assets");
    assert!(
        attested < verified && verified < published,
        "the publishing job does not generate provenance, then verify, then publish, so it can \
         attach assets nothing checked (spec §49.1, §62.5):\n{publish}"
    );
}

/// A stand-in for `gh` on `PATH`, recording the repository each call knew about.
///
/// The gate has no GitHub token and no release to write to. What this owns is the one property
/// the first real tag proved missing: that `scripts/publish-release.sh` knows *which* repository
/// it is publishing to when it runs from the artifact directory, which in the release workflow is
/// beside the checkout rather than inside it. `gh` otherwise reads that from the git remote of the
/// directory it runs in, finds none, and dies after every verification has already passed
/// (ADR-0579).
fn gh_stub(scratch: &Scratch) -> PathBuf {
    let bin = scratch.path().join("bin");
    std::fs::create_dir_all(&bin).expect("a scratch bin directory");
    let stub = bin.join("gh");
    std::fs::write(
        &stub,
        "#!/usr/bin/env bash\n\
         printf '%s | GH_REPO=%s | cwd=%s\\n' \"$*\" \"${GH_REPO:-}\" \"$PWD\" >> \"$ONO_GH_LOG\"\n\
         # `gh` itself refuses without a repository, and so does the stand-in: a test that let it\n\
         # through would pass on exactly the tree the release failed on.\n\
         if [ -z \"${GH_REPO:-}\" ] && ! git rev-parse --git-dir >/dev/null 2>&1; then\n\
         \x20 echo 'failed to run git: fatal: not a git repository' >&2; exit 1\n\
         fi\n\
         case \"$2\" in\n\
         \x20 view) [ -f \"$ONO_GH_LOG.created\" ] || exit 1 ;;\n\
         \x20 create) : > \"$ONO_GH_LOG.created\" ;;\n\
         \x20 download) exit 0 ;;\n\
         esac\n\
         exit 0\n",
    )
    .expect("the gh stand-in is written");
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
        .expect("the stand-in is executable");
    bin
}

/// An attested release directory with a signature bundle over its manifest, and both stand-ins on
/// `PATH` — everything `scripts/publish-release.sh` verifies before it drafts anything.
fn publishable(scratch: &Scratch, name: &str) -> (PathBuf, String) {
    let directory = attested_release(scratch, name);
    let manifest = std::fs::read(directory.join("SHA256SUMS")).expect("the manifest");
    std::fs::write(
        directory.join("SHA256SUMS.sigstore.json"),
        format!("{}\n", xtask::reproducibility::digest(&manifest)),
    )
    .expect("the signature bundle");
    let _ = cosign_stub(scratch);
    let bin = gh_stub(scratch);
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    (directory, path)
}

#[test]
fn should_name_the_repository_it_publishes_to_when_it_runs_from_outside_a_checkout() {
    // §49.4's order held on the first real tag — verify, then draft — and the drafting step died
    // with "not a git repository" because `dist/` sits beside the checkout in the release
    // workflow. Everything before it was green, which is why no test had caught it: the failure
    // is in the environment the script runs in, not in what it does.
    let scratch = scratch();
    let (directory, path) = publishable(&scratch, "outside");
    let log = scratch.path().join("gh.log");
    let _ = std::fs::write(&log, "");

    // `current_dir` is the scratch directory, which is not a git repository — exactly what the
    // workflow gives the script, and what a maintainer running it from `/tmp` would give it too.
    let output = Command::new("bash")
        .arg(this_repository().join("scripts/publish-release.sh"))
        .args([
            "--tag",
            "v9.9.9",
            "--dir",
            directory.to_str().expect("a UTF-8 path"),
        ])
        .current_dir(scratch.path())
        .env("PATH", &path)
        .env("ONO_GH_LOG", &log)
        .env("ONO_COSIGN_LOG", scratch.path().join("cosign.log"))
        .env("GITHUB_REPOSITORY", "godspeed-you/ono-sendai")
        .env_remove("GH_REPO")
        .output()
        .unwrap_or_else(|error| panic!("bash must be runnable in the gate: {error}"));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    let mut report = String::from_utf8_lossy(&output.stdout).into_owned();
    report.push_str(&String::from_utf8_lossy(&output.stderr));

    // What this test owns is the environment, not the whole publication: the stand-in has no
    // asset storage, so the run stops at the inventory check with its own diagnostic. What it may
    // never stop at again is the drafting step, and never for want of a repository.
    assert!(
        !report.contains("not a git repository"),
        "publishing died the way the first real tag died:\n{report}"
    );
    assert!(
        calls.contains("release create"),
        "the release was never drafted, so nothing reached the step that failed on the first \
         tag:\n{report}\ncalls:\n{calls}"
    );
    for call in calls.lines() {
        assert!(
            call.contains("GH_REPO=godspeed-you/ono-sendai"),
            "a `gh` call ran without knowing which repository it was writing to: {call}"
        );
    }
}

#[test]
fn should_take_the_repository_from_its_own_checkout_when_the_environment_names_none() {
    // A maintainer running the script by hand has no `GITHUB_REPOSITORY`. The script belongs to a
    // checkout, and that checkout's `origin` is the repository it publishes to — inferring it from
    // wherever the caller happens to stand is what went wrong.
    let scratch = scratch();
    let (directory, path) = publishable(&scratch, "by-hand");
    let log = scratch.path().join("gh.log");
    let _ = std::fs::write(&log, "");

    let output = Command::new("bash")
        .arg(this_repository().join("scripts/publish-release.sh"))
        .args([
            "--tag",
            "v9.9.9",
            "--dir",
            directory.to_str().expect("a UTF-8 path"),
        ])
        .current_dir(scratch.path())
        .env("PATH", &path)
        .env("ONO_GH_LOG", &log)
        .env("ONO_COSIGN_LOG", scratch.path().join("cosign.log"))
        .env_remove("GITHUB_REPOSITORY")
        .env_remove("GH_REPO")
        .output()
        .unwrap_or_else(|error| panic!("bash must be runnable in the gate: {error}"));
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    let mut report = String::from_utf8_lossy(&output.stdout).into_owned();
    report.push_str(&String::from_utf8_lossy(&output.stderr));

    assert!(
        !report.contains("not a git repository"),
        "publishing by hand died the way the first real tag died:\n{report}"
    );
    assert!(
        calls.contains("release create"),
        "nothing was drafted:\n{report}\ncalls:\n{calls}"
    );
    for call in calls.lines() {
        assert!(
            call.contains("GH_REPO=godspeed-you/ono-sendai"),
            "a `gh` call ran without the repository this checkout's `origin` names: {call}"
        );
    }
}
