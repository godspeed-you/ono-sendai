//! What `xtask spec-check` must refuse about `docs/contracts/change/` and
//! `docs/contracts/recovery/` (spec v0.6 §47).
//!
//! Two halves. The first asserts that the eleven registries this repository ships pass, that each
//! of them is required, and that the hardening inventory indexes all of them. The second is the
//! one the module is worth having: every rule `xtask/src/change.rs` implements is exercised by
//! copying the real registries into a scratch tree, breaking exactly one thing, and asserting the
//! specific refusal.
//!
//! Copying rather than hand-writing a fixture is deliberate, and ADR-0740 is why: the eleven
//! registries cross-reference each other and the vocabulary of `ono-change-core`, so a synthetic
//! minimal set would be a second copy of the specification maintained beside the first. A test
//! that builds a registry from nothing proves the checker can reject *a* document; a test that
//! mutates the real one proves it would catch the mistake somebody will actually make. The second
//! test asserts the untouched copy is clean, so every later assertion is about the break rather
//! than about the fixture.
//!
//! Every assertion matches a distinctive fragment of the refusal, so no test here can pass merely
//! because *some* problem was reported.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use ono_change_core::{LifecycleEvent, PlanState};
use ono_testkit::{Scratch, scratch};
use xtask::change::check;

mod support;
use support::{read, repo, report};

/// §47's eleven registries, in the two directories it puts them in.
const REGISTRIES: [&str; 11] = [
    "docs/contracts/change/plans.yaml",
    "docs/contracts/change/actions.yaml",
    "docs/contracts/change/effects.yaml",
    "docs/contracts/change/risk.yaml",
    "docs/contracts/change/verification.yaml",
    "docs/contracts/change/strategies.yaml",
    "docs/contracts/recovery/providers.yaml",
    "docs/contracts/recovery/assets.yaml",
    "docs/contracts/recovery/consistency.yaml",
    "docs/contracts/recovery/policies.yaml",
    "docs/contracts/recovery/errors.yaml",
];

/// Everything else `change::check` reads while it cross-references the eleven.
const NEIGHBOURS: [&str; 5] = [
    "docs/contracts/commands/change.yaml",
    "docs/contracts/verbs.yaml",
    "docs/contracts/errors.yaml",
    "docs/contracts/capabilities.yaml",
    "docs/contracts/hardening/registries.yaml",
];

/// A scratch copy of everything `change::check` reads.
///
/// The schema directory and the crate directory are copied wholesale rather than by name: the
/// checker resolves `plans.yaml`'s `schemas:` against the first and `providers.yaml`'s `crate:`
/// against the second, and a fixture that listed either by hand would have to be edited every time
/// the repository grew one.
fn copied() -> Scratch {
    let tree = scratch();
    for relative in REGISTRIES.into_iter().chain(NEIGHBOURS) {
        tree.write(relative, read(relative));
    }
    for entry in std::fs::read_dir(repo().join("docs/contracts/schemas"))
        .expect("the schema directory exists")
        .flatten()
    {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.ends_with(".yaml") {
            tree.write(
                format!("docs/contracts/schemas/{name}"),
                read(&format!("docs/contracts/schemas/{name}")),
            );
        }
    }
    for entry in std::fs::read_dir(repo().join("crates"))
        .expect("the crates directory exists")
        .flatten()
    {
        std::fs::create_dir_all(tree.path().join("crates").join(entry.file_name()))
            .expect("a scratch crate directory");
        // §39.2 and §12.1 are checked against what a provider's source says, and Appendix G.2
        // against what its tests say, so those crates come across whole rather than as an empty
        // directory.
        // `Execution`'s variants and `EffectConfidence`'s methods are read out of
        // `ono-change-core`'s source, so it comes across too.
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("ono-recovery-") || name == "ono-change-core" {
            copy_source(&entry.path().join("src"), &tree, &format!("{name}/src"));
        }
        if name.starts_with("ono-recovery-") || name == "ono-change-protection" {
            copy_source(&entry.path().join("tests"), &tree, &format!("{name}/tests"));
        }
    }
    tree
}

/// Every `.rs` file under `source`, written into the scratch tree under `crates/{destination}`.
fn copy_source(source: &std::path::Path, tree: &Scratch, destination: &str) {
    let mut stack = vec![source.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
                continue;
            }
            let relative = path
                .strip_prefix(source)
                .expect("the walk started at `source`")
                .to_string_lossy()
                .into_owned();
            let body = std::fs::read_to_string(&path).expect("a source file the repository ships");
            tree.write(format!("crates/{destination}/{relative}"), body);
        }
    }
}

/// Every refusal `change::check` reports about a tree, as the lines a failing assertion prints.
fn refusals(tree: &Scratch) -> String {
    report(&check(tree.path()))
}

/// Rewrites one file of the copy, replacing `from` with `to` exactly once.
///
/// The precondition is asserted rather than assumed: a registry edited upstream would otherwise
/// leave the mutation silently applied to nothing, and the test would pass over an unbroken
/// fixture.
fn edit(tree: &Scratch, relative: &str, from: &str, to: &str) {
    let path = tree.path().join(relative);
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{relative} was copied: {error}"));
    assert!(
        body.contains(from),
        "the fixture no longer contains {from:?}; update the test with {relative}"
    );
    std::fs::write(&path, body.replacen(from, to, 1)).expect("write");
}

/// A copy with one edit applied, which is the shape almost every test below wants.
fn broken(relative: &str, from: &str, to: &str) -> Scratch {
    let tree = copied();
    edit(&tree, relative, from, to);
    tree
}

/// Asserts that the check refuses `tree`, naming `needle`.
fn assert_refuses(tree: &Scratch, needle: &str) {
    let found = refusals(tree);
    assert!(
        found.contains(needle),
        "v0.6 §47 requires this drift to be reported; expected a refusal mentioning {needle:?}, \
         and the check said:\n{found}"
    );
}

// --- the registries this repository ships ---------------------------------------------------

#[test]
fn should_accept_the_registries_this_repository_ships_when_checked() {
    let found = report(&check(&repo()));
    assert!(
        found.is_empty(),
        "the eleven registries of v0.6 §47 are free of drift as committed:\n{found}"
    );
}

