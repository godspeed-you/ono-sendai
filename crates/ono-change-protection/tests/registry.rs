//! The recovery provider registry (v0.6 §12.2, §55.6 case 29).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;

use ono_change_core::{
    ConsistencyClass, EffectDomain, PersistenceDomain, RecoveryCapability, RecoveryObjective,
    RestoreMethod, error,
};
use ono_change_protection::{MountTable, ProviderRegistry};
use ono_core::ErrorCode;

mod support;

use support::{TestProvider, ZFS_ROOT, candidate, snapshot_cost};

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
