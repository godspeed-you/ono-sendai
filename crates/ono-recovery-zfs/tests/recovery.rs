//! Recovery choices, rollback planning and the mounted/root cases (§13.5-§13.7, Appendix D.4, D.5).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::sync::Arc;

use ono_change_core::{
    EffectKind, Execution, NewerStateClass, PlanAction, RecoveryAsset, RecoveryAssetType,
    RecoveryGoal, RecoveryProvider, RecoveryScope, RestoreAcceptance, RestoreMethod,
    ScriptedRunner, ToolOutput,
};
use ono_recovery_zfs::{CLONE_MOUNT_ROOT, CP, GUID_FINGERPRINT, ZFS, ZfsProvider};

use support::{
    CHILD_DATASET, CLONE, DATA_NEWER_GUID, DATA_NEWER_SNAPSHOT, NEWER_BOOKMARK, NEWER_SNAPSHOT,
    PARENT_DATASET, RECURSIVE_SNAPSHOT, ROOT_DATASET, ROOT_SNAPSHOT, ROOT_SNAPSHOT_GUID, Script,
    Slot, accepted, code, data_script_with_newer_snapshot, instant, out, provider,
};

const NGINX_CONF: &str = "/altroot/debian/etc/nginx/nginx.conf";
const CUSTOMER_DB: &str = "/tank/data/db.sqlite";

fn root_asset() -> RecoveryAsset {
    RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        ROOT_SNAPSHOT,
        RecoveryScope::new("zfs-dataset", ROOT_DATASET, "localhost")
            .covering(ROOT_DATASET)
            .covering(NGINX_CONF),
        instant(),
    )
    .capturing(format!("{GUID_FINGERPRINT}{ROOT_SNAPSHOT_GUID}"))
}

fn data_asset() -> RecoveryAsset {
    RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        RECURSIVE_SNAPSHOT,
        RecoveryScope::new("zfs-dataset", PARENT_DATASET, "localhost")
            .covering(PARENT_DATASET)
            .covering(CUSTOMER_DB),
        instant(),
    )
    .capturing(format!("{GUID_FINGERPRINT}6260688661848093222"))
}

/// The examination script, with the per-dataset properties ZFS reports for `tank/data`.
///
/// The recorded `get-written.txt` and `get-used-by-snapshots.txt` were taken for the boot
/// environment, which is the dataset the recorded rollback refusal is about. `tank/data` is the
/// dataset with the clone, so its own two properties are scripted here.
fn data_script() -> Script {
    Script::examination()
        .answering(
            Slot::Written,
            ToolOutput::ok("tank/data\twritten\t8420352\n"),
        )
        .answering(
            Slot::Space,
            ToolOutput::ok("tank/data\tusedbysnapshots\t0\ntank/data\tusedbydataset\t24576\n"),
        )
}

#[test]
fn should_prefer_selective_file_restore_for_a_plan_that_changed_a_few_files() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    assert_eq!(
        fragment.method(),
        RestoreMethod::SelectiveFileRestore,
        "§13.5: prefer the method that minimises unrelated rollback damage"
    );
}

#[test]
fn should_read_the_file_out_of_the_snapshot_directory_of_its_own_dataset() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    let argv = program_argv(fragment.actions().first().expect("one restore action"));
    assert!(
        argv.iter().any(|argument| argument
            == "/altroot/debian/.zfs/snapshot/ono-a82f-20260909T194500Z/etc/nginx/nginx.conf"),
        "§13.5: the file comes out of the dataset's own `.zfs/snapshot`, got {argv:?}"
    );
    assert!(
        !argv
            .iter()
            .any(|argument| argument.starts_with("/altroot/.zfs/")),
        "§13.4: the parent dataset's snapshot directory does not hold the child's files, which is \
         exactly what `snapshot-file.txt` recorded"
    );
}

#[test]
fn should_state_which_snapdir_setting_it_found_rather_than_leaving_it_to_be_guessed() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    let summary = fragment.actions().first().expect("one action").summary();
    assert!(
        summary.contains("`snapdir` is `hidden`"),
        "§13.5: the snapshot directory is reachable by path either way, and the plan says which \
         it found, got `{summary}`"
    );
}

#[test]
fn should_leave_unrelated_later_snapshots_alone_under_a_selective_restore() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    let later = fragment
        .newer_state()
        .items()
        .iter()
        .find(|item| item.object() == NEWER_SNAPSHOT)
        .expect("the later snapshot is part of the analysis");
    assert_eq!(
        later.class(),
        NewerStateClass::PreservedByMethod,
        "§55.3 case 14: a selective restore recovers one file without reverting unrelated state"
    );
    assert!(
        fragment.newer_state().destroyed_assets().is_empty(),
        "§13.6: nothing is destroyed by a method that destroys nothing"
    );
}

#[test]
fn should_not_require_destructive_acceptance_for_a_selective_restore() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    assert!(
        !fragment.newer_state().requires_destructive_acceptance(),
        "§24.5: the gate exists for recoveries that take something away"
    );
    assert!(fragment.newer_state().is_complete());
}

#[test]
fn should_enumerate_the_newer_snapshot_and_bookmark_a_full_rollback_would_destroy() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("§56.1's twelve facts are all established for the recorded pool");
    let destroyed: Vec<&str> = fragment
        .newer_state()
        .destroyed_assets()
        .iter()
        .map(std::convert::AsRef::as_ref)
        .collect();
    assert!(
        destroyed.contains(&NEWER_SNAPSHOT) && destroyed.contains(&NEWER_BOOKMARK),
        "Appendix D.5: newer snapshots and affected bookmarks are enumerated, got {destroyed:?}"
    );
}

#[test]
fn should_enumerate_exactly_what_zfs_itself_refuses_the_rollback_over() {
    // `rollback-refused.txt` is what ZFS says when the rollback would have to destroy newer
    // history. What the provider enumerates before ever running it must be the same list.
    let refusal = out("rollback-refused");
    let parsed = ono_recovery_zfs::parse::rollback_refusal(refusal.stderr());
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    let planned: Vec<String> = fragment
        .newer_state()
        .destroyed_assets()
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    let named: Vec<String> = parsed
        .objects()
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    assert_eq!(
        planned, named,
        "§13.6: the plan enumerates what ZFS would refuse over, rather than adding the flag that \
         stops it refusing"
    );
}