#[test]
fn should_accept_the_scratch_copy_of_those_registries_when_nothing_is_broken() {
    // Every mutation test below asserts that a break is reported. That is only evidence about the
    // break if the unbroken copy is silent, which is what this states.
    let found = refusals(&copied());
    assert!(
        found.is_empty(),
        "the scratch copy carries the same registries and must be as clean:\n{found}"
    );
}

#[test]
fn should_report_nothing_when_the_change_registries_have_not_arrived() {
    // AGENTS.md §14: a registry arrives with the phase that needs it, so an absent directory is
    // silence rather than a failure. This is what lets the check ship before its consumers.
    let tree = scratch();
    assert!(
        refusals(&tree).is_empty(),
        "a tree without `docs/contracts/change/` has no v0.6 contract set to check"
    );
}

#[test]
fn should_reject_a_required_registry_that_is_missing() {
    // §47 names eleven, and a half-written contract set makes a promise nobody can check. Each is
    // removed from its own copy, so one missing file cannot mask another.
    for relative in REGISTRIES {
        let tree = copied();
        std::fs::remove_file(tree.path().join(relative)).expect("rm");
        assert_refuses(&tree, &format!("{relative} — does not exist"));
    }
}

#[test]
fn should_index_every_one_of_the_eleven_registries_in_the_hardening_inventory() {
    // v0.4.1 §52.3: a registry nothing in the gate holds is a referee nobody holds. The inventory
    // names each file relative to `docs/contracts/hardening/`.
    let inventory = read("docs/contracts/hardening/registries.yaml");
    for relative in REGISTRIES {
        let reference = relative.replace("docs/contracts/", "../");
        assert!(
            inventory.contains(&format!("file: {reference}")),
            "`docs/contracts/hardening/registries.yaml` indexes `{reference}`"
        );
    }
}

#[test]
fn should_reject_a_registry_the_hardening_inventory_does_not_index() {
    let tree = broken(
        "docs/contracts/hardening/registries.yaml",
        "  - file: ../recovery/assets.yaml",
        "  - file: ../recovery/asset.yaml",
    );
    assert_refuses(&tree, "does not index `../recovery/assets.yaml`");
}

// --- the closed vocabularies, in both directions ---------------------------------------------

#[test]
fn should_reject_a_plan_state_the_registry_omits_and_the_machine_emits() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: expired, mutated: false",
        "  # removed: - {id: expired, mutated: false",
    );
    assert_refuses(&tree, "`states` omits the plan state `expired`");
}

#[test]
fn should_reject_a_plan_state_the_registry_declares_and_nothing_implements() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: draft, mutated: false",
        "  - {id: rumoured, mutated: false, terminal: false, appliable: false, retains_assets: \
         false, recoverable: false, doc: \"invented\"}\n  - {id: draft, mutated: false",
    );
    assert_refuses(&tree, "`states` declares the plan state `rumoured`");
}

#[test]
fn should_reject_a_protection_level_whose_persistent_coverage_claim_differs() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: protected, covers_persistent_state: true",
        "  - {id: protected, covers_persistent_state: false",
    );
    assert_refuses(
        &tree,
        "`protected` declares `covers_persistent_state: false`",
    );
    assert_refuses(&tree, "§4.6 forbids the word `protected`");
}

#[test]
fn should_reject_a_protection_level_whose_symbol_differs_from_the_shells() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: unknown, covers_persistent_state: false, symbol: \"?\"",
        "  - {id: unknown, covers_persistent_state: false, symbol: \"??\"",
    );
    assert_refuses(&tree, "`unknown` declares the symbol `??`");
}

// --- §4.1's lifecycle, edge by edge -----------------------------------------------------------

#[test]
fn should_reject_a_lifecycle_transition_the_registry_draws_and_the_machine_does_not() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "transitions:\n",
        "transitions:\n  - {from: closed, event: seal, to: sealed}\n",
    );
    assert_refuses(
        &tree,
        "declares `closed` -> `seal` -> `sealed`, and the machine draws no edge there",
    );
}

#[test]
fn should_reject_a_lifecycle_transition_the_machine_draws_and_the_registry_does_not() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {from: verified, event: close, to: closed}\n",
        "",
    );
    assert_refuses(
        &tree,
        "the machine draws `verified` -> `close` -> `closed` and `transitions` does not",
    );
}

#[test]
fn should_reject_a_transition_whose_destination_differs_from_where_the_machine_goes() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {from: sealed, event: expire, to: expired}",
        "  - {from: sealed, event: expire, to: draft}",
    );
    assert_refuses(
        &tree,
        "declares `sealed` -> `expire` -> `draft`, and the machine goes to `expired`",
    );
}

#[test]
fn should_reject_an_edge_from_prepare_failed_to_applying() {
    // §2.3 is the invariant this whole module is worth having for: if a required recovery asset
    // cannot be created, mutation MUST NOT begin. `check_transitions` guards it twice — through
    // the table, which a registry can break, and directly against the machine, which only
    // `ono-change-core` can break. Both are stated here, because the direct refusal cannot be
    // provoked from a fixture: the absence of the edge in the machine *is* the condition it
    // reports on.
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "transitions:\n",
        "transitions:\n  - {from: prepare-failed, event: begin-apply, to: applying}\n",
    );
    assert_refuses(
        &tree,
        "declares `prepare-failed` -> `begin-apply` -> `applying`, and the machine draws no edge \
         there",
    );
    assert!(
        PlanState::PrepareFailed
            .after(LifecycleEvent::BeginApply)
            .is_none(),
        "§2.3: there is no edge from `prepare-failed` to `applying`, and `check_transitions` \
         reports one directly if the machine ever grows it"
    );
}

#[test]
fn should_reject_a_transition_that_does_not_say_where_it_goes() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {from: draft, event: resolve, to: resolved}",
        "  - {from: draft, event: resolve}",
    );
    assert_refuses(
        &tree,
        "an edge that does not say where it goes is not a contract",
    );
}

#[test]
fn should_reject_a_transition_starting_at_a_state_the_specification_does_not_define() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {from: draft, event: seal, to: sealed}",
        "  - {from: drafted, event: seal, to: sealed}",
    );
    assert_refuses(
        &tree,
        "a transition starts at `drafted`, which is not a state §4.1 defines",
    );
}

