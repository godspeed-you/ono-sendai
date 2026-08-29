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
        "version: 1\nargument_modes:\n  - name: words\n    default: true\n  - name: expression\n    heads: [where, select]\n    option_values:\n      - head: find\n        option: where\n",
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
fn should_reject_an_argument_mode_that_disagrees_with_the_grammar_this_repository_declares() {
    // A check whose input is empty cannot fail. `docs/spec/language.yaml` writes `argument_modes`
    // as a sequence of modes, each naming its own `heads`; a reader that expects one mapping with
    // an `expression_heads` key finds nothing there and waves every command through. So the
    // fixture carries this repository's own `language.yaml` verbatim, and the disagreement it
    // must catch is the same one the fixture-shaped test above catches.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let language = std::fs::read_to_string(root.join("docs").join("spec").join("language.yaml"))
        .expect("this repository declares its argument modes");
    let repo = consistent();
    repo.write("docs/spec/language.yaml", &language);
    repo.write(
        "docs/spec/commands/data.yaml",
        "version: 1\nfamily: data\ncommands:\n  - id: ono.data.where\n    verb: where\n    target: value\n    summary: Filter.\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    phase: B\n    examples: [\"where cpu > 20\"]\n",
    );
    let found = problems(&repo);
    assert!(
        found
            .iter()
            .any(|p| p.contains("argument_mode") && p.contains("where")),
        "the argument-mode check is blind to the registry's own shape, got {found:?}"
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

#[test]
fn should_report_a_documented_example_that_no_longer_parses() {
    // Spec §36.5's third drift case, and spec §50's requirement that documented examples be
    // executable. An example nobody runs is documentation that has quietly become fiction.
    let repo = consistent();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    target: process\n    summary: x\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    phase: C\n    examples: [\"get process | where )\"]\n",
    );
    let found = xtask::contracts::check_examples(repo.path());
    assert!(
        found
            .iter()
            .any(|problem| problem.detail.contains("does not parse")),
        "got {found:?}"
    );
}

#[test]
fn should_accept_an_example_that_parses_cleanly() {
    let repo = consistent();
    assert_eq!(xtask::contracts::check_examples(repo.path()), Vec::new());
}

#[test]
fn should_find_every_documented_example_in_this_repository_parseable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let found = xtask::contracts::check_examples(root);
    assert!(
        found.is_empty(),
        "documented examples that do not parse:\n{}",
        found
            .iter()
            .map(|problem| format!("  {} — {}", problem.location, problem.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn should_reject_an_adapter_pack_the_binary_does_not_bundle_or_that_names_an_unknown_schema() {
    // Spec v0.3 §1.44: the pack format is machine-validated, and a pack file the shell does not
    // bundle promises something nobody keeps (ADR-0055).
    let repo = consistent();
    repo.write(
        "docs/spec/adapters/first-party/example.yaml",
        "format: ono-adapter-pack/1\npackage:\n  id: org.ono.compat.example\n  name: Example\n  version: 0.1.0\n  publisher: org.ono\n  tier: first-party\nroles: [adapter]\ncapabilities:\n  process.exec:\n    executables: [example]\n    argv_policy: declared-invocations-only\nadapters:\n  - id: example\n    summary: An example.\n    executable:\n      names: [example]\n      versions: any\n    tier: A\n    output_demand: [structured]\n    fallback: raw\n    schema: ono.nonesuch/1\n    decoder:\n      kind: json\n    fields: {}\n    invocations:\n      - id: list\n        summary: example\n        match:\n          words: [[]]\n          flags:\n            allow: []\n            allow_with_value: []\n          positionals: forbid\n        plan:\n          argv: [example]\n          append_user_flags: false\n          env: {}\n          stdin: \"null\"\n    limits: []\n    fixtures: example\n",
    );
    let found = problems(&repo);
    assert!(
        found.iter().any(|p| p.contains("not bundled")),
        "an unbundled pack is reported, got {found:?}"
    );
    assert!(
        found.iter().any(|p| p.contains("ono.nonesuch/1")),
        "an unregistered schema is reported, got {found:?}"
    );
    assert!(
        found.iter().any(|p| p.contains("fixture directory")),
        "a missing fixture directory is reported, got {found:?}"
    );
}

/// The four registry documents of spec v0.4 §41, internally consistent.
fn spatial(repo: &Scratch) {
    repo.write(
        "docs/spec/spatial/spatial.yaml",
        "version: 1\nobject_types:\n  aggregates: [System, Compute]\n  objects: [Process, Socket]\n\
         \nconfidence: [exact, inferred]\ndirections: [outbound, bidirectional]\n\
         cost_classes:\n  - name: cheap\n    doc: Local.\nsettings:\n  - key: spatial.landmarks.high_cpu\n    type: int\n    default: 80\n    doc: The threshold.\n",
    );
    repo.write(
        "docs/spec/spatial/spaces.yaml",
        "version: 1\nspaces:\n  - id: system\n    label: SYSTEM\n    parent: null\n    object_type: System\n    schema: ono.process/1\n    enterable: true\n    commands: [look]\n    summary_fields: [hostname]\n\
         \n  - id: compute\n    label: COMPUTE\n    parent: system\n    object_type: Compute\n    enterable: true\n    commands: [look]\n    summary_fields: [process_count]\n",
    );
    repo.write(
        "docs/spec/spatial/relations.yaml",
        "version: 1\nrelations:\n  - id: process.owns_socket\n    source: Process\n    target: Socket\n    direction: outbound\n    canonical_label: socket\n    inverse_label: owner\n    canonical_group: sockets\n    inverse_group: process\n    confidence: exact\n    cost_class: cheap\n",
    );
    let reasons = [
        "high_cpu",
        "high_memory",
        "failed",
        "restarting",
        "recently_changed",
        "public_listener",
        "privileged",
        "storage_pressure",
        "connection_spike",
        "new_object",
        "removed_object",
        "security_boundary",
        "remote_boundary",
        "user_pinned",
    ];
    let mut document = String::from("version: 1\nlandmarks:\n");
    for reason in reasons {
        document.push_str(&format!(
            "  - reason: {reason}\n    domain: compute\n    evidence: Observed.\n    threshold: null\n    severity: notice\n"
        ));
    }
    repo.write("docs/spec/spatial/landmarks.yaml", &document);
}

/// The registry-internal problems of a fixture repository.
///
/// The fixture declares a small world of its own, so only the checks that hold the four
/// documents against *each other* apply to it; the drift check against `ono-spatial-core` is
/// exercised against this repository's own registry below, exactly as the error registry is.
fn spatial_problems(repo: &Scratch) -> Vec<String> {
    xtask::contracts::check_spatial_registry(repo.path())
        .into_iter()
        .map(|problem| format!("{} — {}", problem.location, problem.detail))
        .collect()
}

#[test]
fn should_accept_a_spatial_registry_whose_four_documents_agree() {
    let repo = consistent();
    spatial(&repo);
    assert_eq!(spatial_problems(&repo), Vec::<String>::new());
}

#[test]
fn should_match_the_spatial_registry_against_the_implementation_that_serves_it() {
    // Spec v0.4 §41's Intent: without machine contracts the renderer, the providers, the parser
    // and the documentation drift into different definitions of the world. This is the half of
    // the check that holds `docs/spec/spatial/` against `ono-spatial-core` in both directions.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let found = xtask::contracts::check_spatial_implementation(root);
    assert!(
        found.is_empty(),
        "the spatial contract and the implementation disagree:\n{}",
        found
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn should_reject_a_space_whose_canonical_parent_is_not_a_declared_space() {
    // Spec v0.4 §11.3: the canonical parent must be deterministic, which starts with it
    // existing. A dangling parent makes `up` arrive nowhere.
    let repo = consistent();
    spatial(&repo);
    repo.write(
        "docs/spec/spatial/spaces.yaml",
        "version: 1\nspaces:\n  - id: system\n    label: SYSTEM\n    parent: null\n    object_type: System\n    enterable: true\n    commands: [look]\n    summary_fields: [hostname]\n\
         \n  - id: compute\n    label: COMPUTE\n    parent: nowhere\n    object_type: Compute\n    enterable: true\n    commands: [look]\n    summary_fields: [process_count]\n",
    );
    assert!(
        spatial_problems(&repo)
            .iter()
            .any(|problem| problem.contains("nowhere")),
        "a dangling canonical parent must be reported, got {:?}",
        spatial_problems(&repo)
    );
}

#[test]
fn should_reject_a_space_or_relation_naming_a_type_the_vocabulary_does_not_hold() {
    // §41.3 generates one SDK enum from these documents; a type only one of them knows is the
    // drift §41's Intent exists to prevent.
    let repo = consistent();
    spatial(&repo);
    repo.write(
        "docs/spec/spatial/relations.yaml",
        "version: 1\nrelations:\n  - id: process.owns_socket\n    source: Process\n    target: Unicorn\n    direction: outbound\n    canonical_label: socket\n    inverse_label: owner\n    canonical_group: sockets\n    inverse_group: process\n    confidence: exact\n    cost_class: cheap\n",
    );
    assert!(
        spatial_problems(&repo)
            .iter()
            .any(|problem| problem.contains("Unicorn")),
        "an edge end outside the object-type vocabulary must be reported, got {:?}",
        spatial_problems(&repo)
    );
}

#[test]
fn should_reject_a_relation_whose_confidence_is_outside_the_specified_vocabulary() {
    // Spec v0.4 §11.5 fixes the vocabulary; a relation that invents a sixth value would let a
    // map claim a certainty the model cannot express.
    let repo = consistent();
    spatial(&repo);
    repo.write(
        "docs/spec/spatial/relations.yaml",
        "version: 1\nrelations:\n  - id: process.owns_socket\n    source: Process\n    target: Socket\n    direction: outbound\n    canonical_label: socket\n    inverse_label: owner\n    confidence: probably\n    cost_class: cheap\n",
    );
    assert!(
        spatial_problems(&repo)
            .iter()
            .any(|problem| problem.contains("probably")),
        "a confidence outside §11.5 must be reported, got {:?}",
        spatial_problems(&repo)
    );
}

#[test]
fn should_reject_a_landmark_registry_missing_one_of_the_fourteen_required_reasons() {
    // Spec v0.4 §3.7: "Built-in landmark reasons MUST include" — a reason the engine cannot name
    // is a highlight with no explanation.
    let repo = consistent();
    spatial(&repo);
    repo.write(
        "docs/spec/spatial/landmarks.yaml",
        "version: 1\nlandmarks:\n  - reason: high_cpu\n    domain: compute\n    evidence: Observed.\n    threshold: null\n    severity: notice\n",
    );
    assert!(
        spatial_problems(&repo)
            .iter()
            .any(|problem| problem.contains("user_pinned")),
        "a missing built-in reason must be reported, got {:?}",
        spatial_problems(&repo)
    );
}

#[test]
fn should_reject_a_landmark_threshold_that_disagrees_with_the_setting_that_configures_it() {
    // Spec v0.4 §26.3: thresholds are inspectable and configurable. Two defaults for one
    // threshold means the registry and the shell disagree about when a landmark fires.
    let repo = consistent();
    spatial(&repo);
    repo.write(
        "docs/spec/spatial/landmarks.yaml",
        "version: 1\nlandmarks:\n  - reason: high_cpu\n    domain: compute\n    evidence: The CPU share.\n    threshold:\n      metric: cpu_percent\n      comparison: at_or_above\n      default: 55\n      setting: spatial.landmarks.high_cpu\n    severity: notice\n",
    );
    assert!(
        spatial_problems(&repo)
            .iter()
            .any(|problem| problem.contains("two defaults")),
        "a threshold default that disagrees with its setting must be reported, got {:?}",
        spatial_problems(&repo)
    );
}

// --- fields a command may not omit (B-harn-1) ---------------------------------------------------

#[test]
fn should_reject_a_command_that_declares_no_verb() {
    // Before this rule the cross-check read `!verb.is_empty() && !verbs.contains(&verb)`, so a
    // command with no `verb` was checked against nothing and passed. ADR-0124 makes the point
    // sharp: the spatial commands take a bare name, and `look` is a verb of `verbs.yaml` exactly
    // like `get` — the bare spelling is a fact about the parser, never a licence to leave the
    // registry silent about which verb a command is.
    let repo = consistent();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    target: process\n    summary: x\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    phase: C\n    examples: [\"get process\"]\n",
    );
    let found = problems(&repo);
    assert!(
        found
            .iter()
            .any(|p| p.contains("ono.process.get") && p.contains("`verb`")),
        "a command with no verb must be reported, got {found:?}"
    );
}

#[test]
fn should_reject_a_command_that_declares_no_target() {
    // A transform writes `target: null` and means it (spec §53, ADR-0012). Omitting the key
    // means nothing at all, and the difference has to be visible.
    let repo = consistent();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    summary: x\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    phase: C\n    examples: [\"get process\"]\n",
    );
    let found = problems(&repo);
    assert!(
        found
            .iter()
            .any(|p| p.contains("ono.process.get") && p.contains("`target`")),
        "a command with no target must be reported, got {found:?}"
    );
}

#[test]
fn should_accept_a_command_whose_target_is_explicitly_null() {
    let repo = consistent();
    repo.write(
        "docs/spec/commands/data.yaml",
        "version: 1\nfamily: data\ncommands:\n  - id: ono.data.get\n    verb: get\n    target: null\n    summary: x\n    stability: stable\n    argument_mode: words\n    output: stream<ono.process/1>\n    privilege: none\n    phase: B\n    examples: [\"get\"]\n",
    );
    let found = problems(&repo);
    assert!(
        !found.iter().any(|p| p.contains("`target`")),
        "`target: null` is a declaration, not an omission, got {found:?}"
    );
}

#[test]
fn should_reject_a_command_that_declares_no_argument_mode() {
    // ADR-0009 decides in which mode a head parses, and the check that the registry agrees with
    // the parser was skipped whenever the field was absent — which is the one case where the
    // registry says nothing and completion and help have to guess.
    let repo = consistent();
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    target: process\n    summary: x\n    stability: stable\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    privilege: none\n    phase: C\n    examples: [\"get process\"]\n",
    );
    let found = problems(&repo);
    assert!(
        found
            .iter()
            .any(|p| p.contains("ono.process.get") && p.contains("`argument_mode`")),
        "a command with no argument mode must be reported, got {found:?}"
    );
}

// --- declared options must be honoured (ADR-0233) ---------------------------------------------

/// The fixture registry's `get process`, with `options` spliced in.
fn with_options(repo: &Scratch, options: &str) {
    repo.write(
        "docs/spec/commands/process.yaml",
        format!(
            "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    \
             target: process\n    summary: Enumerate processes.\n    stability: stable\n    \
             argument_mode: words\n    input: \"null\"\n    output: stream<ono.process/1>\n    \
             provider_capability: process.list\n    options:\n{options}    privilege: none\n    \
             streaming: true\n    phase: C\n    examples: [\"get process\"]\n"
        ),
    );
}

#[test]
fn should_report_a_declared_option_no_implementation_names() {
    // An option a command advertises and no code ever names cannot be honoured: it is help text
    // for behaviour that does not exist, and the user finds out by the answer being wrong rather
    // than by being refused (ADR-0233).
    let repo = consistent();
    with_options(
        &repo,
        "      - {name: tree, type: bool, doc: \"The structure.\"}\n",
    );
    repo.write("crates/ono-demo/src/lib.rs", "pub fn nothing() {}\n");

    assert!(
        problems(&repo)
            .iter()
            .any(|problem| problem.contains("--tree") && problem.contains("ono.process.get")),
        "a declared option no source names must be reported, got {:?}",
        problems(&repo)
    );
}

#[test]
fn should_accept_a_declared_option_an_implementation_names() {
    let repo = consistent();
    with_options(
        &repo,
        "      - {name: tree, type: bool, doc: \"The structure.\"}\n",
    );
    repo.write(
        "crates/ono-demo/src/lib.rs",
        "pub fn nest(query: &str) -> bool { query == \"tree\" }\n",
    );

    assert!(
        !problems(&repo)
            .iter()
            .any(|problem| problem.contains("--tree")),
        "an option the sources name is honoured as far as a static check can tell, got {:?}",
        problems(&repo)
    );
}

#[test]
fn should_not_accept_an_option_named_only_by_a_test() {
    // A test naming an option proves the test knows about it, not that the shell does.
    let repo = consistent();
    with_options(
        &repo,
        "      - {name: tree, type: bool, doc: \"The structure.\"}\n",
    );
    repo.write("crates/ono-demo/src/lib.rs", "pub fn nothing() {}\n");
    repo.write(
        "crates/ono-demo/tests/demo.rs",
        "#[test]\nfn t() { assert_eq!(\"tree\", \"tree\"); }\n",
    );

    assert!(
        problems(&repo)
            .iter()
            .any(|problem| problem.contains("--tree")),
        "an option only a test names is still unhonoured, got {:?}",
        problems(&repo)
    );
}

#[test]
fn should_match_the_kuang_contracts_against_the_runtime_that_serves_them() {
    // Spec §36.5's drift rule, for the seven `docs/spec/kuang/` contracts. Every other registry
    // under `docs/spec/` is held against its implementation; these reached `spec-check` only
    // through the generic sweep, which proves they are non-empty valid YAML and nothing else.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let found = xtask::contracts::check_kuang_contracts(root);
    assert!(
        found.is_empty(),
        "the KUANG/11 contracts and the runtime disagree:\n{}",
        found
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn should_report_a_kuang_manifest_field_the_runtime_does_not_implement() {
    // The exit test of `docs/STATE.md`'s B-kuang-6: a manifest section that declares a field the
    // package parser would refuse is drift, and drift is what a contract check exists to find.
    let repo = consistent();
    copy_kuang_contracts(&repo);
    let path = repo.path().join("docs/spec/kuang/manifest.v1.yaml");
    let text = std::fs::read_to_string(&path).expect("the manifest contract");
    let text = text.replace(
        "      homepage:\n        type: string",
        "      hoempage:\n        type: string",
    );
    repo.write("docs/spec/kuang/manifest.v1.yaml", &text);
    let found = xtask::contracts::check_kuang_contracts(repo.path());
    assert!(
        found
            .iter()
            .any(|problem| problem.detail.contains("hoempage")),
        "a field the runtime does not implement is reported, got {found:?}"
    );
}

#[test]
fn should_report_a_kuang_capability_the_runtime_does_not_know() {
    let repo = consistent();
    copy_kuang_contracts(&repo);
    let path = repo.path().join("docs/spec/kuang/capabilities.v1.yaml");
    let text = std::fs::read_to_string(&path).expect("the capability contract");
    let text = text.replace("  - id: object.read", "  - id: object.reed");
    repo.write("docs/spec/kuang/capabilities.v1.yaml", &text);
    let found = xtask::contracts::check_kuang_contracts(repo.path());
    assert!(
        found
            .iter()
            .any(|problem| problem.detail.contains("object.reed")),
        "a capability family outside the runtime's model is reported, got {found:?}"
    );
}

/// Copies this repository's KUANG/11 contracts into a fixture, so a test can break exactly one
/// line of them and see what the check says.
fn copy_kuang_contracts(repo: &Scratch) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("docs/spec/kuang");
    for entry in std::fs::read_dir(&root).expect("the kuang contracts") {
        let path = entry.expect("a directory entry").path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path).expect("a contract");
        repo.write(format!("docs/spec/kuang/{name}"), &text);
    }
}
