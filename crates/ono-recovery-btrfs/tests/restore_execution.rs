#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! Every action a Btrfs recovery plan carries is executed by `restore_with` on this provider
//! (§14.4, §14.5, §14.6, Appendix D.9) — including deriving the writable subvolume — and each
//! whole-subvolume step re-checks what it is about to displace at the moment it does it.

mod support;

use std::sync::Arc;

use ono_change_core::{
    ActionRole, Execution, PlanAction, RecoveryAsset, RecoveryGoal, RecoveryPlanFragment,
    RecoveryProvider, RestoreAcceptance, RestoreMethod, ScriptedRunner, ToolOutput, ToolRunner,
};
use ono_recovery_btrfs::{BtrfsConfig, BtrfsMounts, BtrfsProvider, RecordedFiles, RootRecovery};
use support::{
    NGINX_CONF, ROOT_SNAPSHOT, VAR_SNAPSHOT, at_generation, boot_by_name, booting, content,
    derived_from, fixture, mounts, root_asset, root_recovery_script, runner, var_asset,
    var_recovery_script, var_snapshot_show,
};

const ROOT_DERIVED: &str = "/mnt/top/@snapshots/ono-a82f-root-rw";
const VAR_DERIVED: &str = "/mnt/top/@snapshots/ono-a82f-var-rw";

fn provider(
    runner: &Arc<ScriptedRunner>,
    mounts: BtrfsMounts,
    files: &Arc<RecordedFiles>,
    policy: RootRecovery,
) -> BtrfsProvider {
    BtrfsProvider::new(Arc::clone(runner) as Arc<dyn ToolRunner>)
        .with_mounts(mounts)
        .with_files(Arc::clone(files) as Arc<dyn ono_recovery_btrfs::FileStore>)
        .with_config(BtrfsConfig::default().recovering_root_by(policy))
}

/// The recorded mount table after the operator unmounted `/mnt/root/var` (§14.4).
fn var_unmounted() -> BtrfsMounts {
    let lines: String = fixture("mountinfo")
        .stdout()
        .lines()
        .filter(|line| !line.contains(" /mnt/root/var "))
        .map(|line| format!("{line}\n"))
        .collect();
    BtrfsMounts::from_mountinfo(&lines).identified_by(&[
        ono_recovery_btrfs::parse::parse_filesystem_show(fixture("fs-show").stdout()).unwrap(),
    ])
}

fn run_all(
    provider: &BtrfsProvider,
    fragment: &RecoveryPlanFragment,
    asset: &RecoveryAsset,
    acceptance: &RestoreAcceptance,
) {
    for action in fragment.actions() {
        provider
            .restore_with(action, asset, acceptance)
            .unwrap_or_else(|error| panic!("`{}` runs: {error:?}", action.summary()));
    }
}

fn action_named<'a>(fragment: &'a RecoveryPlanFragment, operation: &str) -> &'a PlanAction {
    fragment
        .actions()
        .iter()
        .find(|action| match action.execution() {
            Execution::RecoveryOperation { arguments, .. } => arguments
                .iter()
                .any(|(key, value)| key.as_ref() == "operation" && value.to_string() == operation),
            _ => false,
        })
        .unwrap_or_else(|| panic!("the plan carries a `{operation}` action"))
}

fn aside(live: &str) -> String {
    format!(
        "{live}.ono-superseded-{}",
        support::source_plan().id().short()
    )
}

