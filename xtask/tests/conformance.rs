//! The generated provider conformance suite of spec §35.3.
//!
//! "Every provider capability gets a generated conformance suite from registry metadata." This
//! is the generator's own test: what it emits has to follow from the declarations, and it has to
//! refuse rather than emit a suite that leaves something a provider advertises unexercised.
//!
//! The last test is the one that keeps the repository honest: it regenerates this workspace's
//! suite and requires the committed file to be identical.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a \
              test does"
)]

use std::path::Path;

use ono_testkit::{Scratch, scratch};
use xtask::conformance::{check_committed, generate};

/// The workspace root.
fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
        .to_path_buf()
}

/// A minimal but complete set of registries: one provider, one schema, one command.
fn registries() -> Scratch {
    let repo = scratch();
    repo.write("docs/spec/capabilities.yaml", CAPABILITIES);
    repo.write("docs/spec/schemas/process.v1.yaml", PROCESS_SCHEMA);
    repo.write("docs/spec/commands/process.yaml", PROCESS_COMMANDS);
    repo.write("docs/spec/providers/linux-procfs.yaml", PROCFS_PROVIDER);
    repo
}

const CAPABILITIES: &str = r"version: 1
provider_capabilities:
  - id: process.list
    summary: Enumerate processes.
    risk: read
    elevation: none
  - id: process.signal
    summary: Signal a process.
    risk: destructive
    elevation: conditional
kuang_capabilities: []
";

const PROCESS_SCHEMA: &str = r"id: ono.process/1
name: Process
summary: A running process.
identity: [pid, started]
fields:
  pid:
    type: int
    required: true
    doc: The process id.
  cpu:
    type: float
    unit: percent
    nullable: true
    doc: Recent CPU share.
default_view:
  columns: [pid, cpu]
";

const PROCESS_COMMANDS: &str = r#"version: 1
family: process
commands:
  - id: ono.process.kill
    verb: kill
    target: process
    summary: Signal a process.
    stability: stable
    argument_mode: words
    input: "null"
    output: stream<ono.action-result/1>
    provider_capability: process.signal
    privilege: conditional
    streaming: true
    phase: C
    examples: ["kill process 1"]
"#;

const PROCFS_PROVIDER: &str = r"providers:
  - id: linux.procfs
    doc: Processes, from /proc.
    targets: [process, signal]
    capabilities: [process.list, process.signal]
    schemas: [ono.process/1]
    conformance:
      process: enumerable
      signal: enumerable
";

#[test]
fn should_write_one_suite_beside_the_shell_when_generated() {
    let repo = registries();
    let pages = generate(repo.path()).expect("generation must succeed");
    let paths: Vec<&str> = pages.iter().map(|page| page.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["crates/ono-cli/tests/provider_conformance.rs"],
        "the suite is one file, and it lives where the shell's own tests do"
    );
}

#[test]
fn should_exercise_every_target_a_provider_declares_when_generated() {
    let repo = registries();
    let suite = generate(repo.path()).expect("generation must succeed")[0]
        .contents
        .clone();
    for target in ["process", "signal"] {
        assert!(
            suite.contains(&format!("target: \"{target}\"")),
            "a target a provider serves must reach the suite; `{target}` did not:\n{suite}"
        );
    }
}

#[test]
fn should_carry_the_declared_field_contract_into_the_generated_case() {
    let repo = registries();
    let suite = generate(repo.path()).expect("generation must succeed")[0]
        .contents
        .clone();
    assert!(
        suite.contains(r#"name: "pid", ty: "int", required: true, nullable: false"#),
        "the field contract is the declaration's, restated where a test can hold the code to \
         it:\n{suite}"
    );
    assert!(
        suite.contains(r#"unit: Some("percent")"#),
        "a unit is part of the contract: the same number means another thing in another unit \
         (spec §10.6):\n{suite}"
    );
    assert!(
        suite.contains(r#"identity: &["pid", "started"]"#),
        "identity is what spec §35.3 exercises first:\n{suite}"
    );
}

#[test]
fn should_account_for_every_capability_a_provider_declares() {
    let repo = registries();
    let suite = generate(repo.path()).expect("generation must succeed")[0]
        .contents
        .clone();
    assert!(
        suite.contains(r#"capability: "process.list", risk: "read""#),
        "a read capability is exercised by the snapshot of the target it reads:\n{suite}"
    );
    assert!(
        suite.contains(r#"capability: "process.signal""#),
        "a capability that changes the world is accounted for by the command that reaches \
         it:\n{suite}"
    );
    assert!(
        suite.contains("ono.process.kill"),
        "the account names the command, so it can be held to being implemented:\n{suite}"
    );
}

#[test]
fn should_refuse_to_generate_when_a_target_has_no_declared_exercise() {
    let repo = registries();
    repo.write(
        "docs/spec/providers/linux-procfs.yaml",
        PROCFS_PROVIDER.replace("      signal: enumerable\n", ""),
    );
    let error = generate(repo.path()).expect_err("an unexercised target must stop generation");
    assert!(
        error.detail.contains("signal"),
        "the refusal names the target nothing would exercise: {}",
        error.detail
    );
}

#[test]
fn should_refuse_to_generate_when_a_capability_reaches_neither_a_snapshot_nor_a_command() {
    let repo = registries();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands: []\n",
    );
    let error = generate(repo.path()).expect_err("an unaccounted capability must stop generation");
    assert!(
        error.detail.contains("process.signal"),
        "the refusal names the capability nothing would exercise: {}",
        error.detail
    );
}

#[test]
fn should_refuse_to_generate_when_an_exercise_names_a_target_the_provider_does_not_serve() {
    let repo = registries();
    repo.write(
        "docs/spec/providers/linux-procfs.yaml",
        PROCFS_PROVIDER.replace(
            "      signal: enumerable\n",
            "      signal: enumerable\n      pipe: enumerable\n",
        ),
    );
    let error = generate(repo.path()).expect_err("an invented target must stop generation");
    assert!(
        error.detail.contains("pipe"),
        "the refusal names the target that is not served: {}",
        error.detail
    );
}

#[test]
fn should_refuse_to_generate_when_an_exercise_is_not_one_the_harness_knows() {
    let repo = registries();
    repo.write(
        "docs/spec/providers/linux-procfs.yaml",
        PROCFS_PROVIDER.replace("      process: enumerable\n", "      process: whenever\n"),
    );
    let error = generate(repo.path()).expect_err("an unknown exercise must stop generation");
    assert!(
        error.detail.contains("whenever"),
        "the refusal names the word nobody implements: {}",
        error.detail
    );
}

#[test]
fn should_match_the_committed_suite_of_this_repository() {
    let problems = check_committed(&repo());
    assert!(
        problems.is_empty(),
        "the committed conformance suite must be what the declarations produce; run `cargo \
         xtask conformance`: {problems:?}"
    );
}
