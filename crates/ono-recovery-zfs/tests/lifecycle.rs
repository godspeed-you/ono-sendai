//! Retention, cleanup, cost and the flag this provider never adds (§37, §38, §13.6, §13.8).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_change_core::{
    ProtectionMode, RecoveryAsset, RecoveryAssetType, RecoveryExclusion, RecoveryGoal,
    RecoveryProvider, RecoveryScope, RestoreMethod, ToolOutput,
};
use ono_recovery_zfs::{ZFS, ZPOOL, render_asset};
use ono_value::ByteSize;

use support::{
    CLONE, PARENT_DATASET, RECURSIVE_SNAPSHOT, ROOT_DATASET, ROOT_SNAPSHOT, Script, asset, code,
    instant, out, provider, runner,
};

#[test]
fn should_remove_exactly_the_snapshot_the_asset_names() {
    let tools = runner(vec![(ZFS, ToolOutput::ok(""))]);
    provider(&tools)
        .cleanup(&asset())
        .expect("the recorded pool removes it");
    let call = tools
        .calls()
        .into_iter()
        .next()
        .expect("cleanup ran a command");
    assert_eq!(
        call.1,
        vec!["destroy".to_owned(), ROOT_SNAPSHOT.to_owned()],
        "§37: exactly this snapshot, never recursively and never with `-R`"
    );
}

#[test]
fn should_report_a_snapshot_with_a_dependent_clone_as_a_refusal_rather_than_escalating() {
    let tools = runner(vec![(ZFS, out("destroy-clone-held"))]);
    let clone_asset = RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        RECURSIVE_SNAPSHOT,
        RecoveryScope::new("zfs-dataset", PARENT_DATASET, "localhost").covering(PARENT_DATASET),
        instant(),
    );
    let error = provider(&tools)
        .cleanup(&clone_asset)
        .expect_err("§13.6: ZFS suggests `-R`, and Ono does not take the suggestion");
    assert_eq!(code(&error), "recovery.destructive_history_not_accepted");
    assert_eq!(
        error.metadata().get("destroyed"),
        Some(&ono_value::Value::list([ono_value::Value::string(CLONE)])),
        "the clone that would go with it is named"
    );
    for (_, argv) in tools.calls() {
        assert!(
            !argv.iter().any(|argument| argument == "-R"),
            "§13.6: the flag is never added, got {argv:?}"
        );
    }
}

#[test]
fn should_report_a_cleanup_refused_for_want_of_privilege_as_a_privilege_refusal() {
    let tools = runner(vec![(ZFS, out("unprivileged-list"))]);
    let error = provider(&tools)
        .cleanup(&asset())
        .expect_err("§43.4: recovery may need more privilege than the mutation did");
    assert_eq!(code(&error), "change.privilege_required");
}

#[test]
fn should_treat_an_already_absent_snapshot_as_removed() {
    let tools = runner(vec![(ZFS, out("list-missing"))]);
    provider(&tools)
        .cleanup(&asset())
        .expect("§37: removal that finds nothing to remove has reached its end state");
}

#[test]
fn should_label_a_snapshot_cost_estimated_because_the_accounting_is_not_what_deleting_frees() {
    let tools = runner(vec![
        (ZFS, out("get-used-by-snapshots")),
        (ZFS, out("list-snapshots")),
    ]);
    let cost = provider(&tools)
        .estimate_cost(&asset())
        .expect("the recorded pool reports its accounting");
    assert!(
        cost.is_estimated(),
        "§37.5: cost numbers MUST be labelled estimated where filesystem accounting is not exact"
    );
}

#[test]
fn should_never_report_a_copy_on_write_snapshot_as_free() {
    let tools = runner(vec![
        (ZFS, out("get-used-by-snapshots")),
        (ZFS, out("list-snapshots")),
    ]);
    let cost = provider(&tools)
        .estimate_cost(&asset())
        .expect("the recorded pool reports its accounting");
    assert_ne!(
        cost.retained_bytes(),
        Some(ByteSize::ZERO),
        "§38.2: Ono MUST NOT display `free` for CoW snapshots, and zero in a cost column is that \
         word spelled differently"
    );
    assert_ne!(cost.initial_bytes(), Some(ByteSize::ZERO));
}

#[test]
fn should_report_what_the_dataset_snapshots_hold_when_the_snapshot_itself_charges_nothing() {
    let tools = runner(vec![
        (
            ZFS,
            ToolOutput::ok("rpool/ROOT/debian\tusedbysnapshots\t131072\n"),
        ),
        (ZFS, out("list-snapshots")),
    ]);
    let cost = provider(&tools)
        .estimate_cost(&asset())
        .expect("the recorded pool reports its accounting");
    assert_eq!(
        cost.retained_bytes(),
        Some(ByteSize::from_bytes(131_072)),
        "§13.2: retained snapshots may cause additional space consumption, and Ono exposes it"
    );
}

#[test]
fn should_ask_zfs_for_the_three_properties_the_cost_model_rests_on() {
    let tools = runner(vec![
        (ZFS, out("get-used-by-snapshots")),
        (ZFS, out("list-snapshots")),
    ]);
    let _ = provider(&tools).estimate_cost(&asset());
    let call = tools
        .calls()
        .into_iter()
        .next()
        .expect("a property query ran");
    assert!(
        call.1.contains(&"used,written,usedbysnapshots".to_owned()),
        "§38.1: the dimensions come from ZFS's own accounting, got {:?}",
        call.1
    );
}

