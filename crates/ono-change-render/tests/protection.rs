//! Protection as coverage: §10.3's matrix, §13.8's asset, and Appendix E.8's prohibition.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::ProtectionSummary;
use ono_change_render::{
    Charset, collapsed_plan, coverage_matrix, plan_view, protection_block, recovery_asset_block,
};

mod support;
use support::{
    contains, protected_summary, ready_asset, sealed_nginx_plan, summary_without_exclusions,
    unprotected_summary, zfs_asset,
};

#[test]
fn should_show_one_row_per_domain_with_its_objective_and_its_level() {
    let lines = coverage_matrix(&protected_summary(), 100, Charset::Ascii);
    assert!(
        contains(&lines, "filesystem-persistent") && contains(&lines, "preserve-exact"),
        "§10.3: the matrix is per effect domain, and the objective is half of what a row says"
    );
    assert!(
        contains(&lines, "process-runtime") && contains(&lines, "restore-semantic"),
        "§10.3: a domain that is only compensatable is still a row of the matrix"
    );
}

#[test]
fn should_never_render_the_level_without_the_exclusions_beside_it() {
    let summary = protected_summary();
    for lines in [
        protection_block(&summary, &[zfs_asset()], 100, Charset::Ascii),
        coverage_matrix(&summary, 100, Charset::Ascii),
        plan_view(&sealed_nginx_plan(), &[zfs_asset()], 100, Charset::Ascii),
        collapsed_plan(&sealed_nginx_plan(), 100, Charset::Ascii),
    ] {
        assert!(
            contains(&lines, "PROTECTED"),
            "the rendering under test is supposed to state the level"
        );
        assert!(
            contains(&lines, "not covered"),
            "Appendix E.8: coverage summaries MUST show exclusions, on every path that states a level"
        );
    }
}

#[test]
fn should_render_the_runtime_exclusions_of_a_protected_plan() {
    let lines = protection_block(&protected_summary(), &[], 100, Charset::Ascii);
    for excluded in [
        "process memory",
        "active TCP sessions",
        "requests already served externally",
    ] {
        assert!(
            contains(&lines, excluded),
            "Appendix A.6 and §62.6: a PROTECTED plan still excludes {excluded}, and hiding it turns protection into permission to be reckless"
        );
    }
}

#[test]
fn should_say_in_words_when_no_exclusion_was_recorded() {
    let lines = protection_block(&summary_without_exclusions(), &[], 100, Charset::Ascii);
    assert!(
        contains(&lines, "no exclusion was recorded"),
        "§10.5: a blank where the residual risk goes reads as an assurance, and it is not one"
    );
    assert!(
        contains(&lines, "not covered"),
        "Appendix E.8: the exclusion block exists even when it is empty"
    );
}

#[test]
fn should_mark_an_irreversible_exclusion_with_the_risk_symbol() {
    let lines = protection_block(&protected_summary(), &[], 100, Charset::Ascii);
    let line = lines
        .iter()
        .find(|line| line.contains("active TCP sessions"))
        .expect("the exclusion is rendered");
    assert!(
        line.trim_start().starts_with('!'),
        "§2.13 and §20.3: an irreversible exclusion is not the same as an uncovered one"
    );
}

#[test]
fn should_mark_an_unmet_required_domain_as_risk_rather_than_leaving_it_to_be_noticed() {
    let lines = coverage_matrix(&unprotected_summary(), 100, Charset::Ascii);
    let row = lines
        .iter()
        .find(|line| line.contains("filesystem-persistent"))
        .expect("the row is rendered");
    assert!(
        row.contains('!'),
        "§2.4: a domain that needs recovery and has none is marked, not merely spelled differently"
    );
}

#[test]
fn should_spell_the_plan_level_word_the_way_section_ten_two_spells_it() {
    let lines = coverage_matrix(&protected_summary(), 100, Charset::Ascii);
    assert!(
        contains(&lines, "PROTECTED <->"),
        "§10.2 fixes the canonical words and §20.3 fixes the mark that goes with them"
    );
    let partial = coverage_matrix(&unprotected_summary(), 100, Charset::Ascii);
    assert!(
        contains(&partial, "UNPROTECTED"),
        "§10.2's word for a plan nothing covers is UNPROTECTED"
    );
}

#[test]
fn should_say_a_matrix_with_no_rows_was_never_analysed() {
    let lines = coverage_matrix(&ProtectionSummary::empty(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "no domain was analysed"),
        "§10.5 and §2.4: an unanalysed plan is not an unprotected one, and neither is it a safe one"
    );
}

#[test]
fn should_render_the_asset_facts_section_thirteen_eight_prints() {
    let lines = recovery_asset_block(&zfs_asset(), 80, Charset::Ascii);
    for fact in [
        "zfs-snapshot",
        "rpool/ROOT/debian",
        "rpool/ROOT/debian@ono-a82f",
        "filesystem-consistent",
        "selective-file-restore",
        "24h after verification",
    ] {
        assert!(
            contains(&lines, fact),
            "§13.8's rendering names {fact} and an operator reads the asset from it"
        );
    }
}

#[test]
fn should_say_that_a_copy_on_write_snapshot_shares_its_failure_domain() {
    let lines = recovery_asset_block(&zfs_asset(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "shares the failure domain"),
        "§11.5 and §14.7: a local snapshot is a recovery point and not a backup, and the view says so"
    );
}

#[test]
fn should_say_that_an_unvalidated_asset_has_not_been_confirmed() {
    let proposed = recovery_asset_block(&zfs_asset(), 80, Charset::Ascii);
    assert!(
        contains(&proposed, "no validation has confirmed it"),
        "§11.4: creating an asset is not enough, and a plan view may not imply it was"
    );
    let ready = recovery_asset_block(&ready_asset(), 80, Charset::Ascii);
    assert!(
        !contains(&ready, "no validation has confirmed it"),
        "§11.4: a validated asset carries no such caveat"
    );
}

#[test]
fn should_list_what_one_asset_does_not_protect() {
    let lines = recovery_asset_block(&zfs_asset(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "excluded") && contains(&lines, "/home"),
        "§13.8: a separate dataset is outside the snapshot, and §62.1's snapshot theatre is exactly not saying so"
    );
}

#[test]
fn should_keep_the_asset_in_the_state_it_is_actually_in() {
    let proposed = recovery_asset_block(&zfs_asset(), 80, Charset::Ascii);
    assert!(
        contains(&proposed, "proposed"),
        "§2.1: a plan that mentions an asset has not created one"
    );
}

#[test]
fn should_lay_the_protection_block_out_at_the_width_it_was_given() {
    for width in [40usize, 80, 120] {
        let lines = protection_block(&protected_summary(), &[zfs_asset()], width, Charset::Ascii);
        for line in &lines {
            assert!(
                line.chars().count() <= width,
                "v0.4 §39.3: the matrix stays inside the terminal it was drawn for"
            );
        }
    }
}

#[test]
fn should_draw_the_unicode_marks_when_the_session_chose_unicode() {
    let lines = protection_block(&protected_summary(), &[], 80, Charset::Unicode);
    assert!(
        contains(&lines, "\u{2194}"),
        "§20.3 permits a better glyph when terminal support is known"
    );
    assert!(
        contains(&lines, "not covered"),
        "Appendix E.8 holds in both alphabets"
    );
}
