#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! §56.2's ten facts, one test each — and §56.3's ten refusals when one of them is missing.
//!
//! Each refusal test is a single-variable experiment: the same recorded script, with exactly one
//! answer replaced by a failing or empty one, so what blocks the recovery is unambiguous.

mod support;

use std::sync::Arc;

use ono_change_core::{
    RecoveryAsset, RecoveryAssetType, RecoveryGoal, RecoveryProvider, ToolOutput,
};
use ono_recovery_btrfs::{BtrfsMounts, BtrfsProvider, RecordedFiles, SafetyChecklist, SafetyFact};
use support::{
    NGINX_CONF, ROOT_ID, ROOT_SNAPSHOT, fixture, mounts, provider_with_files, replacing,
    root_asset, root_recovery_script, runner, var_asset, var_recovery_script,
};

/// The checklist the recorded output establishes in full.
fn complete() -> SafetyChecklist {
    provider_with_files(root_recovery_script())
        .plan_recovery_with_checklist(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded output establishes every one of §56.2's ten facts")
        .1
}

/// The error a recovery blocks with, given a script with one answer replaced.
fn blocked(script: Vec<ToolOutput>) -> ono_value::ErrorValue {
    provider_with_files(script)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect_err("§56.3: a fact that cannot be established blocks the recovery")
}

/// Asserts the refusal is §56.3's, and names `fact`.
fn refuses(error: &ono_value::ErrorValue, fact: SafetyFact) {
    assert_eq!(
        error.code().name(),
        "recovery.plan_incomplete",
        "§56.3: destructive recovery MUST be blocked rather than guessed"
    );
    assert!(
        error.message().contains(fact.description()),
        "and the refusal names the fact that could not be established: expected `{}`, got `{}`",
        fact.description(),
        error.message()
    );
}

// ---------------------------------------------------------------------------------------------
// §56.2: the ten facts the implementation proves.
// ---------------------------------------------------------------------------------------------

#[test]
fn should_prove_the_exact_filesystem_and_subvolume_id() {
    let evidence = complete()
        .evidence(SafetyFact::FilesystemAndSubvolumeId)
        .expect("§56.2: the exact filesystem and subvolume ID")
        .to_owned();
    assert!(evidence.contains(support::FILESYSTEM));
    assert!(evidence.contains(&ROOT_ID.to_string()));
    assert!(
        evidence.contains("rather than from the shape of the path"),
        "Appendix B.9: the id comes from real metadata, and the evidence says which command \
         produced it"
    );
}

#[test]
fn should_prove_that_the_target_snapshot_exists_and_is_valid() {
    let evidence = complete()
        .evidence(SafetyFact::SnapshotExistsAndValid)
        .expect("§56.2: the target snapshot exists and is valid")
        .to_owned();
    assert!(evidence.contains(ROOT_SNAPSHOT));
    assert!(
        evidence.contains("bc1638ef-a9cb-5141-aef0-b9a662b98266"),
        "§11.4: the recovery point is identified by what it is, not by the path it was found at"
    );
}

#[test]
fn should_prove_the_nested_subvolume_boundaries() {
    let evidence = complete()
        .evidence(SafetyFact::NestedBoundaries)
        .expect("§56.2: the nested subvolume boundaries")
        .to_owned();
    assert!(
        evidence.contains("@var/lib-app") && evidence.contains("@var"),
        "§14.3: every boundary beneath the subvolume being recovered is named"
    );
    assert!(evidence.contains("empty directory"));
}

#[test]
fn should_prove_whether_a_selected_restore_is_possible() {
    let evidence = complete()
        .evidence(SafetyFact::SelectiveRestorePossible)
        .expect("§56.2: whether selected restore is possible")
        .to_owned();
    assert!(
        evidence.contains("one at a time"),
        "Appendix C.1: the least-destructive method is available only if the wanted objects can \
         be read out of the snapshot individually"
    );
}

#[test]
fn should_prove_whether_subvolume_replacement_is_required() {
    let evidence = complete()
        .evidence(SafetyFact::SubvolumeReplacementRequired)
        .expect("§56.2: whether subvolume replacement is required")
        .to_owned();
    assert!(
        evidence.contains("replacing the subvolume is not required"),
        "§14.4: for a selective restore it is not, and the checklist says so rather than leaving \
         the question open"
    );
    assert!(
        evidence.contains("b08a487d-1079-ac45-9f7e-12d9f15bea63"),
        "§14.2: the answer rests on the snapshot's lineage — its parent uuid is the subvolume's own"
    );
}

#[test]
fn should_prove_the_default_subvolume_and_boot_impact() {
    let evidence = complete()
        .evidence(SafetyFact::DefaultSubvolumeAndBootImpact)
        .expect("§56.2: default-subvolume/boot impact")
        .to_owned();
    assert!(
        evidence.contains("default"),
        "Appendix D.9: what the machine boots is part of what a root recovery decides"
    );
    assert!(
        evidence.contains("does not change the default subvolume"),
        "and a selective restore does not touch it, which is worth saying explicitly"
    );
}

#[test]
fn should_prove_the_mount_and_reboot_requirement() {
    let evidence = complete()
        .evidence(SafetyFact::MountAndRebootRequirement)
        .expect("§56.2: mount/reboot requirement")
        .to_owned();
    assert!(evidence.contains("/mnt/root"));
    assert!(
        evidence.contains("writable"),
        "Appendix G.2: a read-only mount would prevent the restore, so whether it is writable is \
         part of the fact"
    );
    assert!(evidence.contains("neither an unmount nor a reboot"));
}

#[test]
fn should_prove_the_later_files_and_state_that_would_be_discarded() {
    let evidence = complete()
        .evidence(SafetyFact::LaterStateDiscarded)
        .expect("§56.2: later files/state that would be discarded")
        .to_owned();
    assert!(
        evidence.contains("1 object(s) have changed"),
        "Appendix C.4: the recorded live file differs from the recorded snapshot, so a restore \
         would discard that change, and the count is established rather than assumed"
    );
}

#[test]
fn should_prove_the_correct_treatment_of_the_read_only_snapshot() {
    let evidence = complete()
        .evidence(SafetyFact::ReadOnlySnapshotTreatment)
        .expect("§56.2: correct treatment of the read-only snapshot")
        .to_owned();
    assert!(
        evidence.contains("ro=true"),
        "§14.5: the flag is read from `btrfs property get`, not assumed from the fact that `-r` \
         was passed when it was created"
    );
    assert!(evidence.contains("tracked as its own asset"));
}

#[test]
fn should_prove_that_no_recursive_snapshot_coverage_was_assumed() {
    let evidence = complete()
        .evidence(SafetyFact::NoRecursiveCoverageAssumption)
        .expect("§56.2: no assumption of recursive snapshot coverage")
        .to_owned();
    assert!(
        evidence.contains("does not hold"),
        "§14.3: every nested subvolume is recorded on the asset as something the snapshot does \
         not contain, which is what makes the absence of the assumption checkable"
    );
}

#[test]
fn should_start_with_every_one_of_the_ten_facts_outstanding() {
    let fresh = SafetyChecklist::outstanding();
    assert_eq!(fresh.missing().len(), 10);
    assert!(
        !fresh.is_complete(),
        "§56.3: a checklist nobody filled in blocks, so forgetting to check and checking and \
         failing are the same outcome"
    );
    assert!(fresh.refusal().is_some());
    assert_eq!(SafetyFact::ALL.len(), 10);
    for fact in SafetyFact::ALL {
        assert_eq!(SafetyFact::from_token(fact.token()), Some(fact));
    }
}

// ---------------------------------------------------------------------------------------------
// §56.3: fail closed. One test per fact, each with exactly one answer taken away.
// ---------------------------------------------------------------------------------------------

#[test]
fn should_block_when_the_filesystem_cannot_be_identified() {
    let error = blocked(replacing(
        root_recovery_script(),
        2,
        support::failure("ERROR: not a btrfs filesystem"),
    ));
    refuses(&error, SafetyFact::FilesystemAndSubvolumeId);
}

#[test]
fn should_block_when_the_target_snapshot_has_vanished() {
    let error = blocked(replacing(
        root_recovery_script(),
        0,
        fixture("subvol-show-missing"),
    ));
    refuses(&error, SafetyFact::SnapshotExistsAndValid);
}

#[test]
fn should_block_when_the_nested_boundaries_cannot_be_listed() {
    let error = blocked(replacing(
        root_recovery_script(),
        4,
        fixture("unprivileged-list"),
    ));
    refuses(&error, SafetyFact::NestedBoundaries);
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("Operation not permitted")),
        "§56.3: the refusal carries what the filesystem actually said"
    );
}

