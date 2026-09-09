#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

//! Planning protection, creating the copy, and §11.4's validation (spec v0.6 §4.5, §11.4, C.7).

mod support;

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use ono_change_core::{
    AssetState, ConsistencyClass, DEFAULT_RETENTION, ProtectionMode, RecoveryAsset,
    RecoveryAssetType, RecoveryCandidate, RecoveryObjective, RecoveryProvider, RecoveryScope,
    RestoreMethod, RetentionPolicy,
};
use ono_recovery_files::FileRecoveryProvider;
use support::{Fixture, can_change_owner, supports_xattrs};

fn candidate(fixture: &Fixture, path: &Path) -> RecoveryCandidate {
    let domain = fixture
        .provider
        .resolve_domain(&path.display().to_string())
        .expect("the path resolves")
        .expect("the path is protectable");
    fixture
        .provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery answers")
        .into_iter()
        .next()
        .expect("a persistent file yields a candidate")
}

fn validation_failures(provider: &FileRecoveryProvider, asset: &RecoveryAsset) -> Vec<String> {
    provider
        .validate(asset)
        .expect("the check itself can be made")
        .failures()
        .iter()
        .map(|failure| (*failure).to_owned())
        .collect()
}

#[test]
fn should_propose_an_asset_whose_copy_would_live_in_the_store_when_it_plans_protection() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let actions = fixture
        .provider
        .plan_protection(
            &[candidate(&fixture, &configuration)],
            ProtectionMode::Prefer,
        )
        .expect("planning answers");

    let action = &actions[0];
    assert_eq!(
        action.proposed_asset().state(),
        AssetState::Proposed,
        "§2.1: planning is side-effect free, so a planned asset has not been created"
    );
    assert_eq!(
        action.proposed_asset().asset_type(),
        RecoveryAssetType::FileArchive
    );
    assert!(
        action
            .proposed_asset()
            .reference()
            .starts_with(&fixture.provider.store().root().display().to_string()),
        "§15.3: the copy is proposed inside the protected Ono recovery store"
    );
    assert!(
        !PathBuf::from(action.proposed_asset().reference()).exists(),
        "§2.1: nothing exists until the PREPARE action runs"
    );
}

#[test]
fn should_plan_no_protection_when_the_policy_is_off() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let actions = fixture
        .provider
        .plan_protection(&[candidate(&fixture, &configuration)], ProtectionMode::Off)
        .expect("planning answers");
    assert!(
        actions.is_empty(),
        "§17.2: `off` creates no automatic recovery assets"
    );
}

#[test]
fn should_keep_a_protection_action_required_when_the_policy_prefers_protection() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let actions = fixture
        .provider
        .plan_protection(
            &[candidate(&fixture, &configuration)],
            ProtectionMode::Prefer,
        )
        .expect("planning answers");
    assert!(
        actions[0].is_required(),
        "§2.3: if a required recovery asset cannot be created, mutation MUST NOT begin"
    );
}

#[test]
fn should_make_a_protection_action_optional_when_the_policy_maximises_coverage() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let actions = fixture
        .provider
        .plan_protection(
            &[candidate(&fixture, &configuration)],
            ProtectionMode::Maximize,
        )
        .expect("planning answers");
    assert!(
        !actions[0].is_required(),
        "§17.2: `maximize` adds coverage on top, and a failure there degrades the matrix rather \
         than the plan"
    );
}

#[test]
fn should_ignore_a_candidate_another_provider_offered() {
    let fixture = Fixture::new();
    let foreign = RecoveryCandidate::new(
        "ono.recovery.zfs",
        RecoveryScope::new("zfs-dataset", "tank/data", "localhost"),
        ono_change_core::EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        "snapshot tank/data",
    );
    let actions = fixture
        .provider
        .plan_protection(&[foreign], ProtectionMode::Prefer)
        .expect("planning answers");
    assert!(
        actions.is_empty(),
        "§12.1: a provider answers for its own mechanism and never for another's"
    );
}

#[test]
fn should_create_a_ready_asset_when_the_copy_succeeds() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);

    assert_eq!(
        asset.state(),
        AssetState::Ready,
        "§11.4: an asset is usable only once existence, identity, scope, restore availability and \
         permissions have been checked"
    );
    assert_eq!(
        asset.consistency(),
        ConsistencyClass::ByteConsistent,
        "§11.3: a file copy restores the captured bytes and claims nothing filesystem-wide"
    );
    assert_eq!(asset.restore_method(), RestoreMethod::SelectiveFileRestore);
    assert!(
        !asset.is_local_recovery_point(),
        "§11.5: a copy into an independent store does not share the failure domain a snapshot does"
    );
}

#[test]
fn should_record_the_state_it_captured_so_freshness_can_be_checked() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let fingerprint = asset
        .captured_state()
        .expect("§18.3: an asset states the fingerprint of what it captured")
        .to_owned();
    assert!(
        asset.is_fresh_for(&fingerprint),
        "§18.3: Ono does not pretend an asset of unknown vintage is a just-before-change point"
    );
    assert!(!asset.is_fresh_for("something else"));
}

