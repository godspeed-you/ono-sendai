//! The decomposition of phase H9, held against the tree it produced.
//!
//! H9 cut three files into modules under one rule: no test may change (AGENTS.md §11, v0.4.1
//! §65.12). That is what makes the result trustworthy, and it is also what leaves it undefended —
//! a decomposition whose evidence is an *unchanged* suite has, by construction, no test of its
//! own, and a layout nobody checks reassembles itself.
//!
//! These are outcome tests about the repository's shape, which §66.6 makes a release criterion.
//! Every rule is proved twice: against a fixture that must be reported, so the rule is known to
//! bite, and against this repository, so it is known to hold here.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::collections::BTreeMap;
use std::path::Path;

use ono_testkit::{Scratch, scratch};
use xtask::architecture::check;

mod support;
use support::report;

/// The declaration this repository ships, so a fixture can start from something real.
fn registry() -> String {
    std::fs::read_to_string(repository().join("docs/contracts/hardening/module_architecture.yaml"))
        .expect("the architecture registry")
}

/// The repository root.
fn repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
}

/// A throwaway repository carrying a registry and whichever files the test needs.
fn fixture(files: &[(&str, &str)]) -> Scratch {
    let repo = scratch();
    repo.write(
        "docs/contracts/hardening/module_architecture.yaml",
        registry(),
    );
    for (path, contents) in files {
        repo.write(path, contents);
    }
    repo
}

// --- §29.2, the parser ---------------------------------------------------------------------------

#[test]
fn should_find_every_parser_responsibility_in_its_own_module() {
    let problems: Vec<_> = check(repository())
        .into_iter()
        .filter(|problem| problem.location.contains("ono-parser"))
        .collect();
    assert!(
        problems.is_empty(),
        "the parser's declared responsibilities and its modules disagree:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_parser_responsibility_that_lost_its_module() {
    // Only the declaration is written, so every module it names is absent. A responsibility whose
    // module moved without the declaration following it is how the map stops matching the ground.
    let repo = fixture(&[]);
    let problems = check(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.location.ends_with("statements.rs")),
        "a missing module is reported by name:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_parser_module_no_responsibility_claims() {
    let mut files = declared_modules("src/parser", "ono-parser");
    files.push((
        "crates/ono-parser/src/parser/leftovers.rs".to_owned(),
        "pub(super) fn helper() {}\n".to_owned(),
    ));
    let repo = fixture(&borrow(&files));
    let problems = check(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.location.ends_with("leftovers.rs")),
        "the module nobody claimed is the one a split file reassembles through:\n{}",
        report(&problems)
    );
}

// --- §30.2, the evaluator ------------------------------------------------------------------------

#[test]
fn should_find_every_evaluator_responsibility_in_its_own_module() {
    let problems: Vec<_> = check(repository())
        .into_iter()
        .filter(|problem| problem.location.contains("src/eval"))
        .collect();
    assert!(
        problems.is_empty(),
        "the evaluator's declared responsibilities and its modules disagree:\n{}",
        report(&problems)
    );
}

#[test]
fn should_find_no_domain_logic_moved_up_into_the_composition_root() {
    let problems: Vec<_> = check(repository())
        .into_iter()
        .filter(|problem| {
            problem.location.starts_with("crates/ono-cli/src/")
                && !problem.location.contains("/eval")
        })
        .collect();
    assert!(
        problems.is_empty(),
        "the composition root holds a module nobody declared:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_module_added_to_the_composition_root_without_a_decision() {
    let mut files = declared_modules("src/parser", "ono-parser");
    files.extend(declared_modules("src/eval", "ono-cli"));
    files.extend(declared_modules("src/eval/native", "ono-cli"));
    files.extend(composition_root_modules());
    files.push((
        "crates/ono-cli/src/spatial_index.rs".to_owned(),
        "pub fn place() {}\n".to_owned(),
    ));
    let repo = fixture(&borrow(&files));
    let problems = check(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.location.ends_with("spatial_index.rs")),
        "§30.4: a module that appears in the composition root is a decision, not a diff:\n{}",
        report(&problems)
    );
}

// --- §31.2, the session --------------------------------------------------------------------------

