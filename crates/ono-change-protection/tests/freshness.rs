//! Protection freshness (v0.6 §18.3).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{RecoveryValidation, error};
use ono_change_protection::freshness::{Freshness, assess};
use ono_core::ErrorCode;

mod support;

use support::{NOW, ready_asset};

const BEFORE: &str = "sha256:2f6c1b0a";
const AFTER: &str = "sha256:9d41ee73";

#[test]
fn should_call_an_asset_fresh_when_it_captured_the_state_that_is_about_to_change() {
    let asset = ready_asset("rpool/ROOT/debian@ono-a82f").capturing(BEFORE);
    let verdict = assess(&asset, Some(BEFORE), false);
    assert_eq!(
        verdict.freshness(),
        Freshness::Fresh,
        "§18.1: an asset created immediately before mutation is a just-before-change recovery point"
    );
    assert!(verdict.refusal(&asset).is_none());
    assert!(verdict.is_fresh());
}

#[test]
fn should_never_call_an_asset_with_no_captured_state_fresh() {
    let asset = ready_asset("rpool/ROOT/debian@ono-a82f");
    let verdict = assess(&asset, Some(BEFORE), false);
    assert_eq!(
        verdict.freshness(),
        Freshness::StaleAcceptable,
        "§18.3: an asset of unknown vintage is not evidence about the present state"
    );
    assert!(
        !verdict.is_fresh(),
        "§18.3: Ono MUST NOT pretend an old asset is a just-before-change recovery point"
    );
}

#[test]
fn should_offer_stale_protection_for_explicit_acceptance_when_the_state_drifted() {
    let asset = ready_asset("rpool/ROOT/debian@ono-a82f").capturing(BEFORE);
    let verdict = assess(&asset, Some(AFTER), false);
    assert_eq!(
        verdict.freshness(),
        Freshness::StaleAcceptable,
        "§18.3: the operator may accept stale protection explicitly"
    );
    assert!(
        !verdict.freshness().is_usable_without_acceptance(),
        "§18.3: acceptance is the point; it is not usable silently"
    );
    let refusal = verdict
        .refusal(&asset)
        .expect("§18.3 has a refusal for a stale asset");
    assert_eq!(refusal.code(), ErrorCode::RecoveryAssetStale);
}

#[test]
fn should_require_a_new_asset_when_policy_requires_fresh_protection() {
    let asset = ready_asset("rpool/ROOT/debian@ono-a82f").capturing(BEFORE);
    let verdict = assess(&asset, Some(AFTER), true);
    assert_eq!(
        verdict.freshness(),
        Freshness::MustReplace,
        "§18.3: abort when policy requires fresh protection, and create a new asset instead"
    );
    assert!(verdict.detail().contains("drifted"));
}

#[test]
fn should_require_a_new_asset_when_the_existing_one_is_not_usable() {
    let asset = ready_asset("rpool/ROOT/debian@ono-a82f")
        .capturing(BEFORE)
        .validated(RecoveryValidation::none(NOW, "the snapshot vanished"));
    let verdict = assess(&asset, Some(BEFORE), false);
    assert_eq!(
        verdict.freshness(),
        Freshness::MustReplace,
        "§11.4: only a validated, ready asset is a recovery point at all"
    );
    assert!(verdict.detail().contains("invalid"));
}

#[test]
fn should_treat_an_unfingerprintable_target_as_stale_rather_than_fresh() {
    let asset = ready_asset("rpool/ROOT/debian@ono-a82f").capturing(BEFORE);
    let verdict = assess(&asset, None, false);
    assert_eq!(
        verdict.freshness(),
        Freshness::StaleAcceptable,
        "§56.3: what could not be established is not thereby established to be unchanged"
    );
}

#[test]
fn should_refuse_a_proposed_asset_because_nothing_has_been_created_yet() {
    let asset = ono_change_core::RecoveryAsset::proposed(
        "ono.recovery.zfs",
        ono_change_core::RecoveryAssetType::ZfsSnapshot,
        "rpool/ROOT/debian@ono-planned",
        ono_change_core::RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost"),
        NOW,
    )
    .capturing(BEFORE);
    let verdict = assess(&asset, Some(BEFORE), false);
    assert_eq!(
        verdict.freshness(),
        Freshness::MustReplace,
        "§2.1: a proposed asset does not exist, so it protects nothing yet"
    );
}

#[test]
fn should_name_the_asset_in_its_staleness_refusal() {
    let asset = ready_asset("rpool/ROOT/debian@ono-a82f").capturing(BEFORE);
    let verdict = assess(&asset, Some(AFTER), true);
    let refusal = verdict.refusal(&asset).expect("a stale asset is refused");
    let direct = error::asset_stale(asset.id(), verdict.detail());
    assert_eq!(refusal.code(), direct.code());
    assert_eq!(refusal.message(), direct.message());
}

#[test]
fn should_report_each_freshness_with_a_word_a_rendering_can_show() {
    assert_eq!(Freshness::Fresh.as_str(), "fresh");
    assert_eq!(Freshness::StaleAcceptable.as_str(), "stale-acceptable");
    assert_eq!(Freshness::MustReplace.as_str(), "must-replace");
    assert!(Freshness::Fresh.is_usable_without_acceptance());
    assert!(!Freshness::MustReplace.is_usable_without_acceptance());
}