#[test]
fn should_reject_a_transition_raised_by_an_event_the_specification_does_not_define() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {from: draft, event: resolve, to: resolved}",
        "  - {from: draft, event: resolving, to: resolved}",
    );
    assert_refuses(
        &tree,
        "a transition is raised by `resolving`, which is not an event §4.1 defines",
    );
}

// --- the five predicates a plan state publishes ------------------------------------------------

#[test]
fn should_reject_a_plan_state_whose_mutated_flag_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: applying, mutated: true",
        "  - {id: applying, mutated: false",
    );
    assert_refuses(&tree, "state `applying` declares `mutated: false`");
    assert_refuses(
        &tree,
        "Appendix F reads this to tell a refusal from a partial apply",
    );
}

#[test]
fn should_reject_a_plan_state_whose_appliable_flag_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: sealed, mutated: false, terminal: false, appliable: true",
        "  - {id: sealed, mutated: false, terminal: false, appliable: false",
    );
    assert_refuses(&tree, "state `sealed` declares `appliable: false`");
}

#[test]
fn should_reject_a_plan_state_whose_terminal_flag_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: closed, mutated: true, terminal: true",
        "  - {id: closed, mutated: true, terminal: false",
    );
    assert_refuses(&tree, "state `closed` declares `terminal: false`");
}

#[test]
fn should_reject_a_plan_state_whose_retains_assets_flag_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: prepare-failed, mutated: false, terminal: true, appliable: false, \
         retains_assets: true",
        "  - {id: prepare-failed, mutated: false, terminal: true, appliable: false, \
         retains_assets: false",
    );
    assert_refuses(
        &tree,
        "state `prepare-failed` declares `retains_assets: false`",
    );
}

#[test]
fn should_reject_a_plan_state_whose_recoverable_flag_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: apply-failed, mutated: true, terminal: false, appliable: false, \
         retains_assets: true, recoverable: true",
        "  - {id: apply-failed, mutated: true, terminal: false, appliable: false, \
         retains_assets: true, recoverable: false",
    );
    assert_refuses(&tree, "state `apply-failed` declares `recoverable: false`");
}

// --- §2.17: nothing runs a command line --------------------------------------------------------

#[test]
fn should_reject_an_execution_method_that_admits_a_command_line() {
    let tree = broken(
        "docs/contracts/change/actions.yaml",
        "  - id: program\n    admits_command_line: false",
        "  - id: program\n    admits_command_line: true",
    );
    assert_refuses(&tree, "execution method `program` admits a command line");
    assert_refuses(
        &tree,
        "MUST be structured execution plans, not interpolated shell command strings",
    );
}

#[test]
fn should_reject_an_execution_method_that_declares_no_shape() {
    let tree = broken(
        "docs/contracts/change/actions.yaml",
        "    shape: \"operator description, optional resolved program, argument vector\"\n",
        "",
    );
    assert_refuses(&tree, "execution method `opaque` declares no shape");
}

// --- Appendix A.5's persistence predicate, and §2.13's irreversibility -------------------------

#[test]
fn should_reject_an_effect_domain_whose_persistent_flag_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/change/effects.yaml",
        "  - {id: process-runtime, persistent: false",
        "  - {id: process-runtime, persistent: true",
    );
    assert_refuses(
        &tree,
        "domain `process-runtime` declares `persistent: true`",
    );
}

#[test]
fn should_reject_an_effect_kind_whose_irreversibility_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/change/effects.yaml",
        "  - {id: emit, inherently_irreversible: true",
        "  - {id: emit, inherently_irreversible: false",
    );
    assert_refuses(
        &tree,
        "kind `emit` declares `inherently_irreversible: false`",
    );
}

#[test]
fn should_reject_a_confidence_lattice_that_offers_an_operation_which_strengthens() {
    let tree = broken(
        "docs/contracts/change/effects.yaml",
        "  strengthening_operation: null",
        "  strengthening_operation: strongest_of",
    );
    assert_refuses(&tree, "§2.4 forbids unknown being promoted");
}

#[test]
fn should_reject_a_consistency_lattice_that_offers_an_operation_which_strengthens() {
    let tree = broken(
        "docs/contracts/recovery/consistency.yaml",
        "  strengthening_operation: null",
        "  strengthening_operation: strongest_of",
    );
    assert_refuses(
        &tree,
        "only as consistent as its weakest member (Appendix D.7)",
    );
}

// --- Appendix C.1's least-destructive order ----------------------------------------------------

#[test]
fn should_reject_a_restore_method_whose_destructiveness_order_differs() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  - {id: dataset-rollback, destructiveness: 5",
        "  - {id: dataset-rollback, destructiveness: 1",
    );
    assert_refuses(
        &tree,
        "method `dataset-rollback` declares destructiveness 1",
    );
    assert_refuses(
        &tree,
        "requires the least-destructive method that satisfies the goal",
    );
}

#[test]
fn should_reject_a_restore_method_whose_newer_state_claim_differs() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  - {id: dataset-rollback, destructiveness: 5, discards_newer_state: true",
        "  - {id: dataset-rollback, destructiveness: 5, discards_newer_state: false",
    );
    assert_refuses(
        &tree,
        "method `dataset-rollback` declares `discards_newer_state: false`",
    );
}

#[test]
fn should_reject_a_restore_method_that_calls_compensation_a_restoration_of_prior_state() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  - {id: compensation, destructiveness: 3, discards_newer_state: false, \
         restores_prior_state: false",
        "  - {id: compensation, destructiveness: 3, discards_newer_state: false, \
         restores_prior_state: true",
    );
    assert_refuses(
        &tree,
        "method `compensation` declares `restores_prior_state: true`",
    );
    assert_refuses(&tree, "§27.4 forbids compensation being labelled rollback");
}

#[test]
fn should_reject_a_cost_model_that_permits_the_word_free() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  free_permitted: false",
        "  free_permitted: true",
    );
    assert_refuses(
        &tree,
        "§38.2: Ono MUST NOT display 'free' for a copy-on-write snapshot",
    );
}

