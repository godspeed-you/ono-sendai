//! The gate's supply-chain pinning rules (spec §43, §44, §62.1, §62.2).
//!
//! Every one of these rules exists because a reference that resolves differently tomorrow is a
//! build nobody can reproduce and an attacker nobody can see. They decide whether "the release
//! was built from this commit" means anything, so they are tested against fixtures rather than
//! trusted.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::path::Path;

use ono_testkit::{Scratch, scratch};

mod support;
use support::{read, report, workflow_job};
use xtask::supply_chain::{
    check_action_pins, check_dependency_justifications, check_dependency_policy,
    check_image_digests, check_locked_builds, check_tool_versions, check_workflow_permissions,
};

/// Builds a throwaway repository shaped like this one.
fn fixture(files: &[(&str, &str)]) -> Scratch {
    let repo = scratch();
    for (path, contents) in files {
        repo.write(path, contents);
    }
    repo
}

/// The repository this suite guards.
fn this_repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
}

/// A workflow that satisfies every rule but the one under test.
const SOUND_HEADER: &str =
    "name: w\non:\n  push:\n    branches: [main]\npermissions:\n  contents: read\n";

// --- action references (spec §43.1, §62.1) ------------------------------------------------------

#[test]
fn should_reject_an_action_referenced_by_a_floating_tag() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "jobs:\n  a:\n    steps:\n      - uses: actions/checkout@v4\n",
    )]);
    let problems = check_action_pins(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0]
            .location
            .starts_with(".github/workflows/ci.yml:4"),
        "got {}",
        problems[0].location
    );
    assert!(problems[0].detail.contains("actions/checkout@v4"));
}

#[test]
fn should_reject_an_action_referenced_by_a_branch_name() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "jobs:\n  a:\n    steps:\n      - uses: dtolnay/rust-toolchain@stable\n",
    )]);
    assert_eq!(check_action_pins(repo.path()).len(), 1);
}

#[test]
fn should_accept_an_action_pinned_to_a_commit_sha_with_the_tag_in_a_trailing_comment() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "jobs:\n  a:\n    steps:\n      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n",
    )]);
    assert_eq!(check_action_pins(repo.path()), Vec::new());
}

#[test]
fn should_reject_a_forty_character_reference_that_is_not_a_commit_sha() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "jobs:\n  a:\n    steps:\n      - uses: acme/act@zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz\n",
    )]);
    assert_eq!(check_action_pins(repo.path()).len(), 1);
}

#[test]
fn should_accept_an_action_that_lives_in_this_repository() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "jobs:\n  a:\n    steps:\n      - uses: ./.github/actions/setup\n",
    )]);
    assert_eq!(check_action_pins(repo.path()), Vec::new());
}

#[test]
fn should_reject_an_unpinned_action_inside_a_composite_action() {
    let repo = fixture(&[(
        ".github/actions/setup/action.yml",
        "runs:\n  using: composite\n  steps:\n    - uses: actions/checkout@v4\n",
    )]);
    assert_eq!(check_action_pins(repo.path()).len(), 1);
}

#[test]
fn should_ignore_an_action_reference_that_is_only_written_in_a_comment() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "jobs:\n  a:\n    steps:\n      # uses: actions/checkout@v4 was replaced\n      - run: true\n",
    )]);
    assert_eq!(check_action_pins(repo.path()), Vec::new());
}

#[test]
fn should_report_this_repository_as_pinning_every_action_it_uses() {
    let problems = check_action_pins(this_repository());
    assert!(
        problems.is_empty(),
        "this repository runs third-party actions from mutable references:\n{}",
        report(&problems)
    );
}

// --- container images (spec §44.1, §62.2) -------------------------------------------------------

#[test]
fn should_reject_a_build_image_pulled_by_tag_alone() {
    let repo = fixture(&[(
        "docker/Dockerfile",
        "FROM rust:1.94-slim-bookworm AS builder\n",
    )]);
    let problems = check_image_digests(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].location.starts_with("docker/Dockerfile:1"));
    assert!(problems[0].detail.contains("rust:1.94-slim-bookworm"));
}

#[test]
fn should_accept_a_build_image_pinned_by_digest_with_the_tag_still_readable() {
    let repo = fixture(&[(
        "docker/Dockerfile",
        "FROM rust:1.94-slim-bookworm@sha256:cf9dd0ec73e75f827fe59123fff9dc65af1a1c8363c3c31ee8d7f8ad0b6a5fb2 AS builder\n",
    )]);
    assert_eq!(check_image_digests(repo.path()), Vec::new());
}

#[test]
fn should_accept_a_later_stage_that_builds_on_an_earlier_one() {
    let repo = fixture(&[(
        "docker/Dockerfile",
        "FROM rust@sha256:cf9dd0ec73e75f827fe59123fff9dc65af1a1c8363c3c31ee8d7f8ad0b6a5fb2 AS builder\nFROM builder AS runtime\n",
    )]);
    assert_eq!(check_image_digests(repo.path()), Vec::new());
}

#[test]
fn should_accept_an_image_this_repository_builds_itself() {
    let repo = fixture(&[("docker/demo.Dockerfile", "FROM ono-sendai:demo\n")]);
    assert_eq!(check_image_digests(repo.path()), Vec::new());
}

#[test]
fn should_reject_a_package_validation_image_named_by_a_shell_variable_without_a_digest() {
    let repo = fixture(&[(
        "scripts/package-check.sh",
        "#!/usr/bin/env bash\nFEDORA_IMAGE=\"${ONO_PACKAGE_CHECK_FEDORA:-fedora:latest}\"\n",
    )]);
    let problems = check_image_digests(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("fedora:latest"));
}

