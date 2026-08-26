//! `spec-check` is the contract-drift referee of spec §36.5. These tests fix what it must catch,
//! against fixture registries, and then require this repository's own registries to pass.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::path::Path;

use ono_testkit::{Scratch, scratch};
use xtask::contracts::check_contracts;

/// A minimal but internally consistent set of registries.
fn consistent() -> Scratch {
    let repo = scratch();
    repo.write(
        "docs/spec/verbs.yaml",
        "version: 1\nverbs:\n  - id: ono.verb.get\n    verb: get\n    semantics: obtain state\n    pipeline_role: producer\n    mutating: false\n    stability: stable\n",
    );
    repo.write(
        "docs/spec/targets.yaml",
        "version: 1\ntargets:\n  - id: ono.target.process\n    name: process\n    category: system\n    summary: A process.\n    schema: ono.process/1\n    phase: C\n",
    );
    repo.write(
        "docs/spec/capabilities.yaml",
        "version: 1\nprovider_capabilities:\n  - id: process.list\n    summary: Enumerate processes.\n    risk: read\n    elevation: none\nkuang_capabilities:\n  - id: object.read\n    summary: Read objects.\n    risk: read\n    elevation: none\n",
    );
    repo.write(
        "docs/spec/schemas/process.v1.yaml",
        "id: ono.process/1\nname: Process\nsummary: A process.\nidentity: [pid]\nfields:\n  pid:\n    type: int\n    required: true\n    doc: The process id.\ndefault_view:\n  columns: [pid]\n",
    );
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    target: process\n    summary: Enumerate processes.\n    stability: stable\n    argument_mode: words\n    input: \"null\"\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    streaming: true\n    phase: C\n    examples: [\"get process\"]\n",
    );
    repo.write(
        "docs/spec/language.yaml",
        "version: 1\nargument_modes:\n  expression_heads: [where, select]\n  default: words\n",
    );
    repo
}

fn problems(repo: &Scratch) -> Vec<String> {
    check_contracts(repo.path())
        .into_iter()
        .map(|problem| format!("{} — {}", problem.location, problem.detail))
        .collect()
}

#[test]
fn should_accept_registries_that_agree_with_each_other_when_checked() {
    assert_eq!(problems(&consistent()), Vec::<String>::new());
}

#[test]
fn should_reject_a_command_naming_a_verb_no_registry_defines() {
    let repo = consistent();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.frobnicate\n    verb: frobnicate\n    target: process\n    summary: x\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    phase: C\n    examples: [\"frobnicate process\"]\n",
    );
    let found = problems(&repo);
    assert!(
        found
            .iter()
            .any(|p| p.contains("frobnicate") && p.contains("verbs.yaml")),
        "an undefined verb must be reported, got {found:?}"
    );
}

#[test]
fn should_reject_a_command_naming_a_target_no_registry_defines() {
    let repo = consistent();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    target: widget\n    summary: x\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    phase: C\n    examples: [\"get widget\"]\n",
    );
    assert!(problems(&repo).iter().any(|p| p.contains("widget")));
}

#[test]
fn should_reject_a_command_naming_a_capability_no_registry_defines() {
    let repo = consistent();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    target: process\n    summary: x\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    provider_capability: process.telepathy\n    privilege: none\n    phase: C\n    examples: [\"get process\"]\n",
    );
    assert!(
        problems(&repo)
            .iter()
            .any(|p| p.contains("process.telepathy"))
    );
}

#[test]
fn should_reject_a_stable_command_whose_output_schema_does_not_exist() {
    // Spec §36.5: metadata that promises a schema nobody wrote is exactly the drift this catches.
    let repo = consistent();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    target: process\n    summary: x\n    stability: stable\n    argument_mode: words\n    output: stream<ono.ghost/1>\n    provider_capability: process.list\n    privilege: none\n    phase: C\n    examples: [\"get process\"]\n",
    );
    assert!(problems(&repo).iter().any(|p| p.contains("ono.ghost/1")));
}

#[test]
fn should_allow_a_planned_command_to_reference_a_schema_its_phase_has_not_written_yet() {
    // A registry describes the whole product, not the part that exists today (ADR-0012). A
    // `planned` entry pointing at a schema a later phase will write is the intended state, not
    // drift — otherwise the registry could only ever describe the past.
    let repo = consistent();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    target: process\n    summary: x\n    stability: planned\n    argument_mode: words\n    output: stream<ono.ghost/1>\n    provider_capability: process.list\n    privilege: none\n    phase: planned\n    examples: [\"get process\"]\n",
    );
    assert_eq!(problems(&repo), Vec::<String>::new());
}

