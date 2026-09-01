//! The gate's anti-fake-completion rules. These decide whether "green" means anything, so they
//! are tested against fixtures rather than trusted.

use std::path::Path;

use ono_testkit::{Scratch, scratch};
use xtask::scan::{
    check_acceptance_case_references, check_release_board, check_silent_skips,
    check_unfinished_work,
};

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

// --- acceptance-case references (ADR-0401) ------------------------------------------------------

#[test]
fn should_accept_a_document_that_names_an_acceptance_case_that_exists() {
    let repo = fixture(&[
        ("docker/acceptance/cases/040-object-pipeline.case", "run\n"),
        (
            "docs/ACCEPTANCE.md",
            "- [x] objects cross the boundary — `040-object-pipeline`\n",
        ),
    ]);
    assert_eq!(check_acceptance_case_references(repo.path()), Vec::new());
}

#[test]
fn should_reject_a_document_that_names_an_acceptance_case_that_does_not_exist() {
    let repo = fixture(&[
        ("docker/acceptance/cases/040-object-pipeline.case", "run\n"),
        (
            "docs/ACCEPTANCE.md",
            "- [x] objects cross the boundary — `035-interop-boundary`\n",
        ),
    ]);
    let problems = check_acceptance_case_references(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert_eq!(problems[0].location, "docs/ACCEPTANCE.md:1");
    assert!(
        problems[0].detail.contains("035-interop-boundary"),
        "the problem names the dangling reference: {}",
        problems[0].detail
    );
}

#[test]
fn should_name_the_case_that_carries_the_number_when_a_reference_was_renamed_away() {
    let repo = fixture(&[
        (
            "docker/acceptance/cases/122-mount-propagation-peers.case",
            "run\n",
        ),
        (
            "docs/decisions/ADR-0236-peers.md",
            "Encoded by case `122-privileged-network-and-mount`.\n",
        ),
    ]);
    let problems = check_acceptance_case_references(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("122-mount-propagation-peers"),
        "the reader is pointed at the case that carries the number: {}",
        problems[0].detail
    );
}

#[test]
fn should_ignore_a_case_name_that_is_not_written_as_a_reference() {
    let repo = fixture(&[
        ("docker/acceptance/cases/000-binary-runs.case", "run\n"),
        (
            "docs/notes.md",
            "The 035-interop-boundary idea was dropped, and a 200-column terminal is wide.\n",
        ),
    ]);
    assert_eq!(check_acceptance_case_references(repo.path()), Vec::new());
}

#[test]
fn should_ignore_a_case_name_inside_a_fenced_code_block() {
    let repo = fixture(&[
        ("docker/acceptance/cases/000-binary-runs.case", "run\n"),
        (
            "docs/notes.md",
            "```text\ncases/`035-interop-boundary`.case\n```\n",
        ),
    ]);
    assert_eq!(check_acceptance_case_references(repo.path()), Vec::new());
}

#[test]
fn should_ignore_the_board_and_the_narrative_specifications_when_scanning_case_references() {
    // The board records names that never existed on purpose, and the specifications are
    // immutable (AGENTS.md §5.1), so a name in either is not a claim this check could close.
    // Every name here is inside the range the suite uses, or the check would skip it as prose
    // and the exemptions would go unexercised.
    let repo = fixture(&[
        ("docker/acceptance/cases/000-binary-runs.case", "run\n"),
        ("docker/acceptance/cases/040-object-pipeline.case", "run\n"),
        (
            "docs/STATE.md",
            "The seven names these boxes carried — `040-process-provider` — never existed.\n",
        ),
        (
            "docs/ono_sendai_shell_spec_v0.2.md",
            "See `001-imaginary-case`.\n",
        ),
    ]);
    assert_eq!(check_acceptance_case_references(repo.path()), Vec::new());
}

#[test]
fn should_ignore_a_narrative_specification_whose_name_omits_the_shell_infix() {
    // The v0.5 Temporal & Causal Systems Interface arrived as `ono_sendai_spec_v0.5_...`, without
    // the `shell_spec` infix the earlier three carry. A specification is immutable however it is
    // named, so a name it records is no more fixable than one in the base (ADR-0423).
    let repo = fixture(&[
        ("docker/acceptance/cases/000-binary-runs.case", "run\n"),
        ("docker/acceptance/cases/040-object-pipeline.case", "run\n"),
        (
            "docs/ono_sendai_spec_v0.5_temporal_causal_systems_interface.md",
            "See `001-imaginary-case`.\n",
        ),
    ]);
    assert_eq!(check_acceptance_case_references(repo.path()), Vec::new());
}

#[test]
fn should_report_this_repository_as_naming_only_acceptance_cases_that_exist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let problems = check_acceptance_case_references(root);
    assert!(
        problems.is_empty(),
        "documents name acceptance cases that do not exist:\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn should_ignore_a_three_digit_measurement_that_is_shaped_like_a_case_name() {
    // `200-column` is a terminal width, `512-byte` a frame size. Nothing about the way they are
    // written distinguishes them from a case; the number does, because the suite has none there.
    let repo = fixture(&[
        ("docker/acceptance/cases/040-object-pipeline.case", "run\n"),
        (
            "docs/notes.md",
            "A `200-column` terminal, a `512-byte` frame, a `300-process` host.\n",
        ),
    ]);
    assert_eq!(check_acceptance_case_references(repo.path()), Vec::new());
}

// --- the release board (ADR-0402) ---------------------------------------------------------------

#[test]
fn should_accept_a_board_whose_in_progress_is_empty_and_whose_deferred_entries_name_an_adr() {
    let board = "\
# STATE

## In progress

## Next up (ordered)

- [ ] C-6 — the model broker

## Deferred / blocked

- **`socket.accepts_connection` cannot be observed.** No kernel interface supplies the link
  (ADR-0135), so the relation is declared and honestly empty.
";
    assert_eq!(check_release_board(board), Vec::new());
}

#[test]
fn should_refuse_to_call_the_shell_ready_while_an_agent_holds_a_claim() {
    let board = "\
# STATE

## In progress

- [agent-7 | 2026-08-29] the model broker — files: crates/ono-model-broker

## Deferred / blocked
";
    let problems = check_release_board(board);
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].location.starts_with("docs/STATE.md"));
    assert!(
        problems[0].detail.contains("In progress"),
        "the reason names the section: {}",
        problems[0].detail
    );
}