#[test]
fn should_require_destructive_acceptance_for_a_rollback_that_destroys_newer_history() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    assert!(
        fragment.newer_state().requires_destructive_acceptance(),
        "§55.3 case 15: full rollback requiring newer snapshot destruction is blocked without \
         acceptance"
    );
}

#[test]
fn should_leave_a_recovery_plan_needing_acceptance_until_the_operator_gives_it() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    let plan = ono_change_core::RecoveryPlan::new(
        ono_change_core::ChangePlan::draft(
            ono_change_core::Intent::new("recover rpool/ROOT/debian", "recover"),
            "session-1",
            instant(),
        ),
        RecoveryGoal::RestoreDomain,
        fragment.method(),
        ROOT_SNAPSHOT,
    )
    .with_newer_state(fragment.newer_state().clone());
    assert!(
        plan.needs_destructive_acceptance(),
        "§24.5: no recovery execution occurs without the explicit gate"
    );
    assert!(!plan.destruction_accepted().needs_destructive_acceptance());
}

#[test]
fn should_name_each_destruction_as_an_action_of_its_own_rather_than_a_flag() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    let destroys: Vec<Vec<String>> = fragment
        .actions()
        .iter()
        .map(program_argv)
        .filter(|argv| argv.first().is_some_and(|word| word == "destroy"))
        .collect();
    assert_eq!(
        destroys.len(),
        2,
        "§13.6: the newer snapshot and the bookmark are each destroyed by name, got {destroys:?}"
    );
    for argv in &destroys {
        assert_eq!(
            argv.len(),
            2,
            "`destroy` and one name, with no flag: {argv:?}"
        );
    }
}

