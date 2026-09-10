//! A provider resolves its own persistence domain (v0.6 §11.2, Appendix B, ADR-0807).
//!
//! Appendix B's pipeline runs once, in this crate, and refuses what no local provider may claim.
//! The last step — which dataset, which subvolume id — belongs to the provider that understands
//! it, and a provider whose own discovery only recognises the objects it names must be handed the
//! domain it resolved rather than the core's reading of the mount options.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;

use ono_change_core::{
    ConsistencyClass, EffectDomain, EffectKind, PersistenceDomain, ProtectionLevel,
    RecoveryObjective, RestoreMethod,
};
use ono_change_protection::MountTable;
use ono_change_protection::coverage::{CoverageRequest, MutationDomain, analyse};
use ono_change_protection::policy::ProtectionPolicy;
use ono_core::ErrorCode;
use ono_value::ErrorValue;

mod support;

use support::{BTRFS_ROOT, TestProvider, candidate, snapshot_cost};

const STATE: &str = "/var/lib/app/state.db";
const SUBVOLUME: &str = "fs-5d1c:258:/@var";

fn core_resolution() -> PersistenceDomain {
    MountTable::from_text(BTRFS_ROOT).resolve(Path::new(STATE))
}

/// What the Btrfs provider answers for the path: the subvolume by filesystem and id.
fn provider_resolution() -> PersistenceDomain {
    PersistenceDomain::resolved(
        STATE,
        core_resolution().mount().clone(),
        "btrfs-subvolume",
        SUBVOLUME,
        "subvolume 258 (@var) of filesystem fs-5d1c",
    )
    .with_boundary("/@var")
}

fn subvolume_snapshot() -> ono_change_core::RecoveryCandidate {
    candidate(
        "ono.recovery.btrfs",
        "btrfs-subvolume",
        SUBVOLUME,
        &[STATE],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .costing(snapshot_cost())
}

fn state_change() -> MutationDomain {
    MutationDomain::new(
        EffectDomain::FilesystemPersistent,
        EffectKind::Modify,
        STATE,
        "the application state is rewritten",
    )
}

#[test]
fn should_record_the_domain_a_provider_resolved_rather_than_the_mount_option() {
    // Appendix B.9: a file in a nested subvolume lives in that subvolume, which only the provider
    // can name; the mount's `subvol=` option names the one the mount was made from.
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.btrfs")
            .resolving(STATE, provider_resolution())
            .shared(),
    ]);

    let domain = registry.resolve_at(STATE, &core_resolution());

    assert_eq!(
        domain,
        provider_resolution(),
        "§7.1: the domain a file target records is the object that holds its state"
    );
}

#[test]
fn should_keep_the_mount_table_reading_where_a_provider_names_the_path_itself() {
    // Appendix B.1: the persistence domain is the object that holds the state. A copy provider
    // answers with the path — how it would protect it — and that is not a storage object.
    let copy = PersistenceDomain::resolved(
        STATE,
        core_resolution().mount().clone(),
        "regular-file",
        STATE,
        "a copy of the object into a recovery store",
    )
    .with_boundary("/");
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.file-copy")
            .resolving(STATE, copy)
            .shared(),
    ]);

    let domain = registry.resolve_at(STATE, &core_resolution());

    assert_eq!(
        domain,
        core_resolution(),
        "§7.1: a file's recorded domain is where its state lives, not how a copy would protect it"
    );
}

#[test]
fn should_keep_the_mount_table_reading_where_no_provider_could_map_the_path() {
    let failure = ErrorValue::new(
        ErrorCode::ProviderInconclusive,
        "`btrfs subvolume show` was refused: Operation not permitted",
    );
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.btrfs")
            .failing_resolution(failure)
            .shared(),
    ]);

    let domain = registry.resolve_at(STATE, &core_resolution());

    assert_eq!(
        domain,
        core_resolution(),
        "§56.3: a resolution nobody established replaces nothing; the mount table's reading stands"
    );
}

#[test]
fn should_offer_protection_from_a_provider_that_resolves_the_domain_itself() {
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.btrfs")
            .resolving(STATE, provider_resolution())
            .offering(subvolume_snapshot())
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(core_resolution())
            .mutating(state_change()),
    );

    assert_eq!(
        analysis.level(),
        ProtectionLevel::Protected,
        "ADR-0807: the provider's own resolution is what its discovery is handed; the core's \
         `subvol=` reading ({:?}) names nothing the provider recognises. Rows: {:?}",
        core_resolution().object(),
        analysis.summary().rows()
    );
    assert_eq!(analysis.actions().len(), 1);
    assert_eq!(
        analysis.actions()[0].candidate().scope().domain(),
        SUBVOLUME
    );
}

#[test]
fn should_state_a_failed_domain_resolution_as_that_providers_refusal() {
    let failure = ErrorValue::new(
        ErrorCode::ProviderInconclusive,
        "`btrfs subvolume show` was refused: Operation not permitted",
    );
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.btrfs")
            .failing_resolution(failure)
            .offering(subvolume_snapshot())
            .offering(candidate(
                "ono.recovery.btrfs",
                "btrfs-subvolume",
                "/@var",
                &[STATE],
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
            ))
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(core_resolution())
            .mutating(state_change()),
    );

    assert!(
        analysis.actions().is_empty(),
        "§56.3: a provider that could not establish the domain protects nothing over it"
    );
    let refusal = analysis
        .provider_refusals()
        .iter()
        .find(|refusal| refusal.provider() == "ono.recovery.btrfs")
        .expect("ADR-0807: a resolution error is a stated refusal, not an empty answer");
    assert!(refusal.reason().contains("Operation not permitted"));
    let row = &analysis.summary().rows()[0];
    assert!(
        row.note().contains("§55.6"),
        "§55.6 case 29: the row says the absence of a candidate establishes nothing: {}",
        row.note()
    );
}