#[test]
fn should_accept_a_shell_variable_whose_default_carries_a_digest() {
    let repo = fixture(&[(
        "scripts/package-check.sh",
        "FEDORA_IMAGE=\"${ONO_PACKAGE_CHECK_FEDORA:-fedora:latest@sha256:43b29f65a41eb9c35e1cd5323e3bdf3b655c2357a9f4f1ff2f9c2798e5045d80}\"\n",
    )]);
    assert_eq!(check_image_digests(repo.path()), Vec::new());
}

#[test]
fn should_ignore_a_flag_variable_whose_name_merely_mentions_an_image() {
    let repo = fixture(&[("scripts/acceptance.sh", "KEEP_IMAGE=0\nNO_BUILD=1\n")]);
    assert_eq!(check_image_digests(repo.path()), Vec::new());
}

#[test]
fn should_reject_a_workflow_job_running_in_a_container_image_without_a_digest() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "jobs:\n  a:\n    container:\n      image: fedora:latest\n",
    )]);
    assert_eq!(check_image_digests(repo.path()).len(), 1);
}

#[test]
fn should_report_this_repository_as_pinning_every_release_critical_image() {
    let problems = check_image_digests(this_repository());
    assert!(
        problems.is_empty(),
        "this repository pulls mutable container images:\n{}",
        report(&problems)
    );
}

// --- workflow permissions and untrusted pull requests (spec §43.3, §43.4, §43.5) ----------------

#[test]
fn should_reject_a_workflow_that_declares_no_permissions_at_all() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "name: w\non:\n  push:\njobs:\n  a:\n    steps:\n      - run: true\n",
    )]);
    let problems = check_workflow_permissions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("permissions"));
}

#[test]
fn should_reject_a_workflow_that_grants_write_access_to_every_job() {
    let repo = fixture(&[(
        ".github/workflows/release.yml",
        "name: r\non:\n  push:\n    tags: [\"v*\"]\npermissions:\n  contents: write\nconcurrency:\n  group: r\njobs:\n  build:\n    steps:\n      - run: true\n",
    )]);
    let problems = check_workflow_permissions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("contents: write"));
}

#[test]
fn should_reject_a_workflow_that_grants_every_scope_at_once() {
    let repo = fixture(&[(
        ".github/workflows/release.yml",
        "name: r\non:\n  push:\n    tags: [\"v*\"]\npermissions: write-all\nconcurrency:\n  group: r\njobs:\n  build:\n    steps:\n      - run: true\n",
    )]);
    assert_eq!(check_workflow_permissions(repo.path()).len(), 1);
}

#[test]
fn should_accept_write_access_granted_only_to_the_publishing_job() {
    let repo = fixture(&[(
        ".github/workflows/release.yml",
        "name: r\non:\n  push:\n    tags: [\"v*\"]\npermissions:\n  contents: read\nconcurrency:\n  group: r\njobs:\n  build:\n    steps:\n      - run: true\n  publish:\n    permissions:\n      contents: write\n    steps:\n      - run: true\n",
    )]);
    assert_eq!(check_workflow_permissions(repo.path()), Vec::new());
}

#[test]
fn should_reject_a_workflow_triggered_by_pull_request_target() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "name: w\non:\n  pull_request_target:\npermissions:\n  contents: read\njobs:\n  a:\n    steps:\n      - run: true\n",
    )]);
    let problems = check_workflow_permissions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("pull_request_target"));
}

#[test]
fn should_reject_a_secret_reachable_from_an_untrusted_pull_request() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "name: w\non:\n  pull_request:\npermissions:\n  contents: read\njobs:\n  a:\n    steps:\n      - run: sign\n        env:\n          KEY: ${{ secrets.RELEASE_SIGNING_KEY }}\n",
    )]);
    let problems = check_workflow_permissions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("RELEASE_SIGNING_KEY"));
}

#[test]
fn should_accept_the_automatic_token_in_a_workflow_a_pull_request_can_start() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        "name: w\non:\n  pull_request:\npermissions:\n  contents: read\njobs:\n  a:\n    steps:\n      - run: gh pr view\n        env:\n          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n",
    )]);
    assert_eq!(check_workflow_permissions(repo.path()), Vec::new());
}

#[test]
fn should_reject_a_publishing_job_a_pull_request_can_reach() {
    let repo = fixture(&[(
        ".github/workflows/release.yml",
        "name: r\non:\n  pull_request:\n  push:\n    tags: [\"v*\"]\npermissions:\n  contents: read\nconcurrency:\n  group: r\njobs:\n  publish:\n    permissions:\n      contents: write\n    steps:\n      - run: true\n",
    )]);
    let problems = check_workflow_permissions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("`publish`"),
        "got {}",
        problems[0].detail
    );
    assert!(problems[0].detail.contains("a pull request can start"));
}

#[test]
fn should_reject_a_publishing_workflow_without_a_concurrency_guard() {
    let repo = fixture(&[(
        ".github/workflows/release.yml",
        "name: r\non:\n  push:\n    tags: [\"v*\"]\npermissions:\n  contents: read\njobs:\n  publish:\n    permissions:\n      contents: write\n    steps:\n      - run: true\n",
    )]);
    let problems = check_workflow_permissions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("concurrency"));
}

