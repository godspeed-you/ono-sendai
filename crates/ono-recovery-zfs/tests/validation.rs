//! What §11.4 requires to be checked before an asset is called usable.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_change_core::{
    AssetState, RecoveryAsset, RecoveryAssetType, RecoveryProvider, RecoveryScope, ToolOutput,
};
use ono_recovery_zfs::{GUID_FINGERPRINT, ZFS};

use support::{ROOT_DATASET, ROOT_SNAPSHOT, ROOT_SNAPSHOT_GUID, instant, out, provider, runner};

fn asset() -> RecoveryAsset {
    RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        ROOT_SNAPSHOT,
        RecoveryScope::new("zfs-dataset", ROOT_DATASET, "localhost").covering(ROOT_DATASET),
        instant(),
    )
    .capturing(format!("{GUID_FINGERPRINT}{ROOT_SNAPSHOT_GUID}"))
    .creating()
}

fn validated_with(
    listing: ToolOutput,
    placement: ToolOutput,
    asset: &RecoveryAsset,
) -> ono_change_core::RecoveryValidation {
    let tools = runner(vec![(ZFS, listing), (ZFS, placement)]);
    provider(&tools)
        .validate(asset)
        .expect("a check that ran and failed is a validation, not an error")
}

#[test]
fn should_reach_ready_only_when_every_check_of_section_eleven_four_passed() {
    let validation = validated_with(out("list-snapshots"), out("get-mounted"), &asset());
    assert!(
        validation.is_complete(),
        "§11.4: existence, identity, scope, restore availability and privilege, got {:?}",
        validation.failures()
    );
    assert_eq!(asset().validated(validation).state(), AssetState::Ready);
}

#[test]
fn should_report_a_vanished_snapshot_as_not_existing() {
    let validation = validated_with(ToolOutput::ok(""), out("get-mounted"), &asset());
    assert_eq!(
        validation.failures(),
        vec![
            "the asset does not exist",
            "the asset's identity does not match the planned source",
            "the asset's scope does not match the expected target",
        ],
        "Appendix G.2: a vanished snapshot is a truth test, and the refusal names what failed"
    );
    assert_eq!(asset().validated(validation).state(), AssetState::Invalid);
}

#[test]
fn should_report_a_recreated_snapshot_under_the_same_name_as_a_different_snapshot() {
    let recorded = asset().capturing(format!("{GUID_FINGERPRINT}1"));
    let validation = validated_with(out("list-snapshots"), out("get-mounted"), &recorded);
    assert!(
        validation
            .failures()
            .contains(&"the asset's identity does not match the planned source"),
        "§11.4 and §56.1: identity is the GUID, so a name that came back is not the asset"
    );
}

#[test]
fn should_report_a_snapshot_of_another_dataset_as_out_of_scope() {
    let elsewhere = RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        ROOT_SNAPSHOT,
        RecoveryScope::new("zfs-dataset", "tank/home", "localhost").covering("tank/home"),
        instant(),
    );
    let validation = validated_with(out("list-snapshots"), out("get-mounted"), &elsewhere);
    assert!(
        validation
            .failures()
            .contains(&"the asset's scope does not match the expected target"),
        "§11.4: the asset's scope must match the target the plan expected"
    );
}

#[test]
fn should_report_no_restore_path_when_the_dataset_is_not_mounted() {
    // A scripted answer rather than a recording: the recorded pool has every dataset mounted,
    // and Appendix G.2 asks for the layout that prevents a restore to be presented deliberately.
    let unmounted = ToolOutput::ok(
        "rpool/ROOT/debian\tmounted\tno\nrpool/ROOT/debian\tmountpoint\t/altroot/debian\n\
         rpool/ROOT/debian\treadonly\toff\n",
    );
    let validation = validated_with(out("list-snapshots"), unmounted, &asset());
    assert!(
        validation
            .failures()
            .contains(&"no restore path is available"),
        "§11.4: a snapshot whose dataset is not mounted has no `.zfs/snapshot` to read from"
    );
}

#[test]
fn should_report_no_restore_path_when_the_dataset_is_read_only() {
    let read_only = ToolOutput::ok(
        "rpool/ROOT/debian\tmounted\tyes\nrpool/ROOT/debian\tmountpoint\t/altroot/debian\n\
         rpool/ROOT/debian\treadonly\ton\n",
    );
    let validation = validated_with(out("list-snapshots"), read_only, &asset());
    assert!(
        validation
            .failures()
            .contains(&"no restore path is available"),
        "Appendix G.2: a read-only filesystem preventing restore is a truth test"
    );
}

#[test]
fn should_report_the_privilege_as_missing_when_zfs_refuses_the_listing() {
    let validation = validated_with(out("unprivileged-list"), out("get-mounted"), &asset());
    assert!(
        validation
            .failures()
            .contains(&"the permissions recovery needs are not held"),
        "§11.4 and §43.4: privilege is one of the things validation establishes"
    );
    assert!(!validation.is_complete());
}

#[test]
fn should_report_the_privilege_as_missing_when_zfs_refuses_the_properties() {
    let validation = validated_with(out("list-snapshots"), out("unprivileged-list"), &asset());
    assert!(
        validation
            .failures()
            .contains(&"the permissions recovery needs are not held"),
        "§43.4: recovery may need more privilege than the mutation did"
    );
}

#[test]
fn should_say_which_checks_it_made_rather_than_only_that_it_failed() {
    let validation = validated_with(ToolOutput::ok(""), out("get-mounted"), &asset());
    assert!(
        validation.detail().contains("existence no"),
        "§45: a refusal says which check failed, got `{}`",
        validation.detail()
    );
}

#[test]
fn should_treat_an_asset_with_no_recorded_guid_as_matching_whatever_is_there() {
    let unfingerprinted = RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        ROOT_SNAPSHOT,
        RecoveryScope::new("zfs-dataset", ROOT_DATASET, "localhost").covering(ROOT_DATASET),
        instant(),
    );
    let validation = validated_with(out("list-snapshots"), out("get-mounted"), &unfingerprinted);
    assert!(
        validation.is_complete(),
        "§11.4: an asset that recorded no identity is checked against everything else it can be"
    );
}
