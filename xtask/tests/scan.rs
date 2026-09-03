//! The gate's anti-fake-completion rules. These decide whether "green" means anything, so they
//! are tested against fixtures rather than trusted.

#![allow(
    clippy::expect_used,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

use std::path::Path;

use ono_testkit::{Scratch, scratch};
use xtask::scan::{
    ExpectedSkips, check_acceptance_case_references, check_authentication_flags,
    check_duplicate_helpers, check_expected_skips, check_pty_resize_assertions,
    check_release_board, check_silent_skips, check_unannounced_skips, check_unfinished_work,
    verify_observed_skips,
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
fn should_ignore_a_token_the_decision_records_name_as_not_being_a_case() {
    // ADR-0401 called the range heuristic "one documented hole", and case `200` opened it: both
    // ADR-0401 and ADR-0463 write `200-column` in backticks, as the counter-example they are
    // *about*. Both are accepted records AGENTS.md §8 forbids editing, so the token is named once
    // in the checker instead (ADR-0539).
    let repo = fixture(&[
        (
            "docker/acceptance/cases/200-refusals-name-the-deciding-boundary.case",
            "run\n",
        ),
        (
            "docs/decisions/ADR-0401-a-named-acceptance-case-must-exist.md",
            "A `200-column` terminal and a `512-byte` frame are shaped exactly like case names.\n",
        ),
    ]);
    assert_eq!(check_acceptance_case_references(repo.path()), Vec::new());

    // And a real dangling reference at the same number is still reported, so the exemption is
    // about two named tokens rather than about the number two hundred.
    let renamed = fixture(&[
        (
            "docker/acceptance/cases/200-refusals-name-the-deciding-boundary.case",
            "run\n",
        ),
        (
            "docs/decisions/ADR-0401-a-named-acceptance-case-must-exist.md",
            "Encoded by case `200-refusals-say-which`.\n",
        ),
    ]);
    let problems = check_acceptance_case_references(renamed.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
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

#[test]
fn should_let_an_open_box_name_the_case_the_delivering_increment_must_write() {
    // An unticked box is a commitment, not a claim: docs/ACCEPTANCE.md §4.7 established that a
    // box may name the proof its increment has to create. The case does not exist yet by
    // definition, so resolving the pointer would make writing a checklist ahead of the work
    // impossible — which is the one thing a checklist is for.
    let repo = fixture(&[
        ("docker/acceptance/cases/040-object-pipeline.case", "run\n"),
        (
            "docs/ACCEPTANCE.md",
            "- [ ] **The link authenticates both ends** — case `038-remote-mutual-auth`.\n",
        ),
    ]);
    assert_eq!(check_acceptance_case_references(repo.path()), Vec::new());
}

#[test]
fn should_still_resolve_the_case_a_ticked_box_claims_as_its_proof() {
    // The moment the box is ticked the same sentence stops being a plan and becomes evidence,
    // and evidence that points at nothing is the defect this check exists for.
    let repo = fixture(&[
        ("docker/acceptance/cases/040-object-pipeline.case", "run\n"),
        (
            "docs/ACCEPTANCE.md",
            "- [x] **The link authenticates both ends** — case `038-remote-mutual-auth`.\n",
        ),
    ]);
    let problems = check_acceptance_case_references(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("038-remote-mutual-auth"));
}

#[test]
fn should_read_an_open_box_to_the_end_of_the_lines_that_continue_it() {
    // A box in this repository runs over several indented lines and names its proofs on the
    // last of them. A rule that only looked at the line carrying the bracket would police the
    // continuation and let the first line through, which is exactly backwards.
    let repo = fixture(&[
        ("docker/acceptance/cases/040-object-pipeline.case", "run\n"),
        (
            "docs/ACCEPTANCE.md",
            "- [ ] **The link authenticates both ends.** The listener refuses an anonymous\n                   client before a frame crosses —\n      case `038-remote-mutual-auth`.\n\n             Prose after the box names `039-remote-authorization`.\n",
        ),
    ]);
    let problems = check_acceptance_case_references(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("039-remote-authorization"),
        "the prose reference is still resolved: {}",
        problems[0].detail
    );
}

// --- no flag turns authentication off (v0.4.1 §7.4, ADR-0440) -----------------------------------

#[test]
fn should_report_a_command_line_flag_that_would_switch_client_authentication_off() {
    let repo = fixture(&[(
        "crates/ono-cli/src/invocation.rs",
        "match argument {\n    \"--allow-anonymous\" => agent.anonymous = true,\n}\n",
    )]);
    let problems = check_authentication_flags(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("--allow-anonymous"),
        "the reason names the flag: {}",
        problems[0].detail
    );
}

#[test]
fn should_report_every_spelling_of_the_flag_the_spec_forbids() {
    for flag in [
        "--insecure",
        "--no-client-auth",
        "--noauth",
        "--unauthenticated",
        "--disable-client-authentication",
        "--skip-peer-verify",
        "--no-verify-peer",
    ] {
        let repo = fixture(&[(
            "crates/ono-cli/src/invocation.rs",
            &format!("match argument {{\n    \"{flag}\" => todo!(),\n}}\n"),
        )]);
        assert_eq!(
            check_authentication_flags(repo.path()).len(),
            1,
            "`{flag}` should be refused"
        );
    }
}

#[test]
fn should_accept_the_flags_a_listening_agent_actually_has() {
    // §7.4 leaves one listening form, and every flag it takes says where or which — never
    // whether. `--print-peer-key` carries `auth` in no part of it and must not be caught by a
    // rule aimed at `no-client-auth`.
    let repo = fixture(&[(
        "crates/ono-cli/src/invocation.rs",
        "match argument {\n    \"--agent\" | \"--listen\" | \"--host-key\" \
         | \"--print-peer-key\" | \"--print-host-key\" | \"--config\" => todo!(),\n}\n",
    )]);
    assert_eq!(check_authentication_flags(repo.path()), Vec::new());
}

#[test]
fn should_let_the_test_that_proves_the_absence_name_the_flags_it_refuses() {
    // crates/ono-cli/tests/listening_agent.rs enumerates the forbidden spellings and asserts each
    // is a usage error (ADR-0440). A rule that caught the guard would delete the guard.
    let repo = fixture(&[(
        "crates/ono-cli/tests/listening_agent.rs",
        "for flag in [\"--insecure\", \"--allow-anonymous\"] { assert_usage(flag); }\n",
    )]);
    assert_eq!(check_authentication_flags(repo.path()), Vec::new());
}

#[test]
fn should_find_no_authentication_disabling_flag_in_this_repository() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let problems = check_authentication_flags(root);
    assert!(
        problems.is_empty(),
        "a flag would turn client authentication off:\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
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

// ----------------------------------------------------------------------------------------------
// Confinement syscalls (issue #59, v0.4.1 §16.2, §65.4).
// ----------------------------------------------------------------------------------------------

#[test]
fn should_report_an_unchecked_confinement_syscall_result() {
    // v0.4.1 §65.4, verbatim: "Calling a confinement syscall, discarding its result and executing
    // the plugin anyway is forbidden." §16.2 makes it a rule about *every* such syscall, and a
    // rule about every one of something is a rule a review cannot hold: the next one is added by
    // someone who never read §16.2. This is the check that does not depend on remembering.
    let repo = fixture(&[(
        "crates/ono-kuang-supervisor/src/platform.rs",
        "fn install() {\n    unsafe {\n        libc::setsid();\n    }\n}\n",
    )]);
    let problems = xtask::scan::check_confinement_syscalls(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("setsid"),
        "the problem names the call whose result was dropped, got {problems:?}"
    );
}

#[test]
fn should_report_a_confinement_syscall_result_bound_to_a_discarded_name() {
    // `let _ =` and `let _unused =` are the two spellings of "I read the value and threw it
    // away", which is the same defect with a signature on it.
    let repo = fixture(&[(
        "crates/ono-kuang-supervisor/src/platform.rs",
        "fn install() {\n    let _ = unsafe { libc::prctl(38, 1, 0, 0, 0) };\n}\n",
    )]);
    let problems = xtask::scan::check_confinement_syscalls(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].detail.contains("prctl"), "got {problems:?}");
}

#[test]
fn should_accept_a_confinement_syscall_whose_result_becomes_a_value() {
    let repo = fixture(&[(
        "crates/ono-kuang-supervisor/src/platform.rs",
        "fn install() -> std::io::Result<()> {\n    checked(unsafe { libc::setsid() })\n}\n",
    )]);
    assert_eq!(
        xtask::scan::check_confinement_syscalls(repo.path()),
        Vec::new()
    );
}

#[test]
fn should_find_no_unchecked_confinement_syscall_in_this_repository() {
    // The rule, applied to the tree it exists for.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let problems = xtask::scan::check_confinement_syscalls(root);
    assert!(
        problems.is_empty(),
        "a confinement syscall result is dropped (v0.4.1 §16.2, §65.4):\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// --- the evaluator capture inventory (v0.4.1 §26.1, §65.7) -------------------------------------

/// A fixture whose inventory covers exactly the sites named, with the class each carries.
fn inventory(entries: &[(&str, &str, &str, Option<&str>)]) -> String {
    let mut text = String::from("version: 1\ncaptures:\n");
    for (file, site, class, adr) in entries {
        text.push_str(&format!(
            "  - file: {file}\n    site: {site}\n    class: {class}\n    holds: something\n"
        ));
        if let Some(adr) = adr {
            text.push_str(&format!("    adr: {adr}\n"));
        }
    }
    text
}

#[test]
fn should_report_an_evaluator_capture_the_streaming_inventory_does_not_classify() {
    // v0.4.1 §26.1: "The implementation MUST inventory every `Vec<Value>` or equivalent capture
    // in evaluator execution paths and classify it". §65.7 is why it has to be a rule with teeth
    // — a capture removed from `each` and grown one stage later is the same defect wearing a
    // different function's name.
    let repo = fixture(&[
        (
            "crates/ono-cli/src/eval.rs",
            "fn run_each_block() {\n    let items: Vec<Value> = upstream();\n}\n",
        ),
        (
            "docs/spec/hardening/streaming.yaml",
            &inventory(&[(
                "crates/ono-cli/src/eval.rs",
                "run_for",
                "semantic_materialization",
                None,
            )]),
        ),
    ]);
    let problems = xtask::scan::check_evaluator_captures(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.location.contains("run_each_block")),
        "an unclassified capture is reported against the site that holds it, got {problems:?}"
    );
}

#[test]
fn should_report_an_inventory_entry_whose_capture_is_no_longer_in_the_evaluator() {
    // The reverse direction, and the one that keeps the artifact honest as captures are removed:
    // an entry nothing answers to is a classification of code that is not there.
    let repo = fixture(&[
        ("crates/ono-cli/src/eval.rs", "fn run_for() {}\n"),
        (
            "docs/spec/hardening/streaming.yaml",
            &inventory(&[(
                "crates/ono-cli/src/eval.rs",
                "run_for",
                "semantic_materialization",
                None,
            )]),
        ),
    ]);
    let problems = xtask::scan::check_evaluator_captures(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.detail.contains("no capture")),
        "a stale entry is reported, got {problems:?}"
    );
}

#[test]
fn should_report_an_implementation_convenience_capture_that_no_decision_record_justifies() {
    // v0.4.1 §26.1: "All `implementation convenience` captures on pipeline data MUST be removed
    // or bounded and justified by ADR." An entry of that class without an ADR has done neither.
    let repo = fixture(&[
        (
            "crates/ono-cli/src/eval.rs",
            "fn run_for() {\n    let items: Vec<Value> = subject();\n}\n",
        ),
        (
            "docs/spec/hardening/streaming.yaml",
            &inventory(&[(
                "crates/ono-cli/src/eval.rs",
                "run_for",
                "implementation_convenience",
                None,
            )]),
        ),
    ]);
    let problems = xtask::scan::check_evaluator_captures(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.detail.contains("ADR")),
        "an unjustified convenience capture is reported, got {problems:?}"
    );
}

#[test]
fn should_accept_an_evaluator_capture_the_inventory_classifies() {
    let repo = fixture(&[
        (
            "crates/ono-cli/src/eval.rs",
            "fn run_for() {\n    let items: Vec<Value> = subject();\n}\n",
        ),
        (
            "docs/spec/hardening/streaming.yaml",
            &inventory(&[(
                "crates/ono-cli/src/eval.rs",
                "run_for",
                "semantic_materialization",
                None,
            )]),
        ),
    ]);
    assert_eq!(
        xtask::scan::check_evaluator_captures(repo.path()),
        Vec::new()
    );
}

#[test]
fn should_report_a_capture_whose_class_is_not_one_the_specification_defines() {
    // §26.1 names three classes and no more. A fourth invented in passing is how an inventory
    // stops being a classification.
    let repo = fixture(&[
        (
            "crates/ono-cli/src/eval.rs",
            "fn run_for() {\n    let items: Vec<Value> = subject();\n}\n",
        ),
        (
            "docs/spec/hardening/streaming.yaml",
            &inventory(&[("crates/ono-cli/src/eval.rs", "run_for", "it_is_fine", None)]),
        ),
    ]);
    let problems = xtask::scan::check_evaluator_captures(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.detail.contains("it_is_fine")),
        "an invented class is reported, got {problems:?}"
    );
}

#[test]
fn should_report_this_repository_as_classifying_every_evaluator_capture() {
    // The rule, applied to the tree it exists for.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let problems = xtask::scan::check_evaluator_captures(root);
    assert!(
        problems.is_empty(),
        "an evaluator capture is unclassified (v0.4.1 §26.1, §65.7):\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// --- bounded channels stay mandatory (v0.4.1 §28.1) -------------------------------------------

#[test]
fn should_report_an_unbounded_channel_on_the_pipeline_data_path() {
    let repo = fixture(&[(
        "crates/ono-pipeline/src/stream.rs",
        "fn build() {\n    let (tx, rx) = mpsc::unbounded_channel();\n}\n",
    )]);
    let problems = xtask::scan::check_bounded_channels(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].location.ends_with(":2"), "got {problems:?}");
}

#[test]
fn should_leave_a_bounded_channel_alone_when_scanning_for_unbounded_ones() {
    let repo = fixture(&[(
        "crates/ono-pipeline/src/stream.rs",
        "fn build() {\n    let (tx, rx) = mpsc::channel(64);\n}\n",
    )]);
    assert_eq!(xtask::scan::check_bounded_channels(repo.path()), Vec::new());
}

#[test]
fn should_ignore_prose_about_an_unbounded_channel_when_scanning() {
    let repo = fixture(&[(
        "crates/ono-pipeline/src/stream.rs",
        "// An unbounded_channel( here would be forbidden by v0.4.1 §28.1.\nfn build() {}\n",
    )]);
    assert_eq!(xtask::scan::check_bounded_channels(repo.path()), Vec::new());
}

#[test]
fn should_find_no_unbounded_pipeline_channel_in_this_repository() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let problems = xtask::scan::check_bounded_channels(root);
    assert!(
        problems.is_empty(),
        "an unbounded channel carries pipeline data (v0.4.1 §28.1, §28.2, §65.7):\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// --- v0.4.1 §38.1, §65.10 and Appendix G: a skip is visible or it is not a skip ----------------

#[test]
fn should_reject_a_test_that_announces_a_skip_on_the_line_after_the_macro() {
    // The announcement written across two lines is the same announcement, and reading only the
    // opening line let one of them live in `spatial_map.rs` unnoticed.
    let repo = fixture(&[(
        "crates/a/tests/thing.rs",
        "#[test]\nfn should_do_it() {\n    eprintln!(\n        \"skipped: nothing to cluster here\"\n    );\n}\n",
    )]);
    let problems = check_silent_skips(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0]
            .location
            .starts_with("crates/a/tests/thing.rs:3")
    );
}

#[test]
fn should_reject_a_test_that_returns_before_its_assertion_path_without_a_skip() {
    let repo = fixture(&[(
        "crates/a/tests/thing.rs",
        "#[test]\nfn should_do_it() {\n    if no_mount() {\n        return;\n    }\n    assert!(true);\n}\n",
    )]);
    let problems = check_unannounced_skips(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0]
            .location
            .starts_with("crates/a/tests/thing.rs:4"),
        "got {:?}",
        problems[0].location
    );
    assert!(
        problems[0].detail.contains("ono_testkit::require"),
        "the complaint names the helper of Appendix G, got {:?}",
        problems[0].detail
    );
}

#[test]
fn should_accept_a_test_that_announces_its_skip_before_it_returns() {
    let repo = fixture(&[(
        "crates/a/tests/thing.rs",
        "#[test]\nfn should_do_it() {\n    if no_mount() {\n        skipped(SkipReason::FixtureNotApplicable, \"no second mount\");\n        return;\n    }\n}\n",
    )]);
    assert_eq!(check_unannounced_skips(repo.path()), Vec::new());
}

#[test]
fn should_accept_a_guard_whose_own_helper_announced_the_skip() {
    // `unprivileged()` prints the marker and returns false; `if !unprivileged() { return; }` is
    // the return path Appendix G permits, because the canonical signal was already emitted.
    let repo = fixture(&[(
        "crates/a/tests/thing.rs",
        "fn unprivileged() -> bool {\n    if is_root() {\n        ono_testkit::skipped(SkipReason::MissingPrivilege, \"running as root\");\n        return false;\n    }\n    true\n}\n\n#[test]\nfn should_do_it() {\n    if !unprivileged() {\n        return;\n    }\n    assert!(true);\n}\n",
    )]);
    assert_eq!(check_unannounced_skips(repo.path()), Vec::new());
}

#[test]
fn should_accept_a_branch_that_asserted_before_it_returned() {
    // A branch that asserts has reached an assertion path, which is what §65.10 asks of it.
    let repo = fixture(&[(
        "crates/a/tests/thing.rs",
        "#[test]\nfn should_do_it() {\n    if unavailable() {\n        assert!(reason().len() > 0);\n        return;\n    }\n}\n",
    )]);
    assert_eq!(check_unannounced_skips(repo.path()), Vec::new());
}

#[test]
fn should_leave_a_return_inside_a_closure_alone_when_scanning_for_unannounced_skips() {
    // A `return` inside a closure leaves the closure, not the test: it is flow control in a
    // fixture, and reporting it would teach people to write fixtures differently.
    let repo = fixture(&[(
        "crates/a/tests/thing.rs",
        "#[test]\nfn should_do_it() {\n    spawn(move |sink| async move {\n        if sink.send(1).is_err() {\n            return;\n        }\n    });\n    assert!(true);\n}\n",
    )]);
    assert_eq!(check_unannounced_skips(repo.path()), Vec::new());
}

#[test]
fn should_report_this_repository_as_announcing_every_skip_it_takes() {
    // The whole tree, so a regression is caught where it lands rather than in a fixture.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root is the parent of xtask/");
    assert_eq!(
        check_unannounced_skips(root),
        Vec::new(),
        "every test that gives up on a precondition says so (v0.4.1 §38.1, §65.10)"
    );
}

// --- v0.4.1 §38.2 and §38.3: the declared skip set ---------------------------------------------

/// A registry naming one expected skip, so a fixture reads like the real file.
fn expectation(declared: &str, expected: &str) -> ExpectedSkips {
    ExpectedSkips::parse(&format!(
        "version: 1\ndeclared:\n{declared}canonical_ci:\n  expected_skips:\n{expected}"
    ))
    .expect("the fixture registry parses")
}

#[test]
fn should_fail_on_a_skip_the_expectation_does_not_declare() {
    // §38.3's forward half: a run that skipped something nobody declared is a run whose green
    // summary covers less than it says.
    let expected = expectation(
        "  - id: \"crates/a/tests/thing.rs::should_cross_a_mount\"\n    category: fixture_not_applicable\n",
        "    - \"crates/a/tests/thing.rs::should_cross_a_mount\"\n",
    );
    let problems = verify_observed_skips(
        &expected,
        "running 2 tests\nSKIPPED should_cross_a_mount: fixture_not_applicable: no second mount\nSKIPPED should_read_the_journal: external_tool_unavailable: no journald here\n",
    );
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert_eq!(problems[0].location, "should_read_the_journal");
    assert!(
        problems[0].detail.contains("expected_test_skips.yaml"),
        "the complaint names the registry to declare it in, got {:?}",
        problems[0].detail
    );
}

#[test]
fn should_fail_when_a_declared_skip_no_longer_happens() {
    // §38.3's reverse half, and the one #14 needed: five acceptance cases that had never run were
    // green for as long as nobody counted them.
    let expected = expectation(
        "  - id: \"crates/a/tests/thing.rs::should_cross_a_mount\"\n    category: fixture_not_applicable\n",
        "    - \"crates/a/tests/thing.rs::should_cross_a_mount\"\n",
    );
    let problems = verify_observed_skips(
        &expected,
        "running 1 test\ntest should_cross_a_mount ... ok\n",
    );
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert_eq!(
        problems[0].location,
        "crates/a/tests/thing.rs::should_cross_a_mount"
    );
    assert!(
        problems[0].detail.contains("did not"),
        "got {:?}",
        problems[0].detail
    );
}

#[test]
fn should_accept_a_run_whose_skips_are_exactly_the_declared_ones() {
    let expected = expectation(
        "  - id: \"crates/a/tests/thing.rs::should_cross_a_mount\"\n    category: fixture_not_applicable\n",
        "    - \"crates/a/tests/thing.rs::should_cross_a_mount\"\n",
    );
    assert_eq!(
        verify_observed_skips(
            &expected,
            "SKIPPED should_cross_a_mount: fixture_not_applicable: no second mount\n"
        ),
        Vec::new()
    );
}

#[test]
fn should_report_this_repositorys_observed_skips_as_exactly_the_declared_set() {
    // The registry against the tree, in both directions: a skip the tree can take and the file
    // does not declare, and a row whose test no longer skips, are both failures (§38.2).
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root is the parent of xtask/");
    assert_eq!(
        check_expected_skips(root),
        Vec::new(),
        "docs/spec/hardening/expected_test_skips.yaml declares exactly the skips this tree takes"
    );
}

// --- v0.4.1 §39.1 and §39.2: one helper per job ------------------------------------------------

#[test]
fn should_report_two_test_helpers_that_do_the_same_job_under_different_names() {
    // ADR-0427 found one name over eleven behaviours. This is the same defect from the other end:
    // one behaviour under two names is a helper somebody could not find, so they wrote it again.
    let repo = fixture(&[
        (
            "crates/a/tests/one.rs",
            "fn listening_tcp() -> Vec<u8> {\n    message(20, &socket(2, 10, 0, 0, 0, 4_242, 0x1234_5678))\n        .into_iter()\n        .chain(trailer(\"listening\"))\n        .collect()\n}\n",
        ),
        (
            "crates/a/tests/two.rs",
            "fn unowned_listener() -> Vec<u8> {\n    message(20, &socket(2, 10, 0, 0, 0, 4_242, 0x1234_5678))\n        .into_iter()\n        .chain(trailer(\"listening\"))\n        .collect()\n}\n",
        ),
    ]);
    let problems = check_duplicate_helpers(repo.path());
    assert_eq!(problems.len(), 2, "got {problems:?}");
    assert_eq!(problems[0].location, "crates/a/tests/one.rs::listening_tcp");
    assert_eq!(
        problems[1].location,
        "crates/a/tests/two.rs::unowned_listener"
    );
    assert!(
        problems[0].detail.contains("crates/a/tests/support/mod.rs"),
        "the complaint names the home the copies share, got {:?}",
        problems[0].detail
    );
}

#[test]
fn should_leave_two_helpers_that_differ_alone_when_scanning_for_duplicates() {
    // ADR-0427's rule, and the reason the check is written this way: unifying two helpers that
    // differ picks one of them for callers that were using the other, which changes what a test
    // does. A variant is not a duplicate.
    let repo = fixture(&[
        (
            "crates/a/tests/one.rs",
            "fn ono(script: &str) -> Run {\n    Shell::new()\n        .args([\"-c\", script])\n        .timeout(Duration::from_secs(20))\n        .run()\n}\n",
        ),
        (
            "crates/a/tests/two.rs",
            "fn ono(script: &str) -> Run {\n    Shell::new()\n        .args([\"-c\", script])\n        .timeout(Duration::from_secs(30))\n        .run()\n}\n",
        ),
    ]);
    assert_eq!(check_duplicate_helpers(repo.path()), Vec::new());
}

#[test]
fn should_leave_a_helper_alone_when_it_calls_its_own_files_helper() {
    // `files.rs::single_result` was identical to three others and called `files.rs::text`, which
    // names an ActionResult field in its panic. Two identical bodies over two different callees
    // are two behaviours, and moving one would change the diagnostic a reader depends on
    // (ADR-0427).
    let body = "fn single_result(run: &Run) -> Value {\n    let mut rows = rows(run);\n    assert_eq!(rows.len(), 1, \"one result per target, got {:?}\", run.stdout());\n    rows.remove(0)\n}\n";
    let local = "fn rows(run: &Run) -> Vec<Value> {\n    serde_yaml_ng::from_str(run.stdout()).expect(\"json\")\n}\n";
    let repo = fixture(&[
        ("crates/a/tests/one.rs", &format!("{local}{body}")),
        ("crates/a/tests/two.rs", &format!("{local}{body}")),
    ]);
    let problems = check_duplicate_helpers(repo.path());
    assert!(
        problems
            .iter()
            .all(|problem| !problem.location.ends_with("::single_result")),
        "a helper whose meaning comes from its own file's callee stays put, got {problems:?}"
    );
}

#[test]
fn should_leave_two_crates_helpers_alone_when_they_share_no_home() {
    // §39.2 asks for the check "where a canonical helper exists". A pair spanning two crates has
    // no home that does not put a crate's own types into `ono-testkit`, which ADR-0427 rejected.
    let helper = "fn record(name: &str) -> RecordValue {\n    RecordValue::builder(schema())\n        .set(\"name\", Value::string(name))\n        .build()\n        .expect(\"a well-formed record\")\n}\n";
    let repo = fixture(&[
        ("crates/a/tests/one.rs", helper),
        ("crates/b/tests/two.rs", helper),
    ]);
    assert_eq!(check_duplicate_helpers(repo.path()), Vec::new());
}

#[test]
fn should_report_this_repository_as_using_the_canonical_helper_everywhere() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root is the parent of xtask/");
    assert_eq!(
        check_duplicate_helpers(root),
        Vec::new(),
        "every job a test helper does has one definition (v0.4.1 §39.1, §39.2)"
    );
}

