//! The two language invariants: §25.3's forbidden sentences and Appendix E.8's absent badge.
//!
//! Both are enforced against the crate's own source rather than against one rendering, because a
//! test that only checks the views it happens to call cannot fail for the view somebody adds
//! next month. Every string this crate can emit is a literal in `src/`, so reading `src/` reads
//! every string this crate can emit.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::{
    Charset, collapsed_plan, coverage_matrix, plan_view, protection_block, recovery_asset_block,
    recovery_verification, recovery_view, verification_view,
};
use ono_value::{RecordValue, Value};

mod support;
use support::{
    contains, nginx_results, protected_exclusions, protected_rows, ready_asset, record,
    recovery_results, rollback_recovery, s, sealed_nginx_plan, selective_recovery,
    unanalysed_recovery, unprotected_rows, zfs_asset,
};

/// A plan carrying exactly a protection matrix, its level and its exclusions (§10.3).
fn plan_with(level: &str, rows: Value, exclusions: Value) -> RecordValue {
    record(
        "ono.change-plan",
        &[
            ("id", s("a82f1c0d9e4b7a63")),
            ("state", s("sealed")),
            ("intent", s("replace nginx configuration")),
            ("protection", rows),
            ("protection_level", s(level)),
            ("coverage_exclusions", exclusions),
        ],
    )
}

/// The words §25.3 and §62.2 forbid: each of them claims a scope nothing established.
const FORBIDDEN: [&str; 4] = [
    "rollback successful",
    "fully recovered",
    "fully restored",
    "undone",
];

/// Every source file of the crate, which between them hold every string it can emit.
fn sources() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let entries = std::fs::read_dir(&root).expect("the crate has a source directory");
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path.display().to_string();
            files.push((
                name,
                std::fs::read_to_string(&path).expect("a readable source"),
            ));
        }
    }
    assert!(
        files.len() > 5,
        "the scan found no sources, so it proves nothing"
    );
    files
}

#[test]
fn should_never_carry_a_sentence_claiming_a_scope_it_did_not_verify() {
    for (name, source) in sources() {
        let lowered = source.to_lowercase();
        for phrase in FORBIDDEN {
            assert!(
                !lowered.contains(phrase),
                "§25.3 and §62.2: `{phrase}` claims a scope nothing established, and {name} contains it"
            );
        }
    }
}

#[test]
fn should_never_emit_a_forbidden_sentence_from_any_view() {
    let plan = sealed_nginx_plan();
    let mut emitted: Vec<String> = Vec::new();
    emitted.extend(plan_view(&plan, &[zfs_asset()], 80, Charset::Ascii));
    emitted.extend(collapsed_plan(&plan, 80, Charset::Ascii));
    emitted.extend(recovery_view(&selective_recovery(), 80, Charset::Ascii));
    emitted.extend(recovery_view(&rollback_recovery(), 80, Charset::Ascii));
    emitted.extend(recovery_view(&unanalysed_recovery(), 80, Charset::Ascii));
    emitted.extend(recovery_verification(&recovery_results(), 80));
    emitted.extend(verification_view(&plan, &nginx_results(), 80));
    emitted.extend(protection_block(
        &plan_with("protected", protected_rows(), protected_exclusions()),
        &[ready_asset()],
        80,
        Charset::Ascii,
    ));
    emitted.extend(coverage_matrix(
        &plan_with("unprotected", unprotected_rows(), Value::list([])),
        80,
        Charset::Ascii,
    ));
    emitted.extend(recovery_asset_block(&zfs_asset(), 80, Charset::Ascii));
    let rendered = emitted.join("\n").to_lowercase();
    for phrase in FORBIDDEN {
        assert!(
            !rendered.contains(phrase),
            "§25.3: user-visible language MUST describe the verified scope, and `{phrase}` does not"
        );
    }
}

#[test]
fn should_close_a_recovery_verification_with_the_scope_it_did_not_claim() {
    let lines = recovery_verification(&recovery_results(), 80);
    assert!(
        contains(&lines, "FULL WORLD EQUIVALENCE NOT CLAIMED"),
        "§25.2's result block is what replaces the sentence §25.3 forbids"
    );
}

#[test]
fn should_never_state_a_protection_level_without_the_exclusions_that_bound_it() {
    let protected = plan_with("protected", protected_rows(), protected_exclusions());
    let bare = plan_with("protected", protected_rows(), Value::list([]));
    let unprotected = plan_with("unprotected", unprotected_rows(), Value::list([]));
    let renderings = [
        protection_block(&protected, &[zfs_asset()], 80, Charset::Ascii),
        protection_block(&bare, &[], 80, Charset::Ascii),
        protection_block(&unprotected, &[], 80, Charset::Ascii),
        coverage_matrix(&protected, 80, Charset::Ascii),
        coverage_matrix(&bare, 80, Charset::Ascii),
        plan_view(&sealed_nginx_plan(), &[zfs_asset()], 80, Charset::Ascii),
        collapsed_plan(&sealed_nginx_plan(), 80, Charset::Ascii),
    ];
    for lines in renderings {
        let states_a_level = lines.iter().any(|line| {
            [
                "UNPROTECTED",
                "COMPENSATABLE",
                "PARTIALLY_PROTECTED",
                "PROTECTED",
                "TRANSACTIONAL",
            ]
            .iter()
            .any(|word| line.contains(word))
        });
        assert!(
            states_a_level,
            "the rendering under test is supposed to state a level"
        );
        assert!(
            contains(&lines, "not covered"),
            "Appendix E.8: coverage summaries must show exclusions, so no rendering path emits a level without them"
        );
    }
}

#[test]
fn should_have_no_public_function_that_renders_a_protection_level_alone() {
    // The guarantee is structural: the level line is a private function of `protection.rs`, and
    // every caller of it in this crate is a function that also emits the exclusions. If a level
    // could be rendered on its own, this scan would find a second, exclusion-free call site.
    let source = sources()
        .into_iter()
        .find(|(name, _)| name.ends_with("protection.rs"))
        .map(|(_, source)| source)
        .expect("the protection module exists");
    assert!(
        source.contains("fn level_line("),
        "the level line has a name, so its visibility can be reasoned about"
    );
    assert!(
        !source.contains("pub fn level_line(") && !source.contains("pub(crate) fn level_line("),
        "Appendix E.8: a caller that wants to print PROTECTED and stop must have nothing to call"
    );
    assert_eq!(
        source.matches("level_line(plan, charset").count(),
        3,
        "the three callers are the block, the matrix and the compact summary, and each emits exclusions"
    );
}

#[test]
fn should_keep_the_uncertainty_words_the_specification_uses_rather_than_softening_them() {
    let lines = recovery_view(&unanalysed_recovery(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "did not run"),
        "§62.8: an analysis that did not run is described as one, not as an absence of findings"
    );
}
