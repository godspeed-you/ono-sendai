//! Outcome tests for the data transforms the contract declares:
//! `tail`, `join` and `diff` (`docs/spec/commands/data.yaml`, spec §53 Appendix B, §11.1), and
//! for the one rendering rule the wiki lists as differing from the contract: a narrow terminal
//! switches to stacked records instead of keeping a table it has to mutilate (spec §13.2, §13.3).
//!
//! Every test asserts what a user observes — `to json` output, exit status, structured error
//! codes, the rendered screen — never how a stage is implemented (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::{Duration, Instant};

use ono_process::{Command, Executor, PtySession, WindowSize};
use ono_testkit::Shell;
use serde_yaml_ng::Value;

/// Runs a one-liner and returns the finished run.
fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .timeout(Duration::from_secs(20))
        .run()
}

/// Parses the JSON array `to json` writes (spec §33.5) into its rows.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let text = run.stdout().trim();
    let parsed: Value = serde_yaml_ng::from_str(text)
        .unwrap_or_else(|error| panic!("`to json` must write one JSON document: {error}\n{text}"));
    parsed
        .as_sequence()
        .unwrap_or_else(|| panic!("`to json` writes the stream as an array (spec §33.5): {text}"))
        .clone()
}

/// The compact JSON text of one row, so a test can assert on the values a row carries without
/// fixing whether the transform nests or merges them — spec §53 declines to freeze that shape.
fn text_of(row: &Value) -> String {
    serde_json_text(row)
}

fn serde_json_text(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => format!("{text:?}"),
        Value::Sequence(items) => {
            let inner: Vec<String> = items.iter().map(serde_json_text).collect();
            format!("[{}]", inner.join(","))
        }
        Value::Mapping(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, value)| format!("{}:{}", serde_json_text(key), serde_json_text(value)))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Tagged(tagged) => serde_json_text(&tagged.value),
    }
}

// --- `tail` -----------------------------------------------------------------------------------

#[test]
fn should_emit_the_last_n_values_of_a_finite_stream() {
    let run = ono("echo '[1,2,3,4]' | from json | tail 2 | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[3,4]",
        "data.yaml `ono.data.tail`: the trailing `count` values of a finite stream, in order"
    );
}

#[test]
fn should_keep_whole_records_when_tailing_a_record_stream() {
    let run = ono(r#"echo '[{"n":1},{"n":2},{"n":3}]' | from json | tail 1 | to json"#);
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"n":3}]"#,
        "spec §12.1: `tail` counts values, never lines — a record leaves whole"
    );
}

#[test]
fn should_emit_the_whole_stream_when_the_count_exceeds_it() {
    let run = ono("echo '[1,2]' | from json | tail 9 | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[1,2]",
        "a count larger than the stream is not an error and fabricates nothing (spec §35.3)"
    );
}

#[test]
fn should_follow_a_finite_stream_to_its_end_when_asked_to_follow() {
    // data.yaml: `--follow` keeps the stream open and emits values as they arrive. On a finite
    // input the end arrives at once, so the observable result is the same trailing window — the
    // option must not hang or reject bounded input.
    let run = ono("echo '[1,2,3]' | from json | tail 2 --follow | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[2,3]",
        "`tail --follow` on bounded input ends when the input ends"
    );
}

#[test]
fn should_follow_an_unbounded_stream_instead_of_waiting_for_its_end() {
    // data.yaml: "Emit the last N values of a finite stream, or follow an unbounded one." Spec
    // §11.1: a blocking transform on an unbounded stream needs a window or a structured error;
    // `tail` is declared streaming, so on `watch` it passes values on as they arrive and the
    // `take 1` downstream ends the pipeline.
    let run = ono("watch process --every 100ms | tail 1 | take 1 | count | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[1]",
        "`tail` on an unbounded stream follows it; the pipeline ends when `take` is satisfied"
    );
}