#[test]
fn should_accept_a_read_only_workflow_a_pull_request_can_start() {
    let repo = fixture(&[(
        ".github/workflows/ci.yml",
        &format!("{SOUND_HEADER}jobs:\n  a:\n    steps:\n      - run: true\n"),
    )]);
    assert_eq!(check_workflow_permissions(repo.path()), Vec::new());
}

#[test]
fn should_report_this_repository_as_granting_least_privilege_in_every_workflow() {
    let problems = check_workflow_permissions(this_repository());
    assert!(
        problems.is_empty(),
        "this repository hands its workflows more than they need:\n{}",
        report(&problems)
    );
}

// --- dependency policy (spec §45.1, §45.2, §62.3) -----------------------------------------------

/// A policy file that satisfies every rule but the one under test.
const SOUND_POLICY: &str = r#"
[advisories]
yanked = "deny"

[licenses]
allow = ["MIT"]

[bans]
multiple-versions = "warn"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
"#;

/// A gate that runs the policy, so a fixture only fails on what it is about.
const SOUND_GATE: &str = "cargo deny --locked check\n";

#[test]
fn should_reject_a_repository_with_no_dependency_policy_at_all() {
    let repo = fixture(&[("scripts/gate.sh", SOUND_GATE)]);
    let problems = check_dependency_policy(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert_eq!(problems[0].location, "deny.toml");
}

#[test]
fn should_reject_a_dependency_policy_that_leaves_one_of_the_four_checks_unconfigured() {
    let repo = fixture(&[
        ("scripts/gate.sh", SOUND_GATE),
        (
            "deny.toml",
            "[advisories]\nyanked = \"deny\"\n\n[licenses]\nallow = [\"MIT\"]\n\n[bans]\nmultiple-versions = \"warn\"\n",
        ),
    ]);
    let problems = check_dependency_policy(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("sources"), "got {problems:?}");
}

#[test]
fn should_reject_a_dependency_policy_that_allows_no_licence_at_all() {
    let repo = fixture(&[
        ("scripts/gate.sh", SOUND_GATE),
        (
            "deny.toml",
            &SOUND_POLICY.replace("allow = [\"MIT\"]", "allow = []"),
        ),
    ]);
    let problems = check_dependency_policy(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("licence"), "got {problems:?}");
}

#[test]
fn should_reject_an_ignored_advisory_that_names_no_removal_deadline() {
    let repo = fixture(&[
        ("scripts/gate.sh", SOUND_GATE),
        (
            "deny.toml",
            &SOUND_POLICY.replace(
                "[advisories]\n",
                "[advisories]\nignore = [{ id = \"RUSTSEC-2026-0001\", reason = \"we will look \
                 at it\" }]\n",
            ),
        ),
    ]);
    let problems = check_dependency_policy(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("RUSTSEC-2026-0001"),
        "got {problems:?}"
    );
}

#[test]
fn should_accept_an_ignored_advisory_carrying_a_reason_and_a_removal_deadline() {
    let repo = fixture(&[
        ("scripts/gate.sh", SOUND_GATE),
        (
            "deny.toml",
            &SOUND_POLICY.replace(
                "[advisories]\n",
                "[advisories]\nignore = [{ id = \"RUSTSEC-2026-0001\", reason = \"the vulnerable \
                 path is unreachable, expires 2099-01-01\" }]\n",
            ),
        ),
    ]);
    assert_eq!(check_dependency_policy(repo.path()), Vec::new());
}

#[test]
fn should_reject_an_ignored_advisory_whose_removal_deadline_has_passed() {
    let repo = fixture(&[
        ("scripts/gate.sh", SOUND_GATE),
        (
            "deny.toml",
            &SOUND_POLICY.replace(
                "[advisories]\n",
                "[advisories]\nignore = [{ id = \"RUSTSEC-2026-0001\", reason = \"the vulnerable \
                 path is unreachable, expires 2000-01-01\" }]\n",
            ),
        ),
    ]);
    let problems = check_dependency_policy(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("2000-01-01"),
        "got {problems:?}"
    );
}

#[test]
fn should_reject_a_dependency_policy_that_nothing_in_the_gate_runs() {
    let repo = fixture(&[
        ("scripts/gate.sh", "cargo test --workspace\n"),
        ("deny.toml", SOUND_POLICY),
    ]);
    let problems = check_dependency_policy(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert_eq!(problems[0].location, "scripts/gate.sh");
}

#[test]
fn should_report_this_repository_as_running_its_dependency_policy_in_the_gate() {
    let problems = check_dependency_policy(this_repository());
    assert!(
        problems.is_empty(),
        "this repository's dependency policy does not hold:\n{}",
        report(&problems)
    );
}

// --- recorded justifications (spec §45.3, §45.4) ------------------------------------------------

#[test]
fn should_fail_the_dependency_policy_on_an_unjustified_git_dependency() {
    let repo = fixture(&[(
        "Cargo.toml",
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\nacme = { git = \"https://example.invalid/acme\", rev = \"1111111111111111111111111111111111111111\" }\n",
    )]);
    let problems = check_dependency_justifications(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("acme"), "got {problems:?}");
}

#[test]
fn should_reject_a_git_dependency_that_follows_a_branch_instead_of_a_revision() {
    let repo = fixture(&[(
        "Cargo.toml",
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\nacme = { git = \"https://example.invalid/acme\", branch = \"main\" }\n\n[[workspace.metadata.supply-chain.git]]\ncrate = \"acme\"\nreason = \"nothing else reads the format\"\nadr = \"ADR-0449\"\n",
    )]);
    let problems = check_dependency_justifications(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("branch"), "got {problems:?}");
}

#[test]
fn should_accept_a_git_dependency_pinned_to_a_revision_and_written_down() {
    let repo = fixture(&[(
        "Cargo.toml",
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\nacme = { git = \"https://example.invalid/acme\", rev = \"1111111111111111111111111111111111111111\" }\n\n[[workspace.metadata.supply-chain.git]]\ncrate = \"acme\"\nreason = \"nothing else reads the format\"\nadr = \"ADR-0449\"\n",
    )]);
    assert_eq!(check_dependency_justifications(repo.path()), Vec::new());
}

#[test]
fn should_reject_a_cryptographic_dependency_nobody_recorded_a_review_for() {
    let repo = fixture(&[(
        "Cargo.toml",
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\nacme-tls = \"1\"\n",
    )]);
    let problems = check_dependency_justifications(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("acme-tls"), "got {problems:?}");
}

#[test]
fn should_reject_a_cryptographic_dependency_a_single_crate_pulls_in_on_its_own() {
    let repo = fixture(&[
        ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
        (
            "crates/ono-thing/Cargo.toml",
            "[package]\nname = \"ono-thing\"\n\n[dependencies]\ned25519-dalek = \"3\"\n",
        ),
    ]);
    let problems = check_dependency_justifications(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("ed25519-dalek"),
        "got {problems:?}"
    );
}

#[test]
fn should_accept_a_cryptographic_dependency_whose_review_is_recorded() {
    let repo = fixture(&[(
        "Cargo.toml",
        "[workspace]\nmembers = []\n\n[workspace.dependencies]\nacme-tls = \"1\"\n\n[[workspace.metadata.supply-chain.cryptographic]]\ncrate = \"acme-tls\"\nrole = \"the transport of spec §21.5\"\nadr = \"ADR-0449\"\nreviewed = \"2026-09-02\"\n",
    )]);
    assert_eq!(check_dependency_justifications(repo.path()), Vec::new());
}

#[test]
fn should_ignore_a_crate_that_only_inherits_a_dependency_the_workspace_already_recorded() {
    let repo = fixture(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\nacme-tls = \"1\"\n\n[[workspace.metadata.supply-chain.cryptographic]]\ncrate = \"acme-tls\"\nrole = \"the transport of spec §21.5\"\nadr = \"ADR-0449\"\nreviewed = \"2026-09-02\"\n",
        ),
        (
            "crates/ono-thing/Cargo.toml",
            "[package]\nname = \"ono-thing\"\n\n[dependencies]\nacme-tls.workspace = true\n",
        ),
    ]);
    assert_eq!(check_dependency_justifications(repo.path()), Vec::new());
}

#[test]
fn should_report_a_recorded_justification_for_a_dependency_the_workspace_no_longer_has() {
    let repo = fixture(&[(
        "Cargo.toml",
        "[workspace]\nmembers = []\n\n[[workspace.metadata.supply-chain.cryptographic]]\ncrate = \"acme-tls\"\nrole = \"the transport of spec §21.5\"\nadr = \"ADR-0449\"\nreviewed = \"2026-09-02\"\n",
    )]);
    let problems = check_dependency_justifications(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("acme-tls"), "got {problems:?}");
}

#[test]
fn should_report_this_repository_as_justifying_every_git_and_cryptographic_dependency() {
    let problems = check_dependency_justifications(this_repository());
    assert!(
        problems.is_empty(),
        "this repository trusts dependencies nobody wrote a reason for:\n{}",
        report(&problems)
    );
}

// --- the policy command actually fails (spec §62.3) ---------------------------------------------
//
// A policy nobody has seen fail is a policy nobody has tested. These two run the real
// `cargo deny` against a condition arranged from outside it, so the gate's dependency step is
// known to be load-bearing rather than assumed to be.

/// Runs `cargo deny` and returns its exit status and combined output.
fn cargo_deny(args: &[&str]) -> (bool, String) {
    let output = std::process::Command::new("cargo")
        .arg("deny")
        .args(args)
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "`cargo deny` must be runnable in the gate — cargo install --locked \
                 cargo-deny@0.20.2: {error}"
            )
        });
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

/// Makes `dir` a git repository holding everything under it, which is the only shape
/// `cargo deny` will read an advisory database in.
fn commit_everything(dir: &Path) {
    for args in [
        vec!["-c", "init.defaultBranch=main", "init", "--quiet"],
        vec!["add", "--all"],
        vec![
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "user.name=fixture",
            "commit",
            "--quiet",
            "--message",
            "the seeded database",
        ],
    ] {
        let status = std::process::Command::new("git")
            .current_dir(dir)
            .args(&args)
            .status()
            .expect("git must be runnable in the gate");
        assert!(status.success(), "git {args:?} failed in {}", dir.display());
    }
}

/// Puts every platform's crates in the local registry, because the offline runs below walk the
/// whole graph.
///
/// `cargo metadata` and `cargo tree` resolve for every target, so the graph names crates a Linux
/// build never downloads — `mach2` for macOS and `winapi-util` for Windows arrived with wasmtime
/// (ADR-0569). A machine that only ever built this workspace has them in its lock file and not on
/// its disk, and the offline run then fails on the download rather than on the policy. `cargo
/// fetch` is what completes it: unlike a build it fetches every target's dependencies. On a
/// machine that already has them it is a no-op.
fn complete_the_crate_graph() {
    let status = std::process::Command::new("cargo")
        .args(["fetch", "--locked"])
        .current_dir(this_repository())
        .status()
        .expect("cargo must be runnable in the gate");
    assert!(
        status.success(),
        "`cargo fetch --locked` could not complete the crate graph the offline runs walk"
    );
}

#[test]
fn should_fail_the_dependency_policy_on_a_denied_advisory_fixture() {
    // The advisory arm can only fire on a crate that came from a registry, so the graph under
    // test is this workspace and the database is the fixture: one advisory, invented here,
    // against a crate this repository really depends on.
    complete_the_crate_graph();
    let lock = std::fs::read_to_string(this_repository().join("Cargo.lock")).expect("Cargo.lock");
    let (name, version) = first_registry_crate(&lock);

    let scratch = scratch();
    let db_path = scratch.path().join("advisory-db");
    let config = scratch.write(
        "deny.toml",
        format!(
            "[advisories]\ndb-path = {:?}\ndb-urls = [\"https://example.invalid/ono-fixture-advisory-db\"]\n",
            db_path.display().to_string()
        ),
    );
    let manifest = this_repository().join("Cargo.toml");
    let arguments = [
        "--manifest-path",
        manifest.to_str().expect("a utf-8 path"),
        "--config",
        config.to_str().expect("a utf-8 path"),
        "--offline",
        "check",
        "advisories",
    ];

    // cargo-deny keeps each database in a directory it names after the URL. Rather than
    // reproducing that naming, the fixture asks: the first run fails because the database is
    // absent, and says where it looked.
    let (passed, output) = cargo_deny(&arguments);
    assert!(
        !passed,
        "an absent advisory database must not pass:\n{output}"
    );
    let seeded = output
        .split(|c: char| c == '"' || c == '\'' || c.is_whitespace())
        .find(|word| word.starts_with(&db_path.display().to_string()))
        .unwrap_or_else(|| panic!("cargo-deny did not say where it keeps the database:\n{output}"))
        .to_owned();

    std::fs::create_dir_all(Path::new(&seeded).join("crates").join(&name))
        .expect("the seeded database directory");
    std::fs::write(
        Path::new(&seeded)
            .join("crates")
            .join(&name)
            .join("RUSTSEC-2026-0001.md"),
        format!(
            "```toml\n[advisory]\nid = \"RUSTSEC-2026-0001\"\npackage = \"{name}\"\n\
             date = \"2026-01-01\"\nurl = \"https://example.invalid/seeded\"\n\
             categories = [\"code-execution\"]\nkeywords = [\"fixture\"]\n\n\
             [versions]\npatched = [\">= 99.0.0\"]\n```\n\n\
             # A seeded advisory that exists only to prove the policy fails\n\n\
             Nothing in it is real. It names {name} {version} because this workspace depends on \
             it, so a policy that reports nothing here is a policy that reports nothing at all.\n"
        ),
    )
    .expect("the seeded advisory");
    commit_everything(Path::new(&seeded));

    let (passed, output) = cargo_deny(&arguments);
    assert!(
        !passed,
        "the dependency policy passed with an advisory against {name} {version}:\n{output}"
    );
    assert!(
        output.contains("RUSTSEC-2026-0001"),
        "the failure does not name the advisory:\n{output}"
    );
}

#[test]
fn should_fail_the_dependency_policy_on_a_denied_license_fixture() {
    let workspace = fixture(&[
        (
            "Cargo.toml",
            "[workspace]\n\n[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n\
             edition = \"2021\"\nlicense = \"MIT\"\n\n[dependencies]\n\
             denied = { path = \"denied\" }\n",
        ),
        ("src/lib.rs", ""),
        (
            "denied/Cargo.toml",
            "[package]\nname = \"denied\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             license = \"GPL-3.0\"\n",
        ),
        ("denied/src/lib.rs", ""),
        ("deny.toml", "[licenses]\nallow = [\"MIT\"]\n"),
    ]);
    let manifest = workspace.path().join("Cargo.toml");
    let config = workspace.path().join("deny.toml");
    let (passed, output) = cargo_deny(&[
        "--manifest-path",
        manifest.to_str().expect("a utf-8 path"),
        "--config",
        config.to_str().expect("a utf-8 path"),
        "--offline",
        "check",
        "licenses",
    ]);
    assert!(
        !passed,
        "the dependency policy passed a GPL-3.0 crate under an MIT-only allow list:\n{output}"
    );
    assert!(
        output.contains("GPL-3.0"),
        "the failure does not name the licence:\n{output}"
    );
}

/// The first crate in a lockfile that a registry supplied, with its version.
fn first_registry_crate(lock: &str) -> (String, String) {
    // The lock file is the union over every feature and platform, so a crate it names may be
    // one nothing in the default build activates — and `cargo deny` checks the build that is
    // active. The fixture has to name a crate the policy will actually see.
    let active = std::process::Command::new("cargo")
        .args([
            "tree",
            "--workspace",
            "-e",
            "normal",
            "--prefix",
            "none",
            "--offline",
        ])
        .current_dir(this_repository())
        .output()
        .expect("cargo tree must be runnable in the gate");
    let active: std::collections::BTreeSet<String> = String::from_utf8_lossy(&active.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect();
    let mut name = None;
    let mut version = None;
    for line in lock.lines() {
        if let Some(rest) = line.strip_prefix("name = \"") {
            name = rest.strip_suffix('"').map(str::to_owned);
        } else if let Some(rest) = line.strip_prefix("version = \"") {
            version = rest.strip_suffix('"').map(str::to_owned);
        } else if line.starts_with("source = \"registry+")
            && let (Some(name), Some(version)) = (name.clone(), version.clone())
            && active.contains(&name)
        {
            return (name, version);
        }
    }
    panic!("Cargo.lock names no crate that came from a registry and is in the active build");
}

// --- exact tool versions and locked builds (spec §44.2, §44.3, §44.4) ---------------------------

/// A workspace manifest whose register names the versions the fixtures install.
const SOUND_REGISTER: &str = "[workspace]\nmembers = []\n\n\
     [workspace.metadata.release-tools]\ncargo-deb = \"3.7.0\"\n";

/// A workspace manifest that pins no tool, for the fixtures that are about the toolchain.
const NO_TOOLS: &str = "[workspace]\nmembers = []\n";

/// The pinned toolchain the fixtures agree with.
const SOUND_TOOLCHAIN: &str = "[toolchain]\nchannel = \"1.94\"\n";

#[test]
fn should_reject_a_tool_installed_without_a_version() {
    let repo = fixture(&[
        ("Cargo.toml", SOUND_REGISTER),
        ("rust-toolchain.toml", SOUND_TOOLCHAIN),
        (
            ".github/workflows/ci.yml",
            "jobs:\n  a:\n    steps:\n      - uses: taiki-e/install-action@abc\n        with:\n          tool: cargo-deb\n",
        ),
    ]);
    let problems = check_tool_versions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("cargo-deb"), "got {problems:?}");
}

#[test]
fn should_reject_a_tool_installed_at_a_version_the_register_does_not_name() {
    let repo = fixture(&[
        ("Cargo.toml", SOUND_REGISTER),
        ("rust-toolchain.toml", SOUND_TOOLCHAIN),
        (
            ".github/workflows/ci.yml",
            "jobs:\n  a:\n    steps:\n      - uses: taiki-e/install-action@abc\n        with:\n          tool: cargo-deb@1.0.0\n",
        ),
    ]);
    let problems = check_tool_versions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("3.7.0"), "got {problems:?}");
}