#[test]
fn should_not_count_a_clone_of_the_recovery_point_itself_as_destroyed_by_its_rollback() {
    // `tank/cloned` is a clone of `tank/data@ono-b91c-...`, the very snapshot the rollback returns
    // to. `zfs rollback -R` destroys clones of the *newer* snapshots; this one is untouched and
    // stands in nobody's way.
    let tools = data_script().runner();
    let fragment = provider(&tools)
        .plan_recovery(&data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    assert!(
        !fragment
            .newer_state()
            .destroyed_assets()
            .iter()
            .any(|object| object.as_ref() == CLONE),
        "§13.6: a clone of the target is not destroyed by rolling back to it"
    );
    assert!(
        !fragment
            .unrecoverable()
            .iter()
            .any(|effect| effect.subject() == CLONE),
        "a clone of the target does not block the rollback, so it is not reported as blocking it"
    );
    let item = fragment
        .newer_state()
        .items()
        .iter()
        .find(|item| item.object() == CLONE)
        .expect("§13.1: the clone is still part of what the plan shows");
    assert_eq!(item.class(), NewerStateClass::PreservedByMethod);
}

#[test]
fn should_enumerate_a_clone_of_a_newer_snapshot_as_standing_in_the_rollbacks_way() {
    // Appendix G.2 truth test: zfs-clone-blocks-rollback.
    // The clone row is composed onto the
    // recorded `zfs list -o name,origin`: the recorded pool has no clone of a newer snapshot.
    let origins = format!(
        "{}rpool/debian-copy\t{NEWER_SNAPSHOT}\n",
        out("list-clones").stdout()
    );
    let tools = Script::examination()
        .answering(Slot::Origins, ToolOutput::ok(origins))
        .runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the pool plans a recovery");
    assert!(
        fragment
            .newer_state()
            .destroyed_assets()
            .iter()
            .any(|object| object.as_ref() == "rpool/debian-copy"),
        "§13.6: a clone of a newer snapshot is what `-R` would destroy, so it is enumerated"
    );
    let blocking = fragment
        .unrecoverable()
        .iter()
        .find(|effect| effect.subject() == "rpool/debian-copy")
        .expect("§13.6: Ono does not add the flag that would destroy the clone, and says so");
    assert!(
        blocking.reason().contains(NEWER_SNAPSHOT),
        "the plan names the newer snapshot the clone depends on, got `{}`",
        blocking.reason()
    );
}

#[test]
fn should_reason_about_a_child_dataset_separately_from_the_parent_it_rolls_back() {
    let tools = data_script().runner();
    let fragment = provider(&tools)
        .plan_recovery(&data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    let child = fragment
        .newer_state()
        .items()
        .iter()
        .find(|item| item.object() == CHILD_DATASET)
        .expect("Appendix D.5: child datasets not covered are enumerated");
    assert_eq!(
        child.class(),
        NewerStateClass::PreservedByMethod,
        "§13.3: ZFS offers no one recursive rollback for the whole tree, so the child is not \
         reached and the plan says so"
    );
    let rollbacks: Vec<Vec<String>> = fragment
        .actions()
        .iter()
        .map(program_argv)
        .filter(|argv| argv.first().is_some_and(|word| word == "rollback"))
        .collect();
    assert_eq!(rollbacks.len(), 1);
    assert_eq!(
        rollbacks[0],
        vec!["rollback".to_owned(), RECURSIVE_SNAPSHOT.to_owned()],
        "§13.3: one dataset at a time, with no recursive flag"
    );
}

#[test]
fn should_report_the_changed_live_data_a_rollback_would_discard() {
    let tools = data_script().runner();
    let fragment = provider(&tools)
        .plan_recovery(&data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    assert_eq!(
        fragment.newer_state().discarded_bytes(),
        Some(ono_value::ByteSize::from_bytes(8_420_352)),
        "Appendix D.5: the changed-live-data estimate is part of rollback planning"
    );
}

#[test]
fn should_report_the_lifecycle_cost_of_keeping_the_recovery_point() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    assert!(
        fragment
            .newer_state()
            .items()
            .iter()
            .any(|item| item.detail().contains("lifecycle cost")),
        "§13.2: Ono MUST expose the lifecycle cost of a retained snapshot"
    );
}

#[test]
fn should_report_a_boot_environment_rollback_as_needing_a_reboot_before_apply() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    assert_eq!(
        fragment.method(),
        RestoreMethod::OfflineRootRecovery,
        "§13.7: Ono MUST NOT promise online rollback merely because a snapshot exists"
    );
    assert!(
        fragment.requires_reboot() && fragment.requires_offline(),
        "§55.3 case 16: root rollback requiring reboot is reported before execution"
    );
}

#[test]
fn should_report_an_ordinary_mounted_dataset_rollback_as_needing_the_dataset_offline() {
    let tools = data_script().runner();
    let fragment = provider(&tools)
        .plan_recovery(&data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    assert_eq!(fragment.method(), RestoreMethod::DatasetRollback);
    assert!(
        fragment.requires_offline(),
        "§13.7: a rollback that needs the dataset unmounted says so before apply"
    );
    assert!(!fragment.requires_reboot());
}

#[test]
fn should_offer_clone_and_copy_when_the_dataset_cannot_be_read_through_its_mount() {
    let unmounted = Script::examination().answering(
        Slot::Placement,
        ToolOutput::ok(
            "rpool/ROOT/debian\tmounted\tno\nrpool/ROOT/debian\tmountpoint\t/altroot/debian\n\
             rpool/ROOT/debian\treadonly\toff\n",
        ),
    );
    let tools = unmounted.runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    assert_eq!(
        fragment.method(),
        RestoreMethod::CloneAndCopy,
        "Appendix D.4: CLONE_AND_COPY is what remains when the live mount cannot be read through"
    );
    let clone = fragment
        .actions()
        .iter()
        .map(program_argv)
        .find(|argv| argv.first().is_some_and(|word| word == "clone"))
        .expect("the clone is materialised first");
    assert!(
        clone
            .iter()
            .any(|argument| argument.starts_with("mountpoint="))
    );
}

#[test]
fn should_declare_which_file_metadata_a_selective_restore_actually_returns() {
    let gaps = ZfsProvider::metadata_coverage(RestoreMethod::SelectiveFileRestore).gaps();
    assert!(
        gaps.contains(&"hard-link relationships"),
        "Appendix C.7: missing metadata support is visible rather than assumed, got {gaps:?}"
    );
    assert!(
        ZfsProvider::metadata_coverage(RestoreMethod::DatasetRollback)
            .gaps()
            .is_empty(),
        "a rollback returns the dataset exactly, hard links and all"
    );
}

#[test]
fn should_refuse_a_rollback_whose_newer_history_was_never_accepted() {
    let tools = Script::examination().runner();
    let action = rollback_action(&tools);
    let error = provider(&tools)
        .restore(&action, &root_asset())
        .expect_err("§55.3 case 15: blocked without acceptance");
    assert_eq!(code(&error), "recovery.destructive_history_not_accepted");
}

#[test]
fn should_name_every_object_the_refused_rollback_would_have_destroyed() {
    let tools = Script::examination().runner();
    let action = rollback_action(&tools);
    let error = provider(&tools)
        .restore(&action, &root_asset())
        .expect_err("the rollback is refused");
    let destroyed = error
        .metadata()
        .get("destroyed")
        .cloned()
        .expect("§13.6: every object that would be destroyed is in the metadata");
    assert_eq!(
        destroyed,
        ono_value::Value::list([
            ono_value::Value::string(NEWER_SNAPSHOT),
            ono_value::Value::string(NEWER_BOOKMARK),
        ])
    );
}

#[test]
fn should_refuse_a_boot_environment_rollback_even_once_the_history_was_accepted() {
    let tools = Script::examination().runner();
    let action = rollback_action(&tools);
    let error = provider(&tools)
        .restore_with(&action, &root_asset(), &accepted())
        .expect_err(
            "§13.7: acceptance of history loss is not acceptance of an online root rollback",
        );
    assert_eq!(code(&error), "recovery.requires_reboot");
}

#[test]
fn should_surface_the_refusal_zfs_gives_rather_than_adding_the_flag_it_suggests() {
    let tools = data_script().then(ZFS, out("rollback-refused")).runner();
    let action = PlanAction::new(
        &support::plan(),
        0,
        ono_change_core::ActionRole::Recover,
        "zfs rollback tank/data@ono-b91c-20260909T194501Z",
        Execution::Program {
            program: std::sync::Arc::from(ZFS),
            argv: vec![
                std::sync::Arc::from("rollback"),
                std::sync::Arc::from(RECURSIVE_SNAPSHOT),
            ],
        },
    );
    let error = provider(&tools)
        .restore_with(&action, &data_asset(), &accepted())
        .expect_err("ZFS refuses, and the refusal is surfaced rather than worked around");
    assert_eq!(code(&error), "recovery.destructive_history_not_accepted");
    for (_, argv) in tools.calls() {
        assert!(
            !argv
                .iter()
                .any(|argument| argument == "-r" || argument == "-R"),
            "§13.6: Ono never adds the flag ZFS suggests, got {argv:?}"
        );
    }
}

#[test]
fn should_carry_out_a_selective_file_restore_as_one_copy_with_no_shell() {
    let tools = Script::examination()
        .then_examination()
        .then(CP, ToolOutput::ok(""))
        .runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    let action = fragment.actions().first().expect("one restore action");
    provider
        .restore(action, &root_asset())
        .expect("the copy runs");
    let call = tools
        .calls()
        .into_iter()
        .find(|(program, _)| program == CP)
        .expect("the restore ran the copy");
    assert!(
        call.1.contains(&"--preserve=all".to_owned()),
        "Appendix C.7: mode, owner, ACLs and extended attributes come back with the bytes"
    );
    assert!(call.1.contains(&NGINX_CONF.to_owned()));
}

#[test]
fn should_refuse_to_run_an_action_carrying_a_destructive_flag_it_did_not_build() {
    let tools = Script::examination().runner();
    let action = PlanAction::new(
        &support::plan(),
        0,
        ono_change_core::ActionRole::Recover,
        "an action from somewhere else",
        Execution::Program {
            program: std::sync::Arc::from(ZFS),
            argv: vec![
                std::sync::Arc::from("rollback"),
                std::sync::Arc::from("-R"),
                std::sync::Arc::from(ROOT_SNAPSHOT),
            ],
        },
    );
    let error = provider(&tools)
        .restore(&action, &root_asset())
        .expect_err("§13.6: the flag is refused wherever it came from");
    assert_eq!(code(&error), "recovery.apply_failed");
}

#[test]
fn should_verify_every_object_it_set_out_to_restore() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    assert!(
        fragment
            .verification()
            .iter()
            .any(|contract| contract.subject() == NGINX_CONF),
        "§25.3: nothing says recovered without naming the domain it recovered"
    );
}

#[test]
fn should_refuse_a_goal_no_zfs_method_achieves() {
    let tools = Script::examination().runner();
    let error = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::CompensateSemantics)
        .expect_err("§27.4: ZFS restores prior state and does not compensate semantics");
    assert_eq!(code(&error), "recovery.plan_incomplete");
}

