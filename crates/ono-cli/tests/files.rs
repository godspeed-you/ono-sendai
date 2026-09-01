//! Outcome tests for the filesystem family the contract declares:
//! `read`, `write`, `copy`, `move`, `remove`, `set`, `open`, `tail`, `watch`, `trace` and `enter`
//! over `file`, plus `remove dir` and `set dir`, and glob resolution for native selectors.
//!
//! Contract: `docs/spec/commands/file.yaml`, schemas `ono.file/1` and `ono.action-result/1`.
//! Narrative: spec §9.1 (the filesystem table), §11.5/§11.6 (ActionResult, destructive fan-out),
//! §12.1 (bytes are bytes), §14.3 (object context), §16.5 (no collapsed bulk errors), §17.3 (no
//! ambiguous glob destruction), §17.4 (scripts never wait for a prompt), §18.2 (native live
//! streams begin with a snapshot, ADR-0024), §22.3 (`trace file … --users`), §41.3 (large old
//! files). Every test asserts what the user sees — stdout through `| to json`, the exit status,
//! the structured error code, the state of the scratch directory afterwards — never how a stage
//! is wired (AGENTS.md §11).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;

use ono_testkit::{Scratch, Shell, scratch};
use serde_yaml_ng::Value;

/// Runs a one-liner with the scratch directory as the working directory.
fn ono_in(directory: &Scratch, script: &str) -> ono_testkit::Run {
    Shell::new()
        .cwd(directory.path())
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

/// Parses the JSON document `to json` wrote as the stream's values.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let text = run.stdout().trim().to_owned();
    let stderr = run.stderr();
    let document: Value = serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!("`to json` must emit a JSON document, got {text:?} ({error}); stderr: {stderr:?}")
    });
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!(
                "spec §33.5: `to json` emits the stream as an array, got {text:?}; stderr: {stderr:?}"
            )
        })
        .clone()
}

/// The one `ono.action-result/1` row a single-target mutation emits.
fn single_result(run: &ono_testkit::Run) -> Value {
    let mut rows = rows(run);
    assert_eq!(
        rows.len(),
        1,
        "spec §11.5: one ActionResult per target, got {:?}",
        run.stdout()
    );
    rows.remove(0)
}

fn text(row: &Value, field: &str) -> String {
    row[field]
        .as_str()
        .unwrap_or_else(|| panic!("ActionResult field `{field}` must be a string, got {row:?}"))
        .to_owned()
}

fn permission_bits(path: &Path) -> u32 {
    std::fs::metadata(path).expect("the scratch file").mode() & 0o7777
}

fn assert_success_row(row: &Value, operation: &str) {
    assert_eq!(
        text(row, "operation"),
        operation,
        "spec §11.5: `operation` is the command id, got {row:?}"
    );
    assert_eq!(
        text(row, "status"),
        "success",
        "the mutation reports its outcome as `success`, got {row:?}"
    );
    assert_eq!(
        row["changed"].as_bool(),
        Some(true),
        "spec §11.5: `changed` says the system state moved, got {row:?}"
    );
    assert!(
        row["error"].is_null(),
        "`error` is null for a success (action-result.v1), got {row:?}"
    );
}

fn assert_failed_row(row: &Value, operation: &str, code: &str) {
    assert_eq!(
        text(row, "operation"),
        operation,
        "spec §11.5: `operation` is the command id, got {row:?}"
    );
    assert_eq!(
        text(row, "status"),
        "failed",
        "the mutation reports a failure as a `failed` row, not as text, got {row:?}"
    );
    assert_eq!(
        row["changed"].as_bool(),
        Some(false),
        "a failed mutation changed nothing, got {row:?}"
    );
    assert_eq!(
        row["error"]["code"].as_str(),
        Some(code),
        "spec §43: the failed row carries the structured error {code}, got {row:?}"
    );
}

// --- globs for native selectors (spec §17.3) ----------------------------------------------------