#[test]
fn should_accept_a_tool_installed_at_the_registered_version() {
    let repo = fixture(&[
        ("Cargo.toml", SOUND_REGISTER),
        ("rust-toolchain.toml", SOUND_TOOLCHAIN),
        (
            ".github/workflows/ci.yml",
            "jobs:\n  a:\n    steps:\n      - uses: taiki-e/install-action@abc\n        with:\n          tool: cargo-deb@3.7.0\n",
        ),
    ]);
    assert_eq!(check_tool_versions(repo.path()), Vec::new());
}

#[test]
fn should_reject_a_release_job_asking_for_a_toolchain_the_repository_does_not_pin() {
    let repo = fixture(&[
        ("Cargo.toml", NO_TOOLS),
        ("rust-toolchain.toml", SOUND_TOOLCHAIN),
        (
            ".github/workflows/release.yml",
            "jobs:\n  a:\n    steps:\n      - uses: dtolnay/rust-toolchain@abc\n        with:\n          toolchain: \"1.90\"\n",
        ),
    ]);
    let problems = check_tool_versions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("1.94"), "got {problems:?}");
}

#[test]
fn should_reject_a_rust_toolchain_that_follows_a_channel_instead_of_a_version() {
    let repo = fixture(&[
        ("Cargo.toml", NO_TOOLS),
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\n"),
    ]);
    let problems = check_tool_versions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert_eq!(problems[0].location, "rust-toolchain.toml");
}

