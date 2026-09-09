//! The recovery cost model (v0.6 §38).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_change_core::{RecoveryAssetType, RecoveryCost};
use ono_change_protection::cost::{compose, render};
use ono_value::ByteSize;

mod support;

use support::{archive_cost, snapshot_cost};

#[test]
fn should_add_up_the_space_two_recovery_assets_occupy() {
    let composed = compose(&[snapshot_cost(), archive_cost(2048)]);
    assert_eq!(
        composed.initial_bytes(),
        Some(ByteSize::from_bytes(4096 + 2048)),
        "§38.1: retained storage is a dimension of the cost, and two assets occupy two lots of it"
    );
    assert_eq!(
        composed.creation_latency(),
        Some(Duration::from_millis(160)),
        "§18.1: assets are created one before the other, and the window is the sum"
    );
}

#[test]
fn should_take_the_longest_quiesce_rather_than_the_sum() {
    let short = snapshot_cost().with_quiesce(Duration::from_millis(200));
    let long = snapshot_cost().with_quiesce(Duration::from_secs(3));
    assert_eq!(
        compose(&[short, long]).quiesce(),
        Some(Duration::from_secs(3)),
        "§18.4: two applications are quiesced at once, and the window is the longest of them"
    );
}

#[test]
fn should_label_a_composition_estimated_when_any_part_of_it_is() {
    let exact = archive_cost(1024);
    assert!(!exact.is_estimated());
    assert!(
        compose(&[exact, snapshot_cost()]).is_estimated(),
        "§37.5: cost numbers MUST be labelled estimated where filesystem accounting is not exact"
    );
}

#[test]
fn should_lose_the_total_when_one_part_was_never_measured() {
    let unmeasured = RecoveryCost::unknown();
    let composed = compose(&[archive_cost(1024), unmeasured]);
    assert_eq!(
        composed.initial_bytes(),
        None,
        "§2.4: a total that omits an unmeasured part is not the total, it is a smaller number"
    );
    assert!(composed.is_estimated());
}

#[test]
fn should_carry_a_reboot_or_offline_requirement_into_the_composition() {
    let composed = compose(&[
        snapshot_cost(),
        snapshot_cost().needing_reboot().needing_offline(),
    ]);
    assert!(
        composed.requires_reboot() && composed.requires_offline(),
        "§38.1: the downtime a recovery needs belongs to the cost of the set that contains it"
    );
}

#[test]
fn should_report_an_unmeasured_cost_for_an_empty_set() {
    let composed = compose(&[]);
    assert_eq!(composed.initial_bytes(), None);
    assert!(
        composed.is_estimated(),
        "§37.5: nothing measured is an estimate of nothing, and it says so"
    );
}

#[test]
fn should_render_a_copy_on_write_snapshot_in_the_four_lines_the_spec_permits() {
    let rendering = render(RecoveryAssetType::ZfsSnapshot, &snapshot_cost());
    assert_eq!(rendering.initial_creation(), "near-instant");
    assert!(rendering.initial_space().starts_with("minimal"));
    assert_eq!(rendering.future_growth(), "depends on changed blocks");
    assert!(
        rendering.retention_risk().starts_with("moderate"),
        "§38.2: the four lines Ono MAY display, in the words it may display them in"
    );
}

#[test]
fn should_never_call_a_copy_on_write_snapshot_free() {
    let costs = [
        RecoveryCost::unknown(),
        RecoveryCost::unknown().with_space(Some(ByteSize::ZERO), Some(ByteSize::ZERO), false),
        snapshot_cost(),
        snapshot_cost()
            .with_quiesce(Duration::from_secs(2))
            .needing_reboot(),
    ];
    for asset_type in [
        RecoveryAssetType::ZfsSnapshot,
        RecoveryAssetType::BtrfsSnapshot,
        RecoveryAssetType::LvmSnapshot,
        RecoveryAssetType::TransactionSavepoint,
    ] {
        for cost in &costs {
            let rendering = render(asset_type, cost);
            assert!(
                !rendering.claims_free(),
                "§38.2: Ono MUST NOT display \"free\" for CoW snapshots, and {} rendered {:?}",
                asset_type,
                rendering.lines()
            );
        }
    }
}

#[test]
fn should_never_render_a_zero_measurement_as_no_cost_at_all() {
    let measured_zero =
        RecoveryCost::unknown().with_space(Some(ByteSize::ZERO), Some(ByteSize::ZERO), false);
    let rendering = render(RecoveryAssetType::BtrfsSnapshot, &measured_zero);
    assert_eq!(
        rendering.initial_space(),
        "minimal",
        "§38.2: a snapshot that occupies nothing yet occupies something as soon as blocks change"
    );
}

#[test]
fn should_say_that_a_local_snapshot_shares_a_failure_domain_with_what_it_protects() {
    let rendering = render(RecoveryAssetType::ZfsSnapshot, &snapshot_cost());
    assert!(
        rendering.retention_risk().contains("failure domain"),
        "§11.5: a copy-on-write snapshot is a recovery point and not a backup: {}",
        rendering.retention_risk()
    );
}

#[test]
fn should_render_an_independent_copy_as_fixed_in_size() {
    let rendering = render(RecoveryAssetType::ConfigurationBackup, &archive_cost(4096));
    assert_eq!(
        rendering.future_growth(),
        "none: the copy is fixed at what it captured"
    );
    assert!(rendering.initial_space().contains("4"));
    assert!(!rendering.claims_free());
}

#[test]
fn should_render_a_slow_creation_as_the_time_it_takes() {
    let slow = archive_cost(4096).with_latency(Duration::from_secs(12));
    assert_eq!(
        render(RecoveryAssetType::FileArchive, &slow).initial_creation(),
        "12 s",
        "§38.1: initial latency is a dimension an operator decides with"
    );
}

#[test]
fn should_show_the_four_labels_in_the_order_the_spec_prints_them() {
    let rendering = render(RecoveryAssetType::ZfsSnapshot, &snapshot_cost());
    let labels: Vec<&str> = rendering.lines().iter().map(|(label, _)| *label).collect();
    assert_eq!(
        labels,
        vec![
            "initial creation",
            "initial space",
            "future growth",
            "retention risk"
        ],
        "§38.2 prints them in this order, and an operator reads them in it"
    );
}