#[test]
fn should_find_every_session_state_group_the_specification_names() {
    let problems: Vec<_> = check(repository())
        .into_iter()
        .filter(|problem| problem.location.ends_with("session.rs"))
        .collect();
    assert!(
        problems.is_empty(),
        "§31.2's state groups and the session disagree:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_session_whose_state_has_no_owner() {
    // The flat field list §31.3 asks to be replaced: a session with no group at all.
    let repo = fixture(&[(
        "crates/ono-cli/src/session.rs",
        "pub struct Session { cwd: String, jobs: Vec<u32> }\n",
    )]);
    let problems = check(repo.path());
    assert!(
        problems.iter().any(|problem| {
            problem.location.ends_with("session.rs")
                && problem.detail.contains("ResultHistoryState")
        }),
        "a missing state group is reported by name:\n{}",
        report(&problems)
    );
}

// --- §56, the crate graph ------------------------------------------------------------------------

#[test]
fn should_hold_the_crate_graph_against_the_declared_layering() {
    let problems: Vec<_> = check(repository())
        .into_iter()
        .filter(|problem| problem.location.ends_with("Cargo.toml"))
        .collect();
    assert!(
        problems.is_empty(),
        "a dependency edge points at a layer above its own:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_new_dependency_edge_that_inverts_a_declared_boundary() {
    // `ono-value` is foundation and `ono-protocol` is capability. An edge from the first to the
    // second is the inversion §30.4 and §56 forbid, and it is exactly the shape a refactor takes
    // when a lower crate reaches upward for something convenient.
    let repo = fixture(&[(
        "crates/ono-value/Cargo.toml",
        "[package]\nname = \"ono-value\"\n\n[dependencies]\nono-core.workspace = true\n\
         ono-protocol.workspace = true\n",
    )]);
    let problems = check(repo.path());
    assert!(
        problems.iter().any(|problem| {
            problem.location == "crates/ono-value/Cargo.toml"
                && problem.detail.contains("ono-protocol")
        }),
        "an upward edge is reported with both layers named:\n{}",
        report(&problems)
    );
}

#[test]
fn should_hold_every_crate_to_the_layer_it_is_declared_in() {
    // v0.5 §39.3: "Renderers MUST consume canonical query output and MUST NOT query providers,
    // the ledger or the network directly", and §2's last invariant makes that a release
    // criterion: machine-readable semantics precede rendering.
    //
    // The layering is where that rule lives — a renderer sits in `runtime`, and the providers,
    // the ledger and the transports sit in the layers above it — so the rule holds only if two
    // things are true at once, and this test asserts both against the real tree rather than
    // against the declaration alone.
    //
    // First, every crate the workspace ships is placed by the declaration, and placed exactly
    // once. A crate nobody placed is a crate the rule cannot reach; a crate placed twice has two
    // answers to "which layer is it in", and the more permissive one always wins by accident.
    //
    // Second, no renderer's manifest names a crate the declaration puts above it. That is read
    // off the manifests themselves, so it is a statement about the dependency graph the compiler
    // sees and not about the layering's own consistency. The named families are asserted to be
    // above a renderer first, because a declaration that quietly moved the ledger down into
    // `runtime` would leave the loop below true and meaningless.
    let layers = declared_layers();
    let ranks = crate_ranks(&layers);

    for krate in workspace_crates() {
        let placed: Vec<&str> = layers
            .iter()
            .filter(|(_, members)| members.contains(&krate))
            .map(|(layer, _)| layer.as_str())
            .collect();
        assert_eq!(
            placed.len(),
            1,
            "§56: `{krate}` is placed in {placed:?}. Every crate belongs to exactly one layer — \
             an unplaced crate is outside the rule, and a crate in two layers makes `may I depend \
             on this` a question with two answers"
        );
    }

    let renderers: Vec<String> = workspace_crates()
        .into_iter()
        .filter(|krate| krate.ends_with("-render"))
        .collect();
    assert!(
        !renderers.is_empty(),
        "v0.5 §39.3 is a rule about renderers, and this workspace ships none to hold it against"
    );
    for renderer in &renderers {
        let own = ranks
            .get(renderer)
            .copied()
            .unwrap_or_else(|| panic!("`{renderer}` is placed by the layering"));
        // A provider, the ledger and the network, named one each, so the assertion below is
        // known to be about the things §39.3 names.
        for named in ["ono-provider-linux", "ono-temporal-ledger", "ono-protocol"] {
            let rank = ranks
                .get(named)
                .copied()
                .unwrap_or_else(|| panic!("`{named}` is placed by the layering"));
            assert!(
                rank > own,
                "v0.5 §39.3: `{named}` must sit above the renderer layer `{renderer}` is in, or \
                 the layering stops forbidding a renderer from reaching it"
            );
        }
        for dependency in dependencies_of(renderer) {
            let Some(rank) = ranks.get(&dependency).copied() else {
                continue;
            };
            assert!(
                rank <= own,
                "v0.5 §39.3: `{renderer}` depends on `{dependency}`, which the layering places \
                 above it. A renderer consumes canonical query output and reaches no provider, no \
                 ledger and no network — that is what makes a renderer testable from a value \
                 built by hand"
            );
        }
    }

    // And the rule bites: a renderer that reaches the ledger is reported, so the two loops above
    // are held by a check that runs in the gate rather than by this test alone.
    let repo = fixture(&[(
        "crates/ono-temporal-render/Cargo.toml",
        "[package]\nname = \"ono-temporal-render\"\n\n[dependencies]\nono-value.workspace = true\n\
         ono-temporal-ledger.workspace = true\n",
    )]);
    let problems = check(repo.path());
    assert!(
        problems.iter().any(|problem| {
            problem.location == "crates/ono-temporal-render/Cargo.toml"
                && problem.detail.contains("ono-temporal-ledger")
        }),
        "v0.5 §39.3: a renderer that reaches the ledger is reported by name:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_crate_the_layering_does_not_place() {
    let repo = fixture(&[(
        "crates/ono-newcomer/Cargo.toml",
        "[package]\nname = \"ono-newcomer\"\n",
    )]);
    let problems = check(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.detail.contains("ono-newcomer")),
        "a crate outside the layering is a crate the rule cannot hold:\n{}",
        report(&problems)
    );
}

// --- the declared layering, as data ---------------------------------------------------------------

/// The layers the declaration writes, in order, with the crates each one holds.
///
/// The order is the rule: an edge may point into its own layer or into any layer before it, and
/// never after it (§56).
fn declared_layers() -> Vec<(String, Vec<String>)> {
    let document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&registry()).expect("the architecture registry is YAML");
    document["layering"]["layers"]
        .as_sequence()
        .expect("the layering declares its layers")
        .iter()
        .map(|layer| {
            let name = layer["layer"].as_str().unwrap_or_default().to_owned();
            let members = layer["crates"]
                .as_sequence()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            (name, members)
        })
        .collect()
}

/// How deep each declared crate sits, so two crates can be compared.
fn crate_ranks(layers: &[(String, Vec<String>)]) -> BTreeMap<String, usize> {
    let mut ranks = BTreeMap::new();
    for (depth, (_, members)) in layers.iter().enumerate() {
        for member in members {
            ranks.insert(member.clone(), depth);
        }
    }
    ranks
}

/// The crate directories this workspace actually ships.
fn workspace_crates() -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(repository().join("crates"))
        .expect("the workspace has a `crates` directory")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

/// The workspace crates `krate`'s own manifest depends on.
fn dependencies_of(krate: &str) -> Vec<String> {
    let manifest = repository().join("crates").join(krate).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|_| panic!("`{krate}` has a manifest at {}", manifest.display()));
    text.lines()
        .filter_map(|line| line.trim().strip_suffix(".workspace = true"))
        .filter(|name| name.starts_with("ono-") && *name != krate)
        .map(str::to_owned)
        .collect()
}