#[test]
fn should_reject_a_registered_tool_version_that_disagrees_with_a_script() {
    let repo = fixture(&[
        ("Cargo.toml", SOUND_REGISTER),
        ("rust-toolchain.toml", SOUND_TOOLCHAIN),
        (
            "scripts/package.sh",
            "for spec in cargo-deb@3.5.0; do :; done\n",
        ),
    ]);
    let problems = check_tool_versions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("3.7.0"), "got {problems:?}");
}

#[test]
fn should_report_a_registered_tool_version_nothing_installs() {
    let repo = fixture(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = []\n\n[workspace.metadata.release-tools]\nghost-tool = \"1.0.0\"\n",
        ),
        ("rust-toolchain.toml", SOUND_TOOLCHAIN),
    ]);
    let problems = check_tool_versions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("ghost-tool"),
        "got {problems:?}"
    );
}

#[test]
fn should_find_an_exact_version_for_every_release_tool() {
    let problems = check_tool_versions(this_repository());
    assert!(
        problems.is_empty(),
        "this repository builds a release with tools nothing pins:\n{}",
        report(&problems)
    );
}

#[test]
fn should_reject_a_release_build_that_does_not_lock_the_dependency_graph() {
    let repo = fixture(&[(
        "scripts/package.sh",
        "cargo build --release --target \"$target\" --package ono-cli\n",
    )]);
    let problems = check_locked_builds(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("--locked"), "got {problems:?}");
}

