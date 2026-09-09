#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

//! Planning the way back (spec v0.6 §24, Appendix C.3, C.4, C.6).

mod support;

use ono_change_core::{
    ActionRole, DirectoryRestorePolicy, Idempotency, NewerStateClass, RecoveryAssetType,
    RecoveryGoal, RecoveryProvider, RecoveryScope, RestoreMethod, VerificationClass,
};
use ono_recovery_files::{FileRecoveryProvider, FileRecoveryStore};
use support::{Fixture, plan_touching};

#[test]
fn should_plan_a_selective_file_restore_for_a_ready_asset() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);

    let fragment = fixture
        .provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");
    assert_eq!(
        fragment.method(),
        RestoreMethod::SelectiveFileRestore,
        "Appendix C.1: read the wanted objects out of the asset and leave everything else alone"
    );
    assert_eq!(fragment.actions().len(), 1);
    assert_eq!(fragment.actions()[0].role(), ActionRole::Recover);
    assert_eq!(
        fragment.actions()[0].target(),
        Some(configuration.display().to_string().as_str())
    );
    assert!(
        !fragment.requires_reboot() && !fragment.requires_offline(),
        "§15: putting one file back needs neither a reboot nor an unmounted filesystem"
    );
}

#[test]
fn should_plan_an_idempotent_recovery_action_so_a_resume_may_rerun_it() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let fragment = fixture
        .provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");
    assert_eq!(
        fragment.actions()[0].idempotency(),
        Idempotency::Idempotent,
        "§41.1: writing the same bytes twice is writing them once, so a resume may rerun it"
    );
    assert!(
        fragment.actions()[0]
            .declared_recovery()
            .is_some_and(|semantics| semantics.contains("renamed over")),
        "§15.4: the action states the replacement semantics it will use"
    );
}

#[test]
fn should_verify_the_restored_digest_rather_than_an_exit_code() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let fragment = fixture
        .provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");

    let contract = fragment
        .verification()
        .first()
        .expect("§25.1: recovery states what equivalence it will check");
    assert_eq!(contract.class(), VerificationClass::Required);
    assert!(
        contract.expression().contains("SHA-256"),
        "§62.9: an exit code is not verification; the observed state is"
    );
}

#[test]
fn should_show_a_conflict_when_the_object_was_changed_again_after_the_recovery_point() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    // The plan wrote at 14:03, and the operator edited again at 15:12 (Appendix C.4).
    fixture.write("etc/nginx.conf", "worker_processes 8;\n");
    fixture.write("etc/nginx.conf", "worker_processes 4; # by hand\n");

    let plan = plan_touching(&configuration);
    let fragment = fixture
        .provider
        .plan_recovery(&asset, Some(&plan), RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");

    let conflict = fragment
        .newer_state()
        .items()
        .iter()
        .find(|item| item.object() == configuration.display().to_string())
        .expect("the object the recovery would restore is classified");
    assert_eq!(
        conflict.class(),
        NewerStateClass::Conflicting,
        "Appendix C.4: if the same target file was edited again after the failed plan, Ono MUST \
         show this as a target-level conflict"
    );
    assert!(
        fragment.newer_state().requires_destructive_acceptance(),
        "§24.5: recovery that would discard the newer edit is gated on explicit acceptance"
    );
}

#[test]
fn should_leave_a_recovery_that_discards_nothing_ungated() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let plan = plan_touching(&configuration);

    let fragment = fixture
        .provider
        .plan_recovery(&asset, Some(&plan), RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");
    assert!(
        fragment.newer_state().losses().is_empty(),
        "Appendix C.3: nothing has changed since the copy, so nothing would be lost"
    );
    assert!(
        !fragment.newer_state().requires_destructive_acceptance(),
        "§24.5's gate applies to a recovery that destroys something, and this one does not"
    );
}

#[test]
fn should_complete_the_newer_state_analysis_rather_than_leaving_it_unanalysed() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let fragment = fixture
        .provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");
    assert!(
        fragment.newer_state().is_complete(),
        "§62.8: recovery without drift analysis is a failure mode, so the analysis actually ran"
    );
}

