#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! §14.4: Btrfs recovery is a workflow, and the plan declares which one.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_change_core::{
    ActionRole, Execution, NewerStateClass, ProtectionMode, RecoveryGoal, RecoveryObjective,
    RecoveryProvider, RestoreMethod, ToolOutput,
};
use ono_recovery_btrfs::{
    BtrfsConfig, BtrfsProvider, RecordedFiles, RootRecovery, classify_object,
};
use support::{
    NGINX_CONF, ROOT_SNAPSHOT, fixture, mounts, provider_with_files, root_asset,
    root_recovery_script, runner, var_asset,
};

fn snapshot_conf() -> String {
    format!("{ROOT_SNAPSHOT}/etc/nginx/nginx.conf")
}

#[test]
fn should_declare_selective_file_restore_when_the_wanted_objects_can_be_read_out() {
    let provider = provider_with_files(root_recovery_script());
    let fragment = provider
        .plan_recovery(
            &root_asset(),
            Some(&support::source_plan()),
            RecoveryGoal::RestoreChangedObjects,
        )
        .expect("every §56.2 fact is established by the recorded output");
    assert_eq!(
        fragment.method(),
        RestoreMethod::SelectiveFileRestore,
        "§55.4 case 21: the recovery plan describes the method it will actually use, and \
         Appendix C.1 prefers the one that leaves unrelated state alone"
    );
    assert!(!fragment.requires_reboot());
    assert!(!fragment.requires_offline());
}

#[test]
fn should_name_the_snapshot_path_each_object_is_read_out_of() {
    let provider = provider_with_files(root_recovery_script());
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recovery plans");
    let action = fragment
        .actions()
        .iter()
        .find(|action| action.role() == ActionRole::Recover)
        .expect("one object to restore, one recovery action");
    let Execution::RecoveryOperation { arguments, .. } = action.execution() else {
        panic!("§2.17: a provider operation is structured, never a command line")
    };
    let source = arguments
        .iter()
        .find(|(key, _)| key.as_ref() == "source")
        .map(|(_, value)| value.to_string())
        .expect("the action names what it reads");
    assert!(
        source.contains(&snapshot_conf()),
        "§13.5: a selective restore reads from the read-only snapshot, and the plan says exactly \
         which path"
    );
}

#[test]
fn should_classify_a_file_changed_again_after_the_snapshot_as_conflicting() {
    let provider = provider_with_files(root_recovery_script());
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recovery plans");
    let item = fragment
        .newer_state()
        .items()
        .iter()
        .find(|item| item.object() == NGINX_CONF)
        .expect("the object the recovery would restore is classified");
    assert_eq!(
        item.class(),
        NewerStateClass::Conflicting,
        "Appendix C.4: the snapshot holds `worker_processes 4;` and the live file holds \
         `worker_processes 8;`, so restoring the snapshot would discard the later edit"
    );
    assert!(
        fragment.newer_state().is_complete(),
        "§62.8: the analysis ran, which is a different state from finding nothing"
    );
    assert!(
        fragment.newer_state().requires_destructive_acceptance(),
        "§24.5: a recovery that discards a later edit is gated on explicit acceptance"
    );
}

#[test]
fn should_leave_a_file_outside_the_restore_set_alone() {
    let item = classify_object(
        Path::new("/mnt/root/etc/hosts"),
        false,
        Some(b"snapshot"),
        Some(b"later"),
    );
    assert_eq!(
        item.class(),
        NewerStateClass::PreservedByMethod,
        "§59.6 and Appendix C.6: a selective restore writes only the objects it names, so a file \
         outside the restore set is preserved even though it changed"
    );
    assert!(!item.class().is_loss());
}

#[test]
fn should_classify_an_unchanged_file_as_preserved_rather_than_conflicting() {
    let item = classify_object(Path::new(NGINX_CONF), true, Some(b"same"), Some(b"same"));
    assert_eq!(
        item.class(),
        NewerStateClass::PreservedByMethod,
        "restoring a file that already holds what the snapshot holds discards nothing"
    );
}