#[test]
fn should_bind_the_transform_after_a_native_stage_and_the_program_at_a_byte_boundary() {
    // ADR-0028: a native command whose declared input is a stream of objects binds only where
    // objects reach it; elsewhere the name resolves onward to `PATH`. So `tail -n 1 <file>` at
    // the head of a pipeline is `/usr/bin/tail`, and `| tail 1` after `from json` is
    // `ono.data.tail`. Both spellings in one shell, decided by the types, not by a heuristic.
    let directory = ono_testkit::scratch();
    directory.write("lines.txt", "first\nsecond\nlast\n");
    let file = directory.path().join("lines.txt");

    let external = ono(&format!("tail -n 1 {}", file.display()));
    external.assert_success();
    assert_eq!(
        external.stdout().trim(),
        "last",
        "ADR-0028: with no object stream reaching it, `tail` is the Unix program"
    );

    let native = ono("echo '[1,2,3]' | from json | tail 1 | to json");
    native.assert_success();
    assert_eq!(
        native.stdout().trim(),
        "[3]",
        "ADR-0028: after a native stage that produces objects, `tail` is the transform"
    );
}

// --- `join` -----------------------------------------------------------------------------------

const USERS: &str = r#"let users = [{"uid":0,"name":"root"},{"uid":7,"name":"nobody"}]"#;
const PROCS: &str = r#"let procs = [{"pid":1,"uid":0},{"pid":2,"uid":1}]"#;

#[test]
fn should_pair_records_that_share_the_key_in_an_inner_join() {
    let run = ono(&format!(
        "{USERS}; {PROCS}; $procs | join $users --on uid | to json"
    ));
    run.assert_success();

    let rows = rows(&run);
    assert_eq!(
        rows.len(),
        1,
        "data.yaml: `kind` defaults to `inner`, so only the matching pair survives: {}",
        run.stdout()
    );
    let row = text_of(&rows[0]);
    assert!(
        row.contains("\"pid\":1") && row.contains("\"name\":\"root\""),
        "the joined row carries both sides of the match (spec §53): {row}"
    );
    assert!(
        !row.contains("\"pid\":2"),
        "an inner join drops the unmatched row: {row}"
    );
}

#[test]
fn should_keep_unmatched_left_rows_with_a_null_right_side_when_kind_is_left() {
    let run = ono(&format!(
        "{USERS}; {PROCS}; $procs | join $users --on uid --kind left | to json"
    ));
    run.assert_success();

    let rows = rows(&run);
    assert_eq!(
        rows.len(),
        2,
        "data.yaml: `--kind left` keeps every left row: {}",
        run.stdout()
    );
    let lonely = text_of(&rows[1]);
    assert!(
        lonely.contains("\"pid\":2") && !lonely.contains("root") && lonely.contains("null"),
        "an unmatched left row has no right side, and that is null, not a fabrication \
         (spec §35.3): {lonely}"
    );
}

#[test]
fn should_keep_the_unmatched_rows_of_both_sides_in_an_outer_join() {
    let run = ono(&format!(
        "{USERS}; {PROCS}; $procs | join $users --on uid --kind outer | to json"
    ));
    run.assert_success();

    let rows = rows(&run);
    assert_eq!(
        rows.len(),
        3,
        "data.yaml: `--kind outer` keeps the match, the lonely process and the lonely user: {}",
        run.stdout()
    );
    let all = run.stdout();
    for needle in ["\"name\":\"root\"", "\"pid\":2", "\"name\":\"nobody\""] {
        assert!(
            all.contains(needle),
            "{needle} must appear in an outer join: {all}"
        );
    }
}

#[test]
fn should_join_provider_records_given_as_a_parenthesised_pipeline() {
    // data.yaml's own example spells the right side as `(get socket)`; the process provider
    // stands in for the socket provider so the test needs no privilege.
    let run = ono(
        "get process | where pid == 1 | join (get process | where pid == 1) --on pid \
         | count | to json",
    );
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[1]",
        "the right side of a `join` is any expression producing records (data.yaml example)"
    );
}

// --- `diff` -----------------------------------------------------------------------------------