#[test]
fn should_block_when_a_wanted_object_lies_inside_a_nested_subvolume() {
    let error = provider_with_files(var_recovery_script())
        .plan_recovery(
            &var_asset(&["/mnt/root/var/lib-app/state.db"]),
            None,
            RecoveryGoal::RestoreChangedObjects,
        )
        .expect_err("§14.3: the snapshot of the parent does not hold it");
    refuses(&error, SafetyFact::SelectiveRestorePossible);
}

#[test]
fn should_block_when_the_recovery_point_is_not_a_snapshot_of_the_subvolume() {
    let error = blocked(replacing(
        root_recovery_script(),
        0,
        fixture("subvol-show-var"),
    ));
    refuses(&error, SafetyFact::SubvolumeReplacementRequired);
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("no parent uuid")),
        "§14.2: an object with no lineage is not a snapshot of anything, whatever its path says"
    );
}

#[test]
fn should_block_when_the_default_subvolume_cannot_be_read() {
    let error = blocked(replacing(
        root_recovery_script(),
        5,
        support::failure("ERROR: unable to get default subvolume"),
    ));
    refuses(&error, SafetyFact::DefaultSubvolumeAndBootImpact);
}

#[test]
fn should_block_when_the_subvolume_is_not_mounted_anywhere_visible() {
    let without_root = BtrfsMounts::from_mountinfo(
        &fixture("mountinfo")
            .stdout()
            .lines()
            .filter(|line| !line.contains(" /mnt/root "))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let provider = BtrfsProvider::new(runner(root_recovery_script()))
        .with_mounts(without_root)
        .with_files(Arc::new(support::recorded_files()));
    let error = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect_err("§56.3: where the subvolume is mounted decides what recovery requires");
    refuses(&error, SafetyFact::MountAndRebootRequirement);
}

#[test]
fn should_block_when_the_later_state_cannot_be_read() {
    let provider = BtrfsProvider::new(runner(root_recovery_script()))
        .with_mounts(mounts())
        .with_files(Arc::new(RecordedFiles::new()));
    let error = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect_err("§62.8: recovering without knowing what is newer is not acceptable");
    refuses(&error, SafetyFact::LaterStateDiscarded);
}

#[test]
fn should_block_when_the_retained_snapshot_turns_out_to_be_writable() {
    let error = blocked(replacing(
        root_recovery_script(),
        1,
        ToolOutput::ok("ro=false\n"),
    ));
    refuses(&error, SafetyFact::ReadOnlySnapshotTreatment);
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("no longer provably the state that was captured")),
        "§14.5: a writable recovery point is one anything could have changed since"
    );
}