#[test]
fn should_neither_require_nor_forbid_a_skip_the_host_capability_decides() {
    // A descriptor limit is a property of the runner, not of this repository. Requiring the skip
    // would be red on a machine that can supply the descriptors, and forbidding it red on one
    // that cannot — so the registry lists it with the condition that decides it, which is what
    // §38.2 asks of an intentional skip (ADR-0517).
    let expected = ExpectedSkips::parse(
        "version: 1\ndeclared:\n  - id: \"crates/a/tests/thing.rs::should_place_a_hundred_thousand_sockets\"\n    category: missing_privilege\ncanonical_ci:\n  expected_skips: []\n  permitted_skips:\n    - id: \"crates/a/tests/thing.rs::should_place_a_hundred_thousand_sockets\"\n      condition: \"`ulimit -Hn` is at least 101024\"\n",
    )
    .expect("the fixture registry parses");

    assert_eq!(
        verify_observed_skips(
            &expected,
            "SKIPPED should_place_a_hundred_thousand_sockets: missing_privilege: the host allows 65536\n"
        ),
        Vec::new(),
        "a host that cannot supply the capability may skip"
    );
    assert_eq!(
        verify_observed_skips(
            &expected,
            "test should_place_a_hundred_thousand_sockets ... ok\n"
        ),
        Vec::new(),
        "and a host that can must not be told it should have skipped"
    );
}