#[test]
fn should_reject_an_argument_mode_that_disagrees_with_the_grammar() {
    // ADR-0009 fixes which heads take expressions. A registry that says otherwise would make
    // completion and help describe a language the parser does not implement.
    let repo = consistent();
    repo.write(
        "docs/spec/commands/data.yaml",
        "version: 1\nfamily: data\ncommands:\n  - id: ono.data.where\n    verb: where\n    target: value\n    summary: Filter.\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    phase: B\n    examples: [\"where cpu > 20\"]\n",
    );
    let found = problems(&repo);
    assert!(
        found
            .iter()
            .any(|p| p.contains("argument_mode") && p.contains("where")),
        "got {found:?}"
    );
}

#[test]
fn should_reject_two_commands_claiming_the_same_stable_identity() {
    let repo = consistent();
    repo.write(
        "docs/spec/commands/other.yaml",
        "version: 1\nfamily: other\ncommands:\n  - id: ono.process.get\n    verb: get\n    target: process\n    summary: A second claim.\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    phase: C\n    examples: [\"get process\"]\n",
    );
    assert!(
        problems(&repo)
            .iter()
            .any(|p| p.contains("ono.process.get"))
    );
}

#[test]
fn should_reject_a_schema_whose_identity_names_a_field_it_does_not_have() {
    let repo = consistent();
    repo.write(
        "docs/spec/schemas/process.v1.yaml",
        "id: ono.process/1\nname: Process\nsummary: x\nidentity: [pid, started]\nfields:\n  pid:\n    type: int\n    required: true\n    doc: The pid.\ndefault_view:\n  columns: [pid]\n",
    );
    assert!(problems(&repo).iter().any(|p| p.contains("started")));
}

#[test]
fn should_reject_a_schema_field_without_documentation() {
    // Spec §50 requires complete help for every advertised capability, and help is generated
    // from here. An undocumented field is a help page with a blank in it.
    let repo = consistent();
    repo.write(
        "docs/spec/schemas/process.v1.yaml",
        "id: ono.process/1\nname: Process\nsummary: x\nidentity: [pid]\nfields:\n  pid:\n    type: int\n    required: true\ndefault_view:\n  columns: [pid]\n",
    );
    assert!(problems(&repo).iter().any(|p| p.contains("doc")));
}

#[test]
fn should_reject_a_schema_file_whose_declared_id_does_not_match_its_name() {
    let repo = consistent();
    repo.write(
        "docs/spec/schemas/process.v1.yaml",
        "id: ono.mismatch/2\nname: Process\nsummary: x\nidentity: [pid]\nfields:\n  pid:\n    type: int\n    required: true\n    doc: The pid.\ndefault_view:\n  columns: [pid]\n",
    );
    assert!(problems(&repo).iter().any(|p| p.contains("ono.mismatch/2")));
}

#[test]
fn should_reject_a_command_that_advertises_no_example() {
    // Spec §50: "examples in docs are executable". A command with none cannot meet that bar.
    let repo = consistent();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    target: process\n    summary: x\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    phase: C\n    examples: []\n",
    );
    assert!(problems(&repo).iter().any(|p| p.contains("example")));
}

#[test]
fn should_report_a_file_that_is_not_valid_yaml_rather_than_ignoring_it() {
    let repo = consistent();
    repo.write(
        "docs/spec/commands/broken.yaml",
        "commands: [ this is not\n",
    );
    assert!(problems(&repo).iter().any(|p| p.contains("broken.yaml")));
}

#[test]
fn should_match_the_error_registry_against_the_implementation_when_both_exist() {
    // Spec §36.5's first drift case, in the one place it can be checked exactly today: the error
    // taxonomy exists in both `docs/spec/errors.yaml` and `crates/ono-core/src/error.rs`.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let found = xtask::contracts::check_error_registry(root);
    assert!(
        found.is_empty(),
        "the error contract and the implementation disagree:\n{}",
        found
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn should_report_this_repositorys_own_registries_as_consistent_when_checked() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let found = check_contracts(root);
    assert!(
        found.is_empty(),
        "the committed contracts do not agree with each other:\n{}",
        found
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
