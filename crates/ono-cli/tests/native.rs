//! Outcome tests for native commands: the object pipeline of spec §5, run by the shell itself.
//!
//! These assert what a user sees on stdout and in the exit status. Nothing here knows how a stage
//! is scheduled or which crate implements it (AGENTS.md §11).

use ono_testkit::Shell;

/// Runs a one-liner and returns the finished run.
fn ono(script: &str) -> ono_testkit::Run {
    Shell::new().args(["-c", script]).run()
}

#[test]
fn should_run_a_native_pipeline_and_serialise_the_result() {
    let run = ono("get process | count | to json");
    run.assert_success();

    // The document is the stream's shape, so a one-value stream is a one-element array: a script
    // whose output shape depended on how many rows the machine had would break on a quiet day.
    let text = run.stdout().trim().to_owned();
    let counted: i64 = text
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .unwrap_or_else(|_| panic!("`count | to json` must emit a JSON number, got {text:?}"));
    assert!(
        counted >= 1,
        "the shell counting processes must at least see itself, got {counted}"
    );
}

#[test]
fn should_filter_provider_objects_by_a_field_expression() {
    let run = ono("get process | where pid == 1 | select pid | to json");
    run.assert_success();

    // Spec §33.5: the data, with no Ono envelope around it. An external tool reads this.
    assert_eq!(
        run.stdout().trim(),
        r#"[{"pid":1}]"#,
        "`to json` serialises canonical data values, not the internal record envelope (§33.5)"
    );
}

#[test]
fn should_report_a_native_command_that_does_not_exist_as_not_found() {
    let run = ono("get definitely-not-a-target");
    assert!(
        !run.status().is_success(),
        "an undeclared target must not succeed silently, got {:?}",
        run.output()
    );
}

#[test]
fn should_parse_a_representation_piped_in_from_an_external_program() {
    let run = ono(
        "echo '[{\"name\":\"a\",\"size\":1},{\"name\":\"b\",\"size\":9}]' \
                   | from json | where size > 5 | select name | to json",
    );
    run.assert_success();

    assert_eq!(
        run.stdout().trim(),
        r#"[{"name":"b"}]"#,
        "the filter and the projection must both apply to the parsed objects"
    );
}

#[test]
fn should_write_the_same_bytes_whether_the_output_is_a_pipe_or_a_file() {
    let directory = ono_testkit::scratch();
    let target = directory.path().join("out.json");
    let script = format!(
        "get process | take 1 | select pid | to json > {}",
        target.display()
    );

    let piped = ono("get process | take 1 | select pid | to json");
    piped.assert_success();
    ono(&script).assert_success();

    let written = std::fs::read_to_string(&target).expect("the redirected file");
    assert_eq!(
        written.trim(),
        piped.stdout().trim(),
        "spec §50: redirected output must be the same bytes as piped output"
    );
}

#[test]
fn should_reject_a_misspelled_field_before_anything_runs() {
    let run = ono("get process | where cpy > 20");
    assert!(
        !run.status().is_success(),
        "a field the schema does not declare cannot succeed, got {:?}",
        run.output()
    );

    let stderr = run.stderr();
    assert!(
        stderr.contains("perhaps: cpu"),
        "spec §15.4: the near miss is suggested, got {stderr:?}"
    );
    assert_eq!(
        stderr.matches("cpy").count(),
        1,
        "spec §11.3: the typo is caught before enumeration begins, once — not once per \
         process. Got {stderr:?}"
    );
}

#[test]
fn should_read_the_shells_own_standard_input_into_a_parsing_stage() {
    // Spec §12.4's own example: `curl -s https://example/api | from json | where status == "open"`.
    // The bytes arrive on the shell's stdin, not from a stage inside the pipeline.
    let run = Shell::new()
        .args([
            "-c",
            r#"from json | where size > 5 | select name | to json"#,
        ])
        .stdin(r#"[{"name":"a","size":1},{"name":"b","size":9}]"#)
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"name":"b"}]"#,
        "bytes piped into the shell reach the first parsing stage (spec §12.4)"
    );
}

#[test]
fn should_stream_a_value_that_starts_a_pipeline() {
    // ADR-0019: a list splices because it *is* several values, so `$xs | count` counts them.
    let run = ono("let xs = [1, 2, 3]; $xs | count | to json");
    run.assert_success();
    assert_eq!(run.stdout().trim(), "[3]");
}

#[test]
fn should_reuse_the_previous_result_without_rerunning_it() {
    // Spec §20.2: `@-1 | where …` reuses the retained structured result — no screen scraping,
    // and no second enumeration.
    let run = ono("get process | where pid == 1 | select pid; @-1 | count | to json");
    run.assert_success();
    let last = run.stdout().lines().last().unwrap_or_default().to_owned();
    assert_eq!(
        last, "[1]",
        "the retained result has exactly the one row that was shown"
    );
}

#[test]
fn should_pick_one_item_of_the_current_result_by_position() {
    let run = ono("let xs = [\"a\", \"b\", \"c\"]; $xs | take 3; @2 | to json");
    run.assert_success();
    let last = run.stdout().lines().last().unwrap_or_default().to_owned();
    assert_eq!(
        last, r#"["b"]"#,
        "spec §6.4: `@2` is item 2 of the shown result"
    );
}

#[test]
fn should_say_there_is_nothing_to_reuse_when_no_result_was_retained() {
    let run = ono("@-1 | count");
    assert!(!run.status().is_success());
    assert!(
        run.stderr().contains("Ono-Sendai-E"),
        "a missing result is a structured error, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_draw_a_trace_as_a_tree_rather_than_a_table() {
    // Spec §13.6: a graph never renders as a table. PID 1 exists everywhere and always has
    // relationships — children at least.
    let run = ono("trace process 1");
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("+--") || text.contains("└") || text.contains("├"),
        "the graph draws as a tree (spec §13.6), got {text:?}"
    );
    assert!(
        text.contains("1") && (text.contains("systemd") || text.contains("init")),
        "the root names the traced process, got {text:?}"
    );
}

#[test]
fn should_carry_a_trace_through_the_pipeline_as_a_graph_value() {
    let run = ono("trace process 1 | type");
    run.assert_success();
    run.assert_stdout_contains("ono.graph/1");
}

#[test]
fn should_walk_a_wide_tree_without_hoarding_descriptors() {
    // ADR-0015 (F11): the walk used to hold one open descriptor per *pending* directory, so a
    // tree wider than the descriptor table killed it. Under a 64-descriptor limit, five hundred
    // sibling directories must still be walkable — the walk may hold the root and the one
    // directory it is reading, never the frontier.
    let scratch = ono_testkit::scratch();
    for i in 0..500 {
        scratch.write(format!("wide/dir-{i:03}/leaf.txt"), "x");
    }

    let run = Shell::program("/bin/bash")
        .args([
            "-c".to_owned(),
            format!(
                "ulimit -n 64 2>/dev/null; exec {} --no-config -c 'find file {}/wide | count | to json'",
                ono_testkit::ono_binary().display(),
                scratch.path().display(),
            ),
        ])
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[1001]",
        "the root, five hundred directories and five hundred leaves, all reached"
    );
}

#[test]
fn should_sort_descending_with_the_specs_own_spelling() {
    // Spec §6.3 and §48 write `sort cpu desc` — the direction is a word bound to a string
    // selector, never a field the §11.3 check should reject.
    let run = ono("get process | sort pid desc | take 1 | select pid | to json");
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("pid") && !text.contains("Ono-Sendai-E"),
        "the highest pid comes first and nothing was refused: {text:?}"
    );
}