#[test]
fn should_treat_an_unreadable_object_as_unknown_rather_than_unchanged() {
    let item = classify_object(Path::new("/mnt/root/gone"), true, None, None);
    assert_eq!(
        item.class(),
        NewerStateClass::Unknown,
        "§56.3: an object neither side holds is a fact that was not established, and §2.4 forbids \
         promoting unknown to anything else"
    );
}

#[test]
fn should_put_the_snapshot_content_back_when_the_restore_action_runs() {
    let files = Arc::new(support::recorded_files());
    let provider = BtrfsProvider::new(runner(root_recovery_script()))
        .with_mounts(mounts())
        .with_files(Arc::clone(&files) as Arc<dyn ono_recovery_btrfs::FileStore>);
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recovery plans");
    let action = fragment
        .actions()
        .iter()
        .find(|action| action.role() == ActionRole::Recover)
        .expect("a recovery action");
    provider
        .restore(action, &root_asset())
        .expect("the restore runs");
    assert_eq!(
        files.copies(),
        vec![(snapshot_conf(), NGINX_CONF.to_owned())],
        "§13.5: the object comes out of the read-only snapshot and goes back where it was, and \
         nothing else is touched"
    );
}

#[test]
fn should_block_a_recovery_of_an_object_inside_a_nested_subvolume() {
    let provider = provider_with_files(support::var_recovery_script());
    let error = provider
        .plan_recovery(
            &var_asset(&["/mnt/root/var/lib-app/state.db"]),
            None,
            RecoveryGoal::RestoreChangedObjects,
        )
        .expect_err("§14.3: the snapshot of @var holds an empty directory where lib-app is");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("empty directory")),
        "and the refusal says why rather than reporting a generic failure"
    );
}

#[test]
fn should_declare_subvolume_replacement_rather_than_an_in_place_primitive() {
    let provider = provider_with_files(support::var_recovery_script());
    let fragment = provider
        .plan_recovery(
            &var_asset(&["/mnt/root/var/log/syslog"]),
            None,
            RecoveryGoal::RestoreDomain,
        )
        .expect("the recovery plans");
    assert_eq!(
        fragment.method(),
        RestoreMethod::SubvolumeReplacement,
        "§14.4: v0.6 MUST NOT present Btrfs as having a generic in-place rollback primitive. \
         Returning the whole subvolume means replacing it"
    );
    assert!(
        fragment.requires_offline(),
        "§14.4: replacing a mounted subvolume needs it unmounted first"
    );
    assert!(
        fragment
            .actions()
            .iter()
            .any(|action| action.role() == ActionRole::Prepare),
        "§14.5: the writable subvolume it is replaced with is derived first, and tracked as its \
         own asset"
    );
}