/// `before` is the older snapshot, `now` the newer one. The input of `diff` is the current
/// state and its argument is what it is compared against, the way data.yaml's own example
/// `get service | diff @-1` reads.
const BEFORE: &str = r#"let before = [{"pid":1,"name":"init"},{"pid":2,"name":"x"}]"#;
const NOW: &str = r#"let now = [{"pid":1,"name":"systemd"},{"pid":3,"name":"y"}]"#;

/// The `(change, key)` pairs of a diff result, whatever else a row carries.
fn changes(rows: &[Value]) -> Vec<(String, i64)> {
    rows.iter()
        .map(|row| {
            let map = row.as_mapping().expect("a diff row is a record");
            let change = map
                .get("change")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("a diff row names its `change` (spec §53): {row:?}"))
                .to_owned();
            let key = map.get("key").and_then(Value::as_i64).unwrap_or_else(|| {
                panic!("a diff row carries the identity it differs on: {row:?}")
            });
            (change, key)
        })
        .collect()
}

#[test]
fn should_report_added_removed_and_changed_rows_between_two_snapshots() {
    let run = ono(&format!(
        "{BEFORE}; {NOW}; $now | diff $before --identity [pid] | to json"
    ));
    run.assert_success();

    let mut found = changes(&rows(&run));
    found.sort();
    assert_eq!(
        found,
        [
            ("added".to_owned(), 3),
            ("changed".to_owned(), 1),
            ("removed".to_owned(), 2)
        ],
        "spec §53: a diff by identity reports what appeared, what vanished and what changed: {}",
        run.stdout()
    );
}

#[test]
fn should_carry_both_values_of_a_changed_row() {
    let run = ono(&format!(
        "{BEFORE}; {NOW}; $now | diff $before --identity [pid] | where change == \"changed\" \
         | to json"
    ));
    run.assert_success();

    let rows = rows(&run);
    assert_eq!(rows.len(), 1, "exactly pid 1 changed: {}", run.stdout());
    let row = text_of(&rows[0]);
    assert!(
        row.contains("init") && row.contains("systemd"),
        "a changed row shows the old and the new value, not just that something differs: {row}"
    );
}

#[test]
fn should_report_no_rows_when_the_snapshots_are_identical() {
    let run = ono(&format!(
        "{BEFORE}; $before | diff $before --identity [pid] | to json"
    ));
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[]",
        "an unchanged object is not a change (spec §53)"
    );
}

#[test]
fn should_report_two_fresh_snapshots_of_one_object_as_unchanged() {
    // `diff` answers what changed about the objects, not when they were read. Two readings of
    // the same unchanged user differ only in the instant each was observed, which is provenance
    // (spec §26, §10.7) and not the object's state (ADR-0229).
    let run = ono("get user root | diff (get user root) | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[]",
        "nothing about root changed between the two readings: {}",
        run.output()
    );
}

#[test]
fn should_still_report_a_field_that_moved_between_two_snapshots() {
    // The comparison ignores provenance, not data: a field that differs is still a change.
    let run = ono("let before = [{\"pid\":1,\"name\":\"init\"}]; \
         let now = [{\"pid\":1,\"name\":\"systemd\"}]; \
         $now | diff $before --identity [pid] | select change | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[{\"change\":\"changed\"}]",
        "a field that moved is a change: {}",
        run.output()
    );
}

#[test]
fn should_compare_provider_records_by_their_schema_identity_without_an_override() {
    // `ono.process/1` declares `identity: [pid, started]`, so no `--identity` is needed. The
    // right side is an empty snapshot (no process has that pid), which makes pid 1 `added`.
    let run =
        ono("get process | where pid == 1 | diff (get process | where pid == 99999999) | to json");
    run.assert_success();

    let rows = rows(&run);
    assert_eq!(
        rows.len(),
        1,
        "one process on the left, none on the right, one difference: {}",
        run.stdout()
    );
    assert_eq!(
        rows[0]
            .as_mapping()
            .and_then(|map| map.get("change"))
            .and_then(Value::as_str),
        Some("added"),
        "spec §53: identity comes from the schema (§28.1), and a row only the input has is added: {}",
        run.stdout()
    );
}

