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
const NEIGHBOURS: [&str; 4] = [
    "docs/contracts/commands/change.yaml",
    "docs/contracts/verbs.yaml",
    "docs/contracts/errors.yaml",
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
    }
    tree
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