// --- §11.5: a snapshot is not a backup ---------------------------------------------------------

#[test]
fn should_reject_an_asset_type_whose_failure_domain_claim_differs() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  - {id: zfs-snapshot, shares_failure_domain: true",
        "  - {id: zfs-snapshot, shares_failure_domain: false",
    );
    assert_refuses(
        &tree,
        "type `zfs-snapshot` declares `shares_failure_domain: false`",
    );
    assert_refuses(
        &tree,
        "§11.5 forbids implying that a local snapshot protects against pool loss",
    );
}

#[test]
fn should_reject_an_asset_type_whose_independent_copy_claim_differs() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  - {id: file-archive, shares_failure_domain: false, independent_copy: true",
        "  - {id: file-archive, shares_failure_domain: false, independent_copy: false",
    );
    assert_refuses(
        &tree,
        "type `file-archive` declares `independent_copy: false`",
    );
}

#[test]
fn should_reject_an_asset_state_whose_usability_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  - {id: ready, usable: true",
        "  - {id: ready, usable: false",
    );
    assert_refuses(
        &tree,
        "asset state `ready` disagrees with the shell about `usable`",
    );
}

#[test]
fn should_reject_an_asset_state_whose_storage_occupancy_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  - {id: proposed, usable: false, occupies_storage: false",
        "  - {id: proposed, usable: false, occupies_storage: true",
    );
    assert_refuses(
        &tree,
        "asset state `proposed` disagrees with the shell about `occupies_storage`",
    );
}

// --- §39.2: who may claim a consistency class --------------------------------------------------

#[test]
fn should_reject_a_consistency_class_that_names_no_owner() {
    let tree = broken(
        "docs/contracts/recovery/consistency.yaml",
        "    owned_by: nobody",
        "    owned_by: \"\"",
    );
    assert_refuses(&tree, "class `unknown` declares no owner");
    assert_refuses(&tree, "the owner is what says who may");
}

#[test]
fn should_reject_consistency_ranks_the_shells_weakest_of_does_not_agree_with() {
    let tree = broken(
        "docs/contracts/recovery/consistency.yaml",
        "  - id: crash-consistent\n    rank: 3",
        "  - id: crash-consistent\n    rank: 0",
    );
    assert_refuses(
        &tree,
        "the declared ranks put `crash-consistent` above `application-consistent`",
    );
}

// --- §12.2 and §48.4: what a capability authorises ---------------------------------------------

#[test]
fn should_reject_a_capability_whose_required_flag_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "  - {id: recovery.quiesce, required: false",
        "  - {id: recovery.quiesce, required: true",
    );
    assert_refuses(
        &tree,
        "capability `recovery.quiesce` declares `required: true`",
    );
}

#[test]
fn should_reject_a_capability_whose_mutation_flag_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "  - {id: recovery.prepare, required: true, mutates: true",
        "  - {id: recovery.prepare, required: true, mutates: false",
    );
    assert_refuses(
        &tree,
        "capability `recovery.prepare` disagrees with the shell about whether it mutates",
    );
}

#[test]
fn should_reject_a_change_capability_whose_mutation_flag_disagrees_with_the_shell() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "  - {id: change.action.execute, mutates: true",
        "  - {id: change.action.execute, mutates: false",
    );
    assert_refuses(
        &tree,
        "change capability `change.action.execute` disagrees with the shell about whether it \
         mutates",
    );
}

// --- §5's thirteen commands, in both directions ------------------------------------------------

#[test]
fn should_reject_a_command_the_inventory_names_and_no_command_contract_declares() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: ono.change.verify, spelling: verify",
        "  - {id: ono.change.confirm, spelling: verify",
    );
    assert_refuses(
        &tree,
        "`commands` names `ono.change.confirm`, and no command contract declares it",
    );
}

#[test]
fn should_reject_a_command_a_command_contract_declares_and_the_inventory_omits() {
    let tree = broken(
        "docs/contracts/commands/change.yaml",
        "  - id: ono.change.verify\n",
        "  - id: ono.change.confirm\n",
    );
    assert_refuses(&tree, "`commands` omits `ono.change.confirm`");
}

#[test]
fn should_reject_a_command_whose_mutation_flag_disagrees_with_its_verb() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: ono.change.apply, spelling: apply, mutates: true",
        "  - {id: ono.change.apply, spelling: apply, mutates: false",
    );
    assert_refuses(
        &tree,
        "`ono.change.apply` declares `mutates: false` and the verb `apply` is registered as \
         `mutating: true`",
    );
}

#[test]
fn should_reject_a_v06_command_that_is_not_in_phase_p() {
    let tree = broken(
        "docs/contracts/commands/change.yaml",
        "    phase: P\n",
        "    phase: Q\n",
    );
    assert_refuses(
        &tree,
        "`ono.change.plan` is not in phase `P`, which is the v0.6 tranche of §57",
    );
}

// --- §46's schemas ------------------------------------------------------------------------------

#[test]
fn should_reject_a_schema_naming_a_file_the_schema_directory_does_not_hold() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "  - {id: ono.impact-graph/1, file: impact-graph.v1.yaml",
        "  - {id: ono.impact-graph/1, file: impact-map.v1.yaml",
    );
    assert_refuses(
        &tree,
        "schema `ono.impact-graph/1` names `docs/contracts/schemas/impact-map.v1.yaml`, which \
         does not exist",
    );
}

#[test]
fn should_reject_a_schema_file_whose_own_identity_differs_from_the_registrys() {
    let tree = broken(
        "docs/contracts/schemas/change-plan.v1.yaml",
        "id: ono.change-plan/1",
        "id: ono.change-plan/2",
    );
    assert_refuses(
        &tree,
        "declares `ono.change-plan/2` and `plans.yaml` names it `ono.change-plan/1`",
    );
}

// --- §45's error family, in both directions ----------------------------------------------------

#[test]
fn should_reject_an_error_the_global_registry_defines_and_the_index_omits() {
    let tree = broken(
        "docs/contracts/recovery/errors.yaml",
        "      - {code: Ono-Sendai-E1815, name: recovery.cleanup_blocked, nothing_changed: true}\n",
        "",
    );
    assert_refuses(
        &tree,
        "does not index `recovery.cleanup_blocked` (Ono-Sendai-E1815)",
    );
}

