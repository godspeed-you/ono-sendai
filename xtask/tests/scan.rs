//! The gate's anti-fake-completion rules. These decide whether "green" means anything, so they
//! are tested against fixtures rather than trusted.

use std::path::Path;

use ono_testkit::{Scratch, scratch};
use xtask::scan::check_unfinished_work;

/// Builds a throwaway repository shaped like this one.
fn fixture(files: &[(&str, &str)]) -> Scratch {
    let repo = scratch();
    for (path, contents) in files {
        repo.write(path, contents);
    }
    repo
}

#[test]
fn should_accept_a_repository_with_no_unfinished_work_when_scanned() {
    let repo = fixture(&[("crates/a/src/lib.rs", "pub fn f() -> u8 { 1 }\n")]);
    assert_eq!(check_unfinished_work(repo.path(), ""), Vec::new());
}

#[test]
fn should_reject_a_placeholder_that_panics_in_front_of_a_user_however_it_is_tracked() {
    let repo = fixture(&[("crates/a/src/lib.rs", "pub fn f() -> u8 { todo!() }\n")]);
    // Even a board that names the file cannot excuse it: the marker panics at runtime.
    let problems = check_unfinished_work(repo.path(), "crates/a/src/lib.rs is being worked on");
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].location.starts_with("crates/a/src/lib.rs:1"));
    assert!(problems[0].detail.contains("todo!("));
}

#[test]
fn should_reject_unimplemented_in_any_crate_when_scanned() {
    let repo = fixture(&[(
        "crates/a/src/lib.rs",
        "pub fn f() -> u8 {\n    unimplemented!()\n}\n",
    )]);
    let problems = check_unfinished_work(repo.path(), "");
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].location.ends_with(":2"));
}

#[test]
fn should_reject_an_untracked_todo_comment_when_the_board_does_not_name_its_file() {
    let repo = fixture(&[(
        "crates/a/src/lib.rs",
        "// TODO: finish this\npub fn f() {}\n",
    )]);
    let problems = check_unfinished_work(repo.path(), "# STATE\n\nnothing here\n");
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("docs/STATE.md"));
}

#[test]
fn should_accept_a_todo_comment_when_the_board_names_its_file() {
    let repo = fixture(&[(
        "crates/a/src/lib.rs",
        "// TODO: finish this\npub fn f() {}\n",
    )]);
    let board = "## Deferred\n- narrow the type in `crates/a/src/lib.rs` — ADR-0042\n";
    assert_eq!(check_unfinished_work(repo.path(), board), Vec::new());
}

#[test]
fn should_ignore_the_word_todo_inside_ordinary_prose_when_scanning() {
    let repo = fixture(&[(
        "crates/a/src/lib.rs",
        "// The TODOS of other projects are not ours.\npub fn f() {}\n",
    )]);
    assert_eq!(check_unfinished_work(repo.path(), ""), Vec::new());
}

#[test]
fn should_ignore_a_marker_that_is_only_part_of_a_string_literal_when_scanning() {
    let repo = fixture(&[(
        "crates/a/src/lib.rs",
        "pub fn f() -> &'static str { \"TODO is a word\" }\n",
    )]);
    assert_eq!(check_unfinished_work(repo.path(), ""), Vec::new());
}

#[test]
fn should_reject_an_ignored_test_without_a_reason_when_scanned() {
    let repo = fixture(&[(
        "crates/a/tests/t.rs",
        "#[test]\n#[ignore]\nfn should_do_the_thing() {}\n",
    )]);
    let problems = check_unfinished_work(repo.path(), "crates/a/tests/t.rs");
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("REASON:"));
}

#[test]
fn should_reject_an_ignored_test_the_board_does_not_track_when_scanned() {
    let repo = fixture(&[(
        "crates/a/tests/t.rs",
        "// REASON: the provider does not exist yet\n#[test]\n#[ignore]\nfn should_do_it() {}\n",
    )]);
    let problems = check_unfinished_work(repo.path(), "# STATE\n");
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("Deferred"));
}