#[test]
fn should_derive_the_writable_subvolume_and_copy_out_of_it_for_a_clone_and_copy_recovery() {
    let runner = runner(
        [
            root_recovery_script(),
            vec![
                fixture("snapshot-create"),
                derived_from(&fixture("subvol-show-snapshot"), "ono-a82f-root-rw"),
            ],
        ]
        .concat(),
    );
    let files = Arc::new(support::recorded_files().holding(
        format!("{ROOT_DERIVED}/etc/nginx/nginx.conf"),
        content("snapshot-file"),
    ));
    let provider = provider(
        &runner,
        mounts(),
        &files,
        RootRecovery::OnlineSelectiveRestore,
    );
    let fragment = provider
        .plan_recovery(
            &root_asset(),
            Some(&support::source_plan()),
            RecoveryGoal::RestoreDomain,
        )
        .expect("the recovery plans");
    assert_eq!(fragment.method(), RestoreMethod::CloneAndCopy);

    run_all(
        &provider,
        &fragment,
        &root_asset(),
        &RestoreAcceptance::none(),
    );

    assert!(
        runner
            .calls()
            .iter()
            .any(|(_, argv)| argv == &["subvolume", "snapshot", ROOT_SNAPSHOT, ROOT_DERIVED]),
        "§14.5: the writable subvolume is a snapshot of the read-only recovery point, without \
         `-r`, taken by the provider itself: {:?}",
        runner.calls()
    );
    assert_eq!(
        files.copies(),
        vec![(
            format!("{ROOT_DERIVED}/etc/nginx/nginx.conf"),
            NGINX_CONF.to_owned()
        )],
        "Appendix C.1's clone-and-copy: the object is copied out of the derived subvolume"
    );
    let derived = provider.derived_assets();
    assert_eq!(
        derived.len(),
        1,
        "§14.5: the derived subvolume is tracked separately"
    );
    assert_eq!(derived[0].reference(), ROOT_DERIVED);
    assert_eq!(derived[0].dependencies(), &[root_asset().id().clone()]);
}

#[test]
fn should_swap_the_subvolume_paths_under_the_top_level_mount_when_replacing_a_subvolume() {
    let files = Arc::new(support::recorded_files());
    let planner = provider(
        &runner(var_recovery_script()),
        mounts(),
        &files,
        RootRecovery::NextBoot,
    );
    let asset = var_asset(&["/mnt/root/var/log/syslog"]);
    let fragment = planner
        .plan_recovery(
            &asset,
            Some(&support::source_plan()),
            RecoveryGoal::RestoreDomain,
        )
        .expect("the recovery plans while /var is still mounted");
    assert_eq!(fragment.method(), RestoreMethod::SubvolumeReplacement);

    let runner = runner(vec![
        fixture("snapshot-create"),
        derived_from(&var_snapshot_show(), "ono-a82f-var-rw"),
        fixture("subvol-show-var"),
        var_snapshot_show(),
        derived_from(&var_snapshot_show(), "ono-a82f-var-rw"),
    ]);
    let executor = provider(&runner, var_unmounted(), &files, RootRecovery::NextBoot);
    run_all(&executor, &fragment, &asset, &RestoreAcceptance::none());

    assert_eq!(
        files.renames(),
        vec![
            ("/mnt/top/@var".to_owned(), aside("/mnt/top/@var")),
            (VAR_DERIVED.to_owned(), "/mnt/top/@var".to_owned()),
        ],
        "§14.4: the live subvolume is renamed aside and the derived one takes its name, both \
         through the one top-level mount — never the mountpoint directory, and never across two \
         mounts, where rename(2) fails with EXDEV"
    );
}

#[test]
fn should_refuse_to_replace_a_subvolume_that_is_still_mounted() {
    let files = Arc::new(support::recorded_files());
    let planner = provider(
        &runner(var_recovery_script()),
        mounts(),
        &files,
        RootRecovery::NextBoot,
    );
    let asset = var_asset(&["/mnt/root/var/log/syslog"]);
    let fragment = planner
        .plan_recovery(
            &asset,
            Some(&support::source_plan()),
            RecoveryGoal::RestoreDomain,
        )
        .expect("the recovery plans");
    let runner = runner(Vec::new());
    let executor = provider(&runner, mounts(), &files, RootRecovery::NextBoot);
    let error = executor
        .restore_with(
            action_named(&fragment, "replace-subvolume"),
            &asset,
            &RestoreAcceptance::none().accepting_newer_state_loss(),
        )
        .expect_err("§14.4: /mnt/root/var still shows @var");
    assert_eq!(error.code().name(), "recovery.requires_offline");
    assert!(files.renames().is_empty(), "and nothing was moved");
    assert!(runner.calls().is_empty(), "or even asked");
}

