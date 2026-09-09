//! Recovery asset retention and cleanup (v0.6 §37, §2.15).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use jiff::{SignedDuration, Timestamp};
use ono_change_core::{PlanId, PlanState, RecoveryAsset, RetentionPolicy};
use ono_change_protection::CleanupPreview;
use ono_change_protection::policy::ProtectionPolicy;
use ono_change_protection::retention::{
    CleanupVerdict, PlanRetention, cleanup_preview, expires_at, storage_pressure,
};
use ono_core::ErrorCode;
use ono_value::ByteSize;

mod support;

use support::{NOW, ready_asset};

fn plan() -> PlanId {
    PlanId::of("session-1", "2026-09-09T10:00:00Z", "replace nginx.conf")
}

fn later(hours: i64) -> Timestamp {
    NOW.checked_add(SignedDuration::from_hours(hours))
        .expect("the fixture instant is far from the end of time")
}

fn asset_of_plan() -> RecoveryAsset {
    ready_asset("rpool/ROOT/debian@ono-a82f").for_plan(plan())
}

fn verified(state: PlanState) -> PlanRetention {
    PlanRetention::new(plan(), state)
        .verified_at(NOW)
        .resting_on(asset_of_plan().id().clone())
}

fn preview(state: PlanState, advertises: bool, now: Timestamp) -> CleanupPreview {
    cleanup_preview(
        &[asset_of_plan()],
        &[verified(state).advertising_recovery(advertises)],
        now,
    )
}

#[test]
fn should_expire_an_asset_twenty_four_hours_after_a_successful_verification() {
    let expiry = expires_at(&asset_of_plan(), Some(NOW))
        .expect("§37.1: a verified plan's assets have an end to their retention");
    assert_eq!(
        expiry,
        later(24),
        "§37.1: the default temporary recovery retention is 24 hours after successful verification"
    );
}

#[test]
fn should_keep_an_asset_within_its_retention_window() {
    let preview = preview(PlanState::Verified, false, later(1));
    assert_eq!(
        preview.entries()[0].verdict(),
        CleanupVerdict::WithinRetention,
        "§37.1: an hour after verification the asset is still the way back"
    );
    assert!(preview.removable().is_empty());
}

#[test]
fn should_offer_an_asset_for_cleanup_once_its_retention_has_passed() {
    let preview = preview(PlanState::Verified, false, later(25));
    assert_eq!(preview.entries()[0].verdict(), CleanupVerdict::Removable);
    assert_eq!(preview.removable().len(), 1);
    assert!(preview.entries()[0].detail().contains("§37.1"));
}

#[test]
fn should_keep_the_assets_of_a_failed_plan_out_of_ordinary_success_retention() {
    for state in [
        PlanState::Failed,
        PlanState::Degraded,
        PlanState::ApplyFailed,
        PlanState::PrepareFailed,
        PlanState::RecoveryFailed,
    ] {
        let preview = preview(state, false, later(1_000));
        assert_eq!(
            preview.entries()[0].verdict(),
            CleanupVerdict::RetainedAfterFailure,
            "§37.2: the assets of a {state} plan MUST NOT be deleted by ordinary success retention"
        );
    }
}

#[test]
fn should_never_expire_an_asset_under_an_explicit_hold() {
    let held = asset_of_plan().retained_for(RetentionPolicy::held());
    assert!(
        expires_at(&held, Some(NOW)).is_none(),
        "§37.2: an explicit hold is not a longer retention, it is the absence of an automatic end"
    );
    let preview = cleanup_preview(
        &[held],
        &[verified(PlanState::Verified).advertising_recovery(false)],
        later(1_000),
    );
    assert_eq!(preview.entries()[0].verdict(), CleanupVerdict::Held);
}

#[test]
fn should_refuse_a_cleanup_that_would_take_away_recovery_a_plan_still_offers() {
    let asset = asset_of_plan();
    let preview = preview(PlanState::Verified, true, later(1_000));
    assert_eq!(
        preview.entries()[0].verdict(),
        CleanupVerdict::Blocked,
        "§2.15: recovery assets a retained plan needs MUST NOT be deleted silently"
    );
    let refusal = preview
        .refusal_for(asset.id())
        .expect("§2.15 refuses the removal rather than performing it quietly");
    assert_eq!(refusal.code(), ErrorCode::RecoveryCleanupBlocked);
    assert!(
        refusal
            .help()
            .is_some_and(|help| help.contains("--confirm")),
        "§37.2: the refusal says how an operator overrides it deliberately"
    );
}

#[test]
fn should_name_the_plans_a_removal_would_leave_unrecoverable() {
    let preview = preview(PlanState::Verified, true, later(1_000));
    let entry = &preview.entries()[0];
    assert_eq!(
        entry.unrecoverable_plans(),
        &[plan()],
        "§37.3: before deleting an asset, Ono shows which plans become unrecoverable"
    );
}