// --- rendering ----------------------------------------------------------------------------------

/// Runs one script with `ono -c` on a pseudo-terminal of the given width and returns
/// everything it printed. `-c` keeps the line editor out of the picture: the screen is the
/// rendering alone, laid out for the terminal the shell sees (spec §13.2).
fn screen_of(columns: u16, script: &str) -> String {
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .args(["-c", script])
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    let mut shell: PtySession = executor
        .run_pty(&command, WindowSize::new(30, columns))
        .expect("a pseudo-terminal");

    let mut seen = String::new();
    let mut buffer = [0u8; 8192];
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        match shell.read_timeout(&mut buffer, Duration::from_millis(150)) {
            Ok(Some(0)) | Err(_) => break,
            Ok(Some(count)) => seen.push_str(&String::from_utf8_lossy(&buffer[..count])),
            Ok(None) if shell.try_wait().ok().flatten().is_some() => break,
            Ok(None) => {}
        }
    }
    let _ = shell.wait();
    seen
}

#[test]
fn should_stack_records_instead_of_truncating_a_table_when_the_terminal_is_narrow() {
    // Spec §13.2: a compact table only "when records are homogeneous and terminal width
    // permits"; otherwise stacked records, one field per line, so that every value stays
    // readable. Thirty columns do not permit the process table — today the shell keeps the
    // table and mutilates every cell (`sy...`, `16...`, `on...`), which is the wiki row
    // "narrow terminals switch to stacked records | truncates cells with `...`".
    let seen = screen_of(30, "get process | where pid == 1");
    let output: Vec<&str> = seen
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.trim().is_empty())
        .collect();

    assert!(
        output.iter().any(|line| {
            let lower = line.to_ascii_lowercase();
            let words: Vec<&str> = lower.split_whitespace().collect();
            (words.first() == Some(&"pid") && words.get(1) == Some(&"1"))
                || (words.first() == Some(&"process") && words.get(1) == Some(&"1"))
        }),
        "spec §13.2: a stacked record puts the field and its value on one line (`pid 1` or the \
         `PROCESS 1 …` heading of the spec's example); rendered at 30 columns:\n{}",
        output.join("\n")
    );
    assert!(
        !output.iter().any(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("pid") && lower.contains("name") && lower.contains("cpu")
        }),
        "a table header at 30 columns means the renderer kept a table the width does not \
         permit (spec §13.2):\n{}",
        output.join("\n")
    );
    assert!(
        !output.iter().any(|line| line.contains("...")),
        "every value of pid 1 fits in 30 columns once stacked, so nothing may be truncated \
         (spec §13.3):\n{}",
        output.join("\n")
    );
}

// --- a declared error field is data a path can descend into (ADR-0215, spec §10.5, §11.5) ----

#[test]
fn should_project_the_named_field_of_a_result_s_error_when_a_path_descends_into_it() {
    // Spec §11.5 declares `ActionResult.error` as `ono.error/1`: the error stored there is the
    // field's value, so `error.name` is the dotted selector `catch` and `where` match on, not
    // the whole error record.
    let run = ono("stop process 999999 | select status error.name error.code | to json");

    let rows = rows(&run);
    assert_eq!(
        rows.len(),
        1,
        "one result per target (spec §16.5): {rows:?}"
    );
    let row = &rows[0];
    assert_eq!(
        row.get("status").and_then(Value::as_str),
        Some("failed"),
        "the mutation could not act on a process that is gone: {row:?}"
    );
    assert_eq!(
        row.get("name").and_then(Value::as_str),
        Some("io.not_found"),
        "spec §16.1: `error.name` is the selector, not the record that holds it: {}",
        text_of(row)
    );
    assert_eq!(
        row.get("code").and_then(Value::as_str),
        Some("Ono-Sendai-E0301"),
        "spec §43: `error.code` is the stable code: {}",
        text_of(row)
    );
}

