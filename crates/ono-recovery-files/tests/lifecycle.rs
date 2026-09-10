#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

//! Capabilities, retention, cleanup and cost (spec v0.6 §12.2, §37, §38, Appendix G.5).

mod support;

use ono_change_core::{
    AssetState, RECOVERY_PROVIDER_CONFORMANCE, RecoveryAssetType, RecoveryCapability,
    RecoveryProvider, RecoveryScope, RetentionPolicy,
};
use ono_recovery_files::{FileRecoveryProvider, FileRecoveryStore, PROVIDER_ID};
use support::Fixture;

#[test]
fn should_name_itself_the_first_party_file_copy_provider() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.provider.id(),
        "ono.recovery.file-copy",
        "§15: v0.6 MUST provide a first-party narrow file/config recovery provider"
    );
    assert_eq!(fixture.provider.id(), PROVIDER_ID);
}

#[test]
fn should_declare_every_capability_a_recovery_provider_must_have() {
    let fixture = Fixture::new();
    let capabilities = fixture.provider.capabilities();
    assert!(
        capabilities.missing_required().is_empty(),
        "§12.2: a provider that cannot restore is not a recovery provider, however good its \
         discovery is"
    );
    assert_eq!(capabilities.provider(), PROVIDER_ID);
    assert_eq!(
        capabilities.conformance(),
        RECOVERY_PROVIDER_CONFORMANCE,
        "Appendix G.5: a provider advertises the conformance version it implements"
    );
}

#[test]
fn should_declare_no_capability_it_does_not_have() {
    let fixture = Fixture::new();
    let capabilities = fixture.provider.capabilities();
    assert!(
        !capabilities.has_recovery(RecoveryCapability::Quiesce),
        "§39.3: quiescing an application is an application-aware provider's claim, not a file \
         copier's"
    );
    assert!(
        !capabilities.has_recovery(RecoveryCapability::Transaction),
        "§27.1: the word transaction is reserved for a provider that can state atomicity \
         guarantees for its own domain"
    );
    assert!(
        !capabilities.may_execute(),
        "§48.4: a recovery provider is not thereby permitted to carry out a plan's mutations"
    );
}

#[test]
fn should_remove_the_copy_when_an_asset_is_cleaned_up() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    assert!(std::path::Path::new(asset.reference()).exists());

    fixture
        .provider
        .cleanup(&asset)
        .expect("the copy is removed");
    assert!(
        !std::path::Path::new(asset.reference()).exists(),
        "§44.6: deletion MUST remove the Ono-owned copies"
    );
}

#[test]
fn should_make_the_asset_unusable_once_its_copy_has_been_removed() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    fixture
        .provider
        .cleanup(&asset)
        .expect("the copy is removed");

    let validation = fixture
        .provider
        .validate(&asset)
        .expect("the check is made");
    assert_eq!(
        asset.validated(validation).state(),
        AssetState::Invalid,
        "§11.4: an asset whose copy is gone cannot satisfy protection, and says so"
    );
}

#[test]
fn should_succeed_when_an_asset_is_cleaned_up_twice() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    fixture
        .provider
        .cleanup(&asset)
        .expect("the copy is removed");
    fixture
        .provider
        .cleanup(&asset)
        .expect("§41.1: removing what is already removed is removing it once");
}

#[test]
fn should_refuse_to_clean_up_an_asset_whose_retention_is_held() {
    let fixture = Fixture::new();
    let store = FileRecoveryStore::open(fixture.path("held-store")).expect("a store opens");
    let provider = FileRecoveryProvider::new(store, fixture.provider.now())
        .with_retention(RetentionPolicy::held());
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let domain = provider
        .resolve_domain(&configuration.display().to_string())
        .expect("the path resolves")
        .expect("the path is protectable");
    let candidates = provider
        .discover(&domain, ono_change_core::RecoveryObjective::PreserveExact)
        .expect("discovery answers");
    let actions = provider
        .plan_protection(&candidates, ono_change_core::ProtectionMode::Prefer)
        .expect("planning answers");
    let asset = provider.create(&actions[0]).expect("the copy is made");

    let error = provider
        .cleanup(&asset)
        .expect_err("§37.2: an asset under a hold is not removed by an ordinary cleanup");
    assert_eq!(error.code().name(), "recovery.cleanup_blocked");
    assert!(
        std::path::Path::new(asset.reference()).exists(),
        "§2.15: a recovery asset a retained plan needs MUST NOT be deleted silently"
    );
}

#[test]
fn should_refuse_to_clean_up_an_asset_another_provider_owns() {
    let fixture = Fixture::new();
    let foreign = ono_change_core::RecoveryAsset::proposed(
        "ono.recovery.zfs",
        RecoveryAssetType::ZfsSnapshot,
        "tank/etc@ono-a82f",
        RecoveryScope::new("zfs-dataset", "tank/etc", "localhost"),
        fixture.provider.now(),
    );
    let error = fixture
        .provider
        .cleanup(&foreign)
        .expect_err("§12.1: only the owning provider removes an asset");
    assert_eq!(error.code().name(), "recovery.provider_unavailable");
}

