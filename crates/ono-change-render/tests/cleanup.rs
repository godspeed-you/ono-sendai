//! `remove recovery --dry-run`'s cleanup preview: §37.3 and §2.15.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::cleanup_preview;

mod support;
use support::{contains, ready_asset};

/// The short reference `get recovery` prints for the fixture asset.
fn short() -> String {
    let asset = ready_asset();
    let id = asset
        .get("id")
        .and_then(|value| value.as_str().ok())
        .expect("the asset has an id")
        .to_owned();
    id.rsplit('/')
        .next()
        .unwrap_or(&id)
        .chars()
        .take(4)
        .collect()
}

#[test]
fn should_name_every_plan_the_removal_would_make_unrecoverable() {
    let plans = vec!["a82f".to_owned(), "91aa".to_owned()];
    let lines = cleanup_preview(&ready_asset(), &plans, 80);
    assert!(
        contains(
            &lines,
            &format!(
                "removing recovery/{} would make 2 plans unrecoverable",
                short()
            )
        ),
        "§37.3: the preview says what removal changes about recovery capability. Got {lines:?}"
    );
    for plan in ["plan/a82f", "plan/91aa"] {
        assert!(
            contains(&lines, plan),
            "§37.3: which plans become unrecoverable, by the reference an operator types. Got \
             {lines:?}"
        );
    }
}

#[test]
fn should_say_no_retained_plan_depends_on_an_asset_nothing_needs() {
    let lines = cleanup_preview(&ready_asset(), &[], 80);
    assert!(
        contains(&lines, "no retained plan depends on it"),
        "§37.3: an empty answer is stated in words (§10.5). Got {lines:?}"
    );
}

#[test]
fn should_close_a_preview_by_saying_nothing_was_removed() {
    let lines = cleanup_preview(&ready_asset(), &["a82f".to_owned()], 80);
    assert_eq!(
        lines.last().map(String::as_str),
        Some("NOTHING REMOVED"),
        "`--dry-run` removes nothing, and the operator is told so rather than left to infer it"
    );
}

#[test]
fn should_lay_the_preview_out_at_the_width_it_was_given() {
    let plans = vec!["a82f".to_owned(), "91aa".to_owned()];
    for line in cleanup_preview(&ready_asset(), &plans, 40) {
        assert!(line.chars().count() <= 40, "v0.4 §39.3. Got {line:?}");
    }
}