#[test]
fn should_refuse_to_displace_a_subvolume_written_since_the_recovery_point_unless_accepted() {
    let files = Arc::new(support::recorded_files());
    let planner = provider(
        &runner(var_recovery_script()),
        mounts(),
        &files,
        RootRecovery::NextBoot,
    );
    let asset = var_asset(&["/mnt/root/var/log/syslog"]);
    let fragment = planner
        .plan_recovery(
            &asset,
            Some(&support::source_plan()),
            RecoveryGoal::RestoreDomain,
        )
        .expect("the recovery plans");
    let replace = action_named(&fragment, "replace-subvolume");
    let written_since = || {
        vec![
            at_generation(&fixture("subvol-show-var"), 42),
            var_snapshot_show(),
            derived_from(&var_snapshot_show(), "ono-a82f-var-rw"),
        ]
    };

    let refused = provider(
        &runner(written_since()),
        var_unmounted(),
        &files,
        RootRecovery::NextBoot,
    )
    .restore_with(replace, &asset, &RestoreAcceptance::none())
    .expect_err("§24.5: displacing later writes needs the operator's acceptance");
    assert_eq!(refused.code().name(), "recovery.newer_state_conflict");
    assert!(files.renames().is_empty());

    provider(
        &runner(written_since()),
        var_unmounted(),
        &files,
        RootRecovery::NextBoot,
    )
    .restore_with(
        replace,
        &asset,
        &RestoreAcceptance::none().accepting_newer_state_loss(),
    )
    .expect("with `--accept-newer-state-loss` the replacement runs");
    assert_eq!(files.renames().len(), 2);
}

#[test]
fn should_rename_the_root_for_the_next_boot_when_the_boot_entry_selects_it_by_name() {
    let runner = runner(
        [
            root_recovery_script(),
            vec![
                fixture("snapshot-create"),
                derived_from(&fixture("subvol-show-snapshot"), "ono-a82f-root-rw"),
                fixture("subvol-show-root"),
                fixture("subvol-show-snapshot"),
                derived_from(&fixture("subvol-show-snapshot"), "ono-a82f-root-rw"),
            ],
        ]
        .concat(),
    );
    let files = Arc::new(booting(&boot_by_name()));
    let provider = provider(&runner, mounts(), &files, RootRecovery::NextBoot);
    let fragment = provider
        .plan_recovery(
            &root_asset(),
            Some(&support::source_plan()),
            RecoveryGoal::RestoreDomain,
        )
        .expect("the recovery plans");
    assert!(fragment.requires_reboot());

    run_all(
        &provider,
        &fragment,
        &root_asset(),
        &RestoreAcceptance::none(),
    );

    assert_eq!(
        files.renames(),
        vec![
            ("/mnt/top/@".to_owned(), aside("/mnt/top/@")),
            (ROOT_DERIVED.to_owned(), "/mnt/top/@".to_owned()),
        ],
        "§14.6 next-boot: `rootflags=subvol=@` selects the root by name, so the recovered \
         subvolume takes the name `@`; the running system keeps the subvolume it booted until the \
         reboot"
    );
    assert!(
        !runner
            .calls()
            .iter()
            .any(|(_, argv)| argv.iter().any(|argument| argument == "set-default")),
        "and the default subvolume, which that boot entry never consults, is left alone"
    );
}

fn set_default_action(live: &str) -> PlanAction {
    PlanAction::new(
        &support::plan_id(),
        1,
        ActionRole::Recover,
        "§14.6 next-boot: point the next boot at the derived subvolume",
        Execution::RecoveryOperation {
            provider: Arc::from(ono_recovery_btrfs::PROVIDER_ID),
            capability: Arc::from("recovery.restore"),
            arguments: vec![
                (
                    Arc::from("operation"),
                    ono_value::Value::string("set-default-subvolume"),
                ),
                (Arc::from("source"), ono_value::Value::string(ROOT_DERIVED)),
                (Arc::from("mount"), ono_value::Value::string("/mnt/top")),
                (Arc::from("live"), ono_value::Value::string(live)),
                (Arc::from("subvolume"), ono_value::Value::string("@")),
                (
                    Arc::from("subvolume-id"),
                    ono_value::Value::string(&support::ROOT_ID.to_string()),
                ),
            ],
        },
    )
}

