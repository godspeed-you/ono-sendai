//! `explain` as a value: the plan of spec §42 is one `ono.execution-plan/1` record, and a
//! delimited subject makes `explain` an ordinary producer whose plan flows into the stages after
//! it (issue #173, ADR-0942). The unquoted form of spec §11.3 keeps meaning the whole line.

use ono_testkit::ono;

#[test]
fn should_serialise_the_plan_when_a_quoted_subject_is_piped_into_to_json() {
    let run = ono(r#"explain "get process | sort pid" | to json"#);
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.trim_start().starts_with('['),
        "the plan reaches `to json` as a value, so the output is JSON rather than a rendering, \
         got {text:?}"
    );
    assert!(
        text.contains(r#""subject":"get process | sort pid""#),
        "the plan names what it was asked about, got {text:?}"
    );
    assert!(
        text.contains(r#""command":"ono.process.get""#)
            && text.contains(r#""command":"ono.data.sort""#),
        "both stages of the subject are planned and nothing of `to json`, got {text:?}"
    );
    assert!(
        !text.contains(r#""command":"ono.data.to""#),
        "`to json` stands after `explain`, not inside its subject, got {text:?}"
    );
}

#[test]
fn should_let_a_stage_read_the_plan_when_the_subject_is_a_block() {
    let run = ono("explain { get process | sort pid } | select kind mutating | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"kind":"pipeline","mutating":false}]"#,
        "a braced subject is delimited, so `select` reads the plan's own fields"
    );
}

#[test]
fn should_bind_the_plan_to_a_variable_when_explain_is_parenthesised() {
    let run =
        ono(r#"let p = (explain "get process | sort pid | take 3"); $p.stages | count | to json"#);
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[3]",
        "the parenthesised plan is a value like any other"
    );
}

#[test]
fn should_render_the_plan_when_a_quoted_subject_stands_alone() {
    let run = ono(r#"explain "get process | sort pid""#);
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.starts_with("PIPELINE\n1. get process\n   command      ono.process.get"),
        "at the end of a pipeline the plan renders as spec §42.1 lays it out, got {text:?}"
    );
}

#[test]
fn should_explain_the_whole_line_when_the_subject_is_written_without_quotes() {
    let run = ono("explain get process | sort pid | to json");
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("3. to json") && text.contains("ono.data.to"),
        "spec §11.3: the unquoted subject is the rest of the pipeline, got {text:?}"
    );
}

#[test]
fn should_carry_an_expanded_alias_in_the_plan_value() {
    let run = ono(r#"alias procs = get process; explain "procs | take 1" | to json"#);
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains(r#""aliases":[{"name":"procs","expansion":"get process"}]"#),
        "an alias expanded on the way is part of the plan, got {text:?}"
    );
    assert!(
        text.contains(r#""source":"get process | take 1""#),
        "the expansion is what was planned, got {text:?}"
    );
}

#[test]
fn should_carry_a_user_function_in_the_plan_value() {
    let run = ono(r#"fn mine() { where pid > 0 }; explain "get process | mine" | to json"#);
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains(r#""resolution":"function""#),
        "the stage that calls a user function says so, got {text:?}"
    );
    assert!(
        text.contains("`mine` is a user function declared at"),
        "the resolution sentence the rendering prints is in the value, got {text:?}"
    );
}

#[test]
fn should_render_the_same_resolution_sentences_it_carries() {
    let run = ono(r#"fn mine() { where pid > 0 }; explain "get process | mine""#);
    run.assert_success();
    assert!(
        run.stdout()
            .contains("`mine` is a user function declared at"),
        "got {:?}",
        run.stdout()
    );
}

// --- prefix assignments (spec §54, issue #223, ADR-0943) ---------------------------------------

#[test]
fn should_plan_the_command_after_a_prefix_assignment_when_explaining_it() {
    let run = ono("explain FOO=1 get process");
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("command      ono.process.get"),
        "the stage is `get process` run with FOO set, exactly as execution strips it, got {text:?}"
    );
    assert!(
        !text.contains("is not a native command"),
        "`FOO=1` is an assignment, never a program to look up, got {text:?}"
    );
    assert!(
        text.contains("environment  FOO=1"),
        "the plan states the environment the stages would run with, got {text:?}"
    );
}

#[test]
fn should_carry_the_environment_of_a_prefix_assignment_in_the_plan_value() {
    let run = ono(r#"explain "FOO=1 BAR=two ls -l" | to json"#);
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains(r#""environment":[{"name":"FOO","value":"1"},{"name":"BAR","value":"two"}]"#),
        "every assignment, in order, with its value, got {text:?}"
    );
    assert!(
        text.contains(r#""resolution":"external","head":"ls""#),
        "the external stage is `ls`, not `FOO=1`, got {text:?}"
    );
}

#[test]
fn should_evaluate_a_prefix_assignment_value_as_execution_does() {
    let run = ono(r#"let who = "me"; explain { GREETING=$who get process } | to json"#);
    run.assert_success();
    assert!(
        run.stdout()
            .contains(r#""environment":[{"name":"GREETING","value":"me"}]"#),
        "the value is expanded exactly as running the line would expand it, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_neutralise_control_characters_in_an_environment_value_when_rendering_the_plan() {
    let run = ono(r#"let t = "a\u{1b}]0;pwn\u{7}b"; explain { T=$t ls }"#);
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("environment  T=a"),
        "the variable is stated, got {text:?}"
    );
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}'),
        "a value must never drive the terminal it is shown on (ADR-0015 T1), got {text:?}"
    );
}
