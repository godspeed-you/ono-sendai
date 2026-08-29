//! Generated documentation must be reproducible from the registries, and what is committed must
//! match what the generator produces (`docs/ACCEPTANCE.md` §4.5, spec §36.2).
//!
//! The test that matters is the last one: it regenerates this repository's reference docs and
//! requires the committed files to be identical. A generator nobody checks against the tree is a
//! generator whose output has quietly diverged.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::path::Path;

use ono_testkit::{Scratch, scratch};
use xtask::reference::{check_committed, generate};

/// The workspace root.
fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
        .to_path_buf()
}

fn registries() -> Scratch {
    let repo = scratch();
    repo.write(
        "docs/spec/verbs.yaml",
        "version: 1\nverbs:\n  - id: ono.verb.get\n    verb: get\n    semantics: obtain current objects\n    typical_targets: [process, file]\n    pipeline_role: producer\n    mutating: false\n    stability: stable\n",
    );
    repo.write(
        "docs/spec/targets.yaml",
        "version: 1\ntargets:\n  - id: ono.target.process\n    name: process\n    category: system\n    summary: A running process.\n    schema: ono.process/1\n    phase: C\n",
    );
    repo.write(
        "docs/spec/errors.yaml",
        "version: 1\nerrors:\n  - code: Ono-Sendai-E0001\n    name: parse.syntax\n    kind: parse\n    summary: Not valid syntax.\n    help: Read the span.\n",
    );
    repo.write(
        "docs/spec/capabilities.yaml",
        "version: 1\nprovider_capabilities:\n  - id: process.list\n    summary: Enumerate processes.\n    risk: read\n    elevation: none\nkuang_capabilities: []\n",
    );
    repo.write(
        "docs/spec/schemas/process.v1.yaml",
        "id: ono.process/1\nname: Process\nsummary: A running process.\nidentity: [pid]\nfields:\n  pid:\n    type: int\n    required: true\n    doc: The process id.\n  cpu:\n    type: float\n    unit: percent\n    nullable: true\n    doc: Recent CPU share.\ndefault_view:\n  columns: [pid, cpu]\n",
    );
    repo.write(
        "docs/spec/commands/process.yaml",
        "version: 1\nfamily: process\ncommands:\n  - id: ono.process.get\n    verb: get\n    target: process\n    summary: Enumerate processes.\n    stability: stable\n    argument_mode: words\n    input: \"null\"\n    output: stream<ono.process/1>\n    provider_capability: process.list\n    selectors:\n      - name: pid\n        type: int\n        doc: Select one process.\n    options:\n      - name: tree\n        type: bool\n        doc: Render as a tree.\n    privilege: none\n    streaming: true\n    phase: C\n    examples: [\"get process\", \"get process | where cpu > 20\"]\n",
    );
    repo
}

#[test]
fn should_write_a_page_for_every_registry_when_generated() {
    let repo = registries();
    let written = generate(repo.path()).expect("generation must succeed");
    let names: Vec<&str> = written.iter().map(|page| page.path.as_str()).collect();
    for expected in [
        "docs/reference/README.md",
        "docs/reference/commands.md",
        "docs/reference/verbs.md",
        "docs/reference/targets.md",
        "docs/reference/errors.md",
        "docs/reference/schemas.md",
        "docs/reference/capabilities.md",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }
}

#[test]
fn should_mark_every_page_as_generated_so_nobody_edits_one_by_hand() {
    // AGENTS.md §2: docs/reference/ is generated and never hand-edited. Saying so on the page is
    // the difference between a rule and a rule someone knows about.
    for page in generate(registries().path()).expect("generation") {
        assert!(
            page.contents
                .lines()
                .take(3)
                .any(|line| line.contains("generated")),
            "{} does not say it is generated:\n{}",
            page.path,
            page.contents.lines().take(3).collect::<Vec<_>>().join("\n")
        );
    }
}

#[test]
fn should_carry_every_commands_contract_onto_its_page_when_generated() {
    let pages = generate(registries().path()).expect("generation");
    let commands = pages
        .iter()
        .find(|page| page.path.ends_with("commands.md"))
        .expect("a command reference");
    for expected in [
        "ono.process.get",
        "get process",
        "Enumerate processes.",
        "stream<ono.process/1>",
        "process.list",
        "--tree",
        "get process | where cpu > 20",
    ] {
        assert!(
            commands.contents.contains(expected),
            "{expected:?} missing from the command reference:\n{}",
            commands.contents
        );
    }
}

#[test]
fn should_carry_every_schema_field_and_its_documentation_onto_its_page_when_generated() {
    let pages = generate(registries().path()).expect("generation");
    let schemas = pages
        .iter()
        .find(|page| page.path.ends_with("schemas.md"))
        .expect("a schema reference");
    for expected in [
        "ono.process/1",
        "pid",
        "The process id.",
        "cpu",
        "Recent CPU share.",
        "percent",
    ] {
        assert!(
            schemas.contents.contains(expected),
            "{expected:?} missing:\n{}",
            schemas.contents
        );
    }
}

#[test]
fn should_say_which_field_may_be_unknown_so_null_stays_meaningful_when_generated() {
    // Spec §10.5: a nullable field is a promise that absence is information. A reference that
    // did not say which fields are nullable would hide the shell's most distinctive property.
    let pages = generate(registries().path()).expect("generation");
    let schemas = pages
        .iter()
        .find(|page| page.path.ends_with("schemas.md"))
        .expect("a schema reference");
    assert!(schemas.contents.contains("nullable") || schemas.contents.contains("may be null"));
}