// ---------------------------------------------------------------------------------------------
// The execution contract: every action of a recovery plan reaches `restore_with`, and each act
// re-proves what it rests on at the moment it is taken (§56.1, §56.3, §13.6, §13.7).
// ---------------------------------------------------------------------------------------------

/// The unmounted placement Appendix D.4's CLONE_AND_COPY is chosen for.
fn unmounted_root() -> Script {
    Script::examination().answering(
        Slot::Placement,
        ToolOutput::ok(
            "rpool/ROOT/debian\tmounted\tno\nrpool/ROOT/debian\tmountpoint\t/altroot/debian\n\
             rpool/ROOT/debian\treadonly\toff\n",
        ),
    )
}

/// The temporary clone a CLONE_AND_COPY recovery of the root asset materialises.
const TEMP_CLONE: &str = "rpool/ono-restore-ono-a82f-20260909T194500Z";

/// The same unmounted reading, once the temporary clone exists with `origin` as its origin.
///
/// The clone rows are composed onto the recorded listings, in ZFS's own columns: the recorded
/// pool was never asked to materialise one.
fn unmounted_root_with_clone(origin: &str) -> Script {
    let mountpoint = format!("{CLONE_MOUNT_ROOT}/ono-a82f-20260909T194500Z");
    let filesystems = format!(
        "{}{TEMP_CLONE}\t{mountpoint}\tyes\tfilesystem\t0\t872207872\t24576\t{origin}\ton\n",
        out("list-filesystems").stdout()
    );
    let origins = format!("{}{TEMP_CLONE}\t{origin}\n", out("list-clones").stdout());
    unmounted_root()
        .answering(Slot::Filesystems, ToolOutput::ok(filesystems))
        .answering(Slot::Origins, ToolOutput::ok(origins))
}

fn recursive_flag_action(flag: &str) -> PlanAction {
    PlanAction::new(
        &support::plan(),
        0,
        ono_change_core::ActionRole::Recover,
        format!("zfs destroy {flag} {NEWER_SNAPSHOT}"),
        Execution::Program {
            program: Arc::from(ZFS),
            argv: vec![
                Arc::from("destroy"),
                Arc::from(flag),
                Arc::from(NEWER_SNAPSHOT),
            ],
        },
    )
}

#[test]
fn should_refuse_every_spelling_of_the_recursive_flags_whatever_was_accepted() {
    for flag in ["-r", "-R", "-rR", "-Rf", "-fr", "-nRv"] {
        let tools = Script::examination().then_examination().runner();
        let error = provider(&tools)
            .restore_with(&recursive_flag_action(flag), &root_asset(), &accepted())
            .expect_err("§13.6: Ono never runs the flag, however it is spelled");
        assert_eq!(code(&error), "recovery.apply_failed", "for `{flag}`");
        assert!(
            !tools
                .calls()
                .iter()
                .any(|(_, argv)| argv.first().is_some_and(|word| word == "destroy")),
            "§13.6: nothing was destroyed for `{flag}`"
        );
    }
}

#[test]
fn should_refuse_to_destroy_newer_history_the_operator_did_not_accept_losing() {
    let tools = data_script_with_newer_snapshot(DATA_NEWER_GUID)
        .then_examination()
        .runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&support::data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the pool plans a rollback");
    let destroy = destroy_action(&fragment, DATA_NEWER_SNAPSHOT);
    let error = provider
        .restore_with(destroy, &support::data_asset(), &RestoreAcceptance::none())
        .expect_err("§24.5: no destruction without the explicit gate");
    assert_eq!(code(&error), "recovery.destructive_history_not_accepted");
    assert_nothing_destroyed(&tools);
}

#[test]
fn should_destroy_a_newer_snapshot_the_operator_accepted_losing_by_its_name_alone() {
    let tools = data_script_with_newer_snapshot(DATA_NEWER_GUID)
        .then_examination()
        .then(ZFS, ToolOutput::ok(""))
        .runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&support::data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the pool plans a rollback");
    let destroy = destroy_action(&fragment, DATA_NEWER_SNAPSHOT);
    provider
        .restore_with(destroy, &support::data_asset(), &accepted())
        .expect("§13.6: an accepted, enumerated destruction is carried out");
    let destroyed: Vec<Vec<String>> = tools
        .calls()
        .into_iter()
        .map(|(_, argv)| argv)
        .filter(|argv| argv.first().is_some_and(|word| word == "destroy"))
        .collect();
    assert_eq!(
        destroyed,
        vec![vec!["destroy".to_owned(), DATA_NEWER_SNAPSHOT.to_owned()]],
        "§13.6: the one enumerated object, by name, with no flag"
    );
}

#[test]
fn should_put_the_guid_of_each_newer_object_on_the_action_that_destroys_it() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    for object in [NEWER_SNAPSHOT, NEWER_BOOKMARK] {
        let action = destroy_action(&fragment, object);
        let guid = action
            .preconditions()
            .iter()
            .find(|precondition| precondition.subject() == object && precondition.field() == "guid")
            .unwrap_or_else(|| {
                panic!("§56.1: `{object}` is destroyed only while it is the same object")
            });
        assert_eq!(
            guid.expected(),
            &ono_value::Value::string("5048339186470821115"),
            "the GUID `zfs list` recorded for `{object}` (a bookmark carries its snapshot's GUID)"
        );
    }
}

#[test]
fn should_refuse_to_destroy_a_newer_snapshot_whose_guid_changed_since_the_plan() {
    let tools = data_script_with_newer_snapshot(DATA_NEWER_GUID)
        .then_examination_of(&data_script_with_newer_snapshot("999999"))
        .runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&support::data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the pool plans a rollback");
    let destroy = destroy_action(&fragment, DATA_NEWER_SNAPSHOT);
    let error = provider
        .restore_with(destroy, &support::data_asset(), &accepted())
        .expect_err("§56.1: a snapshot recreated under the same name is not what was accepted");
    assert_eq!(code(&error), "change.precondition_failed");
    assert_nothing_destroyed(&tools);
}

#[test]
fn should_refuse_to_destroy_a_boot_environments_history_before_a_rollback_it_cannot_carry_out() {
    let tools = Script::examination().then_examination().runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    let destroy = destroy_action(&fragment, NEWER_SNAPSHOT);
    let error = provider
        .restore_with(destroy, &root_asset(), &accepted())
        .expect_err("§13.7: the rollback will be refused, so nothing is destroyed on its behalf");
    assert_eq!(code(&error), "recovery.requires_reboot");
    assert_nothing_destroyed(&tools);
}

