//! §56.1's destructive-recovery safety checklist, one test per fact, twice over.
//!
//! §56 makes the checklist mandatory *before enabling a first-party destructive recovery path*,
//! and §56.3 says what happens when one of its twelve facts cannot be established: destructive
//! recovery is blocked rather than guessed. Every fact therefore appears here twice — once proved
//! against the recorded pool, and once with the query it rests on failing or empty, asserting
//! that the provider refuses with `recovery.plan_incomplete` and names the fact it could not
//! establish.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::sync::Arc;

use ono_change_core::{
    RecoveryAsset, RecoveryAssetType, RecoveryGoal, RecoveryProvider, RecoveryScope, ToolOutput,
    ToolRunner,
};
use ono_recovery_zfs::{GUID_FINGERPRINT, MountTable, SafetyChecklist, ZFS, ZfsFact, ZfsProvider};
use ono_value::ErrorValue;

use support::{
    NEWER_BOOKMARK, NEWER_SNAPSHOT, ROOT_DATASET, ROOT_SNAPSHOT, ROOT_SNAPSHOT_GUID, Script, Slot,
    asset, blocked_facts, code, instant, out, provider,
};

/// The checklist the recorded pool proves, with every query answered as it really answered.
fn proven() -> SafetyChecklist {
    let tools = Script::examination().runner();
    provider(&tools)
        .safety_checklist(&asset())
        .expect("the recorded pool answers every query")
}

/// The evidence recorded for one fact, which is the sentence a refusal or a plan shows.
fn evidence(fact: ZfsFact) -> String {
    proven()
        .evidence(fact)
        .expect("every fact has an entry")
        .detail()
        .to_owned()
}

/// The refusal `plan_recovery` gives for a destructive goal when `script` cannot answer.
fn blocked_by(script: Script) -> ErrorValue {
    let tools = script.runner();
    provider(&tools)
        .plan_recovery(&asset(), None, RecoveryGoal::RestoreDomain)
        .expect_err("§56.3: destructive recovery is blocked rather than guessed")
}

/// Asserts the §56.3 refusal names `fact` among the ones it could not establish.
fn assert_blocked(error: &ErrorValue, fact: ZfsFact) {
    assert_eq!(
        code(error),
        "recovery.plan_incomplete",
        "§56.3: a critical recovery fact that could not be established blocks the path"
    );
    assert!(
        blocked_facts(error).contains(&fact.as_str().to_owned()),
        "§56.1: the refusal names `{}` among the facts it could not prove, got {:?}",
        fact.as_str(),
        blocked_facts(error)
    );
}

// ---------------------------------------------------------------------------------------------
// The twelve facts, proved.
// ---------------------------------------------------------------------------------------------

#[test]
fn should_prove_the_exact_dataset_identity() {
    assert!(proven().is_established(ZfsFact::DatasetIdentity));
    assert!(
        evidence(ZfsFact::DatasetIdentity).contains(ROOT_DATASET),
        "§56.1: the dataset is named, out of ZFS's own listing"
    );
}

#[test]
fn should_prove_the_exact_snapshot_identity() {
    assert!(proven().is_established(ZfsFact::SnapshotIdentity));
    assert!(
        evidence(ZfsFact::SnapshotIdentity).contains(ROOT_SNAPSHOT_GUID),
        "§56.1: identity is the GUID, because a name can be reused"
    );
}

#[test]
fn should_prove_the_target_snapshot_still_exists() {
    assert!(proven().is_established(ZfsFact::SnapshotExists));
    assert!(evidence(ZfsFact::SnapshotExists).contains(ROOT_SNAPSHOT));
}

#[test]
fn should_prove_whether_it_is_the_latest_relevant_snapshot() {
    assert!(proven().is_established(ZfsFact::LatestRelevantSnapshot));
    assert!(
        evidence(ZfsFact::LatestRelevantSnapshot).contains("followed by 1 newer snapshot"),
        "§56.1: whether it is the latest is what decides whether history is destroyed"
    );
}

#[test]
fn should_prove_all_newer_snapshots_and_bookmarks_affected_by_rollback() {
    assert!(proven().is_established(ZfsFact::NewerSnapshotsAndBookmarks));
    assert!(
        evidence(ZfsFact::NewerSnapshotsAndBookmarks)
            .contains("1 newer snapshot(s) and 1 affected bookmark(s)"),
        "Appendix D.5: both are enumerated before a rollback is offered"
    );
}

#[test]
fn should_prove_all_clones_affected_by_destructive_flags() {
    assert!(proven().is_established(ZfsFact::AffectedClones));
    assert!(
        evidence(ZfsFact::AffectedClones).contains("no clone"),
        "§13.6: the recorded boot environment has none, and the absence is a proven fact"
    );
}

