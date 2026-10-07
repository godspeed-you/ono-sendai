//! `explain` runs nothing (spec §15.3, ADR-0942, ADR-0939): not the subject, not a prefix
//! assignment's value, not an argument written as a capture, and not text that a glob match or a
//! variable smuggles in. Every test here plants a command that would create a marker file if any
//! part of it ran, and asserts the marker is absent afterwards — the filesystem is the outcome a
//! user would see.

use ono_testkit::{Scratch, scratch};

mod support;

/// The file every planted command would create.
const MARKER: &str = "pwn";

/// Runs `script` in `directory`, with the session's state kept inside it.
fn explain_in(directory: &Scratch, script: &str) -> ono_testkit::Run {
    support::isolated(directory)
        .cwd(directory.path())
        .args(["-c", script])
        .run()
}

fn assert_ran_nothing(directory: &Scratch, script: &str, run: &ono_testkit::Run) {
    assert!(
        !directory.exists(MARKER),
        "spec §15.3: `explain` runs no part of what it explains, yet `{script}` created the \
         marker file; stdout {:?}, stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_run_nothing_in_a_prefix_assignment_value_when_explaining_it() {
    for script in [
        r#"explain { X="$(touch pwn)" ls }"#,
        "explain { X=(touch pwn) ls }",
        r#"explain X="$(touch pwn)" ls"#,
        "explain X=(touch pwn) get process | count",
    ] {
        let directory = scratch();
        let run = explain_in(&directory, script);
        assert_ran_nothing(&directory, script, &run);
    }
}

#[test]
fn should_state_a_prefix_assignment_value_as_written_when_it_would_run_code() {
    let directory = scratch();
    let script = r#"explain { X="$(touch pwn)" ls } | to json"#;
    let run = explain_in(&directory, script);
    assert_ran_nothing(&directory, script, &run);
    run.assert_success();
    assert!(
        run.stdout()
            .contains(r#""environment":[{"name":"X","value":"\"$(touch pwn)\""}]"#),
        "a value explain may not evaluate is shown as its source text, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_run_nothing_an_argument_capture_spells_when_explaining_a_single_stage() {
    for script in [
        "explain ls (touch pwn)",
        r#"explain ls "$(touch pwn)""#,
        "explain ls $(touch pwn)",
    ] {
        let directory = scratch();
        let run = explain_in(&directory, script);
        assert_ran_nothing(&directory, script, &run);
    }
}

#[test]
fn should_run_nothing_a_glob_matched_filename_spells_when_explaining_a_removal() {
    let directory = scratch();
    directory.write(r#"a.tmp | X="$(touch pwn)" ls .tmp"#, "crafted");
    directory.write("b.tmp", "b");
    let script = "explain remove file *.tmp";
    let run = explain_in(&directory, script);
    assert_ran_nothing(&directory, script, &run);
    run.assert_success();
    assert!(
        directory.exists(r#"a.tmp | X="$(touch pwn)" ls .tmp"#) && directory.exists("b.tmp"),
        "explaining a removal removes nothing"
    );
    let text = run.stdout();
    assert!(
        text.contains("command      ono.file.remove") && !text.contains("2. "),
        "a filename is one target of the one stage, never a second stage of the plan, got {text:?}"
    );
}

#[test]
fn should_run_nothing_a_variable_subject_spells_when_explaining_it() {
    let directory = scratch();
    // Single quotes interpolate nothing, so the text reaches `explain` as written.
    let script =
        r#"let line = 'get process | X="$(touch pwn)" ls | Y=(touch pwn) wc'; explain $line"#;
    let run = explain_in(&directory, script);
    assert_ran_nothing(&directory, script, &run);
    run.assert_success();
}

#[test]
fn should_refuse_a_quoted_subject_whose_interpolation_would_run_a_pipeline() {
    let directory = scratch();
    let script = r#"explain "ls $(touch pwn)""#;
    let run = explain_in(&directory, script);
    assert_ran_nothing(&directory, script, &run);
    assert!(
        !run.status().is_success(),
        "a subject that cannot be built without running something is refused"
    );
    assert!(
        run.stderr().contains("safety.policy_denied") || run.stderr().contains("E0702"),
        "the refusal is the structured `safety.policy_denied`, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_still_interpolate_a_variable_into_a_quoted_subject() {
    let directory = scratch();
    let run = explain_in(
        &directory,
        r#"let n = 1; explain "get process | where pid == $n" | to json"#,
    );
    run.assert_success();
    assert!(
        run.stdout()
            .contains(r#""subject":"get process | where pid == 1""#),
        "a variable reads nothing but the session, so a quoted subject still interpolates it, \
         got {:?}",
        run.stdout()
    );
}