#[test]
fn should_resolve_a_glob_to_exactly_the_matching_files_when_getting_files() {
    let directory = scratch();
    directory.write("a.txt", "a");
    directory.write("b.txt", "b");
    directory.write("c.md", "c");

    // Spec §17.3: native commands receive resolved objects; `*.txt` names two files here, not a
    // literal path the provider has never heard of.
    let run = ono_in(&directory, "get file *.txt | select name | to json");
    run.assert_success();
    assert!(
        run.stderr().is_empty(),
        "a glob that matches is not an error, got {:?}",
        run.stderr()
    );
    let mut names: Vec<String> = rows(&run).iter().map(|row| text(row, "name")).collect();
    names.sort();
    assert_eq!(
        names,
        ["a.txt", "b.txt"],
        "spec §9.1: `get file <glob>` returns one File record per match"
    );
}

// --- read file (spec §9.1, §12.1) ---------------------------------------------------------------

#[test]
fn should_return_the_content_as_text_when_an_encoding_is_named() {
    let directory = scratch();
    directory.write("hello.txt", "hello world\n");

    let run = ono_in(&directory, "read file hello.txt --encoding utf-8 | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"["hello world\n"]"#,
        "file.yaml: `--encoding` decodes the content as text (spec §12.1)"
    );
}

#[test]
fn should_return_raw_bytes_when_no_encoding_is_named() {
    let directory = scratch();
    directory.write("data.bin", [0x00, 0xff, 0x41]);

    // The contract's own example, `read file ./data.bin | to json`. Without an encoding the
    // content stays bytes — Ono does not guess (spec §12.1) — and bytes serialise as hex, the way
    // external output already does.
    let run = ono_in(&directory, "read file data.bin | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"["00ff41"]"#,
        "undecoded content is bytes, serialised without loss"
    );
}

#[test]
fn should_read_the_file_that_arrives_through_the_pipeline() {
    let directory = scratch();
    directory.write("piped.txt", "from the stream");

    // file.yaml: `input: null | stream<ono.file/1>` — the File record names the file to read.
    let run = ono_in(
        &directory,
        "get file piped.txt | read file --encoding utf-8 | to json",
    );
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"["from the stream"]"#,
        "a File record on the pipeline selects the file to read"
    );
}

// --- write file (spec §9.1, §11.5) --------------------------------------------------------------

#[test]
fn should_create_a_file_from_the_pipeline_string_when_writing() {
    let directory = scratch();

    let run = ono_in(
        &directory,
        r#"let s = "hello"; $s | write file out.txt | to json"#,
    );
    run.assert_success();
    assert_eq!(
        directory.read("out.txt"),
        "hello",
        "file.yaml: `--create` defaults to true, so a missing target is created"
    );
    assert_success_row(&single_result(&run), "ono.file.write");
}

#[test]
fn should_write_external_bytes_unchanged() {
    let directory = scratch();

    // Spec §12.1: external stdout is bytes, and the shell must not pretend otherwise — what echo
    // produced is what lands on disk, newline included.
    let run = ono_in(&directory, "echo hi | write file out.txt");
    run.assert_success();
    assert_eq!(
        directory.read("out.txt"),
        "hi\n",
        "bytes from an external stage are written byte for byte"
    );
}

#[test]
fn should_refuse_to_replace_an_existing_file_without_overwrite() {
    let directory = scratch();
    directory.write("out.txt", "old");

    let run = ono_in(
        &directory,
        r#"let s = "new"; $s | write file out.txt | to json"#,
    );
    run.assert_status(1);
    assert_eq!(
        directory.read("out.txt"),
        "old",
        "file.yaml: without `--overwrite` an existing target is left as it was"
    );
    assert_failed_row(&single_result(&run), "ono.file.write", "Ono-Sendai-E0303");
}

#[test]
fn should_replace_an_existing_file_when_overwrite_is_given() {
    let directory = scratch();
    directory.write("out.txt", "old");

    let run = ono_in(
        &directory,
        r#"let s = "new"; $s | write file out.txt --overwrite | to json"#,
    );
    run.assert_success();
    assert_eq!(
        directory.read("out.txt"),
        "new",
        "file.yaml: `--overwrite` permits replacing an existing file"
    );
    assert_success_row(&single_result(&run), "ono.file.write");
}

#[test]
fn should_append_to_an_existing_file_when_append_is_given() {
    let directory = scratch();
    directory.write("log.txt", "old\n");

    let run = ono_in(
        &directory,
        r#"let s = "new\n"; $s | write file log.txt --append | to json"#,
    );
    run.assert_success();
    assert_eq!(
        directory.read("log.txt"),
        "old\nnew\n",
        "file.yaml: `--append` adds to the end rather than replacing"
    );
    assert_success_row(&single_result(&run), "ono.file.write");
}