#[test]
fn should_never_claim_that_btrfs_performs_a_rollback() {
    let mut spoken: Vec<String> = Vec::new();

    let provider = provider_with_files(
        [
            vec![
                fixture("fs-show-mount"),
                fixture("subvol-show-root"),
                fixture("subvol-list-root"),
                fixture("subvol-list-root"),
                fixture("fs-usage"),
            ],
            root_recovery_script(),
        ]
        .concat(),
    )
    .for_plan(support::plan_id());

    let domain = provider
        .resolve_domain(NGINX_CONF)
        .expect("resolution runs")
        .expect("on Btrfs");
    spoken.push(domain.detail().to_owned());
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs");
    for candidate in &candidates {
        spoken.push(candidate.detail().to_owned());
        spoken.extend(candidate.exclusions().iter().flat_map(|exclusion| {
            [
                exclusion.subject().to_owned(),
                exclusion.reason().to_owned(),
            ]
        }));
        spoken.extend(
            candidate
                .creation_requirements()
                .iter()
                .map(std::string::ToString::to_string),
        );
        spoken.extend(
            candidate
                .restore_requirements()
                .iter()
                .map(std::string::ToString::to_string),
        );
    }
    let actions = provider
        .plan_protection(&candidates, ProtectionMode::Prefer)
        .expect("planning runs");
    spoken.extend(actions.iter().map(|action| action.summary().to_owned()));

    let (fragment, checklist) = provider
        .plan_recovery_with_checklist(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recovery plans");
    spoken.push(fragment.method().as_str().to_owned());
    spoken.extend(
        fragment
            .actions()
            .iter()
            .map(|action| action.summary().to_owned()),
    );
    spoken.extend(
        fragment
            .newer_state()
            .items()
            .iter()
            .map(|item| item.detail().to_owned()),
    );
    spoken.extend(
        fragment
            .unrecoverable()
            .iter()
            .map(|effect| effect.reason().to_owned()),
    );
    spoken.extend(
        fragment
            .verification()
            .iter()
            .map(|contract| contract.expression().to_owned()),
    );
    spoken.extend(
        checklist
            .established()
            .iter()
            .map(|(_, evidence)| evidence.to_string()),
    );
    for policy in [
        RootRecovery::OnlineSelectiveRestore,
        RootRecovery::OfflineSubvolumeReplacement,
        RootRecovery::NextBoot,
    ] {
        spoken.push(policy.token().to_owned());
    }

    for sentence in &spoken {
        assert!(
            !sentence.to_lowercase().contains("rollback"),
            "§14.4 and §2.10: `rollback` belongs to a provider that really implements one. This \
             provider said: {sentence}"
        );
    }
    assert!(
        spoken.len() > 20,
        "the sweep has to cover enough of what the provider can say to mean anything"
    );
}

#[test]
fn should_offer_only_the_four_methods_section_fourteen_point_four_lists() {
    for (goal, config, expected) in [
        (
            RecoveryGoal::RestoreChangedObjects,
            BtrfsConfig::default(),
            RestoreMethod::SelectiveFileRestore,
        ),
        (
            RecoveryGoal::RestoreDomain,
            BtrfsConfig::default().permitting_offline_replacement(true),
            RestoreMethod::SubvolumeReplacement,
        ),
        (
            RecoveryGoal::RestoreDomain,
            BtrfsConfig::default().permitting_offline_replacement(false),
            RestoreMethod::CloneAndCopy,
        ),
    ] {
        let provider = BtrfsProvider::new(runner(Vec::new())).with_config(config);
        assert_eq!(
            provider.method_for(goal, false, &[PathBuf::from(NGINX_CONF)]),
            Some(expected),
            "§14.4: every recovery is one of selective restore, clone and copy, subvolume \
             replacement or offline root recovery — and never a dataset rollback"
        );
    }
    let provider = BtrfsProvider::new(runner(Vec::new()));
    assert_eq!(
        provider.method_for(RecoveryGoal::RestoreChangedObjects, false, &[]),
        None,
        "§56.3: with nothing to restore, no method is chosen and the recovery blocks"
    );
    assert_eq!(
        provider.method_for(
            RecoveryGoal::CompensateSemantics,
            false,
            &[PathBuf::from(NGINX_CONF)]
        ),
        None,
        "§27.4: this provider has no compensating action, and a restore is not one"
    );
}

#[test]
fn should_name_each_nested_subvolume_the_recovery_cannot_bring_back() {
    let provider = provider_with_files(root_recovery_script());
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recovery plans");
    let unrecoverable: Vec<&str> = fragment
        .unrecoverable()
        .iter()
        .map(ono_change_core::UnrecoverableEffect::subject)
        .collect();
    assert_eq!(
        unrecoverable,
        vec!["@home", "@var", "@var/lib-app"],
        "§14.3 and §24.3: recovering `@` does not recover what is mounted inside it, and the plan \
         names each one rather than implying the recovery is complete"
    );
    assert!(
        fragment.unrecoverable()[0]
            .compensation()
            .is_some_and(|action| action.contains("its own")),
        "and says what would recover it instead: an asset of its own"
    );
}

#[test]
fn should_carry_a_verification_contract_for_the_recovered_state() {
    let provider = provider_with_files(root_recovery_script());
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recovery plans");
    assert!(
        !fragment.verification().is_empty(),
        "§25.3: a recovery says which domain it recovered, and that needs a contract to check"
    );
}

#[test]
fn should_refuse_to_derive_a_writable_subvolume_through_the_restore_call() {
    let provider = provider_with_files(support::var_recovery_script());
    let fragment = provider
        .plan_recovery(
            &var_asset(&["/mnt/root/var/log/syslog"]),
            None,
            RecoveryGoal::RestoreDomain,
        )
        .expect("the recovery plans");
    let prepare = fragment
        .actions()
        .iter()
        .find(|action| action.role() == ActionRole::Prepare)
        .expect("the writable subvolume is derived first");
    let error = provider
        .restore(prepare, &var_asset(&[]))
        .expect_err("§14.5: a derived writable subvolume is an asset, and `restore` returns none");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("derive_writable")),
        "the refusal names the call that records the derived asset and its dependency"
    );
}