#[test]
fn should_prove_the_child_dataset_boundaries() {
    assert!(proven().is_established(ZfsFact::ChildDatasetBoundaries));
    assert!(
        evidence(ZfsFact::ChildDatasetBoundaries).contains("descendant dataset"),
        "§13.4: a child dataset is a separate boundary a rollback does not reach"
    );
}

#[test]
fn should_prove_the_mount_and_unmount_requirement() {
    assert!(proven().is_established(ZfsFact::MountRequirement));
    assert!(
        evidence(ZfsFact::MountRequirement).contains("mounted at `/altroot/debian`"),
        "§13.7: where the dataset is, and whether it is there now"
    );
}

#[test]
fn should_prove_the_reboot_or_offline_requirement() {
    assert!(proven().is_established(ZfsFact::RebootOrOfflineRequirement));
    assert!(
        evidence(ZfsFact::RebootOrOfflineRequirement).contains("boot environment"),
        "§13.7: the recorded dataset is a boot environment, so recovery takes effect on reboot"
    );
}

#[test]
fn should_prove_the_expected_discarded_live_data() {
    assert!(proven().is_established(ZfsFact::DiscardedLiveData));
    assert!(
        evidence(ZfsFact::DiscardedLiveData).contains("written"),
        "Appendix D.5: the changed-live-data estimate comes from `zfs get written`"
    );
}

#[test]
fn should_prove_sufficient_privilege() {
    assert!(proven().is_established(ZfsFact::SufficientPrivilege));
    assert!(
        evidence(ZfsFact::SufficientPrivilege).contains("permission refusal"),
        "§11.4 and §43.4: privilege is established from what the queries themselves reported"
    );
}

#[test]
fn should_prove_that_explicit_acceptance_for_history_destruction_is_required_and_enumerated() {
    assert!(proven().is_established(ZfsFact::HistoryDestructionAccepted));
    assert!(
        evidence(ZfsFact::HistoryDestructionAccepted).contains("2 object(s) would be destroyed"),
        "§13.6: acceptance is explicit only when what it covers has been enumerated"
    );
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    let destroyed: Vec<String> = fragment
        .newer_state()
        .destroyed_assets()
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    assert_eq!(
        destroyed,
        vec![NEWER_SNAPSHOT.to_owned(), NEWER_BOOKMARK.to_owned()]
    );
    assert!(fragment.newer_state().requires_destructive_acceptance());
}