// --- copy file ----------------------------------------------------------------------------------

#[test]
fn should_copy_a_file_and_report_the_result() {
    let directory = scratch();
    directory.write("a.txt", "payload");

    // The contract's example: `copy file ./a.txt ./b.txt`.
    let run = ono_in(&directory, "copy file a.txt b.txt | to json");
    run.assert_success();
    assert_eq!(
        directory.read("b.txt"),
        "payload",
        "the destination holds the source's content"
    );
    assert_eq!(
        directory.read("a.txt"),
        "payload",
        "a copy leaves the source in place"
    );
    assert_success_row(&single_result(&run), "ono.file.copy");
}

#[test]
fn should_copy_a_tree_when_recursive_is_given() {
    let directory = scratch();
    directory.write("src/inner/deep.txt", "deep");
    directory.write("src/top.txt", "top");

    let run = ono_in(&directory, "copy file src dst --recursive");
    run.assert_success();
    assert_eq!(
        directory.read("dst/inner/deep.txt"),
        "deep",
        "file.yaml: `--recursive` copies directories and their contents"
    );
    assert_eq!(directory.read("dst/top.txt"), "top");
}

#[test]
fn should_preserve_the_mode_when_preserve_is_given() {
    let directory = scratch();
    let source = directory.write("secret.txt", "s");
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o640))
        .expect("chmod on a scratch file");

    let run = ono_in(&directory, "copy file secret.txt copy.txt --preserve");
    run.assert_success();
    assert_eq!(
        permission_bits(&directory.path().join("copy.txt")),
        0o640,
        "file.yaml: `--preserve` keeps the mode where permitted"
    );
}

#[test]
fn should_preserve_the_timestamps_of_a_copied_tree_when_preserve_is_given() {
    // file.yaml: `--preserve` keeps "the mode, the timestamps and — where permitted — the
    // ownership of every copied entry". A directory has timestamps like any other entry, and
    // copying a tree without them turns an archive into something that all happened today
    // (ADR-0234).
    let directory = scratch();
    directory.write("src/inner/deep.txt", "deep");
    let then = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_577_836_800);
    let times = std::fs::FileTimes::new()
        .set_accessed(then)
        .set_modified(then);
    for entry in ["src/inner/deep.txt", "src/inner", "src"] {
        let path = directory.path().join(entry);
        let handle = std::fs::File::options()
            .read(true)
            .open(&path)
            .expect("the scratch entry");
        handle.set_times(times).expect("the fixture timestamps");
    }

    let run = ono_in(&directory, "copy file src dst --recursive --preserve");
    run.assert_success();

    for entry in ["dst", "dst/inner", "dst/inner/deep.txt"] {
        let modified = std::fs::symlink_metadata(directory.path().join(entry))
            .and_then(|metadata| metadata.modified())
            .expect("the copied entry");
        assert_eq!(
            modified, then,
            "`--preserve` keeps the modification time of every copied entry, and `{entry}` is \
             one: a directory is not exempt because its timestamps need a syscall that does not \
             open it for writing"
        );
    }
}

#[test]
fn should_refuse_to_copy_over_an_existing_destination_without_overwrite() {
    let directory = scratch();
    directory.write("a.txt", "new");
    directory.write("b.txt", "old");

    let run = ono_in(&directory, "copy file a.txt b.txt | to json");
    run.assert_status(1);
    assert_eq!(
        directory.read("b.txt"),
        "old",
        "an existing destination is untouched without `--overwrite`"
    );
    assert_failed_row(&single_result(&run), "ono.file.copy", "Ono-Sendai-E0303");
}

// --- move file ----------------------------------------------------------------------------------

#[test]
fn should_move_a_file_and_leave_no_source_behind() {
    let directory = scratch();
    directory.write("a.txt", "payload");
    std::fs::create_dir(directory.path().join("archive")).expect("the archive directory");

    // The contract's example: `move file ./a.txt ./archive/a.txt`.
    let run = ono_in(&directory, "move file a.txt archive/a.txt | to json");
    run.assert_success();
    assert_eq!(directory.read("archive/a.txt"), "payload");
    assert!(
        !directory.exists("a.txt"),
        "spec §9.1: `move file` relocates — the source is gone"
    );
    assert_success_row(&single_result(&run), "ono.file.move");
}