#[test]
fn should_read_nothing_from_a_store_that_holds_nothing_and_block_rather_than_guess() {
    let provider = BtrfsProvider::new(runner(root_recovery_script()))
        .with_mounts(mounts())
        .with_files(Arc::new(RecordedFiles::new()));
    let error = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect_err("§56.3: what would be discarded could not be established");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
}

#[test]
fn should_report_which_argument_vector_a_set_default_recovery_would_run() {
    let provider = BtrfsProvider::new(runner(vec![
        fixture("subvol-show-snapshot"),
        ToolOutput::ok(""),
    ]))
    .with_mounts(mounts());
    let plan = support::plan_id();
    let action = ono_change_core::PlanAction::new(
        &plan,
        0,
        ActionRole::Recover,
        "point the next boot at the derived subvolume",
        Execution::RecoveryOperation {
            provider: Arc::from(ono_recovery_btrfs::PROVIDER_ID),
            capability: Arc::from("recovery.restore"),
            arguments: vec![
                (
                    Arc::from("operation"),
                    ono_value::Value::string("set-default-subvolume"),
                ),
                (
                    Arc::from("source"),
                    ono_value::Value::string("/mnt/top/@snapshots/ono-a82f-root-rw"),
                ),
                (Arc::from("mount"), ono_value::Value::string("/mnt/root")),
            ],
        },
    );
    provider
        .restore(&action, &root_asset())
        .expect("the default subvolume is set");
}

#[test]
fn should_report_a_refused_set_default_as_a_recovery_failure() {
    let provider = BtrfsProvider::new(runner(vec![
        fixture("subvol-show-snapshot"),
        fixture("set-default-refused"),
    ]))
    .with_mounts(mounts());
    let plan = support::plan_id();
    let action = ono_change_core::PlanAction::new(
        &plan,
        0,
        ActionRole::Recover,
        "point the next boot at the derived subvolume",
        Execution::RecoveryOperation {
            provider: Arc::from(ono_recovery_btrfs::PROVIDER_ID),
            capability: Arc::from("recovery.restore"),
            arguments: vec![
                (
                    Arc::from("operation"),
                    ono_value::Value::string("set-default-subvolume"),
                ),
                (
                    Arc::from("source"),
                    ono_value::Value::string("/mnt/top/@snapshots/ono-a82f-root-rw"),
                ),
                (Arc::from("mount"), ono_value::Value::string("/mnt/root")),
            ],
        },
    );
    let error = provider
        .restore(&action, &root_asset())
        .expect_err("the recorded `set-default` refusal is a recovery failure");
    assert_eq!(error.code().name(), "recovery.apply_failed");
}
