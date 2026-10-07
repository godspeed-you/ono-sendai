//! `ono --complete <line> [--cursor N]`: the completion the prompt offers, without a terminal
//! (issue #176, ADR-0945). An editor or a tool asks with a line and a cursor and reads one
//! `ono.completion/1` document as JSON: the candidates with their kind and doc, the span they
//! replace, and whether the set is whole (ADR-0944).

use ono_testkit::{Shell, scratch};

fn complete(arguments: &[&str]) -> ono_testkit::Run {
    let mut words = vec!["--no-config", "--complete"];
    words.extend_from_slice(arguments);
    Shell::new().args(words).run()
}

#[test]
fn should_answer_a_completion_as_one_json_document_without_a_terminal() {
    let run = complete(&["get pro"]);
    run.assert_success();
    let text = run.stdout();
    assert_eq!(
        text.lines().count(),
        1,
        "one document on one line, for a reader to parse, got {text:?}"
    );
    assert!(
        text.starts_with(r#"{"line":"get pro","cursor":7,"start":4,"end":7,"complete":true,"#),
        "the line, the cursor, the span the candidates replace and whether the set is whole, \
         got {text:?}"
    );
    assert!(
        text.contains(r#"{"text":"process","kind":"target","doc":"#),
        "each candidate carries its text, its kind and its doc, got {text:?}"
    );
}

#[test]
fn should_complete_at_the_cursor_when_one_is_given() {
    let run = complete(&["get pro | count", "--cursor", "7"]);
    run.assert_success();
    assert!(
        run.stdout().contains(r#""text":"process""#),
        "the word before the cursor is the one completed, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_name_a_shell_builtin_and_an_option_by_their_kinds() {
    let builtin = complete(&["c"]);
    builtin.assert_success();
    assert!(
        builtin
            .stdout()
            .contains(r#"{"text":"cd","kind":"builtin""#),
        "got {:?}",
        builtin.stdout()
    );
    let option = complete(&["get process --us"]);
    option.assert_success();
    assert!(
        option
            .stdout()
            .contains(r#"{"text":"--user","kind":"option""#),
        "got {:?}",
        option.stdout()
    );
}

#[test]
fn should_offer_provider_values_through_the_same_completer_the_prompt_uses() {
    // The budgets are widened so the cold first read is inside them; what is offered is then the
    // providers' answer, `root` among the accounts every Linux host has.
    let directory = scratch();
    directory.write(
        "ono/config.ono",
        "set config limits.completion_soft_ms = 10000\nset config limits.completion_hard_ms = 10000\n",
    );
    let run = Shell::new()
        .args(["--complete", "get user roo"])
        .env("XDG_CONFIG_HOME", directory.path().display().to_string())
        .run();
    run.assert_success();
    assert!(
        run.stdout().contains(r#"{"text":"root","kind":"value""#)
            && run.stdout().contains(r#""complete":true"#),
        "got {:?}",
        run.stdout()
    );
}

#[test]
fn should_say_the_set_is_incomplete_when_it_was_cut_short() {
    // `lib` begins more package names than one completion offers (ADR-0944).
    let run = complete(&["get package lib"]);
    run.assert_success();
    assert!(
        run.stdout().contains(r#""complete":false"#),
        "got {:?}",
        run.stdout()
    );
}

#[test]
fn should_refuse_a_cursor_outside_the_line_with_the_usage_status() {
    complete(&["get pro", "--cursor", "99"]).assert_status(2);
    complete(&[]).assert_status(2);
}

// --- functions and aliases (issue #223, part 2) -------------------------------------------------

fn with_definitions(line: &str) -> ono_testkit::Run {
    let directory = scratch();
    directory.write(
        "ono/config.ono",
        "fn myfancyfn(limit) { where pid > $limit }\nalias procfancy = get process | sort pid\n",
    );
    Shell::new()
        .args(["--complete", line])
        .env("XDG_CONFIG_HOME", directory.path().display().to_string())
        .run()
}

#[test]
fn should_offer_a_user_function_with_its_signature_at_the_head_of_a_line() {
    let run = with_definitions("myfan");
    run.assert_success();
    assert!(
        run.stdout()
            .contains(r#"{"text":"myfancyfn","kind":"function","doc":"fn myfancyfn(limit)"}"#),
        "got {:?}",
        run.stdout()
    );
}

#[test]
fn should_offer_an_alias_with_its_expansion_at_the_head_of_a_line() {
    let run = with_definitions("procfa");
    run.assert_success();
    assert!(
        run.stdout()
            .contains(r#"{"text":"procfancy","kind":"alias","doc":"get process | sort pid"}"#),
        "got {:?}",
        run.stdout()
    );
}

#[test]
fn should_offer_a_user_function_after_a_pipe() {
    // A function stands at any stage position (ADR-0951), so it completes at any of them.
    let run = with_definitions("get process | myfan");
    run.assert_success();
    assert!(
        run.stdout()
            .contains(r#""text":"myfancyfn","kind":"function""#),
        "got {:?}",
        run.stdout()
    );
}