// --- remove file (spec §9.1, §11.6, §16.5, §17.3, §17.4, §41.3) ---------------------------------

#[test]
fn should_remove_a_single_file_and_report_the_result() {
    let directory = scratch();
    directory.write("build.tmp", "x");

    // The contract's example: `remove file ./build.tmp`.
    let run = ono_in(&directory, "remove file build.tmp | to json");
    run.assert_success();
    assert!(
        !directory.exists("build.tmp"),
        "spec §9.1: `remove file` deletes the resource"
    );
    assert_success_row(&single_result(&run), "ono.file.remove");
}

#[test]
fn should_resolve_a_glob_to_exact_targets_before_removing() {
    let directory = scratch();
    directory.write("a.txt", "a");
    directory.write("b.txt", "b");
    directory.write("c.md", "c");

    // Spec §17.3: `remove file *.tmp` knows its exact targets before mutating, and §16.5 forbids
    // collapsing the fan-out — one row per target.
    let run = ono_in(&directory, "remove file *.txt | to json");
    run.assert_success();
    assert!(
        !directory.exists("a.txt") && !directory.exists("b.txt"),
        "both matches are removed"
    );
    assert!(
        directory.exists("c.md"),
        "spec §17.3: a file the glob did not match is never touched"
    );
    let results = rows(&run);
    assert_eq!(
        results.len(),
        2,
        "spec §16.5: one ActionResult per resolved target, got {:?}",
        run.stdout()
    );
    for row in &results {
        assert_success_row(row, "ono.file.remove");
    }
    let targets = run.stdout();
    assert!(
        targets.contains("a.txt") && targets.contains("b.txt"),
        "each row names the file it acted on, got {targets:?}"
    );
}

#[test]
fn should_report_a_missing_removal_target_as_a_failed_row() {
    let directory = scratch();

    let run = ono_in(&directory, "remove file nope.txt | to json");
    run.assert_status(1);
    assert_failed_row(&single_result(&run), "ono.file.remove", "Ono-Sendai-E0301");
}

#[test]
fn should_refuse_a_bulk_removal_in_a_script_without_confirm() {
    let directory = scratch();
    directory.write("a.txt", "a");
    directory.write("b.txt", "b");

    // Spec §11.6 makes the threshold configurable and §17.4 forbids waiting for a prompt: over
    // the threshold, a script gets `safety.confirmation_required` and nothing is deleted.
    let run = ono_in(
        &directory,
        "set config safety.confirm.bulk_threshold = 1; remove file *.txt",
    );
    assert!(
        !run.status().is_success(),
        "spec §17.4: a destructive policy violation fails, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0701"),
        "spec §17.4: the refusal is `safety.confirmation_required`, got {:?}",
        run.stderr()
    );
    assert!(
        directory.exists("a.txt") && directory.exists("b.txt"),
        "spec §11.6: scope is calculated before execution, so a refused bulk removal deletes nothing"
    );
}

#[test]
fn should_perform_a_bulk_removal_when_confirm_is_given() {
    let directory = scratch();
    directory.write("a.txt", "a");
    directory.write("b.txt", "b");

    let run = ono_in(
        &directory,
        "set config safety.confirm.bulk_threshold = 1; remove file *.txt --confirm | to json",
    );
    run.assert_success();
    assert!(
        !directory.exists("a.txt") && !directory.exists("b.txt"),
        "file.yaml: `--confirm` confirms the bulk deletion non-interactively"
    );
    assert_eq!(rows(&run).len(), 2, "one row per target (spec §16.5)");
}

#[test]
fn should_remove_the_files_a_finite_query_selected() {
    let directory = scratch();
    directory.write("keep.log", "k");
    directory.write("drop.log", "d");

    // Spec §41.3's shape: a query selects, `remove file` consumes the File records.
    let run = ono_in(
        &directory,
        r#"get file *.log | where name == "drop.log" | remove file | to json"#,
    );
    run.assert_success();
    assert!(
        !directory.exists("drop.log"),
        "the selected file is removed"
    );
    assert!(
        directory.exists("keep.log"),
        "a file the query did not select is untouched"
    );
    assert_success_row(&single_result(&run), "ono.file.remove");
}