#[test]
fn should_reject_an_error_the_index_names_and_the_global_registry_does_not_define() {
    let tree = broken(
        "docs/contracts/recovery/errors.yaml",
        "name: recovery.cleanup_blocked",
        "name: recovery.cleanup_refused",
    );
    assert_refuses(
        &tree,
        "indexes `recovery.cleanup_refused`, and `docs/contracts/errors.yaml` does not define it",
    );
}

#[test]
fn should_reject_an_indexed_error_whose_code_differs_from_the_registrys() {
    let tree = broken(
        "docs/contracts/recovery/errors.yaml",
        "{code: Ono-Sendai-E1815, name: recovery.cleanup_blocked",
        "{code: Ono-Sendai-E1899, name: recovery.cleanup_blocked",
    );
    assert_refuses(
        &tree,
        "indexes `recovery.cleanup_blocked` as Ono-Sendai-E1899 and the registry defines it as \
         Ono-Sendai-E1815",
    );
}

#[test]
fn should_reject_an_indexed_error_that_does_not_say_whether_anything_changed() {
    let tree = broken(
        "docs/contracts/recovery/errors.yaml",
        "{code: Ono-Sendai-E1815, name: recovery.cleanup_blocked, nothing_changed: true}",
        "{code: Ono-Sendai-E1815, name: recovery.cleanup_blocked}",
    );
    assert_refuses(
        &tree,
        "`recovery.cleanup_blocked` does not say whether it means nothing was changed",
    );
}

// --- §19.4's gates and §19.2's rule table ------------------------------------------------------

#[test]
fn should_reject_a_risk_gate_naming_an_error_nothing_raises() {
    let tree = broken(
        "docs/contracts/change/risk.yaml",
        "error: change.risk_not_accepted",
        "error: change.risk_declined",
    );
    assert_refuses(
        &tree,
        "gate `risk` names the error `change.risk_declined`, which nothing raises",
    );
}

#[test]
fn should_reject_a_risk_gate_that_names_no_flag() {
    let tree = broken(
        "docs/contracts/change/risk.yaml",
        "flag: \"--accept-risk\", ",
        "",
    );
    assert_refuses(&tree, "gate `risk` names no flag");
    assert_refuses(&tree, "MUST fail rather than prompt");
}

#[test]
fn should_reject_a_risk_gate_whose_flag_a_caller_cannot_write() {
    let tree = broken(
        "docs/contracts/change/risk.yaml",
        "flag: \"--accept-risk\"",
        "flag: \"accept-risk\"",
    );
    assert_refuses(
        &tree,
        "gate `risk` names `accept-risk`, which is not a flag a caller can write",
    );
}

#[test]
fn should_reject_a_risk_rule_the_engine_registers_and_the_registry_omits() {
    let tree = broken(
        "docs/contracts/change/risk.yaml",
        "  - {id: risk.reboot.required, dimension: reboot-requirement",
        "  # removed: - {id: risk.reboot.required, dimension: reboot-requirement",
    );
    assert_refuses(&tree, "`rules` omits the risk rule `risk.reboot.required`");
}

#[test]
fn should_reject_a_risk_rule_the_registry_declares_and_the_engine_does_not_register() {
    let tree = broken(
        "docs/contracts/change/risk.yaml",
        "  - {id: risk.scope.single-object,",
        "  - {id: risk.invented.rule, dimension: scope, emits: low, doc: \"nothing implements \
         this\"}\n  - {id: risk.scope.single-object,",
    );
    assert_refuses(
        &tree,
        "`rules` declares the risk rule `risk.invented.rule`, which nothing implements",
    );
}

#[test]
fn should_reject_a_risk_rule_whose_dimension_disagrees_with_the_engines() {
    let tree = broken(
        "docs/contracts/change/risk.yaml",
        "  - {id: risk.reboot.required, dimension: reboot-requirement",
        "  - {id: risk.reboot.required, dimension: downtime",
    );
    assert_refuses(
        &tree,
        "rule `risk.reboot.required` is registered against the dimension `downtime`",
    );
}

// --- §28.4's strategies -------------------------------------------------------------------------

#[test]
fn should_reject_a_strategy_that_is_not_bounded() {
    let tree = broken(
        "docs/contracts/change/strategies.yaml",
        "  - id: parallel\n    parameters: [width]\n    bounded: true",
        "  - id: parallel\n    parameters: [width]\n    bounded: false",
    );
    assert_refuses(&tree, "strategy `parallel` is not bounded");
    assert_refuses(
        &tree,
        "unlimited parallel mutation is not a default strategy",
    );
}

#[test]
fn should_reject_two_strategies_marked_default() {
    let tree = broken(
        "docs/contracts/change/strategies.yaml",
        "  - id: batch\n    parameters: [size]\n    bounded: true\n    gated_first_wave: false\n    default: false",
        "  - id: batch\n    parameters: [size]\n    bounded: true\n    gated_first_wave: false\n    default: true",
    );
    assert_refuses(&tree, "2 strategies are marked default");
}

#[test]
fn should_reject_a_strategy_table_with_no_default_at_all() {
    let tree = broken(
        "docs/contracts/change/strategies.yaml",
        "  - id: sequential\n    parameters: []\n    bounded: true\n    gated_first_wave: false\n    default: true",
        "  - id: sequential\n    parameters: []\n    bounded: true\n    gated_first_wave: false\n    default: false",
    );
    assert_refuses(&tree, "0 strategies are marked default");
}

#[test]
fn should_reject_a_bulk_plan_whose_membership_is_not_frozen() {
    let tree = broken(
        "docs/contracts/change/strategies.yaml",
        "  frozen_membership: true",
        "  frozen_membership: false",
    );
    assert_refuses(
        &tree,
        "§2.6: newly matching objects MUST NOT silently join a bulk plan at apply time",
    );
}

// --- §12's providers and Appendix G's fixtures --------------------------------------------------

#[test]
fn should_reject_a_provider_naming_a_crate_that_does_not_exist() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "    crate: ono-recovery-zfs",
        "    crate: ono-recovery-zed",
    );
    assert_refuses(
        &tree,
        "provider `ono.recovery.zfs` names the crate `ono-recovery-zed`, which does not exist",
    );
}