#[test]
fn should_expire_an_asset_after_the_default_retention() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let expected = fixture
        .provider
        .now()
        .checked_add(jiff::SignedDuration::try_from(DEFAULT_RETENTION).expect("a valid span"))
        .expect("the epoch plus a day is a timestamp");
    assert_eq!(
        asset.expires_at(),
        Some(expected),
        "§37.1: the default temporary recovery retention is 24 hours"
    );
}

#[test]
fn should_carry_a_held_retention_onto_the_asset_it_creates() {
    let fixture = Fixture::new();
    let store = ono_recovery_files::FileRecoveryStore::open(fixture.path("held-store"))
        .expect("a second store can be opened");
    let provider = FileRecoveryProvider::new(store, fixture.provider.now())
        .with_retention(RetentionPolicy::held());
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");

    let domain = provider
        .resolve_domain(&configuration.display().to_string())
        .expect("the path resolves")
        .expect("the path is protectable");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery answers");
    let actions = provider
        .plan_protection(&candidates, ProtectionMode::Prefer)
        .expect("planning answers");
    let asset = provider.create(&actions[0]).expect("the copy is made");
    assert!(
        asset.retention().is_held(),
        "§37.2: assets for failed plans MUST NOT be removed by ordinary success retention"
    );
}

#[test]
fn should_measure_the_cost_of_the_copy_exactly_when_it_creates_an_asset() {
    let fixture = Fixture::new();
    let body = "worker_processes 1;\n";
    let configuration = fixture.write("etc/nginx.conf", body);
    let asset = fixture.protect(&configuration);
    assert!(
        !asset.cost().is_estimated(),
        "§37.5: this provider copied the bytes, so its cost is exact rather than estimated"
    );
    assert_eq!(
        asset.cost().initial_bytes().map(ono_value::ByteSize::bytes),
        Some(body.len() as u128)
    );
}

#[test]
fn should_refuse_a_protection_action_another_provider_owns() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let mine = candidate(&fixture, &configuration);
    let actions = fixture
        .provider
        .plan_protection(std::slice::from_ref(&mine), ProtectionMode::Prefer)
        .expect("planning answers");
    let foreign = ono_change_core::ProtectionAction::new(
        "ono.recovery.btrfs",
        "snapshot @rootfs",
        mine,
        actions[0].proposed_asset().clone(),
    );
    let error = fixture
        .provider
        .create(&foreign)
        .expect_err("§12.1: a provider does not act for another provider's action");
    assert_eq!(error.code().name(), "recovery.asset_create_failed");
}

#[test]
fn should_refuse_when_the_objects_changed_between_planning_and_creating() {
    let fixture = Fixture::new();
    fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let candidate = candidate(&fixture, &fixture.path("etc"));
    let actions = fixture
        .provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning answers");
    fixture.write("etc/conf.d/gzip.conf", "gzip on;\n");

    let error = fixture
        .provider
        .create(&actions[0])
        .expect_err("§7.3: the scope the plan showed is the scope that is protected");
    assert_eq!(error.code().name(), "recovery.scope_mismatch");
}

#[test]
fn should_validate_a_freshly_created_asset_against_every_check_in_the_list() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let validation = fixture
        .provider
        .validate(&asset)
        .expect("the check is made");
    assert!(
        validation.is_complete(),
        "§11.4: existence, identity, scope, restore availability and permissions all hold"
    );
    assert_eq!(validation.at(), fixture.provider.now());
}

#[test]
fn should_call_an_asset_invalid_when_its_copy_is_no_longer_in_the_store() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    std::fs::remove_dir_all(asset.reference()).expect("the copy can be removed");

    let validation = fixture
        .provider
        .validate(&asset)
        .expect("the check is made");
    assert!(
        validation.failures().contains(&"the asset does not exist"),
        "§11.4's first check: the asset exists"
    );
    assert_eq!(
        asset.validated(validation).state(),
        AssetState::Invalid,
        "§11.4: an asset that fails a check is INVALID rather than merely unmentioned"
    );
}

#[test]
fn should_call_an_asset_invalid_when_the_stored_copy_was_changed_underneath_it() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let blob = std::path::Path::new(asset.reference()).join("objects/00000001");
    std::fs::write(&blob, "worker_processes 8;\n").expect("the copy can be overwritten");

    let validation = fixture
        .provider
        .validate(&asset)
        .expect("the check is made");
    assert!(
        validation
            .failures()
            .contains(&"the asset's identity does not match the planned source"),
        "§11.4: the identity check re-derives the digest recorded at creation"
    );
    assert_eq!(asset.validated(validation).state(), AssetState::Invalid);
}

#[test]
fn should_call_an_asset_invalid_when_its_manifest_no_longer_matches_its_fingerprint() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let manifest = std::path::Path::new(asset.reference()).join("manifest");
    let text = std::fs::read_to_string(&manifest).expect("the manifest can be read");
    std::fs::write(&manifest, format!("{text}\n")).expect("the manifest can be rewritten");

    let failures = validation_failures(&fixture.provider, &asset);
    assert!(
        failures.iter().any(|failure| failure.contains("identity")),
        "§11.4: a manifest that is not the one that was written is not the planned source"
    );
}