// --- remove dir ---------------------------------------------------------------------------------

#[test]
fn should_refuse_to_remove_a_non_empty_directory_without_recursive() {
    let directory = scratch();
    directory.write("build/artifact.o", "o");

    let run = ono_in(&directory, "remove dir build | to json");
    run.assert_status(1);
    assert!(
        directory.exists("build/artifact.o"),
        "file.yaml: without `--recursive` a non-empty directory is refused, and refused means untouched"
    );
    let row = single_result(&run);
    assert_eq!(text(&row, "status"), "failed", "got {row:?}");
    assert_eq!(text(&row, "operation"), "ono.dir.remove", "got {row:?}");
    assert!(
        row["error"]["code"]
            .as_str()
            .is_some_and(|code| code.starts_with("Ono-Sendai-E")),
        "spec §43: the refusal carries a structured error, got {row:?}"
    );
}

#[test]
fn should_remove_a_directory_tree_when_recursive_is_given() {
    let directory = scratch();
    directory.write("build/inner/artifact.o", "o");

    // The contract's example: `remove dir ./build --recursive`.
    let run = ono_in(&directory, "remove dir build --recursive | to json");
    run.assert_success();
    assert!(
        !directory.exists("build"),
        "file.yaml: `--recursive` deletes the contents as well"
    );
    assert_success_row(&single_result(&run), "ono.dir.remove");
}

// --- set file / set dir -------------------------------------------------------------------------

#[test]
fn should_change_the_mode_of_a_file() {
    let directory = scratch();
    let script = directory.write("script.sh", "#!/bin/sh\n");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644))
        .expect("chmod on a scratch file");

    // The contract's example: `set file ./script.sh --mode 0755`.
    let run = ono_in(&directory, "set file script.sh --mode 0755 | to json");
    run.assert_success();
    assert_eq!(
        permission_bits(&script),
        0o755,
        "file.yaml: `--mode` sets the permission bits given as four octal digits"
    );
    assert_success_row(&single_result(&run), "ono.file.set");
}

#[test]
fn should_report_no_change_when_the_mode_already_matches() {
    let directory = scratch();
    let file = directory.write("same.txt", "");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))
        .expect("chmod on a scratch file");

    let run = ono_in(&directory, "set file same.txt --mode 0644 | to json");
    run.assert_success();
    let row = single_result(&run);
    assert_eq!(
        row["changed"].as_bool(),
        Some(false),
        "action-result.v1: a no-op is honest — `changed` is false, got {row:?}"
    );
    assert_ne!(
        text(&row, "status"),
        "failed",
        "asking for the state that already holds is not a failure, got {row:?}"
    );
}

#[test]
fn should_change_the_mode_of_a_directory() {
    let directory = scratch();
    let secrets = directory.path().join("secrets");
    std::fs::create_dir(&secrets).expect("the scratch directory");
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o755))
        .expect("chmod on a scratch directory");

    // The contract's example: `set dir ./secrets --mode 0700`.
    let run = ono_in(&directory, "set dir secrets --mode 0700 | to json");
    run.assert_success();
    assert_eq!(
        permission_bits(&secrets),
        0o700,
        "file.yaml: `set dir --mode` changes the directory's permission bits"
    );
    assert_success_row(&single_result(&run), "ono.dir.set");
}

// --- open file ----------------------------------------------------------------------------------

#[test]
fn should_open_a_file_with_the_handler_named_explicitly() {
    let directory = scratch();
    directory.write("report.txt", "r");

    // file.yaml: `--with` names the handler instead of the association. `true` accepts any
    // argument and exits 0, so the association table of the test host never matters.
    let run = ono_in(&directory, "open file report.txt --with true | to json");
    run.assert_success();
    assert_success_row(&single_result(&run), "ono.file.open");
}

// --- tail file ----------------------------------------------------------------------------------

#[test]
fn should_emit_the_last_lines_of_a_file_when_tailing() {
    let directory = scratch();
    directory.write("app.log", "one\ntwo\nthree\nfour\nfive\n");

    // `--lines` bounds what is emitted before following; `take` bounds the follow so that the
    // document can end (spec §18.3).
    let run = ono_in(&directory, "tail file app.log --lines 2 | take 2 | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"["four","five"]"#,
        "file.yaml: `--lines N` emits the last N existing lines, as strings"
    );
}