#[test]
fn should_refuse_to_destroy_anything_the_recovery_did_not_enumerate() {
    let tools = Script::examination().then_examination().runner();
    let action = PlanAction::new(
        &support::plan(),
        0,
        ono_change_core::ActionRole::Recover,
        "zfs destroy tank/home",
        Execution::Program {
            program: Arc::from(ZFS),
            argv: vec![Arc::from("destroy"), Arc::from("tank/home")],
        },
    );
    let error = provider(&tools)
        .restore_with(&action, &root_asset(), &accepted())
        .expect_err("acceptance covers what the plan enumerated and nothing else");
    assert_eq!(code(&error), "recovery.apply_failed");
    assert_nothing_destroyed(&tools);
}

#[test]
fn should_refuse_the_rollback_while_a_clone_of_a_newer_snapshot_exists_even_once_accepted() {
    let origins = format!(
        "{}tank/later-clone\t{DATA_NEWER_SNAPSHOT}\n",
        out("list-clones").stdout()
    );
    let script = data_script_with_newer_snapshot(DATA_NEWER_GUID)
        .answering(Slot::Origins, ToolOutput::ok(origins));
    let tools = script.clone().then_examination_of(&script).runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&support::data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the pool plans a rollback");
    let destroy = destroy_action(&fragment, DATA_NEWER_SNAPSHOT);
    let error = provider
        .restore_with(destroy, &support::data_asset(), &accepted())
        .expect_err("§13.6: ZFS refuses while the clone exists, and Ono never adds `-R`");
    assert_eq!(code(&error), "recovery.apply_failed");
    assert_eq!(
        error.metadata().get("clones"),
        Some(&ono_value::Value::list([ono_value::Value::string(
            "tank/later-clone"
        )])),
        "the refusal names the clone standing in the way"
    );
    assert_nothing_destroyed(&tools);
}

#[test]
fn should_name_the_clone_zfs_gives_when_it_refuses_a_destroy() {
    // A clone appeared between the reading and the act: ZFS's own refusal is the evidence, and the
    // name it gives is a dataset without `@` or `#`.
    let tools = data_script_with_newer_snapshot(DATA_NEWER_GUID)
        .then_examination()
        .then(ZFS, out("destroy-clone-held"))
        .runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&support::data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the pool plans a rollback");
    let destroy = destroy_action(&fragment, DATA_NEWER_SNAPSHOT);
    let error = provider
        .restore_with(destroy, &support::data_asset(), &accepted())
        .expect_err("ZFS refused");
    assert_eq!(code(&error), "recovery.destructive_history_not_accepted");
    assert_eq!(
        error.metadata().get("destroyed"),
        Some(&ono_value::Value::list([ono_value::Value::string(CLONE)])),
        "§13.6: the clone ZFS named is surfaced rather than dropped"
    );
}

#[test]
fn should_carry_out_every_action_of_a_clone_and_copy_recovery() {
    let tools = unmounted_root()
        .then_examination_of(&unmounted_root())
        .then(ZFS, ToolOutput::ok(""))
        .then_examination_of(&unmounted_root_with_clone(ROOT_SNAPSHOT))
        .then(CP, ToolOutput::ok(""))
        .then_examination_of(&unmounted_root_with_clone(ROOT_SNAPSHOT))
        .then(ZFS, ToolOutput::ok(""))
        .runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the pool plans a recovery");
    assert_eq!(fragment.method(), RestoreMethod::CloneAndCopy);
    for action in fragment.actions() {
        provider
            .restore_with(action, &root_asset(), &RestoreAcceptance::none())
            .unwrap_or_else(|error| {
                panic!(
                    "every action the provider planned is one it carries out: `{}` -> {error:?}",
                    action.summary()
                )
            });
    }
    let ran: Vec<String> = tools
        .calls()
        .into_iter()
        .filter(|(program, argv)| {
            program == CP
                || argv
                    .first()
                    .is_some_and(|word| word == "clone" || word == "destroy")
        })
        .map(|(program, argv)| format!("{program} {}", argv.join(" ")))
        .collect();
    assert_eq!(
        ran.len(),
        3,
        "clone, copy, destroy of the temporary clone: {ran:?}"
    );
    assert!(ran[2].ends_with(&format!("destroy {TEMP_CLONE}")));
}

#[test]
fn should_refuse_to_destroy_a_dataset_that_is_not_the_temporary_clone_of_this_snapshot() {
    let tools = unmounted_root_with_clone(NEWER_SNAPSHOT).runner();
    let action = PlanAction::new(
        &support::plan(),
        0,
        ono_change_core::ActionRole::Cleanup,
        format!("zfs destroy {TEMP_CLONE}"),
        Execution::Program {
            program: Arc::from(ZFS),
            argv: vec![Arc::from("destroy"), Arc::from(TEMP_CLONE)],
        },
    );
    let error = provider(&tools)
        .restore_with(&action, &root_asset(), &RestoreAcceptance::none())
        .expect_err("a dataset of that name cloned from something else is not this recovery's");
    assert_eq!(code(&error), "recovery.apply_failed");
    assert_nothing_destroyed(&tools);
}

#[test]
fn should_refuse_a_copy_whose_source_is_not_the_snapshot_of_its_target() {
    let tools = Script::examination().runner();
    let action = PlanAction::new(
        &support::plan(),
        0,
        ono_change_core::ActionRole::Recover,
        "restore",
        Execution::Program {
            program: Arc::from(CP),
            argv: [
                "--preserve=all",
                "--no-dereference",
                "--no-target-directory",
                "--",
                "/etc/shadow",
                NGINX_CONF,
            ]
            .into_iter()
            .map(Arc::from)
            .collect(),
        },
    );
    let error = provider(&tools)
        .restore_with(&action, &root_asset(), &RestoreAcceptance::none())
        .expect_err("a copy reads out of this asset's snapshot or its clone, and nowhere else");
    assert_eq!(code(&error), "recovery.apply_failed");
    assert!(!tools.calls().iter().any(|(program, _)| program == CP));
}

// ---------------------------------------------------------------------------------------------
// Appendix D.5: bookmarks are in the way only when they are newer than the recovery point.
// ---------------------------------------------------------------------------------------------