#[test]
fn should_reject_a_provider_creating_an_asset_type_the_asset_registry_does_not_define() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "    asset_types: [zfs-snapshot]",
        "    asset_types: [zfs-clone]",
    );
    assert_refuses(
        &tree,
        "provider `ono.recovery.zfs` creates the asset type `zfs-clone`, which `assets.yaml` does \
         not define",
    );
}

#[test]
fn should_reject_a_fixture_set_shorter_than_appendix_g_ones_eleven() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "  - capacity-failure\n",
        "",
    );
    assert_refuses(&tree, "`required_fixtures` lists 10 entries");
}

#[test]
fn should_reject_destructive_tests_that_are_not_forbidden_on_production_filesystems() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "  production_filesystems: forbidden",
        "  production_filesystems: discouraged",
    );
    assert_refuses(
        &tree,
        "production host filesystems MUST never be used for test rollback",
    );
}

// --- §44's privacy rules ------------------------------------------------------------------------

#[test]
fn should_reject_a_privacy_policy_that_states_fewer_rules_than_section_forty_four_does() {
    // §44.6 is the last rule of the file, so folding its keys into the rule above it leaves five
    // entries where §44 states six — the shape a rule that fell out of the list really has.
    let tree = copied();
    let policies = "docs/contracts/recovery/policies.yaml";
    edit(
        &tree,
        policies,
        "  - id: deletion-is-real\n    rule: \"§44.6",
        "    deletion_rule: \"§44.6",
    );
    edit(
        &tree,
        policies,
        "    mechanism: >-\n      `cleanup` removes what the provider owns",
        "    deletion_mechanism: >-\n      `cleanup` removes what the provider owns",
    );
    assert_refuses(&tree, "`privacy` lists 5 rules; §44 states six");
}

#[test]
fn should_reject_a_privacy_rule_that_names_no_mechanism() {
    let tree = broken(
        "docs/contracts/recovery/policies.yaml",
        "  - id: store-is-private\n    rule: \"§44.3: local recovery stores MUST use restrictive permissions.\"\n    mechanism: >-",
        "  - id: store-is-private\n    rule: \"§44.3: local recovery stores MUST use restrictive permissions.\"\n    mechanism_notes: >-",
    );
    assert_refuses(
        &tree,
        "privacy rule `store-is-private` states no `mechanism`",
    );
}

// --- §53's configuration --------------------------------------------------------------------

#[test]
fn should_reject_a_default_that_is_not_the_one_the_shell_uses() {
    // §53's closing line — configuration MUST NOT silently weaken explicit plan requirements —
    // is unverifiable if the documented defaults and the real ones are different documents.
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "{key: change.default_protection, type: string, default: prefer",
        "{key: change.default_protection, type: string, default: require",
    );
    assert_refuses(
        &tree,
        "the declared defaults do not produce the shell's own defaults",
    );
}

#[test]
fn should_reject_a_settings_key_the_shell_does_not_read() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "{key: recovery.retention,",
        "{key: recovery.retention_window,",
    );
    assert_refuses(&tree, "`settings` lists");
}

#[test]
fn should_reject_a_default_whose_declared_type_it_is_not() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "{key: change.bulk.warn_targets, type: int, default: 10",
        "{key: change.bulk.warn_targets, type: int, default: ten",
    );
    assert_refuses(&tree, "which do not go together");
}

// --- §23's verification model ---------------------------------------------------------------

#[test]
fn should_reject_a_class_whose_declared_failure_is_not_the_verdict_the_shell_reaches() {
    let tree = broken(
        "docs/contracts/change/verification.yaml",
        "{id: required, on_failure: failed",
        "{id: required, on_failure: degraded",
    );
    assert_refuses(&tree, "class `required` declares `on_failure: degraded`");
}

#[test]
fn should_reject_a_registry_that_counts_an_unanswered_check_as_a_pass() {
    // §23.5's specific prohibition: a timeout is not a success. This row is what would authorise
    // treating one as a success, so breaking it must be the thing the gate names.
    let tree = broken(
        "docs/contracts/change/verification.yaml",
        "{id: unknown, counts_as_pass: false",
        "{id: unknown, counts_as_pass: true",
    );
    assert_refuses(&tree, "status `unknown` declares `counts_as_pass: true`");
}

#[test]
fn should_reject_a_verification_registry_that_permits_waiting_forever() {
    let tree = broken(
        "docs/contracts/change/verification.yaml",
        "  unbounded_permitted: false",
        "  unbounded_permitted: true",
    );
    assert_refuses(&tree, "`timeouts.unbounded_permitted` is not `false`");
}

#[test]
fn should_reject_a_declared_timeout_default_that_is_not_the_one_a_contract_gets() {
    let tree = broken(
        "docs/contracts/change/verification.yaml",
        "  default: 30s",
        "  default: 5m",
    );
    assert_refuses(&tree, "`timeouts.default` is `5m`");
}

#[test]
fn should_reject_an_equivalence_domain_the_shell_cannot_report_on() {
    // §25.3 forbids "rollback successful" without a scope, and these are the scopes. A registry
    // that invents a fourth would be promising a scope no verification can produce.
    let tree = broken(
        "docs/contracts/change/verification.yaml",
        "  - {id: external-side-effect,",
        "  - {id: cloud-side-effect, doc: \"invented\"}\n  - {id: external-side-effect,",
    );
    assert_refuses(
        &tree,
        "`equivalence_domains` declares the equivalence domain",
    );
}

// --- Appendix H's profiles ------------------------------------------------------------------

#[test]
fn should_reject_a_profile_that_expands_to_settings_the_shell_does_not_apply() {
    let tree = broken(
        "docs/contracts/recovery/policies.yaml",
        "  - id: cautious\n    protection: require",
        "  - id: cautious\n    protection: prefer",
    );
    assert_refuses(&tree, "profile `cautious` declares `protection: prefer`");
}