// --- fixture material ----------------------------------------------------------------------------

/// The modules this repository actually has under `directory`, as fixture files.
///
/// A fixture that invented its own module names would test the check against a repository that
/// does not exist; these are the real ones, so a fixture failure is about the rule under test.
fn declared_modules(directory: &str, krate: &str) -> Vec<(String, String)> {
    let base = repository().join("crates").join(krate).join(directory);
    let Ok(entries) = std::fs::read_dir(&base) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.path().is_dir() || entry.path().extension().is_some_and(|ext| ext == "rs")
        })
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if entry.path().is_dir() {
                format!("crates/{krate}/{directory}/{name}/mod.rs")
            } else {
                format!("crates/{krate}/{directory}/{name}")
            };
            (path, "// fixture\n".to_owned())
        })
        .collect()
}

/// The composition root's declared modules, as fixture files.
fn composition_root_modules() -> Vec<(String, String)> {
    let mut files = declared_modules("src", "ono-cli");
    files.push((
        "crates/ono-cli/src/session.rs".to_owned(),
        session_with_every_group(),
    ));
    files
}

/// A session carrying every group §31.2 names, so a fixture about something else stays quiet.
fn session_with_every_group() -> String {
    [
        "EnvironmentState",
        "ScopeState",
        "ExecutionState",
        "NavigationState",
        "ResultHistoryState",
        "JobState",
        "ProviderState",
        "PresentationState",
    ]
    .iter()
    .map(|group| format!("struct {group} {{}}\n"))
    .collect()
}

/// Borrows owned fixture rows into the shape [`fixture`] takes.
fn borrow(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.as_str()))
        .collect()
}