#[test]
fn should_reject_a_fallback_that_builds_again_without_the_lock() {
    let repo = fixture(&[(
        "docker/Dockerfile",
        "RUN cargo build --release --locked \\\n  || cargo build --release\n",
    )]);
    let problems = check_locked_builds(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].location.ends_with(":2"),
        "got {}",
        problems[0].location
    );
}

#[test]
fn should_accept_a_developer_script_that_only_mentions_a_build_command() {
    let repo = fixture(&[(
        "scripts/demo/make.sh",
        "echo \"demo: cargo build --release -p ono-cli first\"\n",
    )]);
    assert_eq!(check_locked_builds(repo.path()), Vec::new());
}

#[test]
fn should_build_the_release_with_a_locked_dependency_graph() {
    let problems = check_locked_builds(this_repository());
    assert!(
        problems.is_empty(),
        "this repository builds a release that may re-resolve its dependencies:\n{}",
        report(&problems)
    );
}

// --- v0.4.1 §41 and §42: the scheduled tiers -----------------------------------------------------

/// The text of a workflow, or a panic naming the one that is missing.
fn workflow(name: &str) -> String {
    let path = this_repository().join(".github/workflows").join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!(".github/workflows/{name} is readable: {error}"))
}

#[test]
fn should_declare_a_scheduled_coverage_guided_fuzzing_job_for_every_declared_target() {
    // v0.4.1 §41.2 requires a coverage-guided tier over seven named entry points, and §41.3 puts
    // it on a schedule: "at least daily on the default branch or a minimum aggregate time of 30
    // minutes per day across the critical targets". A list of targets in a workflow can drift
    // from the targets themselves, so it is read against them.
    let fuzz = workflow("fuzz.yml");
    assert!(
        fuzz.contains("schedule:") && fuzz.contains("cron:"),
        "§41.3: the coverage-guided tier runs on a schedule rather than when somebody remembers"
    );
    assert!(
        fuzz.contains("cargo fuzz run"),
        "§41.2: the tier is coverage-guided — `cargo-fuzz`/libFuzzer, or an equivalent engine"
    );
    for target in ono_fuzz::TARGETS {
        assert!(
            fuzz.contains(&format!("- {}\n", target.name)),
            "the `{}` target is declared and the scheduled tier does not run it: a target that \
             only ever runs for four hundred bounded iterations a gate run is not fuzzed (§41.2)",
            target.name
        );
    }
    // Seven targets at five minutes each is thirty-five, which clears §41.3's daily aggregate.
    assert!(
        fuzz.contains("default: \"5\"") && fuzz.contains("MINUTES * 60"),
        "§41.3's aggregate is a number the workflow states, not one a reader adds up"
    );
    assert!(
        ono_fuzz::TARGETS.len() * 5 >= 30,
        "§41.3: at least thirty minutes a day across the critical targets"
    );
}

