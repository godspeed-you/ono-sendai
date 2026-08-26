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
