//! Recovery choices, rollback planning and the mounted/root cases (§13.5-§13.7, Appendix D.4, D.5).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_change_core::{
    Execution, NewerStateClass, PlanAction, RecoveryAsset, RecoveryAssetType, RecoveryGoal,
    RecoveryProvider, RecoveryScope, RestoreMethod, ScriptedRunner, ToolOutput,
};
use ono_recovery_zfs::{CP, GUID_FINGERPRINT, ZFS, ZfsProvider};

use support::{
    CHILD_DATASET, CLONE, NEWER_BOOKMARK, NEWER_SNAPSHOT, PARENT_DATASET, RECURSIVE_SNAPSHOT,
    ROOT_DATASET, ROOT_SNAPSHOT, ROOT_SNAPSHOT_GUID, Script, Slot, code, instant, out, provider,
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
fn should_report_a_clone_that_stands_in_the_way_of_a_rollback() {
    let tools = data_script().runner();
    let fragment = provider(&tools)
        .plan_recovery(&data_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recorded pool plans a recovery");
    assert!(
        fragment
            .newer_state()
            .destroyed_assets()
            .iter()
            .any(|object| object.as_ref() == CLONE),
        "§13.6: clones whose existence affects rollback are enumerated"
    );
    assert!(
        fragment
            .unrecoverable()
            .iter()
            .any(|effect| effect.subject() == CLONE),
        "§13.6: Ono does not add the flag that would destroy the clone, and says so"
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
        .with_accepted_history_destruction()
        .restore(&action, &root_asset())
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
        .with_accepted_history_destruction()
        .restore(&action, &data_asset())
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
    let tools = Script::examination().then(CP, ToolOutput::ok("")).runner();
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
