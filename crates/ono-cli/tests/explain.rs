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