#[test]
fn should_point_the_default_at_the_derived_subvolume_by_its_own_id() {
    let runner = runner(vec![
        fixture("subvol-show-root"),
        fixture("subvol-show-snapshot"),
        derived_from(&fixture("subvol-show-snapshot"), "ono-a82f-root-rw"),
        ToolOutput::ok(""),
    ]);
    let files = Arc::new(support::recorded_files());
    provider(&runner, mounts(), &files, RootRecovery::NextBoot)
        .restore_with(
            &set_default_action("/mnt/root"),
            &root_asset(),
            &RestoreAcceptance::none(),
        )
        .expect("the default subvolume is set");
    assert_eq!(
        runner.calls().last().map(|(_, argv)| argv.clone()),
        Some(
            [
                "subvolume",
                "set-default",
                &support::DERIVED_ID.to_string(),
                "/mnt/top"
            ]
            .map(str::to_owned)
            .to_vec()
        ),
        "Appendix D.9: the next boot is pointed at the derived subvolume, never at the read-only \
         recovery point itself"
    );
}

#[test]
fn should_report_a_refused_set_default_as_a_recovery_failure() {
    let runner = runner(vec![
        fixture("subvol-show-root"),
        fixture("subvol-show-snapshot"),
        derived_from(&fixture("subvol-show-snapshot"), "ono-a82f-root-rw"),
        fixture("set-default-refused"),
    ]);
    let files = Arc::new(support::recorded_files());
    let error = provider(&runner, mounts(), &files, RootRecovery::NextBoot)
        .restore_with(
            &set_default_action("/mnt/root"),
            &root_asset(),
            &RestoreAcceptance::none(),
        )
        .expect_err("the recorded `set-default` refusal is a recovery failure");
    assert_eq!(error.code().name(), "recovery.apply_failed");
}

#[test]
fn should_refuse_to_point_the_boot_at_something_not_derived_from_this_recovery_point() {
    let runner = runner(vec![
        fixture("subvol-show-root"),
        fixture("subvol-show-snapshot"),
        fixture("subvol-show-var"),
    ]);
    let files = Arc::new(support::recorded_files());
    let error = provider(&runner, mounts(), &files, RootRecovery::NextBoot)
        .restore_with(
            &set_default_action("/mnt/root"),
            &root_asset(),
            &RestoreAcceptance::none(),
        )
        .expect_err("§56.2: what the boot is pointed at must provably come from the snapshot");
    assert_eq!(error.code().name(), "recovery.apply_failed");
    assert!(
        !runner
            .calls()
            .iter()
            .any(|(_, argv)| argv.iter().any(|argument| argument == "set-default"))
    );
}

#[test]
fn should_refuse_to_derive_into_a_path_that_already_exists() {
    let files = Arc::new(support::recorded_files().holding(VAR_DERIVED, "a leftover"));
    let planner = provider(
        &runner(var_recovery_script()),
        mounts(),
        &files,
        RootRecovery::NextBoot,
    );
    let asset = var_asset(&["/mnt/root/var/log/syslog"]);
    let fragment = planner
        .plan_recovery(
            &asset,
            Some(&support::source_plan()),
            RecoveryGoal::RestoreDomain,
        )
        .expect("the recovery plans");
    let runner = runner(Vec::new());
    let error = provider(&runner, mounts(), &files, RootRecovery::NextBoot)
        .restore_with(
            action_named(&fragment, "derive-writable-subvolume"),
            &asset,
            &RestoreAcceptance::none(),
        )
        .expect_err(
            "`btrfs subvolume snapshot` into an existing directory creates the snapshot inside \
             it, which is not the path the plan named",
        );
    assert_eq!(error.code().name(), "recovery.apply_failed");
    assert!(runner.calls().is_empty());
    let _ = VAR_SNAPSHOT;
}
