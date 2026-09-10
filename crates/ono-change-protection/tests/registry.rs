//! The recovery provider registry (v0.6 §12.2, §55.6 case 29).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;

use ono_change_core::{
    AssetState, ConsistencyClass, EffectDomain, PersistenceDomain, RecoveryCapability,
    RecoveryObjective, ResolvedMount, RestoreMethod, error,
};
use ono_change_protection::{MountTable, ProviderRegistry};
use ono_core::ErrorCode;

mod support;

use support::{
    CONTAINER, TestProvider, ZFS_ROOT, candidate, ready_asset, registry_with, snapshot_cost,
};

fn root_dataset() -> PersistenceDomain {
    MountTable::from_text(ZFS_ROOT).resolve(Path::new("/etc/nginx/nginx.conf"))
}

fn tmpfs_path() -> PersistenceDomain {
    MountTable::from_text(ZFS_ROOT).resolve(Path::new("/run/nginx.pid"))
}

fn zfs_candidate() -> ono_change_core::RecoveryCandidate {
    candidate(
        "ono.recovery.zfs",
        "zfs-dataset",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf", "/etc/hosts"],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .costing(snapshot_cost())
}

#[test]
fn should_register_a_provider_that_declares_every_required_capability() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(TestProvider::new("ono.recovery.zfs").shared())
        .expect(
            "§12.2: a provider declaring all five required capabilities is a recovery provider",
        );
    assert_eq!(registry.ids(), vec!["ono.recovery.zfs"]);
    assert!(registry.get("ono.recovery.zfs").is_some());
}

#[test]
fn should_refuse_a_provider_that_cannot_restore() {
    let mut registry = ProviderRegistry::new();
    let refusal = registry
        .register(
            TestProvider::new("ono.recovery.halfway")
                .without(RecoveryCapability::Restore)
                .shared(),
        )
        .expect_err("§12.2: a provider that cannot restore is not a recovery provider");
    assert_eq!(refusal.code(), ErrorCode::RecoveryProviderUnavailable);
    assert!(
        refusal
            .help()
            .is_some_and(|help| help.contains("recovery.restore")),
        "§12.2: the refusal names the capability that is missing"
    );
    assert!(
        registry.ids().is_empty(),
        "§62.1: a candidate nobody can restore from is snapshot theatre, and it was not kept"
    );
}

#[test]
fn should_refuse_a_provider_for_any_missing_required_capability() {
    for capability in RecoveryCapability::REQUIRED {
        let mut registry = ProviderRegistry::new();
        let refusal = registry
            .register(
                TestProvider::new("ono.recovery.partial")
                    .without(*capability)
                    .shared(),
            )
            .expect_err("§12.2 requires all five capabilities");
        assert!(
            refusal
                .help()
                .is_some_and(|help| help.contains(capability.as_str())),
            "§12.2: the refusal names {} as the missing capability",
            capability.as_str()
        );
    }
}

#[test]
fn should_refuse_a_second_provider_answering_to_a_registered_id() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(TestProvider::new("ono.recovery.zfs").shared())
        .expect("the first registration succeeds");
    let refusal = registry
        .register(TestProvider::new("ono.recovery.zfs").shared())
        .expect_err("§11.1: an asset's provider is its provenance, and two of them is ambiguous");
    assert_eq!(refusal.code(), ErrorCode::RecoveryProviderUnavailable);
    assert_eq!(registry.ids().len(), 1);
}

#[test]
fn should_report_an_unavailable_provider_rather_than_answering_with_nothing() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(
            TestProvider::new("ono.recovery.zfs")
                .unavailable("the `zfs` command is not installed")
                .shared(),
        )
        .expect("an unavailable provider is still a registered one");

    let outcome = registry.discover(&root_dataset(), RecoveryObjective::PreserveExact);
    assert!(outcome.candidates().is_empty());
    assert!(
        !outcome.is_conclusive(),
        "§55.6 case 29: an empty list from a provider that could not be asked establishes nothing"
    );
    assert_eq!(outcome.refusals().len(), 1);
    assert_eq!(outcome.refusals()[0].provider(), "ono.recovery.zfs");
    assert!(
        outcome.refusals()[0].reason().contains("zfs"),
        "§12.2: the skip says why, in a sentence an operator can act on"
    );
}