#[test]
fn should_reject_a_profile_that_permits_opaque_actions() {
    // §6.2 keeps an arbitrary external command unplannable by default, and Appendix H.5 forbids a
    // profile weakening a safety constraint. A profile is the one place both could be undone at
    // once, for every plan run under it.
    let tree = broken(
        "docs/contracts/recovery/policies.yaml",
        "  - id: fleet\n    protection: prefer\n    risk_gate: high+\n    strategy: canary 1 then batch 10%\n    remote_unknown: stop-new-batches\n    retention: 24h\n    opaque_actions: false",
        "  - id: fleet\n    protection: prefer\n    risk_gate: high+\n    strategy: canary 1 then batch 10%\n    remote_unknown: stop-new-batches\n    retention: 24h\n    opaque_actions: true",
    );
    assert_refuses(
        &tree,
        "profile `fleet` does not declare `opaque_actions: false`",
    );
}

#[test]
fn should_reject_an_authority_block_that_lets_configuration_weaken_a_plan() {
    let tree = broken(
        "docs/contracts/recovery/policies.yaml",
        "  configuration_may_weaken_plan: false",
        "  configuration_may_weaken_plan: true",
    );
    assert_refuses(&tree, "`authority.configuration_may_weaken_plan` is not");
}

#[test]
fn should_reject_an_auto_recovery_policy_that_is_on_by_default() {
    let tree = broken(
        "docs/contracts/recovery/policies.yaml",
        "auto_recovery:\n  default: off",
        "auto_recovery:\n  default: on",
    );
    assert_refuses(&tree, "`auto_recovery.default` is not `off`");
}

#[test]
fn should_reject_an_auto_recovery_policy_missing_one_of_its_six_conditions() {
    // §26.3's conditions are conjunctive, so a list one short is a declaration that would be
    // accepted where the specification rejects it.
    let tree = broken(
        "docs/contracts/recovery/policies.yaml",
        "    - \"recovery verification exists\"\n",
        "",
    );
    assert_refuses(&tree, "lists 5 entries and §26.3 names six");
}

#[test]
fn should_reject_an_auto_recovery_declaration_rejected_later_than_seal() {
    let tree = broken(
        "docs/contracts/recovery/policies.yaml",
        "  rejected_at: seal",
        "  rejected_at: apply",
    );
    assert_refuses(&tree, "`auto_recovery.rejected_at` is not `seal`");
}

// --- §11.4, §37 and §38: what an asset costs and how long it lives ---------------------------

#[test]
fn should_reject_a_validation_check_nothing_records() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  - {id: permissions-present,",
        "  - {id: quota-available, doc: \"invented\"}\n  - {id: permissions-present,",
    );
    assert_refuses(&tree, "`validation_checks` declares the validation check");
}

#[test]
fn should_reject_a_cost_dimension_the_model_cannot_carry() {
    // §38.1 names six. A registry that names a seventh promises a figure no asset can hold, and
    // one that drops one hides a cost §38.2 forbids showing as free.
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "dimensions: [initial-latency, retained-storage-growth, io-overhead,",
        "dimensions: [initial-latency, retained-storage-growth,",
    );
    assert_refuses(&tree, "`cost.dimensions` omits the cost dimension");
}

#[test]
fn should_reject_a_registry_that_permits_calling_a_snapshot_free() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  free_permitted: false",
        "  free_permitted: true",
    );
    assert_refuses(&tree, "`cost.free_permitted` is not `false`");
}

#[test]
fn should_reject_a_retention_default_that_is_not_the_one_an_asset_gets() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "retention:\n  default: 24h",
        "retention:\n  default: 48h",
    );
    assert_refuses(&tree, "`retention.default` is `48h`");
}

#[test]
fn should_reject_a_failure_state_that_ordinary_retention_would_still_delete() {
    // §37.2: the assets of a failed plan are exactly the ones somebody may still need. A state
    // missing from this list is a state whose assets the 24-hour rule quietly reclaims.
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "failure_states_exempt: [failed, degraded, apply-failed, prepare-failed, recovery-failed]",
        "failure_states_exempt: [failed, degraded, apply-failed, prepare-failed]",
    );
    assert_refuses(
        &tree,
        "`retention.failure_states_exempt` omits the plan state",
    );
}

#[test]
fn should_reject_a_policy_limit_the_shell_cannot_apply() {
    let tree = broken(
        "docs/contracts/recovery/assets.yaml",
        "  - {key: snapshot-count,",
        "  - {key: snapshot-quota, doc: \"invented\"}\n  - {key: snapshot-count,",
    );
    assert_refuses(&tree, "`limits` declares the policy limit `snapshot-quota`");
}

// --- §39.2 and §12.1: which provider may claim what -------------------------------------------

#[test]
fn should_reject_a_storage_provider_that_claims_application_consistency() {
    // §39.2's own example: a filesystem snapshot of a running database is crash-consistent, and a
    // storage provider labelling it application-consistent is asserting a guarantee on the
    // database's behalf. This is the check that catches it in the source rather than in review.
    let tree = copied();
    edit(
        &tree,
        "crates/ono-recovery-zfs/src/provider.rs",
        ".at_consistency(ConsistencyClass::FilesystemConsistent)",
        ".at_consistency(ConsistencyClass::ApplicationConsistent)",
    );
    assert_refuses(&tree, "claims `application-consistent` consistency");
}

#[test]
fn should_reject_a_provider_row_that_states_no_role() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "  - id: ono.recovery.btrfs\n    role: storage-provider\n",
        "  - id: ono.recovery.btrfs\n",
    );
    assert_refuses(&tree, "provider `ono.recovery.btrfs` declares no `role`");
}

#[test]
fn should_reject_a_provider_row_whose_identity_no_crate_declares() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "  - id: ono.recovery.file-copy",
        "  - id: ono.recovery.file-archive",
    );
    assert_refuses(&tree, "`providers` declares the recovery provider");
}

#[test]
fn should_reject_a_provider_capability_the_broker_does_not_know() {
    // §48.3's names are a boundary only where the capability broker knows them. One that is not in
    // `capabilities.yaml` cannot be granted or denied, so a provider running under it runs under
    // nothing. The break is on the broker's side, because that is the side that goes missing: the
    // vocabulary check already catches a capability the shell does not implement.
    let tree = broken(
        "docs/contracts/capabilities.yaml",
        "id: recovery.discover",
        "id: recovery.enumerate",
    );
    assert_refuses(
        &tree,
        "`capabilities` declares `recovery.discover`, which \
         `docs/contracts/capabilities.yaml` does not define",
    );
}

