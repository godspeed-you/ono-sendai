//! The gate's supply-chain pinning rules (spec §43, §44, §62.1, §62.2).
//!
//! Every one of these rules exists because a reference that resolves differently tomorrow is a
//! build nobody can reproduce and an attacker nobody can see. They decide whether "the release
//! was built from this commit" means anything, so they are tested against fixtures rather than
//! trusted.

#![allow(
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::path::Path;

use ono_testkit::{Scratch, scratch};
use xtask::supply_chain::{check_action_pins, check_image_digests, check_workflow_permissions};

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

fn report(problems: &[xtask::scan::Problem]) -> String {
    problems
        .iter()
        .map(|problem| format!("  {} — {}", problem.location, problem.detail))
        .collect::<Vec<_>>()
        .join("\n")
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