#[test]
fn should_follow_appended_lines_when_tailing() {
    let directory = scratch();
    let log = directory.write("app.log", "existing\n");

    // A writer appends after the tail has started; `--follow` is the default, so the new line
    // arrives without the existing ones (`--lines 0`).
    let script = format!(
        r#"sh -c "sleep 0.5; echo appended >> {log}" &; tail file app.log --lines 0 | take 1 | to json"#,
        log = log.display()
    );
    let run = ono_in(&directory, &script);
    run.assert_success();
    assert!(
        run.stdout().contains(r#"["appended"]"#),
        "file.yaml: the stream stays open as the file grows, got {:?}",
        run.stdout()
    );
}

// --- watch file (spec §18.2, ADR-0024) ----------------------------------------------------------

#[test]
fn should_begin_a_file_watch_with_a_snapshot() {
    let directory = scratch();
    directory.write("src/main.rs", "fn main() {}\n");

    // ADR-0024: a subscription always begins with the current state as `snapshot` events.
    let run = ono_in(
        &directory,
        "watch file src | take 1 | select kind | to json",
    );
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"kind":"snapshot"}]"#,
        "spec §18.2 + ADR-0024: the first event of a file watch is the snapshot"
    );
}

#[test]
fn should_report_a_created_file_as_an_event_when_watching() {
    let directory = scratch();
    let watched = directory.path().join("src");
    std::fs::create_dir(&watched).expect("the watched directory");

    let script = format!(
        r#"sh -c "sleep 0.5; touch {dir}/new.txt" &; watch file src | where kind != "snapshot" | take 1 | to json"#,
        dir = watched.display()
    );
    let run = ono_in(&directory, &script);
    run.assert_success();
    let event = run.stdout();
    assert!(
        event.contains("added") && event.contains("new.txt"),
        "spec §18.2: a file created under the watched path is reported as an `added` event \
         naming the file, got {event:?}"
    );
}

#[test]
fn should_report_a_created_file_before_the_next_poll_would_have_come() {
    // ADR-0034 and ADR-0078: `watch file` polled every two seconds, so nothing it reported could
    // arrive sooner than the next tick of that grid. The file here is created two and a half
    // seconds in, between the tick that would have missed it and the tick that would have found
    // it, and the event must arrive before that later tick (ADR-0235). The event also says where
    // it came from: §18.2 requires the cost of a watch to be explicit, and `source` is where a
    // consumer reads whether the shell is being told or is asking.
    let directory = scratch();
    let watched = directory.path().join("src");
    std::fs::create_dir(&watched).expect("the watched directory");

    let script = format!(
        r#"sh -c "sleep 2.5; touch {dir}/new.txt" &; watch file src | where kind != "snapshot" | take 1 | select kind source | to json"#,
        dir = watched.display()
    );
    let started = std::time::Instant::now();
    let run = ono_in(&directory, &script);
    let elapsed = started.elapsed();
    run.assert_success();

    assert_eq!(
        run.stdout().trim(),
        r#"[{"kind":"added","source":"subscription"}]"#,
        "spec §18.2: the file was created, the kernel said so, and the event says the change \
         came from a subscription rather than from a poll; got {:?}",
        run.output()
    );
    assert!(
        elapsed < Duration::from_millis(3_500),
        "the poll grid would not have looked again until four seconds in; an answer in \
         {elapsed:?} is the kernel's, not a poll's"
    );
}

// --- trace file (spec §22.3) --------------------------------------------------------------------

#[test]
fn should_name_the_process_holding_a_file_when_tracing() {
    let directory = scratch();
    let held = directory.write("held.txt", "h");

    // A child of this very script holds the file open on its stdin, so the trace has exactly
    // one holder it can be sure of — and it is a `sleep`.
    let script = format!(
        "sleep 2 < {held} &; trace file held.txt | to json",
        held = held.display()
    );
    let run = ono_in(&directory, &script);
    run.assert_success();
    let graph = run.stdout();
    assert!(
        graph.contains("ono.process/1") && graph.contains("sleep"),
        "spec §22.3: the graph names the process holding the file, got {graph:?}"
    );
}