#[test]
fn should_call_an_asset_invalid_when_the_destination_is_no_longer_the_same_domain() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    std::fs::remove_dir_all(fixture.path("etc")).expect("the directory can be removed");
    std::fs::write(fixture.path("etc"), "not a directory any more")
        .expect("a file can take its place");

    let failures = validation_failures(&fixture.provider, &asset);
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("scope does not match")),
        "§11.4: the restore path must still resolve to the domain the copy came from"
    );
}

#[test]
fn should_report_the_restore_path_as_available_exactly_when_it_can_be_written() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let directory = fixture.path("etc");
    let narrowed = std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500));
    narrowed.expect("the mode can be narrowed");

    let failures = validation_failures(&fixture.provider, &asset);
    let can_write = std::fs::write(directory.join("probe"), b"x").is_ok();
    assert_eq!(
        failures
            .iter()
            .any(|failure| failure.contains("no restore path is available")),
        !can_write,
        "§11.4: the restore-availability check answers what this process can actually do"
    );
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
        .expect("the mode can be restored so the scratch directory can be removed");
}

#[test]
fn should_report_the_permissions_as_missing_when_the_objects_belong_to_another_user() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let manifest = std::path::Path::new(asset.reference()).join("manifest");
    let text = std::fs::read_to_string(&manifest).expect("the manifest can be read");
    let doctored: Vec<String> = text
        .lines()
        .map(|line| {
            let mut fields: Vec<&str> = line.split('\u{1f}').collect();
            if fields.first() == Some(&"entry") {
                fields[4] = "12345";
            }
            fields.join("\u{1f}")
        })
        .collect();
    std::fs::write(&manifest, format!("{}\n", doctored.join("\n")))
        .expect("the manifest can be rewritten");
    let fingerprint = fixture
        .provider
        .store()
        .manifest_fingerprint(std::path::Path::new(asset.reference()))
        .expect("the fingerprint can be read");
    let asset = asset.capturing(fingerprint);

    let failures = validation_failures(&fixture.provider, &asset);
    assert_eq!(
        failures
            .iter()
            .any(|failure| failure.contains("permissions recovery needs are not held")),
        !can_change_owner(fixture.root()),
        "§11.4 and §43.4: the permission check answers whether this process holds what the \
         restore needs, and an unprivileged process cannot give a file to another user"
    );
}

#[test]
fn should_refuse_to_validate_an_asset_another_provider_owns() {
    let fixture = Fixture::new();
    let foreign = RecoveryAsset::proposed(
        "ono.recovery.zfs",
        RecoveryAssetType::ZfsSnapshot,
        "tank/data@ono-a82f",
        RecoveryScope::new("zfs-dataset", "tank/data", "localhost"),
        fixture.provider.now(),
    );
    let error = fixture
        .provider
        .validate(&foreign)
        .expect_err("§12.1: only the owning provider answers for an asset");
    assert_eq!(error.code().name(), "recovery.provider_unavailable");
}

#[test]
fn should_report_content_and_mode_as_restorable_whatever_the_host_is() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let coverage = fixture
        .provider
        .metadata_coverage(&configuration)
        .expect("the coverage can be measured");
    assert!(
        coverage.content,
        "Appendix C.7: the archive holds the bytes"
    );
    assert!(
        coverage.mode,
        "Appendix C.7: the manifest holds the permission bits"
    );
    assert!(
        !coverage.hardlinks,
        "Appendix C.7: an archive writes one file per name, so hard links are not restored"
    );
}

#[test]
fn should_report_owner_coverage_only_when_the_process_can_restore_ownership() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let coverage = fixture
        .provider
        .metadata_coverage(&configuration)
        .expect("the coverage can be measured");
    assert_eq!(
        coverage.owner,
        can_change_owner(fixture.root()),
        "Appendix C.7 and §43.4: a provider that cannot give a file to another user reports \
         `owner: false` rather than claiming coverage it does not have"
    );
}

#[test]
fn should_report_xattr_coverage_matching_what_the_filesystem_actually_supports() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let coverage = fixture
        .provider
        .metadata_coverage(&configuration)
        .expect("the coverage can be measured");
    assert_eq!(
        coverage.xattrs,
        supports_xattrs(fixture.root()),
        "Appendix C.7: extended-attribute coverage is probed on the destination filesystem rather \
         than assumed from the platform"
    );
}

#[test]
fn should_name_every_piece_of_metadata_it_does_not_restore() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let coverage = fixture
        .provider
        .metadata_coverage(&configuration)
        .expect("the coverage can be measured");
    assert!(
        coverage.gaps().contains(&"hard-link relationships"),
        "Appendix C.7: missing metadata support reduces coverage and MUST be visible"
    );
    assert!(
        !coverage.gaps().contains(&"content"),
        "Appendix C.7: what is restored is not listed as a gap"
    );
}