#[test]
fn should_call_an_empty_answer_conclusive_when_every_provider_could_be_asked() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(TestProvider::new("ono.recovery.btrfs").shared())
        .expect("a fully capable provider registers");
    let outcome = registry.discover(&root_dataset(), RecoveryObjective::PreserveExact);
    assert!(outcome.candidates().is_empty());
    assert!(
        outcome.is_conclusive(),
        "§55.6 case 29: nothing on offer from providers that were all asked is an answer"
    );
}

#[test]
fn should_report_a_provider_whose_discovery_failed() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(
            TestProvider::new("ono.recovery.zfs")
                .failing_discovery(error::tool_failed("zfs", "the pool is suspended"))
                .shared(),
        )
        .expect("the provider is capable and available");
    let outcome = registry.discover(&root_dataset(), RecoveryObjective::PreserveExact);
    assert!(outcome.candidates().is_empty());
    assert!(
        !outcome.is_conclusive(),
        "Appendix A.3: discovery that failed is not discovery that found nothing"
    );
    assert_eq!(
        outcome.refusals()[0].error().code(),
        ErrorCode::RecoveryProviderUnavailable
    );
}

#[test]
fn should_collect_candidates_from_every_available_provider() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(
            TestProvider::new("ono.recovery.zfs")
                .offering(zfs_candidate())
                .shared(),
        )
        .expect("a capable provider registers");
    registry
        .register(
            TestProvider::new("ono.recovery.files")
                .offering(support::file_archive(
                    "ono.recovery.files",
                    "/etc/nginx/nginx.conf",
                ))
                .shared(),
        )
        .expect("a capable provider registers");

    let outcome = registry.discover(&root_dataset(), RecoveryObjective::PreserveExact);
    assert_eq!(
        outcome.candidates().len(),
        2,
        "Appendix A.3: every registered provider is asked what it could protect"
    );
    assert!(outcome.is_conclusive());
}

#[test]
fn should_not_ask_a_provider_about_a_path_with_no_protectable_domain() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(
            TestProvider::new("ono.recovery.zfs")
                .offering(zfs_candidate())
                .shared(),
        )
        .expect("a capable provider registers");
    let outcome = registry.discover(&tmpfs_path(), RecoveryObjective::PreserveExact);
    assert!(
        outcome.candidates().is_empty(),
        "§32.4: a tmpfs beneath `/` is never presented as snapshot-protected"
    );
}

#[test]
fn should_keep_both_halves_of_two_merged_outcomes() {
    let mut with_candidates = ProviderRegistry::new();
    with_candidates
        .register(
            TestProvider::new("ono.recovery.zfs")
                .offering(zfs_candidate())
                .shared(),
        )
        .expect("a capable provider registers");
    let mut with_refusal = ProviderRegistry::new();
    with_refusal
        .register(
            TestProvider::new("ono.recovery.btrfs")
                .unavailable("no btrfs filesystem is mounted")
                .shared(),
        )
        .expect("an unavailable provider is registered");

    let merged = with_candidates
        .discover(&root_dataset(), RecoveryObjective::PreserveExact)
        .merged_with(with_refusal.discover(&root_dataset(), RecoveryObjective::PreserveExact));
    assert_eq!(merged.candidates().len(), 1);
    assert_eq!(merged.refusals().len(), 1);
    assert!(
        !merged.is_conclusive(),
        "§55.6 case 29: one unasked provider makes the whole answer inconclusive"
    );
}