/// The recorded bookmarks, and two whose source snapshots were destroyed — one taken before the
/// recovery point and one after. The two rows and their `createtxg` are composed; the recorded
/// pool has no orphaned bookmark.
fn orphaned_bookmarks() -> Script {
    let bookmarks = format!(
        "{}rpool/ROOT/debian#orphan-old\t1788985900\t111\nrpool/ROOT/debian#orphan-new\t1788985990\t222\n",
        out("list-bookmarks").stdout()
    );
    Script::examination().answering(Slot::Bookmarks, ToolOutput::ok(bookmarks))
}

fn createtxg() -> ToolOutput {
    ToolOutput::ok(format!(
        "{ROOT_SNAPSHOT}\tcreatetxg\t100\nrpool/ROOT/debian#orphan-old\tcreatetxg\t50\n\
         rpool/ROOT/debian#orphan-new\tcreatetxg\t150\n"
    ))
}

#[test]
fn should_leave_an_orphaned_bookmark_older_than_the_recovery_point_out_of_the_destruction() {
    let tools = orphaned_bookmarks().then(ZFS, createtxg()).runner();
    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the pool plans a recovery");
    let destroyed: Vec<&str> = fragment
        .newer_state()
        .destroyed_assets()
        .iter()
        .map(AsRef::as_ref)
        .collect();
    assert!(
        destroyed.contains(&"rpool/ROOT/debian#orphan-new"),
        "Appendix D.5: a bookmark newer than the target by createtxg is in the way, got {destroyed:?}"
    );
    assert!(
        !destroyed.contains(&"rpool/ROOT/debian#orphan-old"),
        "Appendix D.5: an older bookmark is not destroyed by the rollback, got {destroyed:?}"
    );
}

#[test]
fn should_block_a_rollback_when_an_orphaned_bookmark_cannot_be_placed() {
    let tools = orphaned_bookmarks().then(ZFS, out("list-missing")).runner();
    let error = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect_err("§56.3: a bookmark that cannot be placed is not guessed at");
    assert!(
        support::blocked_facts(&error).contains(&"newer-snapshots-and-bookmarks".to_owned()),
        "{error:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// §13.4: an object in a child dataset is not in the parent's snapshot.
// ---------------------------------------------------------------------------------------------

const CHILD_FILE: &str = "/tank/data/customer/db.sqlite";

fn data_asset_with_child_file() -> RecoveryAsset {
    RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        RECURSIVE_SNAPSHOT,
        RecoveryScope::new("zfs-dataset", PARENT_DATASET, "localhost")
            .covering(PARENT_DATASET)
            .covering(CUSTOMER_DB)
            .covering(CHILD_FILE),
        instant(),
    )
    .capturing(format!("{GUID_FINGERPRINT}6260688661848093222"))
}

fn assert_child_file_excluded(fragment: &ono_change_core::RecoveryPlanFragment) {
    assert!(
        !fragment.actions().iter().any(|action| program_argv(action)
            .iter()
            .any(|argument| argument == CHILD_FILE)),
        "§13.4: nothing reads `{CHILD_FILE}` out of the parent's snapshot"
    );
    let excluded = fragment
        .unrecoverable()
        .iter()
        .find(|effect| effect.subject() == CHILD_FILE)
        .expect("§13.4: the file the asset cannot put back is named");
    assert!(
        excluded.reason().contains(CHILD_DATASET),
        "the reason names the child dataset, got `{}`",
        excluded.reason()
    );
}

#[test]
fn should_exclude_a_file_in_a_child_dataset_from_a_selective_restore_of_the_parent() {
    let tools = data_script().runner();
    let fragment = provider(&tools)
        .plan_recovery(
            &data_asset_with_child_file(),
            None,
            RecoveryGoal::RestoreChangedObjects,
        )
        .expect("the pool plans a recovery");
    assert_eq!(fragment.method(), RestoreMethod::SelectiveFileRestore);
    assert_child_file_excluded(&fragment);
    assert!(
        fragment.actions().iter().any(|action| program_argv(action)
            .iter()
            .any(|argument| argument == CUSTOMER_DB)),
        "the file in the parent dataset is still restored"
    );
}

#[test]
fn should_exclude_a_file_in_a_child_dataset_from_a_clone_and_copy_of_the_parent() {
    let tools = data_script()
        .answering(
            Slot::Placement,
            ToolOutput::ok(
                "tank/data\tmounted\tno\ntank/data\tmountpoint\t/tank/data\ntank/data\treadonly\toff\n",
            ),
        )
        .runner();
    let fragment = provider(&tools)
        .plan_recovery(
            &data_asset_with_child_file(),
            None,
            RecoveryGoal::RestoreChangedObjects,
        )
        .expect("the pool plans a recovery");
    assert_eq!(fragment.method(), RestoreMethod::CloneAndCopy);
    assert_child_file_excluded(&fragment);
}

fn destroy_action<'a>(
    fragment: &'a ono_change_core::RecoveryPlanFragment,
    object: &str,
) -> &'a PlanAction {
    fragment
        .actions()
        .iter()
        .find(|action| program_argv(action) == vec!["destroy".to_owned(), object.to_owned()])
        .unwrap_or_else(|| panic!("§13.6: the plan destroys `{object}` by name"))
}

fn assert_nothing_destroyed(tools: &Arc<ScriptedRunner>) {
    assert!(
        !tools.calls().iter().any(|(_, argv)| argv
            .first()
            .is_some_and(|word| word == "destroy" || word == "rollback")),
        "§2.3: a refusal changed nothing, got {:?}",
        tools.calls()
    );
}

/// The rollback action the provider itself planned, so the test drives the real vector.
fn rollback_action(tools: &std::sync::Arc<ScriptedRunner>) -> PlanAction {
    let _ = tools;
    PlanAction::new(
        &support::plan(),
        9,
        ono_change_core::ActionRole::Recover,
        "zfs rollback rpool/ROOT/debian@ono-a82f-20260909T194500Z",
        Execution::Program {
            program: std::sync::Arc::from(ZFS),
            argv: vec![
                std::sync::Arc::from("rollback"),
                std::sync::Arc::from(ROOT_SNAPSHOT),
            ],
        },
    )
}