#[test]
fn should_produce_the_same_bytes_for_the_same_registries_when_generated_twice() {
    let repo = registries();
    assert_eq!(
        generate(repo.path()).expect("generation"),
        generate(repo.path()).expect("generation"),
        "generation must be reproducible, or the gate can never compare it to the tree"
    );
}

#[test]
fn should_report_a_committed_page_that_no_longer_matches_the_registries() {
    let repo = registries();
    for page in generate(repo.path()).expect("generation") {
        repo.write(&page.path, &page.contents);
    }
    assert_eq!(check_committed(repo.path()), Vec::new());

    repo.write("docs/reference/commands.md", "# hand-edited\n");
    let problems = check_committed(repo.path());
    assert!(
        problems.iter().any(|p| p.location.contains("commands.md")),
        "a stale generated page must be reported, got {problems:?}"
    );
}

#[test]
fn should_report_a_page_that_was_never_committed_at_all() {
    let repo = registries();
    let problems = check_committed(repo.path());
    assert!(
        !problems.is_empty(),
        "reference docs that were never generated must be reported"
    );
}

#[test]
fn should_find_this_repositorys_committed_reference_docs_up_to_date_when_checked() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let problems = check_committed(root);
    assert!(
        problems.is_empty(),
        "the committed reference docs do not match the registries; run `cargo xtask docs`:\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn adapter_registries() -> Scratch {
    let repo = registries();
    let pack = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/spec/adapters/first-party/util-linux.yaml");
    repo.write(
        "docs/spec/adapters/first-party/util-linux.yaml",
        std::fs::read_to_string(pack).expect("the util-linux pack is part of the repository"),
    );
    repo
}

#[test]
fn should_publish_a_page_per_adapter_pack_and_a_compatibility_matrix_when_generated() {
    // Spec v0.3 §1.66, §2.6: adapter reference pages and the compatibility matrix are derived
    // from the contracts, so a support claim is a contract line, never a hand-written promise.
    let repo = adapter_registries();
    let written = generate(repo.path()).expect("generation must succeed");
    let page = |path: &str| {
        written
            .iter()
            .find(|page| page.path == path)
            .unwrap_or_else(|| {
                panic!(
                    "{path} missing from {:?}",
                    written.iter().map(|p| &p.path).collect::<Vec<_>>()
                )
            })
            .contents
            .clone()
    };
    let matrix = page("docs/reference/adapters/README.md");
    for expected in [
        "org.ono.compat.util-linux",
        "lsblk",
        "findmnt",
        "lsns",
        ">=2.37",
        "ono.block-device/1",
        "ono.mount/1",
    ] {
        assert!(
            matrix.contains(expected),
            "the matrix names {expected}; got:\n{matrix}"
        );
    }
    let pack = page("docs/reference/adapters/org.ono.compat.util-linux.md");
    for expected in [
        "org.ono.compat.util-linux.lsblk",
        "lsblk [-a\\|--all] [-d\\|--nodeps] [device …]",
        "are not adapted",
        "ono.block-device/1",
        "raw",
        "util-linux/lsblk",
    ] {
        assert!(
            pack.contains(expected),
            "the pack page states {expected}; got:\n{pack}"
        );
    }
    assert!(
        matrix.contains("org.ono.compat.util-linux.md"),
        "the matrix links every pack page; got:\n{matrix}"
    );
}

#[test]
fn should_publish_no_adapter_pages_when_no_pack_is_declared() {
    let repo = registries();
    let written = generate(repo.path()).expect("generation must succeed");
    assert!(
        written.iter().all(|page| !page.path.contains("/adapters/")),
        "a repository without packs has no adapter pages to keep up to date"
    );
}

// --- the checklist's own generation claims (B-harn-4) -------------------------------------------

fn generated_pages() -> Vec<String> {
    xtask::reference::generate(&repo())
        .expect("the registries generate")
        .into_iter()
        .map(|page| page.path)
        .collect()
}

#[test]
fn should_report_a_box_that_claims_a_generation_nobody_wrote() {
    // The two claims this rule was written for: §4.1 D said "docs and provider conformance tests
    // are generated from them" and §4.7.4 said the spatial conformance suite is "generated from
    // `docs/spec/providers/*.yaml`". Only `docs/reference/` is generated; both suites are
    // hand-written and checked against the registries instead.
    let claim = "- [x] **D — Consistency.** Registries exist under `docs/spec/`; docs and \
                 provider conformance tests are generated from them.\n";
    let problems = xtask::reference::check_generation_claims(claim, &generated_pages());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert_eq!(problems[0].location, "docs/ACCEPTANCE.md");
    assert!(problems[0].detail.contains("generated from"));
}

#[test]
fn should_accept_a_box_that_names_the_page_it_claims_is_generated() {
    let claim = "- [x] **Support claims are published.** `docs/reference/adapters/` — a page per \
                 adapter — is generated from the contracts.\n";
    assert_eq!(
        xtask::reference::check_generation_claims(claim, &generated_pages()),
        Vec::new()
    );
}

#[test]
fn should_find_every_generation_claim_of_this_repositorys_checklist_true() {
    let text = std::fs::read_to_string(repo().join("docs/ACCEPTANCE.md"))
        .expect("docs/ACCEPTANCE.md is readable");
    let problems = xtask::reference::check_generation_claims(&text, &generated_pages());
    assert!(
        problems.is_empty(),
        "docs/ACCEPTANCE.md claims a generation the tree does not perform:\n{}",
        problems
            .iter()
            .map(|problem| format!("  {}", problem.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
