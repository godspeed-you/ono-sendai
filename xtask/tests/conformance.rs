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

use ono_testkit::{Scratch, scratch};
use xtask::conformance::{check_committed, generate};

mod support;
use support::repo;

/// A minimal but complete set of registries: one provider, one schema, one command.
fn registries() -> Scratch {
    let repo = scratch();
    repo.write("docs/contracts/capabilities.yaml", CAPABILITIES);
    repo.write("docs/contracts/schemas/process.v1.yaml", PROCESS_SCHEMA);
    repo.write("docs/contracts/commands/process.yaml", PROCESS_COMMANDS);
    repo.write(
        "docs/contracts/providers/linux-procfs.yaml",
        PROCFS_PROVIDER,
    );
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
fn should_write_the_provider_and_the_command_suite_beside_the_shell_when_generated() {
    let repo = registries();
    let pages = generate(repo.path()).expect("generation must succeed");
    let paths: Vec<&str> = pages.iter().map(|page| page.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "crates/ono-cli/tests/provider_conformance.rs",
            "crates/ono-cli/tests/command_conformance.rs",
        ],
        "the providers' suite and the commands' suite (issue #149) live where the shell's own \
         tests do, and share one harness"
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
        "docs/contracts/providers/linux-procfs.yaml",
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
        "docs/contracts/commands/process.yaml",
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
        "docs/contracts/providers/linux-procfs.yaml",
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
        "docs/contracts/providers/linux-procfs.yaml",
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

// --- issue #149: every command produces what it declares ---------------------------------------

/// The registries plus what the command half reads: verbs, a read-only command with two
/// examples, and a register of exemptions.
fn command_registries() -> Scratch {
    let repo = registries();
    repo.write("docs/contracts/verbs.yaml", VERBS);
    repo.write(
        "docs/contracts/commands/process.yaml",
        format!("{PROCESS_COMMANDS}{GET_PROCESS}"),
    );
    repo
}

const VERBS: &str = r"version: 1
verbs:
  - verb: get
    mutating: false
    pipeline_role: producer
  - verb: kill
    mutating: true
    pipeline_role: consumer
  - verb: watch
    mutating: false
    pipeline_role: stream producer
";

const GET_PROCESS: &str = r#"  - id: ono.process.get
    verb: get
    target: process
    summary: Enumerate processes.
    stability: stable
    argument_mode: words
    input: "null"
    output: stream<ono.process/1>
    provider_capability: process.list
    privilege: none
    streaming: true
    phase: C
    examples: ["get process", "get process 4419"]
"#;

fn command_suite(repo: &Scratch) -> String {
    generate(repo.path()).expect("generation must succeed")[1]
        .contents
        .clone()
}

#[test]
fn should_run_an_example_its_contracts_let_run_against_the_declared_output() {
    let suite = command_suite(&command_registries());
    assert!(
        suite.contains(r#"example: "get process","#)
            && suite.contains(r#"output: "stream<ono.process/1>","#),
        "a read-only, unprivileged example is held to the command's declared output:\n{suite}"
    );
}

#[test]
fn should_skip_an_example_whose_verb_mutates_and_say_why() {
    let suite = command_suite(&command_registries());
    assert!(
        !suite.contains(r#"example: "kill process 1","#),
        "an example that changes the system never runs in the gate:\n{suite}"
    );
    assert!(
        suite.contains("`ono.process.kill` `kill process 1` — runs `ono.process.kill`, whose verb `kill` changes the system"),
        "the suite says why it did not run it:\n{suite}"
    );
    assert!(
        suite.contains("- `ono.process.kill` —"),
        "a command none of whose examples runs is named at the head of the suite:\n{suite}"
    );
}

#[test]
fn should_skip_an_example_whose_output_schema_is_still_deferred() {
    let repo = command_registries();
    repo.write(
        "docs/contracts/schemas/deferred.yaml",
        "version: 1\ndeferred:\n  - id: ono.process/1\n    phase: C\n    required_by: [ono.process.get]\n",
    );
    let suite = command_suite(&repo);
    assert!(
        !suite.contains(r#"example: "get process","#) && suite.contains("deferred.yaml"),
        "a declared output nobody has written yet cannot be held to anything:\n{suite}"
    );
}

#[test]
fn should_carry_an_exempted_example_and_its_reason_into_the_suite() {
    let repo = command_registries();
    repo.write(
        "docs/contracts/conformance/command_examples.yaml",
        "version: 1\nexemptions:\n  - command: ono.process.get\n    example: get process 4419\n    reason: names a process a test host need not run.\n",
    );
    let suite = command_suite(&repo);
    assert!(
        !suite.contains(r#"example: "get process 4419","#)
            && suite.contains("exempt: names a process a test host need not run."),
        "an exempted example does not run, and the reason is in the suite:\n{suite}"
    );
}

#[test]
fn should_refuse_an_exemption_naming_an_example_no_command_documents() {
    let repo = command_registries();
    repo.write(
        "docs/contracts/conformance/command_examples.yaml",
        "version: 1\nexemptions:\n  - command: ono.process.get\n    example: get process 1\n    reason: stale.\n",
    );
    let error = generate(repo.path()).expect_err("a stale exemption must stop generation");
    assert!(
        error.detail.contains("get process 1") && error.detail.contains("no command documents"),
        "the refusal names the stale entry: {}",
        error.detail
    );
}

#[test]
fn should_refuse_an_exemption_for_an_example_the_contracts_already_skip() {
    let repo = command_registries();
    repo.write(
        "docs/contracts/conformance/command_examples.yaml",
        "version: 1\nexemptions:\n  - command: ono.process.kill\n    example: kill process 1\n    reason: redundant.\n",
    );
    let error = generate(repo.path()).expect_err("a redundant exemption must stop generation");
    assert!(
        error.detail.contains("kill process 1") && error.detail.contains("already skip"),
        "the refusal names the redundant entry: {}",
        error.detail
    );
}

#[test]
fn should_refuse_an_exemption_without_a_reason() {
    let repo = command_registries();
    repo.write(
        "docs/contracts/conformance/command_examples.yaml",
        "version: 1\nexemptions:\n  - command: ono.process.get\n    example: get process 4419\n",
    );
    let error =
        generate(repo.path()).expect_err("an exemption without a reason must stop generation");
    assert!(
        error.detail.contains("reason"),
        "the refusal says what is missing: {}",
        error.detail
    );
}

#[test]
fn should_report_a_committed_command_suite_that_drifted_from_the_contracts() {
    let repo = command_registries();
    for page in generate(repo.path()).expect("generation") {
        repo.write(&page.path, &page.contents);
    }
    assert_eq!(check_committed(repo.path()), Vec::new());
    repo.write(
        "docs/contracts/commands/process.yaml",
        format!(
            "{PROCESS_COMMANDS}{}",
            GET_PROCESS.replace("stream<ono.process/1>", "ono.process/1")
        ),
    );
    let problems = check_committed(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.location == "crates/ono-cli/tests/command_conformance.rs"),
        "a declaration that changed without the suite following is reported: {problems:?}"
    );
}