#[test]
fn should_list_only_the_providers_that_can_run_here() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(TestProvider::new("ono.recovery.zfs").shared())
        .expect("a capable provider registers");
    registry
        .register(
            TestProvider::new("ono.recovery.btrfs")
                .unavailable("btrfs-progs is not installed")
                .shared(),
        )
        .expect("an unavailable provider is registered");
    assert_eq!(registry.available().len(), 1);
    assert_eq!(registry.unavailable().len(), 1);
    assert_eq!(registry.providers().len(), 2);
}

#[test]
fn should_name_the_capabilities_a_provider_would_have_to_add() {
    let capabilities = TestProvider::new("ono.recovery.partial")
        .without(RecoveryCapability::Cleanup)
        .capabilities_for_test();
    assert_eq!(
        ProviderRegistry::registration_shortfall(&capabilities),
        vec!["recovery.cleanup"],
        "§12.2: the shortfall is stated as capability names a plugin manifest can carry"
    );
}

#[test]
fn should_hand_each_provider_the_domain_it_resolved_itself() {
    let own = PersistenceDomain::resolved(
        "/etc/nginx/nginx.conf",
        root_dataset().mount().clone(),
        "zfs-dataset",
        "rpool/ROOT/debian#guid-7731",
        "the provider's own identity for the dataset",
    );
    let mut registry = ProviderRegistry::new();
    registry
        .register(
            TestProvider::new("ono.recovery.zfs")
                .resolving("/etc/nginx/nginx.conf", own)
                .offering(candidate(
                    "ono.recovery.zfs",
                    "zfs-dataset",
                    "rpool/ROOT/debian#guid-7731",
                    &["/etc/nginx/nginx.conf"],
                    EffectDomain::FilesystemPersistent,
                    RecoveryObjective::PreserveExact,
                ))
                .shared(),
        )
        .expect("a capable provider registers");

    let outcome = registry.discover_at(
        "/etc/nginx/nginx.conf",
        Some(&root_dataset()),
        RecoveryObjective::PreserveExact,
    );
    assert_eq!(
        outcome.candidates().len(),
        1,
        "ADR-0807: the provider's discovery is handed the domain it resolved"
    );
    assert_eq!(
        outcome
            .resolved_by("ono.recovery.zfs")
            .and_then(PersistenceDomain::object),
        Some("rpool/ROOT/debian#guid-7731")
    );
}

#[test]
fn should_not_ask_a_provider_to_resolve_a_path_the_core_refused() {
    let own = PersistenceDomain::resolved(
        "/run/nginx.pid",
        root_dataset().mount().clone(),
        "zfs-dataset",
        "rpool/ROOT/debian",
        "a provider that would claim a tmpfs path",
    );
    let mut registry = ProviderRegistry::new();
    registry
        .register(
            TestProvider::new("ono.recovery.zfs")
                .resolving("/run/nginx.pid", own)
                .offering(zfs_candidate())
                .shared(),
        )
        .expect("a capable provider registers");

    let outcome = registry.discover_at(
        "/run/nginx.pid",
        Some(&tmpfs_path()),
        RecoveryObjective::PreserveExact,
    );
    assert!(
        outcome.candidates().is_empty(),
        "Appendix B.7 and §32.4: the core's refusal of a tmpfs stands whatever a provider says"
    );
}