#[test]
fn should_refuse_to_call_the_shell_ready_while_a_claim_is_written_as_a_table_row() {
    let board = "\
# STATE

## In progress

| Agent | Worktree | Claim |
|---|---|---|
| KUANG/11 | `../wt-k11` | the wasm tier |

## Deferred / blocked
";
    let problems = check_release_board(board);
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("In progress"));
}

#[test]
fn should_refuse_a_deferred_entry_that_explains_itself_with_no_adr() {
    let board = "\
# STATE

## In progress

## Deferred / blocked

- **the thing is blocked.** Nobody wrote down why it does not block the release.
";
    let problems = check_release_board(board);
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("ADR"),
        "the reason asks for the ADR: {}",
        problems[0].detail
    );
}

#[test]
fn should_ignore_an_unticked_box_under_next_up_when_judging_the_board() {
    // *Next up* is the deliberate post-release backlog (docs/ACCEPTANCE.md §4.5); a shell with an
    // empty backlog is not what the stopping rule asks for.
    let board = "\
# STATE

## In progress

## Next up (ordered)

- [ ] C-2 — the fuzz targets
- [ ] C-6 — the model broker

## Deferred / blocked
";
    assert_eq!(check_release_board(board), Vec::new());
}

#[test]
fn should_refuse_a_board_that_has_no_in_progress_section_at_all() {
    let problems = check_release_board("# STATE\n\n## Next up\n");
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("In progress"));
}

#[test]
fn should_reject_a_test_that_announces_a_skip_with_its_own_print() {
    let repo = fixture(&[(
        "crates/a/tests/thing.rs",
        "#[test]\nfn should_do_it() {\n    eprintln!(\"skipped: no mount here\");\n}\n",
    )]);
    let problems = check_silent_skips(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0]
            .location
            .starts_with("crates/a/tests/thing.rs:3")
    );
    assert!(
        problems[0].detail.contains("ono_testkit::skipped"),
        "the complaint names the helper to use, got {:?}",
        problems[0].detail
    );
}

#[test]
fn should_accept_a_test_that_announces_a_skip_through_the_helper() {
    let repo = fixture(&[(
        "crates/a/tests/thing.rs",
        "#[test]\nfn should_do_it() {\n    ono_testkit::skipped(\"no mount here\");\n}\n",
    )]);
    assert_eq!(check_silent_skips(repo.path()), Vec::new());
}

#[test]
fn should_leave_a_print_that_is_not_a_skip_notice_alone_when_scanning() {
    // The rule is about the announcement, not about printing: a test that reports what it saw is
    // doing its job.
    let repo = fixture(&[(
        "crates/a/tests/thing.rs",
        "#[test]\nfn should_do_it() {\n    eprintln!(\"the host answered {answer:?}\");\n}\n",
    )]);
    assert_eq!(check_silent_skips(repo.path()), Vec::new());
}

#[test]
fn should_leave_a_skip_notice_in_crate_sources_alone_when_scanning() {
    // `ono-testkit` prints the marker itself, and a library is not a test.
    let repo = fixture(&[(
        "crates/a/src/lib.rs",
        "pub fn skipped(why: &str) {\n    eprintln!(\"skipped: {why}\");\n}\n",
    )]);
    assert_eq!(check_silent_skips(repo.path()), Vec::new());
}

#[test]
fn should_report_this_repository_as_announcing_every_skip_through_the_helper() {
    let problems = check_silent_skips(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("root"),
    );
    assert_eq!(problems, Vec::new(), "got {problems:?}");
}