#[test]
fn should_preserve_the_objects_the_restore_set_does_not_touch() {
    let fixture = Fixture::new();
    let touched = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let untouched = fixture.write("etc/mime.types", "text/plain txt;\n");
    let asset = fixture.protect(&fixture.path("etc"));
    fixture.write("etc/mime.types", "text/plain txt; text/csv csv;\n");

    let plan = plan_touching(&touched);
    let fragment = fixture
        .provider
        .plan_recovery(&asset, Some(&plan), RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");

    let item = fragment
        .newer_state()
        .items()
        .iter()
        .find(|item| item.object() == untouched.display().to_string())
        .expect("a changed object outside the restore set is still classified");
    assert_eq!(
        item.class(),
        NewerStateClass::PreservedByMethod,
        "Appendix C.3: a selective file restore leaves objects outside the restore set alone"
    );
    assert_eq!(
        fragment.actions().len(),
        1,
        "Appendix C.2: the goal is to put back the objects the plan changed, and nothing else"
    );
}

#[test]
fn should_restrict_the_restore_set_to_what_the_source_plan_changed() {
    let fixture = Fixture::new();
    let touched = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    fixture.write("etc/mime.types", "text/plain txt;\n");
    let asset = fixture.protect(&fixture.path("etc"));

    let plan = plan_touching(&touched);
    let fragment = fixture
        .provider
        .plan_recovery(&asset, Some(&plan), RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");
    let targets: Vec<&str> = fragment
        .actions()
        .iter()
        .filter_map(ono_change_core::PlanAction::target)
        .collect();
    assert_eq!(targets, vec![touched.display().to_string().as_str()]);
}

#[test]
fn should_restore_everything_the_archive_holds_when_no_plan_names_a_target() {
    let fixture = Fixture::new();
    fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    fixture.write("etc/mime.types", "text/plain txt;\n");
    let asset = fixture.protect(&fixture.path("etc"));

    let fragment = fixture
        .provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");
    assert_eq!(
        fragment.actions().len(),
        3,
        "Appendix C.2: without a plan to narrow it, the recovery restores what the asset holds"
    );
}

#[test]
fn should_refuse_to_plan_when_the_source_plan_touched_nothing_the_asset_holds() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let elsewhere = fixture.write("srv/index.html", "<h1>hello</h1>\n");
    let asset = fixture.protect(&configuration);

    let plan = plan_touching(&elsewhere);
    let error = fixture
        .provider
        .plan_recovery(&asset, Some(&plan), RecoveryGoal::RestoreChangedObjects)
        .expect_err("§11.2: an asset covers what it covers, and not what is near it");
    assert_eq!(error.code().name(), "recovery.scope_mismatch");
}

#[test]
fn should_refuse_a_recovery_goal_a_file_archive_cannot_satisfy() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);

    for goal in [
        RecoveryGoal::RestoreDomain,
        RecoveryGoal::CompensateSemantics,
        RecoveryGoal::RestoreServiceHealth,
    ] {
        let error = fixture
            .provider
            .plan_recovery(&asset, None, goal)
            .expect_err("Appendix C.1: a file archive restores the objects it holds");
        assert_eq!(
            error.code().name(),
            "recovery.plan_incomplete",
            "§56.3: a recovery fact that cannot be established blocks rather than guesses"
        );
    }
}

#[test]
fn should_refuse_to_plan_from_an_asset_that_was_never_validated() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration).invalidated();

    let error = fixture
        .provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect_err("§11.4: an INVALID asset can no longer satisfy protection");
    assert_eq!(error.code().name(), "recovery.asset_invalid");
}

#[test]
fn should_refuse_to_plan_from_an_asset_another_provider_owns() {
    let fixture = Fixture::new();
    let foreign = ono_change_core::RecoveryAsset::proposed(
        "ono.recovery.btrfs",
        RecoveryAssetType::BtrfsSnapshot,
        "/.snapshots/ono-a82f",
        RecoveryScope::new("btrfs-subvolume", "@rootfs", "localhost"),
        fixture.provider.now(),
    );
    let error = fixture
        .provider
        .plan_recovery(&foreign, None, RecoveryGoal::RestoreChangedObjects)
        .expect_err("§12.1: only the owning provider answers for an asset");
    assert_eq!(error.code().name(), "recovery.provider_unavailable");
}

#[test]
fn should_show_a_newer_file_as_preserved_when_the_policy_keeps_extra_files() {
    let fixture = Fixture::new();
    fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&fixture.path("etc"));
    let newer = fixture.write("etc/local.conf", "added afterwards\n");

    let fragment = fixture
        .provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");
    let item = fragment
        .newer_state()
        .items()
        .iter()
        .find(|item| item.object() == newer.display().to_string())
        .expect("a file the archive never held is classified");
    assert_eq!(
        item.class(),
        NewerStateClass::PreservedByMethod,
        "Appendix C.6: default selective directory restore MUST NOT delete newer extra files"
    );
}

#[test]
fn should_show_a_newer_file_as_discarded_when_the_policy_requires_an_exact_tree() {
    let fixture = Fixture::new();
    let store = FileRecoveryStore::open(fixture.path("exact-store")).expect("a store opens");
    let provider = FileRecoveryProvider::new(store, fixture.provider.now())
        .with_directory_policy(DirectoryRestorePolicy::ExactTree);
    fixture.write("etc/nginx.conf", "worker_processes 1;\n");

    let domain = provider
        .resolve_domain(&fixture.path("etc").display().to_string())
        .expect("the path resolves")
        .expect("the path is protectable");
    let candidates = provider
        .discover(&domain, ono_change_core::RecoveryObjective::PreserveExact)
        .expect("discovery answers");
    let actions = provider
        .plan_protection(&candidates, ono_change_core::ProtectionMode::Prefer)
        .expect("planning answers");
    let asset = provider.create(&actions[0]).expect("the copy is made");
    let newer = fixture.write("etc/local.conf", "added afterwards\n");

    let fragment = provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");
    let item = fragment
        .newer_state()
        .items()
        .iter()
        .find(|item| item.object() == newer.display().to_string())
        .expect("a file the archive never held is classified");
    assert_eq!(
        item.class(),
        NewerStateClass::DiscardedByMethod,
        "Appendix C.6: exact-tree equivalence deletes what the archive never held, and says so"
    );
    assert!(
        fragment.newer_state().requires_destructive_acceptance(),
        "§24.5: a recovery that removes a newer file is gated on explicit acceptance"
    );
}

#[test]
fn should_report_an_object_it_cannot_read_as_unknown_rather_than_as_unchanged() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    std::fs::set_permissions(&configuration, std::fs::Permissions::from_mode(0o000))
        .expect("the mode can be narrowed");

    let fragment = fixture
        .provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");
    let unreadable = std::fs::read(&configuration).is_err();
    assert_eq!(
        fragment
            .newer_state()
            .items()
            .iter()
            .any(|item| item.class() == NewerStateClass::Unknown),
        unreadable,
        "§56.3: what recovery would do to an object that cannot be read stays unknown rather \
         than being reported as unchanged"
    );
    std::fs::set_permissions(&configuration, std::fs::Permissions::from_mode(0o600))
        .expect("the mode can be restored");
}