#[test]
fn should_refuse_a_provider_resolution_that_goes_through_a_different_mount_than_the_kernels() {
    let table = MountTable::from_text(support::SRV_TREE);
    let core = table.resolve(Path::new("/srv/legacy/report.csv"));
    let root = table.resolve(Path::new("/etc/hosts"));
    let claimed = PersistenceDomain::resolved(
        "/srv/legacy/report.csv",
        root.mount().clone(),
        "btrfs-subvolume",
        "fs-5d1c:256:/@",
        "a provider that only reads its own filesystem's mounts and so sees `/` as the deepest",
    );
    let mut registry = ProviderRegistry::new();
    registry
        .register(
            TestProvider::new("ono.recovery.btrfs")
                .resolving("/srv/legacy/report.csv", claimed)
                .offering(candidate(
                    "ono.recovery.btrfs",
                    "btrfs-subvolume",
                    "fs-5d1c:256:/@",
                    &["/srv/legacy/report.csv"],
                    EffectDomain::FilesystemPersistent,
                    RecoveryObjective::PreserveExact,
                ))
                .shared(),
        )
        .expect("a capable provider registers");

    let outcome = registry.discover_at(
        "/srv/legacy/report.csv",
        Some(&core),
        RecoveryObjective::PreserveExact,
    );
    assert!(
        outcome.candidates().is_empty(),
        "Appendix B.1 and §56.3: the kernel serves the path from the ext4 disk at /srv/legacy, \
         and a snapshot of the filesystem at `/` holds none of it"
    );
    let refusal = outcome
        .refusals()
        .iter()
        .find(|refusal| refusal.provider() == "ono.recovery.btrfs")
        .expect("two readings that disagree are a stated refusal rather than a silent skip");
    assert!(
        refusal.reason().contains("/srv/legacy"),
        "{}",
        refusal.reason()
    );
}

#[test]
fn should_not_let_a_provider_answer_turn_a_tmpfs_path_into_a_persistence_domain() {
    // Acceptance 287 pr3a: Docker's `/dev/shm` is a tmpfs sourced `shm`. Whatever a provider says
    // about the path, Appendix B.7 makes it no persistence domain at all.
    let path = "/dev/shm/ono-volatile";
    let volatile = MountTable::from_text(CONTAINER).resolve(Path::new(path));
    let claimed = PersistenceDomain::resolved(
        path,
        ResolvedMount::new("0:319", "/dev/shm", "ext4", "shm", "/"),
        "filesystem",
        "shm",
        "a provider that misread the mount",
    );
    let offered = candidate(
        "ono.recovery.file-copy",
        "filesystem",
        "shm",
        &[path],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::ByteConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore);
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.file-copy")
            .resolving(path, claimed)
            .offering(offered)
            .shared(),
    ]);

    let outcome = registry.discover_at(path, Some(&volatile), RecoveryObjective::PreserveExact);
    assert!(
        outcome.candidates().is_empty(),
        "Appendix B.7: a tmpfs is never a persistent recovery domain, whatever a provider answers"
    );
    assert!(
        outcome.resolutions().is_empty(),
        "the provider's own resolution of a refused path is not recorded as a domain"
    );
}

#[test]
fn should_mark_a_ready_asset_invalid_when_its_provider_no_longer_finds_it() {
    // NEW-11: the stored bytes were deleted behind Ono's back, and the record still says ready.
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .failing_validation("the snapshot is no longer there")
            .shared(),
    ]);
    let asset = ready_asset("rpool/ROOT/debian@ono-1");
    assert_eq!(asset.state(), AssetState::Ready, "the record says ready");

    let checked = registry.revalidate(&asset).expect("the provider answered");
    assert_eq!(
        checked.state(),
        AssetState::Invalid,
        "§11.4: an asset that no longer passes validation is INVALID, not READY"
    );
    assert!(
        checked
            .validation()
            .expect("the fresh validation travels with the asset")
            .failures()
            .contains(&"the asset does not exist"),
        "§11.4's first check is the one that failed"
    );
}

#[test]
fn should_keep_a_ready_asset_ready_when_its_provider_still_validates_it() {
    let registry = registry_with(vec![TestProvider::new("ono.recovery.zfs").shared()]);
    let checked = registry
        .revalidate(&ready_asset("rpool/ROOT/debian@ono-1"))
        .expect("the provider answered");
    assert_eq!(checked.state(), AssetState::Ready);
}

#[test]
fn should_refuse_to_vouch_for_an_asset_whose_provider_cannot_be_asked() {
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .unavailable("zfs is not installed")
            .shared(),
    ]);
    let error = registry
        .revalidate(&ready_asset("rpool/ROOT/debian@ono-1"))
        .expect_err("§56.3: an asset nobody could check is not confirmed ready");
    assert_eq!(error.code(), ErrorCode::RecoveryProviderUnavailable);
}
