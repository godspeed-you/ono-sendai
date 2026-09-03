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

use std::os::unix::fs::PermissionsExt as _;
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