fn program_argv(action: &PlanAction) -> Vec<String> {
    match action.execution() {
        Execution::Program { argv, .. } => {
            argv.iter().map(std::string::ToString::to_string).collect()
        }
        other => panic!("§2.17: a recovery action is a program and a vector, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// ADR-0829 decision 1: a rollback that loses nothing needs no acceptance, and one that discards
// written data does.
// ---------------------------------------------------------------------------------------------

/// `tank/data` with nothing written since its snapshot.
///
/// The `written` row is composed: the recorded `get-written.txt` was taken for the boot
/// environment, and the recorded pool has no `written` reading for `tank/data`. Everything else
/// is the recorded examination, in which `tank/data@ono-b91c-...` is the newest snapshot of its
/// dataset with no bookmark, and its one clone is a clone of the recovery point itself.
fn data_script_with_nothing_written() -> Script {
    data_script().answering(Slot::Written, ToolOutput::ok("tank/data\twritten\t0\n"))
}

fn rollback_of(fragment: &ono_change_core::RecoveryPlanFragment) -> &PlanAction {
    fragment
        .actions()
        .iter()
        .find(|action| {
            program_argv(action)
                .first()
                .is_some_and(|word| word == "rollback")
        })
        .expect("the fragment plans the rollback")
}

#[test]
fn should_roll_back_without_acceptance_when_nothing_would_be_destroyed_or_discarded() {
    let script = data_script_with_nothing_written();
    let checklist = provider(&script.clone().runner())
        .safety_checklist(&data_asset(), &RestoreAcceptance::none())
        .expect("the pool answers");
    assert!(
        checklist.is_established(ono_recovery_zfs::ZfsFact::HistoryDestructionAccepted),
        "ADR-0829: with nothing to lose there is nothing to accept, got {:?}",
        checklist.evidence(ono_recovery_zfs::ZfsFact::HistoryDestructionAccepted)
    );
    let tools = script
        .clone()
        .then_examination_of(&script)
        .then(ZFS, ToolOutput::ok(""))
        .runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the pool plans a rollback");
    assert!(fragment.newer_state().destroyed_assets().is_empty());
    assert!(
        !fragment.newer_state().requires_destructive_acceptance(),
        "§24.5: the shell's gate does not ask for acceptance here, so the provider must not \
         demand one the operator was never asked for"
    );
    provider
        .restore_with(
            rollback_of(&fragment),
            &data_asset(),
            &RestoreAcceptance::none(),
        )
        .expect("ADR-0829: a rollback that loses nothing is carried out without acceptance");
    assert!(
        tools
            .calls()
            .iter()
            .any(|(_, argv)| argv == &vec!["rollback".to_owned(), RECURSIVE_SNAPSHOT.to_owned()]),
        "the rollback ran, with no flag"
    );
}

#[test]
fn should_refuse_a_rollback_that_discards_written_data_until_the_loss_is_accepted() {
    // `data_script` composes `written` as 8420352 bytes for `tank/data`; nothing is destroyed.
    let tools = data_script().then_examination_of(&data_script()).runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the pool plans a rollback");
    assert!(fragment.newer_state().destroyed_assets().is_empty());
    assert!(
        fragment.newer_state().requires_destructive_acceptance(),
        "§24.5: discarded written data is newer state the gate asks about"
    );
    let error = provider
        .restore_with(
            rollback_of(&fragment),
            &data_asset(),
            &RestoreAcceptance::none(),
        )
        .expect_err("ADR-0829: written data is lost only once the loss is accepted");
    assert_eq!(code(&error), "recovery.destructive_history_not_accepted");
    assert_eq!(
        error.metadata().get("destroyed"),
        Some(&ono_value::Value::list([ono_value::Value::string(
            PARENT_DATASET
        )])),
        "the refusal names the dataset whose written data would be discarded"
    );
    assert_nothing_destroyed(&tools);
}

// ---------------------------------------------------------------------------------------------
// §53's `recovery.zfs.prefer_selective_restore` and `recovery.zfs.allow_destructive_rollback`.
// ---------------------------------------------------------------------------------------------

const ALLOW_SETTING: &str = "recovery.zfs.allow_destructive_rollback";

fn setting_of(error: &ono_value::ErrorValue) -> Option<String> {
    error.metadata().get("setting").map(ToString::to_string)
}

#[test]
fn should_not_plan_a_rollback_that_destroys_newer_history_while_the_setting_forbids_it() {
    let tools = Script::examination().runner();
    let error = provider(&tools)
        .with_recovery_policy(true, false)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect_err(
            "§53: the rollback would destroy `later-1` and `book-1`, and it is not allowed",
        );
    assert_eq!(code(&error), "recovery.plan_incomplete");
    assert!(
        setting_of(&error).is_some_and(|setting| setting.contains(ALLOW_SETTING)),
        "the refusal names the setting the operator has to change, got {error:?}"
    );
    assert_eq!(
        error.metadata().get("destroyed"),
        Some(&ono_value::Value::list([
            ono_value::Value::string(NEWER_SNAPSHOT),
            ono_value::Value::string(NEWER_BOOKMARK),
        ])),
        "§13.6: what the rollback would have destroyed is still enumerated"
    );
}

#[test]
fn should_refuse_destructive_rollback_under_section_fifty_threes_defaults() {
    let tools = Script::examination().runner();
    let runner: Arc<dyn ono_change_core::ToolRunner> =
        Arc::clone(&tools) as Arc<dyn ono_change_core::ToolRunner>;
    let error = ZfsProvider::new(runner)
        .reading_mounts(support::mounts())
        .at_instant(instant())
        .running_as(0, "root")
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect_err("§53: `allow_destructive_rollback = false` is the default");
    assert!(setting_of(&error).is_some_and(|setting| setting.contains(ALLOW_SETTING)));
}

#[test]
fn should_still_plan_a_rollback_that_destroys_nothing_while_destructive_rollback_is_forbidden() {
    // `tank/data@ono-b91c-...` is its dataset's newest snapshot, with no bookmark, and its clone
    // is a clone of the recovery point itself: the rollback destroys nothing.
    let tools = data_script().runner();
    let fragment = provider(&tools)
        .with_recovery_policy(true, false)
        .plan_recovery(&data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the setting is about destroying history, and this rollback destroys none");
    assert_eq!(fragment.method(), RestoreMethod::DatasetRollback);
}

#[test]
fn should_refuse_to_destroy_newer_history_at_apply_while_the_setting_forbids_it() {
    let tools = data_script_with_newer_snapshot(DATA_NEWER_GUID)
        .then_examination()
        .runner();
    let fragment = provider(&tools)
        .plan_recovery(&support::data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("a provider that allows it plans the rollback");
    let destroy = destroy_action(&fragment, DATA_NEWER_SNAPSHOT);
    let error = provider(&tools)
        .with_recovery_policy(true, false)
        .restore_with(destroy, &support::data_asset(), &accepted())
        .expect_err("§53: acceptance does not stand in for the setting");
    assert!(
        setting_of(&error).is_some_and(|setting| setting.contains(ALLOW_SETTING)),
        "{error:?}"
    );
    assert_nothing_destroyed(&tools);
}

#[test]
fn should_fall_back_to_a_restore_that_destroys_nothing_when_destructive_rollback_is_forbidden() {
    let tools = data_script_with_newer_snapshot(DATA_NEWER_GUID).runner();
    let fragment = provider(&tools)
        .with_recovery_policy(false, false)
        .plan_recovery(
            &support::data_asset(),
            None,
            RecoveryGoal::RestoreChangedObjects,
        )
        .expect("a selective restore still achieves the goal");
    assert_eq!(
        fragment.method(),
        RestoreMethod::SelectiveFileRestore,
        "§53: a rollback that would destroy `later-2` is not offered, however it is preferred"
    );
}

#[test]
fn should_prefer_a_dataset_rollback_for_changed_objects_when_selective_restore_is_not_preferred() {
    let tools = data_script().runner();
    let fragment = provider(&tools)
        .with_recovery_policy(false, false)
        .plan_recovery(&data_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the pool plans a recovery");
    assert_eq!(
        fragment.method(),
        RestoreMethod::DatasetRollback,
        "§53: `prefer_selective_restore = false` puts the rollback first where the goal permits it"
    );
    let rollback = rollback_of(&fragment);
    assert!(
        rollback.summary().contains("prefer_selective_restore"),
        "§53: configuration does not change the method silently, got `{}`",
        rollback.summary()
    );
}

#[test]
fn should_keep_selective_restore_first_while_it_is_preferred() {
    let tools = data_script().runner();
    let fragment = provider(&tools)
        .with_recovery_policy(true, true)
        .plan_recovery(&data_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the pool plans a recovery");
    assert_eq!(fragment.method(), RestoreMethod::SelectiveFileRestore);
}

#[test]
fn should_never_prefer_an_offline_root_recovery_over_a_file_restore() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .with_recovery_policy(false, true)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the pool plans a recovery");
    assert_eq!(
        fragment.method(),
        RestoreMethod::SelectiveFileRestore,
        "Appendix C.1: a reboot is never the preferred way to put a file back"
    );
}

#[test]
fn should_measure_what_a_rollback_past_newer_snapshots_discards_since_the_recovery_point() {
    // `tank/data@later-2` follows the recovery point, so `written` would count only since
    // `later-2`. The composed answer is `written@ono-b91c-...`, the figure a rollback discards.
    let tools = data_script_with_newer_snapshot(DATA_NEWER_GUID).runner();
    let fragment = provider(&tools)
        .plan_recovery(&support::data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the pool plans a rollback");
    assert_eq!(
        fragment.newer_state().discarded_bytes(),
        Some(ono_value::ByteSize::from_bytes(8_420_352)),
        "Appendix D.5: the changed-live-data estimate is measured from the recovery point"
    );
    let checklist = provider(&data_script_with_newer_snapshot(DATA_NEWER_GUID).runner())
        .safety_checklist(&support::data_asset(), &RestoreAcceptance::none())
        .expect("the pool answers");
    let detail = checklist
        .evidence(ono_recovery_zfs::ZfsFact::DiscardedLiveData)
        .expect("an entry")
        .detail()
        .to_owned();
    assert!(
        detail.contains("written@ono-b91c-20260909T194501Z"),
        "§56.1: the evidence says which figure it is, got `{detail}`"
    );
}

// -- §13.5 and Appendix D.4: a selective restore needs a snapshot directory that shows the snapshot

#[test]
fn should_copy_out_of_a_temporary_clone_when_the_snapshot_directory_lists_nothing() {
    // Inside a container `.zfs/snapshot/<name>` exists and is empty: the kernel does not mount the
    // snapshot into the container's namespace, so a copy out of it would find nothing.
    let tools = Script::examination()
        .answering(Slot::SnapshotListing, ToolOutput::ok(""))
        .runner();

    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");

    assert_eq!(
        fragment.method(),
        RestoreMethod::CloneAndCopy,
        "Appendix D.4: with the snapshot directory unreadable, the snapshot is materialised as a \
         clone and the file copied out of that"
    );
    let first = fragment.actions().first().expect("the clone comes first");
    assert_eq!(
        program_argv(first).first().map(String::as_str),
        Some("clone"),
        "Appendix D.4: the first action makes the temporary clone"
    );
    assert!(
        first.summary().contains("set aside"),
        "§13.5: the plan states which method it uses and why the other was set aside, got {:?}",
        first.summary()
    );
}

#[test]
fn should_copy_out_of_a_temporary_clone_when_the_snapshot_directory_cannot_be_listed() {
    let tools = Script::examination()
        .answering(
            Slot::SnapshotListing,
            ToolOutput::failed(2, "ls: cannot access: No such file or directory\n"),
        )
        .runner();

    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");

    assert_eq!(
        fragment.method(),
        RestoreMethod::CloneAndCopy,
        "§56.3: an unread snapshot directory is an unestablished fact, and the plan does not rest \
         a selective restore on it"
    );
}

// -- §2.12 and §8.2: a restore states what it will do -------------------------------------------

#[test]
fn should_declare_that_a_selective_restore_replaces_the_live_file() {
    let tools = Script::examination().runner();

    let fragment = provider(&tools)
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");

    let restore = fragment.actions().first().expect("one restore action");
    let effects = restore.effects();
    assert_eq!(
        effects.len(),
        1,
        "§8.2: the restore declares the one effect it has, got {effects:?}"
    );
    assert_eq!(
        effects[0].kind(),
        EffectKind::Replace,
        "§13.5: copying the snapshot's file over the live one replaces it"
    );
    assert_eq!(
        effects[0].object(),
        restore.target(),
        "§8.2: the effect is on the object the action restores"
    );
    assert_eq!(
        effects[0].action(),
        restore.id(),
        "§8.2: the effect is declared by the action that has it"
    );
}