#[test]
fn should_accept_an_ignored_test_with_a_reason_the_board_tracks_when_scanned() {
    let repo = fixture(&[(
        "crates/a/tests/t.rs",
        "// REASON: the provider does not exist yet\n#[test]\n#[ignore = \"see STATE\"]\nfn should_do_it() {}\n",
    )]);
    let board = "## Deferred\n- `crates/a/tests/t.rs` — waits on C8 — ADR-0042\n";
    assert_eq!(check_unfinished_work(repo.path(), board), Vec::new());
}

#[test]
fn should_report_this_repository_as_free_of_unfinished_work_when_scanned() {
    // The rule is only worth having if the repository it guards obeys it.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let state = std::fs::read_to_string(root.join("docs/STATE.md")).unwrap_or_default();
    let problems = check_unfinished_work(root, &state);
    assert!(
        problems.is_empty(),
        "the repository carries untracked unfinished work:\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// --- what the scan is allowed not to look at (B-harn-2) -----------------------------------------

#[test]
fn should_reject_a_placeholder_in_an_xtask_test_that_is_not_the_scanners_own() {
    // Only `xtask/tests/scan.rs` has to name the markers, because it asserts on them. Excusing
    // the whole directory hides a `todo!()` in any other xtask test from the gate.
    let repo = fixture(&[("xtask/tests/packaging.rs", "fn f() -> u8 { todo!() }\n")]);
    let problems = check_unfinished_work(repo.path(), "");
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0]
            .location
            .starts_with("xtask/tests/packaging.rs:1")
    );
}

#[test]
fn should_accept_the_markers_the_scanners_own_test_has_to_name() {
    // The two files that necessarily quote every marker: the scanner and the test that drives it.
    let repo = fixture(&[
        ("xtask/tests/scan.rs", "// TODO: \"todo!(\" is the marker\n"),
        ("xtask/src/scan.rs", "const M: &str = \"todo!(\";\n"),
    ]);
    assert_eq!(check_unfinished_work(repo.path(), ""), Vec::new());
}

// --- which trees the scan walks (B-harn-3) ------------------------------------------------------

#[test]
fn should_scan_every_rust_tree_the_repository_layout_allows() {
    // AGENTS.md §2 puts cross-crate suites in `tests/` and spec §35.6 puts fuzz targets in
    // `fuzz/`. Neither exists yet, so nothing proves the scan would look there — and a scan that
    // silently stops covering a tree the moment it is created is the same dead guard as a check
    // that is never called.
    for tree in [
        "tests/pipeline.rs",
        "fuzz/fuzz_targets/parser.rs",
        "examples/demo.rs",
    ] {
        let repo = fixture(&[(tree, "fn f() -> u8 { todo!() }\n")]);
        let problems = check_unfinished_work(repo.path(), "");
        assert_eq!(problems.len(), 1, "`{tree}` is unscanned: got {problems:?}");
        assert!(problems[0].location.starts_with(tree), "got {problems:?}");
    }
}

#[test]
fn should_report_a_rust_tree_the_scan_does_not_walk() {
    // The list of trees is fixed, so the moment Rust appears outside it the scan is quietly
    // partial. Saying so is the only thing that keeps the list honest.
    let repo = fixture(&[("benches/parse.rs", "fn f() {}\n")]);
    let problems = check_unfinished_work(repo.path(), "");
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert_eq!(problems[0].location, "benches");
    assert!(
        problems[0].detail.contains("does not walk"),
        "got {problems:?}"
    );
}

#[test]
fn should_not_report_a_directory_that_holds_no_rust() {
    let repo = fixture(&[("dist/ono_0.3.0_amd64.deb", "not rust\n")]);
    assert_eq!(check_unfinished_work(repo.path(), ""), Vec::new());
}