#[test]
fn should_block_when_the_asset_says_nothing_about_a_nested_subvolume() {
    let silent = RecoveryAsset::proposed(
        ono_recovery_btrfs::PROVIDER_ID,
        RecoveryAssetType::BtrfsSnapshot,
        ROOT_SNAPSHOT,
        support::scope(ROOT_ID, "@", &[NGINX_CONF]),
        jiff::Timestamp::UNIX_EPOCH,
    );
    let error = provider_with_files(root_recovery_script())
        .plan_recovery(&silent, None, RecoveryGoal::RestoreChangedObjects)
        .expect_err("§14.3: an asset that names no boundary is one that assumed there were none");
    refuses(&error, SafetyFact::NoRecursiveCoverageAssumption);
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("recursive assumption")),
        "§56.2's tenth fact is exactly this: no assumption of recursive snapshot coverage"
    );
}

#[test]
fn should_name_the_other_outstanding_facts_in_the_refusal_it_raises() {
    let error = blocked(replacing(
        root_recovery_script(),
        2,
        support::failure("ERROR: not a btrfs filesystem"),
    ));
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("Also outstanding")),
        "§56.3: one fact is named in the message and the rest travel with it, so a person fixes \
         the first and does not discover the second afterwards"
    );
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("Nothing was changed")),
        "and a blocked recovery has changed nothing, which the refusal says outright"
    );
}