#[test]
fn should_keep_a_row_whose_error_matches_when_a_predicate_reads_the_error_s_name() {
    let run = ono("stop process 999999 | where error.name == \"io.not_found\" | count");

    assert_eq!(
        run.stdout()
            .lines()
            .last()
            .map(str::trim)
            .unwrap_or_default(),
        "1",
        "a predicate over `error.name` selects the failed result: {}",
        run.output()
    );
    assert_ne!(
        run.status().code(),
        0,
        "ADR-0006: a failed ActionResult still decides the aggregate exit status"
    );
}

#[test]
fn should_bind_a_caught_error_as_a_value_whose_fields_can_be_read() {
    // Spec §16: `catch e` binds the structured error. Reading a field of it must answer the
    // field, not re-raise the error the block just caught.
    let run = ono("try { get file /nope/nope/nope } catch e { echo $e.name }");

    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "io.not_found",
        "the caught error's `name` is readable: {}",
        run.output()
    );
}

// --- what the pipeline dropped is said, not only counted (ADR-0014, ADR-0261) ----------------

#[test]
fn should_say_how_many_values_a_condition_could_not_be_decided_on() {
    // ADR-0014: a row a predicate could not decide is excluded and counted, so "a user who is
    // surprised by a row count has somewhere to look that is not the source code".
    let run = Shell::new()
        .args(["-c", "from json | where a > 1 | count"])
        .stdin("[{\"a\":1},{\"a\":null},{\"a\":3}]")
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout().lines().last().map(str::trim),
        Some("1"),
        "the answer is unchanged: {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains('1') && run.stderr().contains("could not be decided"),
        "the excluded row is reported: {:?}",
        run.stderr()
    );
}

#[test]
fn should_say_how_many_unknown_values_an_aggregate_skipped() {
    let run = Shell::new()
        .args([
            "-c",
            "from json | measure a | select count skipped | to json",
        ])
        .stdin("[{\"a\":1},{\"a\":null},{\"a\":3}]")
        .run();
    run.assert_success();
    assert!(
        run.stderr().contains("skipped"),
        "the skipped null is reported: {:?}",
        run.stderr()
    );
}

#[test]
fn should_say_nothing_when_the_pipeline_dropped_nothing() {
    let run = Shell::new()
        .args(["-c", "from json | where a > 0 | count"])
        .stdin("[{\"a\":1}]")
        .run();
    run.assert_success();
    assert_eq!(
        run.stderr(),
        "",
        "a run that dropped nothing says nothing: {:?}",
        run.stderr()
    );
}

// --- a projection keeps the declaration the renderer reads (issue #23, ADR-0419) --------------

/// The last non-empty line of a rendered table: the one row a single-object query answers with.
fn last_row(run: &ono_testkit::Run) -> String {
    run.stdout()
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_owned()
}

#[test]
fn should_render_a_projected_field_the_way_the_field_it_was_projected_from_is_rendered() {
    // `ono.process/1` declares `cpu` as a `float` carrying `unit: percent`, and ADR-0419 made a
    // cell read that declaration. A projection of `cpu` is still that field.
    let whole = Shell::new()
        .args(["-c", "get process 1"])
        .env("NO_COLOR", "1")
        .run();
    whole.assert_success();
    let projected = Shell::new()
        .args(["-c", "get process 1 | select cpu"])
        .env("NO_COLOR", "1")
        .run();
    projected.assert_success();

    let cell = last_row(&projected);
    assert!(
        cell.ends_with('%') && cell.matches('.').count() == 1,
        "spec §13.2 prints a percentage for this field, and a projection of it is the same \
         field; `get process 1` printed {:?} and `select cpu` printed {cell:?}",
        last_row(&whole)
    );
}

#[test]
fn should_leave_the_serialisation_of_a_projected_field_untouched() {
    // The rounding above is the human rendering only (spec §33.5): `to json` keeps every digit.
    let run = Shell::new()
        .args(["-c", "get process 1 | select cpu | to json"])
        .run();
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("\"cpu\":"),
        "the projected field keeps its name: {text:?}"
    );
    assert!(
        !text.contains('%'),
        "a serialisation carries the number, never the rendering: {text:?}"
    );
}