#[test]
fn should_prove_all_twelve_facts_against_the_recorded_pool() {
    assert!(
        proven().is_complete(),
        "§56.1: twelve facts, and the recorded pool answers every one, missing {:?}",
        proven()
            .unestablished()
            .iter()
            .map(|fact| fact.as_str())
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------------------------
// §56.3, twelve times over: a fact that cannot be established blocks rather than being guessed.
// ---------------------------------------------------------------------------------------------

#[test]
fn should_block_when_the_exact_dataset_identity_cannot_be_established() {
    let error = blocked_by(Script::examination().answering(Slot::Filesystems, ToolOutput::ok("")));
    assert_blocked(&error, ZfsFact::DatasetIdentity);
}

#[test]
fn should_block_when_the_exact_snapshot_identity_cannot_be_established() {
    let tools = Script::examination().runner();
    let recreated = asset().capturing(format!("{GUID_FINGERPRINT}1"));
    let error = provider(&tools)
        .plan_recovery(&recreated, None, RecoveryGoal::RestoreDomain)
        .expect_err("§56.3: a name that came back is not the snapshot the asset recorded");
    assert_blocked(&error, ZfsFact::SnapshotIdentity);
}

#[test]
fn should_block_when_the_target_snapshot_no_longer_exists() {
    let error = blocked_by(Script::examination().answering(Slot::Snapshots, ToolOutput::ok("")));
    assert_blocked(&error, ZfsFact::SnapshotExists);
}

#[test]
fn should_block_when_whether_it_is_the_latest_relevant_snapshot_cannot_be_established() {
    let error =
        blocked_by(Script::examination().answering(Slot::SnapshotOrder, out("list-missing")));
    assert_blocked(&error, ZfsFact::LatestRelevantSnapshot);
}

#[test]
fn should_block_when_the_newer_snapshots_and_bookmarks_cannot_be_enumerated() {
    let error = blocked_by(Script::examination().answering(Slot::Bookmarks, out("list-missing")));
    assert_blocked(&error, ZfsFact::NewerSnapshotsAndBookmarks);
}

#[test]
fn should_block_when_the_clones_affected_by_destructive_flags_cannot_be_enumerated() {
    let error = blocked_by(Script::examination().answering(Slot::Clones, out("list-missing")));
    assert_blocked(&error, ZfsFact::AffectedClones);
}

#[test]
fn should_block_when_the_child_dataset_boundaries_cannot_be_established() {
    let error = blocked_by(Script::examination().answering(Slot::Filesystems, out("list-missing")));
    assert_blocked(&error, ZfsFact::ChildDatasetBoundaries);
}

#[test]
fn should_block_when_the_mount_requirement_cannot_be_established() {
    let error = blocked_by(Script::examination().answering(Slot::Placement, ToolOutput::ok("")));
    assert_blocked(&error, ZfsFact::MountRequirement);
}

#[test]
fn should_block_when_the_reboot_or_offline_requirement_cannot_be_established() {
    // The dataset's own properties still answer, and the kernel mount table does not — so
    // whether this dataset carries the running system is exactly the fact that is missing.
    let tools = Script::examination().runner();
    let runner: Arc<dyn ToolRunner> = Arc::clone(&tools) as Arc<dyn ToolRunner>;
    let error = ZfsProvider::new(runner)
        .reading_mounts(MountTable::default())
        .at_instant(instant())
        .plan_recovery(&asset(), None, RecoveryGoal::RestoreDomain)
        .expect_err("§56.3: without the mount table the reboot requirement is a guess");
    assert_blocked(&error, ZfsFact::RebootOrOfflineRequirement);
}

#[test]
fn should_block_when_the_expected_discarded_live_data_cannot_be_established() {
    let error = blocked_by(Script::examination().answering(Slot::Written, ToolOutput::ok("")));
    assert_blocked(&error, ZfsFact::DiscardedLiveData);
}

#[test]
fn should_block_when_sufficient_privilege_cannot_be_established() {
    let error = blocked_by(Script::examination().answering(Slot::Space, out("unprivileged-list")));
    assert_blocked(&error, ZfsFact::SufficientPrivilege);
}

#[test]
fn should_block_when_what_acceptance_would_cover_cannot_be_enumerated() {
    let error =
        blocked_by(Script::examination().answering(Slot::Clones, ToolOutput::failed(1, "")));
    assert_blocked(&error, ZfsFact::HistoryDestructionAccepted);
}

// ---------------------------------------------------------------------------------------------
// What the fail-closed rule does and does not gate.
// ---------------------------------------------------------------------------------------------

#[test]
fn should_name_the_first_missing_fact_in_the_order_section_fifty_six_one_lists_them() {
    let error = blocked_by(Script::examination().answering(Slot::Filesystems, out("list-missing")));
    assert_eq!(
        error.metadata().get("fact"),
        Some(&ono_value::Value::string("exact dataset identity")),
        "§56.1: the refusal leads with the first fact in the specification's own order"
    );
}

#[test]
fn should_still_allow_a_selective_restore_when_only_a_destructive_fact_is_missing() {
    let tools = Script::examination()
        .answering(Slot::Bookmarks, out("list-missing"))
        .runner();
    let fragment = provider(&tools)
        .plan_recovery(&asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("§56.1 governs destructive paths; a file restore destroys nothing");
    assert_eq!(
        fragment.method(),
        ono_change_core::RestoreMethod::SelectiveFileRestore
    );
}

#[test]
fn should_block_every_path_when_the_snapshot_itself_cannot_be_found() {
    let tools = Script::examination()
        .answering(Slot::Snapshots, ToolOutput::ok(""))
        .runner();
    let error = provider(&tools)
        .plan_recovery(&asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect_err("§56.3: a snapshot nobody can find restores nothing, destructively or not");
    assert_eq!(code(&error), "recovery.plan_incomplete");
}

#[test]
fn should_block_a_restore_action_whose_facts_stopped_holding_between_plan_and_apply() {
    let tools = Script::examination()
        .answering(Slot::Snapshots, ToolOutput::ok(""))
        .runner();
    let action = ono_change_core::PlanAction::new(
        &support::plan(),
        0,
        ono_change_core::ActionRole::Recover,
        "zfs rollback rpool/ROOT/debian@ono-a82f-20260909T194500Z",
        ono_change_core::Execution::Program {
            program: Arc::from(ZFS),
            argv: vec![Arc::from("rollback"), Arc::from(ROOT_SNAPSHOT)],
        },
    );
    let error = provider(&tools)
        .with_accepted_history_destruction()
        .restore(&action, &asset())
        .expect_err("§56.3: the checklist is proved again before the destructive act, not once");
    assert_eq!(code(&error), "recovery.plan_incomplete");
}

#[test]
fn should_start_a_checklist_with_nothing_proven_so_a_forgotten_branch_fails_closed() {
    let empty = SafetyChecklist::unproven(ROOT_SNAPSHOT);
    assert_eq!(empty.unestablished().len(), ZfsFact::ALL.len());
    assert!(empty.blocking_error(false).is_some());
}