// --- v0.4.1 §65.10 at a terminal ---------------------------------------------------------------

#[test]
fn should_report_a_pty_assertion_that_an_earlier_repaint_can_satisfy() {
    // The shape issue #6 found: resize, then wait for "new output naming the place". The repaint
    // the earlier arrow key was still producing satisfies it, so the test passed on runs whose
    // whole key history was `Down`, `Esc` — with no resize in them at all.
    let repo = fixture(&[(
        "crates/a/tests/tui.rs",
        "#[test]\nfn should_keep_the_place() {\n    session.keys(DOWN);\n    let mark = session.seen().len();\n    session.resize(WindowSize::new(20, 60));\n    assert!(session.wait_until(BUDGET, |seen| seen.len() > mark\n        && plain(&seen[mark..]).contains(\"compute\")));\n}\n",
    )]);
    let problems = check_pty_resize_assertions(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(problems[0].location.starts_with("crates/a/tests/tui.rs:5"));
    assert!(
        problems[0].detail.contains("20 rows"),
        "the complaint names the size nothing asserted on, got {:?}",
        problems[0].detail
    );
}

#[test]
fn should_accept_a_pty_assertion_that_names_the_frame_at_the_new_row_count() {
    let repo = fixture(&[(
        "crates/a/tests/tui.rs",
        "#[test]\nfn should_keep_the_place() {\n    session.resize(WindowSize::new(20, 60));\n    assert!(session.wait_until(BUDGET, |seen| {\n        let rows = rows_addressed(seen);\n        rows.contains(&20) && rows.iter().all(|row| *row <= 20)\n    }));\n}\n",
    )]);
    assert_eq!(check_pty_resize_assertions(repo.path()), Vec::new());
}

#[test]
fn should_accept_a_resize_asserted_by_the_signal_it_delivers() {
    // A row count is the usual way to name a resize and not the only one: a test that waits for
    // the child's `SIGWINCH` trap to fire has named something only a resize produces.
    let repo = fixture(&[(
        "crates/a/tests/pty.rs",
        "#[test]\nfn should_deliver_the_signal() {\n    session.resize(WindowSize::new(30, 90)).expect(\"resizing succeeds\");\n    let seen = drain(&mut session, DEADLINE);\n    assert!(seen.contains(\"WINCHED\"));\n}\n",
    )]);
    assert_eq!(check_pty_resize_assertions(repo.path()), Vec::new());
}

#[test]
fn should_report_this_repository_as_asserting_on_every_resize_it_makes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root is the parent of xtask/");
    assert_eq!(
        check_pty_resize_assertions(root),
        Vec::new(),
        "a test that resizes a terminal says what the new size produced (v0.4.1 §43.4, §65.10)"
    );
}