#[test]
fn should_keep_the_deterministic_fuzz_tier_inside_the_gate() {
    // §41.1: "The existing lightweight/deterministic fuzz targets remain valuable and MUST stay
    // in the normal gate where they are fast enough." Adding the scheduled tier is not permission
    // to take the fast one out — it is the regression suite that replays every past finding.
    let gate = std::fs::read_to_string(this_repository().join("scripts/gate.sh"))
        .expect("scripts/gate.sh is readable");
    assert!(
        gate.contains("--package ono-fuzz") && gate.contains("--iterations"),
        "§41.1: the gate runs the deterministic tier over a bounded iteration count"
    );
    assert!(
        gate.contains("--per-input-ms"),
        "§41.5: the gate's tier enforces a per-input ceiling too, so a pathological input is a \
         finding rather than a slow gate"
    );
}

#[test]
fn should_declare_a_miri_job_covering_every_unsafe_boundary_module() {
    // §42.1: unsafe code is concentrated, and v0.4.1 "MUST exploit that architecture with
    // targeted verification". §42.2 names the areas Miri covers and excuses the process layer,
    // which it cannot execute.
    let verification = workflow("verification.yml");
    assert!(
        verification.contains("schedule:") && verification.contains("cargo miri test"),
        "§42.2: a scheduled Miri job exists"
    );
    for area in [
        "--package ono-value",
        "--package ono-parser",
        "--package ono-protocol",
        "--package ono-kuang-protocol",
    ] {
        assert!(
            verification.contains(area),
            "§42.2 names value ownership, parser data structures and protocol serialization, and \
             the Miri job does not run `{area}`"
        );
    }
    assert!(
        !verification.contains("continue-on-error"),
        "§42.4: a reproducible Miri or sanitizer finding is a release blocker, not a warning"
    );
}

#[test]
fn should_declare_an_address_and_undefined_behaviour_sanitizer_job_for_the_release_commit() {
    // §42.3: "Linux scheduled CI SHOULD run AddressSanitizer and UndefinedBehaviorSanitizer on
    // selected integration tests... The unsafe process crate and FFI/syscall wrappers are
    // priority targets." §66.5 requires the jobs to be green for the release commit, which is
    // what a schedule on the default branch delivers.
    let verification = workflow("verification.yml");
    assert!(
        verification.contains("-Zsanitizer=address"),
        "§42.3 names AddressSanitizer, and the tests are built with it rather than beside it"
    );
    // §42.3 asks for both "where Rust/toolchain support permits", and `rustc` has never accepted
    // `-Zsanitizer=undefined`: UndefinedBehaviorSanitizer instruments C and C++ semantics. What
    // Rust offers for the same question is the library's own preconditions, compiled in
    // (ADR-0574).
    assert!(
        verification.contains("-Zub-checks=yes"),
        "§42.3's undefined-behaviour half runs the checks the toolchain does have (ADR-0574)"
    );
    assert!(
        verification.contains("-Z build-std"),
        "both jobs rebuild the standard library, so the instrumentation reaches across it"
    );

    // Every crate that holds an `unsafe` block is one the sanitizer job runs. The list is read
    // from the tree, so a fifth crate that grows one fails here rather than going unverified.
    let mut unsafe_crates: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in walk(&this_repository().join("crates")) {
        let Some(name) = entry.to_str() else { continue };
        if !name.ends_with(".rs") || name.contains("/tests/") || name.contains("/target/") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&entry) else {
            continue;
        };
        if !text.lines().any(|line| {
            let trimmed = line.trim_start();
            (trimmed.starts_with("unsafe ") || trimmed.contains("unsafe {"))
                && !trimmed.starts_with("//")
        }) {
            continue;
        }
        let relative = name
            .strip_prefix(this_repository().to_str().unwrap_or_default())
            .unwrap_or(name);
        if let Some(crate_name) = relative.trim_start_matches('/').split('/').nth(1) {
            unsafe_crates.insert(crate_name.to_owned());
        }
    }
    assert!(
        !unsafe_crates.is_empty(),
        "the tree holds unsafe code somewhere, or this test is reading nothing"
    );
    for crate_name in &unsafe_crates {
        assert!(
            verification.contains(&format!("--package {crate_name}")),
            "§42.1, §42.3: `{crate_name}` holds an `unsafe` block and the sanitizer job does not \
             run it. The whole argument of §42.1 is that the boundary is small enough to verify; \
             a crate outside the job is a crate outside the argument. Found: {unsafe_crates:?}"
        );
    }
}

