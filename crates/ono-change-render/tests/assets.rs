//! §37.5's `get recovery` table, and the cost labels §37.5 and v0.2 §35.3 require.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::recovery_assets;
use ono_value::RecordValue;

mod support;
use support::{
    contains, expired_asset, held_asset, later, ready_asset, unmeasured_asset, zfs_asset,
};

fn table(assets: &[RecordValue]) -> Vec<String> {
    recovery_assets(assets, later(840), 100)
}

#[test]
fn should_carry_the_columns_section_thirty_seven_five_names() {
    let lines = table(&[ready_asset()]);
    let header = lines.first().expect("a header row");
    for column in ["ID", "TYPE", "PLAN", "AGE", "COST", "EXPIRES", "STATUS"] {
        assert!(
            header.contains(column),
            "§37.5's table has a `{column}` column, and an operator scans by column"
        );
    }
}

#[test]
fn should_reference_the_asset_by_the_short_identity_an_operator_can_type() {
    let lines = table(&[ready_asset()]);
    assert!(
        contains(&lines, "recovery/b817"),
        "§36.4: a printed reference is one the operator can type back, never a full digest"
    );
}

#[test]
fn should_name_the_plan_the_asset_belongs_to() {
    let lines = table(&[ready_asset()]);
    assert!(
        contains(&lines, "a82f"),
        "§37.5's PLAN column is how an operator gets from an asset back to what created it"
    );
}

#[test]
fn should_say_none_for_an_asset_that_predates_any_plan() {
    let lines = table(&[unmeasured_asset()]);
    assert!(
        contains(&lines, "none"),
        "§11.1: an asset with no source plan has none, and `unknown` would claim otherwise"
    );
}

#[test]
fn should_render_the_age_as_of_the_instant_the_caller_gave() {
    let lines = table(&[ready_asset()]);
    assert!(
        contains(&lines, "14m"),
        "§50: the age is a difference from the `now` the caller states, never from a clock"
    );
}

#[test]
fn should_render_the_remaining_retention_in_two_units() {
    let lines = table(&[ready_asset()]);
    assert!(
        contains(&lines, "23h46m"),
        "§37.5's EXPIRES column is the number an operator plans the next hour around"
    );
}

#[test]
fn should_label_an_estimated_size_as_estimated() {
    let lines = table(&[zfs_asset()]);
    assert!(
        contains(&lines, "estimated"),
        "§37.5: cost numbers MUST be labeled estimated where filesystem accounting is not exact"
    );
}

#[test]
fn should_not_label_an_exact_size_as_estimated() {
    let lines = table(&[ready_asset()]);
    assert!(
        !contains(&lines, "estimated"),
        "§37.5 asks for the label where accounting is inexact, and a label everywhere means nothing"
    );
    assert!(
        contains(&lines, "MiB"),
        "the exact figure is still a size a reader understands (§13.4)"
    );
}

#[test]
fn should_render_an_unmeasured_size_as_unknown_rather_than_as_zero() {
    let lines = table(&[unmeasured_asset()]);
    assert!(
        contains(&lines, "unknown"),
        "v0.2 §35.3: unknown is null and never fabricated, and §38.2 forbids showing a snapshot as free"
    );
    assert!(
        !lines.iter().any(|line| line.contains(" 0 B")),
        "a zero in a size column is a measurement nobody took"
    );
}

#[test]
fn should_render_an_asset_with_no_expiry_as_unknown() {
    let lines = table(&[zfs_asset()]);
    assert!(
        contains(&lines, "unknown"),
        "§37.1: an asset with no fixed expiry has none, and an infinite one would be a claim"
    );
}

#[test]
fn should_say_an_asset_under_a_hold_is_held_rather_than_expiring() {
    let lines = table(&[held_asset()]);
    assert!(
        contains(&lines, "held"),
        "§37.2: a hold is what stops automatic removal, and a countdown beside one would mislead"
    );
}

#[test]
fn should_say_an_asset_past_its_window_has_expired() {
    let lines = table(&[expired_asset()]);
    assert!(
        contains(&lines, "expired"),
        "§37.1: an asset past its retention is no longer a way back, and the table says so"
    );
}

#[test]
fn should_print_the_state_the_asset_is_actually_in() {
    let lines = table(&[ready_asset(), zfs_asset()]);
    assert!(
        contains(&lines, "ready") && contains(&lines, "proposed"),
        "§11.1 and §2.1: a proposed asset does not exist, and the STATUS column is where that shows"
    );
}

#[test]
fn should_render_the_same_bytes_for_the_same_assets_and_instant() {
    let assets = [ready_asset(), unmeasured_asset()];
    assert_eq!(
        recovery_assets(&assets, later(840), 100),
        recovery_assets(&assets, later(840), 100),
        "§50: rendering is deterministic and the instant is a parameter"
    );
}

#[test]
fn should_say_unknown_when_the_asset_was_created_after_the_stated_instant() {
    let future = ready_asset();
    let lines = recovery_assets(&[future], later(-60), 100);
    assert!(
        contains(&lines, "unknown"),
        "v0.2 §35.3: a disagreement between the instants is a fact, and rounding it to zero hides it"
    );
}

#[test]
fn should_render_an_empty_asset_list_without_inventing_a_row() {
    let lines = recovery_assets(&[], later(0), 100);
    assert!(
        lines.len() <= 1,
        "§37.5: a table of no assets has no rows, and a placeholder row would be one"
    );
}

#[test]
fn should_carry_a_cost_that_was_never_measured_as_unknown_even_when_it_is_estimated() {
    let lines = table(&[unmeasured_asset()]);
    assert!(
        contains(&lines, "unknown") && !contains(&lines, "estimated"),
        "§37.5's label belongs to a figure, and there is no figure to label here"
    );
}