#[test]
fn should_reject_a_provider_that_executes_semantics_it_has_not_validated() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "version_variance:\n  degrade_to: unsupported",
        "version_variance:\n  degrade_to: best-effort",
    );
    assert_refuses(&tree, "`version_variance.degrade_to` is not `unsupported`");
}

#[test]
fn should_reject_a_truth_test_no_test_presents() {
    // Appendix G.2 calls its eight layouts examples, so the list stays open and what is held is
    // that each row names a test that exists. A row nobody tests is a provider trusted for
    // nothing.
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "  - {id: read-only-filesystem,",
        "  - {id: read-only-filesystem-renamed-away,",
    );
    assert_refuses(
        &tree,
        "`truth_tests` declares `read-only-filesystem-renamed-away` and no test carries the marker",
    );
}

#[test]
fn should_reject_a_truth_test_set_smaller_than_appendix_g_two_names() {
    let tree = broken(
        "docs/contracts/recovery/providers.yaml",
        "  - {id: read-only-filesystem, doc: \"A read-only filesystem that prevents a restore. §11.4.\"}\n",
        "",
    );
    assert_refuses(&tree, "`truth_tests` lists 7 entries");
}

// --- §39.3's quiesce protocol -----------------------------------------------------------------

#[test]
fn should_reject_a_quiesce_protocol_that_does_not_have_to_resume_the_application() {
    // §18.4 makes a failure to resume its own critical error, because a still-paused application
    // is a different fact from a snapshot that did not happen.
    let tree = broken(
        "docs/contracts/recovery/consistency.yaml",
        "  resume_on_failure_required: true",
        "  resume_on_failure_required: false",
    );
    assert_refuses(
        &tree,
        "`quiesce_protocol.resume_on_failure_required` is not `true`",
    );
}

#[test]
fn should_reject_a_quiesce_protocol_missing_one_of_its_five_steps() {
    let tree = broken(
        "docs/contracts/recovery/consistency.yaml",
        "  steps: [prepare_quiesce, verify_quiesced, create_storage_asset, resume, verify_resumed]",
        "  steps: [prepare_quiesce, create_storage_asset, resume, verify_resumed]",
    );
    assert_refuses(&tree, "lists 4 steps and §39.3 names five");
}

// --- The checks that read source rather than a registry --------------------------------------

#[test]
fn should_reject_an_execution_method_row_that_names_no_variant_of_execution() {
    // Both directions at once: the renamed row names nothing, and the variant it described is
    // left without a row.
    let tree = broken(
        "docs/contracts/change/actions.yaml",
        "  - id: recovery-operation\n",
        "  - id: recovery-call\n",
    );
    assert_refuses(
        &tree,
        "`execution_methods` declares the execution method `recovery-call`, which nothing \
         implements",
    );
    assert_refuses(
        &tree,
        "`execution_methods` omits the execution method `recovery-operation`",
    );
}

#[test]
fn should_reject_an_execution_variant_the_registry_has_no_row_for() {
    // §2.17: a way of running an action that nobody has held to "no command line" is exactly the
    // method that would admit one.
    let tree = broken(
        "crates/ono-change-core/src/action.rs",
        "    /// An action the operator declared opaque",
        "    /// A command line.\n    ShellString {\n        line: Arc<str>,\n    },\n    /// An action \
         the operator declared opaque",
    );
    assert_refuses(
        &tree,
        "`execution_methods` omits the execution method `shell-string`",
    );
}

#[test]
fn should_reject_a_confidence_method_that_combines_two_confidences_beside_weakest_of() {
    let tree = broken(
        "crates/ono-change-core/src/effect.rs",
        "impl EffectConfidence {",
        "impl EffectConfidence {\n    /// The stronger of two confidences.\n    pub fn \
         strongest_of(self, other: Self) -> Self {\n        other\n    }\n",
    );
    assert_refuses(
        &tree,
        "`EffectConfidence::strongest_of` is an operation beside `weakest_of`",
    );
}

#[test]
fn should_reject_a_tool_driving_provider_that_validated_no_versions() {
    let tree = broken(
        "crates/ono-recovery-zfs/src/provider.rs",
        "pub const VALIDATED_VERSIONS: &[&str] = &[\"2.4.1\"];",
        "pub const VALIDATED_VERSIONS: &[&str] = &[];",
    );
    assert_refuses(
        &tree,
        "provider `ono.recovery.zfs` drives `zfs`, and its crate `ono-recovery-zfs` declares no \
         non-empty `pub const VALIDATED_VERSIONS: &[&str]`",
    );
}

#[test]
fn should_reject_a_tool_driving_provider_that_never_tests_a_version_against_its_list() {
    let tree = broken(
        "crates/ono-recovery-btrfs/src/provider.rs",
        "VALIDATED_VERSIONS.contains(",
        "[\"any\"].contains(",
    );
    assert_refuses(
        &tree,
        "provider `ono.recovery.btrfs`'s crate `ono-recovery-btrfs` never tests a version against \
         `VALIDATED_VERSIONS`",
    );
}

#[test]
fn should_reject_a_first_party_provider_that_declares_quiesce() {
    // §39.1: a capability declared without §39.3's protocol behind it is the overstatement.
    let tree = broken(
        "crates/ono-recovery-files/src/provider.rs",
        ".tested_against(\"ono.recovery.file-copy\"",
        ".recovering(RecoveryCapability::Quiesce)\n            \
         .tested_against(\"ono.recovery.file-copy\"",
    );
    assert_refuses(
        &tree,
        "names `recovery.quiesce`, and no first-party recovery provider may declare it",
    );
}

#[test]
fn should_name_the_settings_row_whose_declared_default_differs_from_the_shells() {
    let tree = broken(
        "docs/contracts/change/plans.yaml",
        "{key: change.bulk.warn_targets, type: int, default: 10,",
        "{key: change.bulk.warn_targets, type: int, default: 11,",
    );
    assert_refuses(&tree, "Differing: `change.bulk.warn_targets` reads `11`");
}