#[test]
fn should_include_the_users_behind_the_holders_when_asked() {
    let directory = scratch();
    let held = directory.write("held.txt", "h");

    // The contract's example shape: `trace file <path> --users`.
    let script = format!(
        "sleep 2 < {held} &; trace file held.txt --users | to json",
        held = held.display()
    );
    let run = ono_in(&directory, &script);
    run.assert_success();
    let graph = run.stdout();
    assert!(
        graph.contains("ono.user/1"),
        "spec §22.3: `--users` adds the users behind the holding processes to the graph, got {graph:?}"
    );
}

// --- enter file (spec §14.3) --------------------------------------------------------------------

#[test]
fn should_push_a_file_frame_when_entering_a_file() {
    let directory = scratch();
    let file = directory.write("Cargo.toml", "[package]\n");

    // The contract's example: `enter file ./Cargo.toml`. The frame is visible on the stack with
    // the file as its identity (spec §14.1, ADR-0023).
    let run = ono_in(&directory, "enter file Cargo.toml; get context | to json");
    run.assert_success();
    assert!(
        run.stderr().is_empty(),
        "entering a real file is not an error, got {:?}",
        run.stderr()
    );
    let stack = rows(&run);
    assert_eq!(
        stack.len(),
        2,
        "the ground frame plus the entered file, got {:?}",
        run.stdout()
    );
    let frame = &stack[1];
    assert_eq!(
        text(frame, "target"),
        "file",
        "context.v1: the frame names what it narrows to, got {frame:?}"
    );
    let identity = text(frame, "identity");
    assert!(
        identity.ends_with("Cargo.toml") && file.ends_with(identity.trim_start_matches("./")),
        "context.v1: the identity is the entered file's path, got {frame:?}"
    );
}

#[test]
fn should_pop_the_file_frame_when_leaving() {
    let directory = scratch();
    directory.write("Cargo.toml", "[package]\n");

    let run = ono_in(
        &directory,
        "enter file Cargo.toml; leave; get context | to json",
    );
    run.assert_success();
    assert!(
        run.stderr().is_empty(),
        "leaving a frame that was pushed says nothing (ADR-0023 reserves the diagnostic for an \
         empty stack), got {:?}",
        run.stderr()
    );
    let stack = rows(&run);
    assert_eq!(
        stack.len(),
        1,
        "spec §14: `leave` pops the file frame, got {:?}",
        run.stdout()
    );
}

// --- what running the shell showed, pinned so it cannot regress silently (B-harn-6) -------------

#[test]
fn should_answer_a_large_file_with_one_value_rather_than_a_value_per_chunk() {
    // Spec §12.1: `read file` answers with *the content*, one value. A reader that emitted a
    // value per buffer would still print the same bytes, so nothing downstream would notice —
    // until `| count`, `| each` or `| to json` saw twenty megabytes as thousands of rows. The
    // file is big enough to force many reads and small enough to cost nothing on a tmpfs.
    let directory = scratch();
    directory.write("large.txt", vec![b'x'; 20 * 1024 * 1024]);

    let run = ono_in(
        &directory,
        "read file large.txt --encoding utf-8 | count | to json",
    );
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[1]",
        "spec §12.1: the content of one file is one value however many reads it took, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_name_the_files_a_glob_resolved_to_when_explaining_a_removal() {
    // Spec §17.3: "`remove file *.tmp` knows its exact targets before mutating", and §15.3 says
    // `explain` reports what the shell would actually do. Together that means the plan names the
    // resolved files, not the pattern — the one line an operator reads before a destructive
    // command. Nothing is removed: `explain` never executes its subject.
    let directory = scratch();
    directory.write("a.txt", "a");
    directory.write("b.txt", "b");
    directory.write("c.md", "c");

    let run = ono_in(&directory, "explain remove file *.txt");
    run.assert_success();
    let plan = run.stdout().to_owned();
    assert!(
        plan.contains("remove file a.txt b.txt"),
        "spec §17.3: the plan names the exact targets the glob resolved to, got {plan:?}"
    );
    assert!(
        !plan.contains("*.txt"),
        "an unexpanded pattern in the plan tells the operator nothing about scope, got {plan:?}"
    );
    assert!(
        directory.exists("a.txt") && directory.exists("b.txt") && directory.exists("c.md"),
        "spec §15.3: explaining a removal removes nothing"
    );
}