#[test]
fn should_report_the_retained_cost_as_measured_rather_than_estimated() {
    let fixture = Fixture::new();
    let body = "worker_processes 1;\n";
    let configuration = fixture.write("etc/nginx.conf", body);
    let asset = fixture.protect(&configuration);

    let cost = fixture
        .provider
        .estimate_cost(&asset)
        .expect("the store can be measured");
    assert!(
        !cost.is_estimated(),
        "§37.5: cost numbers MUST be labelled estimated where filesystem accounting is not exact, \
         and a copy in a directory this provider owns is exact"
    );
    assert_eq!(
        cost.initial_bytes().map(ono_value::ByteSize::bytes),
        Some(body.len() as u128),
        "§38.1: the initial size is the bytes that were copied"
    );
    assert!(
        cost.retained_bytes()
            .is_some_and(|retained| retained.bytes() > body.len() as u128),
        "§38.1: what is retained is the copy plus the manifest that describes it"
    );
}

#[test]
fn should_report_no_downtime_for_a_file_copy() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let cost = fixture
        .provider
        .estimate_cost(&asset)
        .expect("the store can be measured");
    assert!(
        !cost.requires_reboot() && !cost.requires_offline(),
        "§38.1: copying a configuration file costs no downtime, and claiming otherwise would \
         weight the coverage algorithm against the least destructive candidate"
    );
}

#[test]
fn should_refuse_to_estimate_the_cost_of_a_copy_that_is_gone() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    fixture
        .provider
        .cleanup(&asset)
        .expect("the copy is removed");

    let error = fixture
        .provider
        .estimate_cost(&asset)
        .expect_err("§37.4: a cost is measured rather than remembered");
    assert_eq!(error.code().name(), "recovery.asset_not_found");
}

#[test]
fn should_refuse_to_create_the_same_copy_twice() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let domain = fixture
        .provider
        .resolve_domain(&configuration.display().to_string())
        .expect("the path resolves")
        .expect("the path is protectable");
    let candidates = fixture
        .provider
        .discover(&domain, ono_change_core::RecoveryObjective::PreserveExact)
        .expect("discovery answers");
    let actions = fixture
        .provider
        .plan_protection(&candidates, ono_change_core::ProtectionMode::Prefer)
        .expect("planning answers");
    let asset = fixture
        .provider
        .create(&actions[0])
        .expect("the copy is made");

    let error = fixture
        .provider
        .create(&actions[0])
        .expect_err("§62.3: an existing recovery copy is never silently overwritten");
    assert_eq!(error.code().name(), "recovery.asset_create_failed");
    let validation = fixture
        .provider
        .validate(&asset)
        .expect("the check is made");
    assert!(
        validation.is_complete(),
        "the copy that was already there is still the one that was validated"
    );
}

#[test]
fn should_refuse_to_remove_a_path_that_is_not_a_recovery_copy() {
    let fixture = Fixture::new();
    let outside = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let forged = ono_change_core::RecoveryAsset::proposed(
        PROVIDER_ID,
        RecoveryAssetType::FileArchive,
        fixture.path("etc").display().to_string(),
        RecoveryScope::new("file", outside.display().to_string(), "localhost"),
        fixture.provider.now(),
    );

    let error = fixture
        .provider
        .cleanup(&forged)
        .expect_err("§48.4: a reference is not authority to remove a path the store never held");
    assert_eq!(error.code().name(), "recovery.asset_not_found");
    assert!(
        outside.exists(),
        "§43.2: the provider acts inside its own store and nowhere else"
    );
}

#[test]
fn should_keep_working_when_the_asset_is_attributed_to_the_plan_that_asked_for_it() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let attributed = asset
        .clone()
        .for_plan(ono_change_core::PlanId::derive(&["a82f"]));

    assert_ne!(
        attributed.id(),
        asset.id(),
        "§11.1: attributing an asset to its source plan is part of its identity"
    );
    let validation = fixture
        .provider
        .validate(&attributed)
        .expect("the check is made");
    assert!(
        validation.is_complete(),
        "§11.1: the copy is found through the asset's own reference, so a re-derived identity \
         does not lose it"
    );
}

#[test]
fn should_say_a_copy_on_the_same_device_as_its_target_shares_its_failure_domain() {
    // NEW-7: the fixture's store and its target are both beneath `target/`, on one device.
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);

    assert!(
        fixture
            .provider
            .shares_failure_domain(&asset)
            .expect("the store and the target can both be inspected"),
        "§11.5: a copy on the device that holds the original is lost with it, and says so"
    );
}