#[test]
fn should_keep_the_default_retention_section_thirty_seven_names() {
    assert_eq!(
        asset().retention().window(),
        ono_change_core::DEFAULT_RETENTION,
        "§37.1: twenty-four hours after successful verification"
    );
}

#[test]
fn should_render_the_recovery_asset_block_section_thirteen_eight_prints() {
    let rendered = render_asset(
        &asset()
            .restored_by(RestoreMethod::SelectiveFileRestore)
            .excluding(RecoveryExclusion::new("tank/home", "separate dataset"))
            .excluding(RecoveryExclusion::new(
                "process state",
                "runtime, not persistence",
            )),
    );
    assert!(rendered.contains("RECOVERY ASSET"));
    assert!(rendered.contains("type          ZFS snapshot"));
    assert!(rendered.contains("dataset       rpool/ROOT/debian"));
    assert!(rendered.contains("consistency   "));
    assert!(rendered.contains("restore       selective-file-restore"));
    assert!(
        rendered.contains("retained      24h after verification"),
        "§13.8: the retention is part of what the operator is shown, got:\n{rendered}"
    );
    assert!(
        rendered.contains("excluded\n  tank/home     separate dataset"),
        "§13.8 and §10.3: what it does not protect is stated where the protection is stated"
    );
}

#[test]
fn should_call_a_zfs_snapshot_a_local_recovery_point_rather_than_a_backup() {
    assert!(
        asset().is_local_recovery_point(),
        "§11.5: a local CoW snapshot shares the storage failure domain and is labelled as one"
    );
}

#[test]
fn should_emit_no_destructive_flag_across_the_whole_provider_lifecycle() {
    // Discovery, protection planning, creation, validation, recovery planning, restore, cleanup
    // and cost, driven end to end through one runner, so §13.6's prohibition is asserted over
    // every argument vector this provider produces rather than over the ones a test remembered.
    let tools = Script::examination()
        // the survey `discover` performs
        .then(ZFS, out("list-filesystems"))
        .then(ZFS, out("list-snapshots"))
        .then(ZFS, out("list-snapshots-sorted"))
        .then(ZFS, out("list-bookmarks"))
        .then(ZFS, out("list-clones"))
        .then(ZPOOL, out("zpool-list"))
        .then(ZPOOL, out("zpool-status"))
        // the pool reading of `plan_protection` and of `create`
        .then(ZPOOL, out("zpool-list"))
        .then(ZPOOL, out("zpool-status"))
        .then(ZPOOL, out("zpool-list"))
        .then(ZPOOL, out("zpool-status"))
        // `create`
        .then(ZFS, ToolOutput::ok(""))
        .then(ZFS, out("list-snapshots"))
        // `validate`
        .then(ZFS, out("list-snapshots"))
        .then(ZFS, out("get-mounted"))
        // `estimate_cost`
        .then(ZFS, out("get-used-by-snapshots"))
        .then(ZFS, out("list-snapshots"))
        // `cleanup`
        .then(ZFS, ToolOutput::ok(""))
        .runner();
    let provider = provider(&tools);
    let fragment = provider
        .plan_recovery(&asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    assert_eq!(fragment.method(), RestoreMethod::SelectiveFileRestore);
    let candidates = provider
        .discover(
            &ono_change_core::PersistenceDomain::refused(
                "/altroot/debian/etc/nginx/nginx.conf",
                ono_change_core::ResolvedMount::new("0:0", "/", "zfs", "-", "/"),
                ono_change_core::NonPersistentReason::Unresolved,
                "the caller supplies the path only",
            ),
            ono_change_core::RecoveryObjective::PreserveExact,
        )
        .expect("the recorded pool is discoverable");
    assert!(!candidates.is_empty());
    let actions = provider
        .plan_protection(
            &[ono_change_core::RecoveryCandidate::new(
                ono_recovery_zfs::PROVIDER_ID,
                RecoveryScope::new("zfs-dataset", ROOT_DATASET, "localhost").covering(ROOT_DATASET),
                ono_change_core::EffectDomain::FilesystemPersistent,
                ono_change_core::RecoveryObjective::PreserveExact,
                "snapshot rpool/ROOT/debian",
            )],
            ProtectionMode::Prefer,
        )
        .expect("the recorded pool has room");
    let _ = provider.create(actions.first().expect("one action"));
    let _ = provider.validate(&asset());
    let _ = provider.estimate_cost(&asset());
    let _ = provider.cleanup(&asset());

    let mut recursive_flags = 0_usize;
    for (_, argv) in tools.calls() {
        assert!(
            !argv.iter().any(|argument| argument == "-R"),
            "§13.6: no argument vector this provider produces contains `-R`, got {argv:?}"
        );
        if argv.iter().any(|argument| argument == "-r") {
            assert_eq!(
                argv.first().map(String::as_str),
                Some("snapshot"),
                "§13.3: `-r` appears only where recursive creation asked for it, got {argv:?}"
            );
            recursive_flags += 1;
        }
    }
    assert_eq!(
        recursive_flags, 0,
        "this lifecycle asked for no recursive creation, so no `-r` was emitted"
    );
}

#[test]
fn should_record_which_metadata_the_chosen_method_returns_on_the_fragment() {
    let tools = Script::examination().runner();
    let fragment = provider(&tools)
        .plan_recovery(&asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recorded pool plans a recovery");
    assert!(
        fragment.metadata().content,
        "Appendix C.7: the provider is the one that knows what its restore returns"
    );
    assert!(!fragment.metadata().hardlinks);
}