#[test]
fn should_not_refuse_a_cleanup_that_takes_nothing_away() {
    let asset = asset_of_plan();
    let preview = preview(PlanState::Verified, false, later(25));
    assert!(
        preview.refusal_for(asset.id()).is_none(),
        "§37.1: an expired asset no plan still offers is exactly what cleanup is for"
    );
}

#[test]
fn should_report_an_asset_that_is_already_gone_rather_than_planning_to_remove_it() {
    let removed = asset_of_plan().removed();
    let preview = cleanup_preview(
        &[removed],
        &[verified(PlanState::Verified).advertising_recovery(false)],
        later(1_000),
    );
    assert_eq!(preview.entries()[0].verdict(), CleanupVerdict::AlreadyGone);
    assert!(preview.removable().is_empty());
    assert!(preview.retained().is_empty());
}

#[test]
fn should_hold_an_asset_whose_plan_has_never_been_verified() {
    let preview = cleanup_preview(
        &[asset_of_plan()],
        &[PlanRetention::new(plan(), PlanState::Applying).advertising_recovery(false)],
        later(1_000),
    );
    assert_eq!(
        preview.entries()[0].verdict(),
        CleanupVerdict::WithinRetention,
        "§37.1: retention starts at a successful verification, so an unverified plan has no clock"
    );
    assert!(preview.entries()[0].expires_at().is_none());
}

#[test]
fn should_add_up_the_space_cleanup_would_reclaim() {
    let preview = cleanup_preview(
        &[
            asset_of_plan(),
            ready_asset("rpool/ROOT/debian@ono-91aa").for_plan(plan()),
        ],
        &[verified(PlanState::Verified).advertising_recovery(false)],
        later(25),
    );
    assert_eq!(
        preview.reclaimable(),
        ByteSize::from_bytes(8 * 1024),
        "§37.5: the cost of what would be reclaimed is shown, labelled estimated"
    );
}

#[test]
fn should_surface_a_landmark_and_recommend_cleanup_under_storage_pressure() {
    let policy = ProtectionPolicy::default();
    let preview = preview(PlanState::Verified, false, later(25));
    let pressure = storage_pressure(
        "rpool",
        ByteSize::from_bytes(2 * 1024 * 1024 * 1024),
        ByteSize::from_bytes(100 * 1024 * 1024 * 1024),
        &policy,
        &preview,
    )
    .expect("§37.4: two percent free is below the ten percent floor");

    assert_eq!(pressure.recommended().len(), 1);
    assert!(
        pressure.landmark().contains("could be cleaned up"),
        "§37.4: Ono surfaces landmarks and recommends cleanup: {}",
        pressure.landmark()
    );
    assert!(
        !pressure.may_remove_early(),
        "§37.4: Ono MUST NOT delete assets early without policy authorization"
    );
    assert_eq!(
        pressure.refusal().code(),
        ErrorCode::RecoveryStoragePressure
    );
}

#[test]
fn should_authorise_early_removal_only_when_policy_says_so() {
    let policy = ProtectionPolicy::default().authorising_early_removal();
    let preview = preview(PlanState::Verified, false, later(25));
    let pressure = storage_pressure(
        "rpool",
        ByteSize::from_bytes(1024),
        ByteSize::from_bytes(1024 * 1024),
        &policy,
        &preview,
    )
    .expect("the floor is not cleared");
    assert!(
        pressure.may_remove_early(),
        "§37.4: early deletion needs policy authorization, and this policy carries it"
    );
}

#[test]
fn should_raise_no_landmark_when_the_free_space_floor_is_cleared() {
    let policy = ProtectionPolicy::default();
    let preview = preview(PlanState::Verified, false, later(25));
    assert!(
        storage_pressure(
            "rpool",
            ByteSize::from_bytes(50 * 1024 * 1024 * 1024),
            ByteSize::from_bytes(100 * 1024 * 1024 * 1024),
            &policy,
            &preview,
        )
        .is_none(),
        "§37.4: a landmark reports a condition, and raising one without the condition is noise"
    );
}

#[test]
fn should_block_cleanup_of_an_asset_a_failed_plan_still_advertises() {
    let asset = asset_of_plan();
    let preview = preview(PlanState::Failed, true, later(1_000));
    assert_eq!(
        preview.entries()[0].verdict(),
        CleanupVerdict::Blocked,
        "§2.15 and §37.2 agree: the way back from a failed plan is not removed quietly"
    );
    assert!(preview.refusal_for(asset.id()).is_some());
    assert_eq!(preview.retained().len(), 1);
}