/// Every file under `directory`, recursively.
fn walk(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

// --- build once, promote after proof (spec §49, ADR-0532) ---------------------------------------

#[test]
fn should_promote_an_already_tested_artifact_rather_than_rebuilding_it() {
    // §49.1's pipeline ends with "publish the already-tested bytes", and §48.4 spells out what
    // that forbids: the workflow MUST NOT rebuild packages after tests and then upload the
    // untested rebuild. The publishing job is therefore not allowed to build anything.
    let workflow = read(".github/workflows/release.yml");
    let publish = workflow_job(&workflow, "publish");

    for builder in [
        "scripts/package.sh",
        "cargo build",
        "cargo deb",
        "cargo generate-rpm",
        "cross build",
    ] {
        assert!(
            !publish.contains(builder),
            "the publishing job runs `{builder}`, so what it attaches is a rebuild rather than \
             the artifact package validation installed (spec §48.4, §49.1):\n{publish}"
        );
    }
    assert!(
        publish.contains("download-artifact"),
        "the publishing job does not take its artifacts from the job that proved them:\n{publish}"
    );
    let package = workflow_job(&workflow, "package");
    assert!(
        package.contains("scripts/package.sh") && package.contains("scripts/package-check.sh"),
        "the job that produces the artifacts is not the job that proves them, so `build once` is \
         not what the pipeline does (spec §49.1):\n{package}"
    );

    // §49.2: no hidden local step. Everything the release does is a script in this repository or
    // a pinned action, started by a tag push — nothing waits for a maintainer to run something.
    assert!(
        workflow.contains("tags:"),
        "the release is not started by a tag push, so something outside the repository starts it \
         (spec §49.2):\n{workflow}"
    );
    for step in ["run: scripts/", "run: repository/scripts/", "uses:"] {
        assert!(
            workflow.contains(step),
            "the release workflow has no `{step}` steps at all, which cannot be right"
        );
    }

    // §49.3: a final tag reruns the complete check even when an earlier release candidate
    // passed, so no step that proves anything may be skipped for some tags and not others.
    for proof in ["package-check.sh", "rebuild-check.sh", "verify-release.sh"] {
        let line = workflow
            .lines()
            .find(|line| line.contains(proof))
            .unwrap_or_else(|| panic!("the release workflow runs `{proof}`"));
        assert!(
            !line.contains("if:") && !line.contains("prerelease"),
            "`{proof}` runs conditionally, so a final publication can inherit a release \
             candidate's result instead of rerunning the check (spec §49.3): {line}"
        );
    }
}

#[test]
fn should_publish_the_release_only_after_the_asset_inventory_verifies() {
    // §49.4: "A failed publishing step SHOULD avoid leaving a partially populated final GitHub
    // release that appears complete. The workflow MAY create a draft release, upload everything,
    // verify asset inventory and only then publish it." Four steps whose order is the whole
    // point — a release that is visible before its inventory is checked is the failure this
    // guards against.
    let script = read("scripts/publish-release.sh");
    let at = |needle: &str| -> usize {
        script.find(needle).unwrap_or_else(|| {
            panic!("`scripts/publish-release.sh` never does `{needle}`:\n{script}")
        })
    };
    let verified = at("verify-release.sh");
    let drafted = at("--draft");
    let uploaded = at("release upload");
    let inventory = at("asset inventory");
    let published = at("--draft=false");
    assert!(
        verified < drafted && drafted < uploaded && uploaded < inventory && inventory < published,
        "publication does not verify, then draft, then upload, then check the inventory, then \
         publish. Out of that order a failure leaves a release that looks complete (spec \
         §49.4):\n{script}"
    );

    // The inventory check is a comparison against the digests, not a count of files: an asset
    // uploaded truncated has the right name and the wrong bytes.
    assert!(
        script.contains("sha256sum") || script.contains("SHA256SUMS"),
        "the asset inventory is not checked by digest, so a truncated upload passes it (spec \
         §49.4, §62.6):\n{script}"
    );

    // And the workflow publishes through it rather than attaching assets directly.
    let workflow = read(".github/workflows/release.yml");
    let publish = workflow_job(&workflow, "publish");
    assert!(
        publish.contains("publish-release.sh"),
        "the publishing job attaches assets without the draft-upload-verify-publish sequence \
         (spec §49.4):\n{publish}"
    );
}
