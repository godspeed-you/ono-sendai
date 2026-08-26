//! Error recovery, spans and the incomplete/syntax distinction (ADR-0009 "Recovery").

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_core::ErrorCode;
use ono_parser::{Parsed, parse};

fn codes(parsed: &Parsed) -> Vec<ErrorCode> {
    parsed
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect()
}

fn assert_incomplete(source: &str) {
    let parsed = parse(source);
    assert!(
        !parsed.diagnostics().is_empty(),
        "{source:?} should report that it is unfinished"
    );
    assert!(
        codes(&parsed)
            .iter()
            .all(|code| *code == ErrorCode::ParseIncomplete),
        "{source:?} should only be incomplete, got {:?}",
        parsed.diagnostics()
    );
    assert!(!parsed.is_complete(), "{source:?} is not complete");
    assert!(
        !parsed.has_errors(),
        "{source:?} is unfinished, not wrong: {:?}",
        parsed.diagnostics()
    );
}

fn assert_syntax_error(source: &str) {
    let parsed = parse(source);
    assert!(
        codes(&parsed).contains(&ErrorCode::ParseSyntax),
        "{source:?} should be a syntax error, got {:?}",
        parsed.diagnostics()
    );
    assert!(parsed.has_errors(), "{source:?} has errors");
}

#[test]
fn should_report_incomplete_when_a_string_is_not_terminated() {
    assert_incomplete(r#"git commit -m "wip"#);
    assert_incomplete("echo 'raw");
}

#[test]
fn should_report_incomplete_when_a_delimiter_is_still_open() {
    assert_incomplete("echo (get process");
    assert_incomplete("where a in [1, 2");
    assert_incomplete("each { restart service @");
    assert_incomplete("each {a: 1");
}

#[test]
fn should_report_incomplete_when_the_line_ends_on_an_operator() {
    assert_incomplete("get process |");
    assert_incomplete("get process | where cpu >");
    assert_incomplete("ls &&");
    assert_incomplete("ls ||");
    assert_incomplete("where a and");
    assert_incomplete("where a +");
}

#[test]
fn should_report_incomplete_when_a_statement_keyword_has_no_operand_yet() {
    assert_incomplete("let");
    assert_incomplete("let x");
    assert_incomplete("let x =");
    assert_incomplete("if");
    assert_incomplete("if a > 1");
    assert_incomplete("for");
    assert_incomplete("for p");
    assert_incomplete("for p in");
    assert_incomplete("fn f");
    assert_incomplete("fn f()");
    assert_incomplete("try");
    assert_incomplete("match a");
}

#[test]
fn should_report_incomplete_when_a_redirection_has_no_target_yet() {
    assert_incomplete("cat a.txt >");
    assert_incomplete("cat a.txt >>");
    assert_incomplete("cmd 2>&");
}

#[test]
fn should_point_the_incomplete_diagnostic_at_the_construct_that_is_open() {
    let source = "echo (get process";
    let parsed = parse(source);
    let diagnostic = &parsed.diagnostics()[0];
    assert_eq!(diagnostic.code(), ErrorCode::ParseIncomplete);
    assert_eq!(
        diagnostic.span().of(source),
        "(",
        "the diagnostic points at the delimiter still waiting to be closed"
    );
}

#[test]
fn should_point_the_incomplete_diagnostic_at_the_string_that_is_open() {
    let source = r#"echo "wip"#;
    let parsed = parse(source);
    assert_eq!(parsed.diagnostics()[0].span().of(source), r#""wip"#);
}

#[test]
fn should_report_a_syntax_error_when_a_words_mode_construct_appears_in_expression_mode() {
    let source = "get process | count > out.txt";
    let parsed = parse(source);
    assert_syntax_error(source);
    let diagnostic = parsed
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == ErrorCode::ParseSyntax)
        .expect("a syntax diagnostic");
    assert_eq!(diagnostic.span().of(source), ">");
    let help = diagnostic.help().expect("a help text for the redirection");
    assert!(
        help.contains("to text"),
        "the help must point at `| to text > file`, got {help:?}"
    );
}

#[test]
fn should_report_a_syntax_error_when_a_closing_delimiter_has_no_opener() {
    assert_syntax_error("echo ) pwd");
    assert_syntax_error("echo ] pwd");
}

#[test]
fn should_report_a_syntax_error_when_a_stage_has_no_head() {
    assert_syntax_error("| ls");
    assert_syntax_error("get process | | count");
}

#[test]
fn should_report_a_syntax_error_when_a_binding_has_no_name() {
    assert_syntax_error("let 5 = 3");
}

#[test]
fn should_recover_at_the_next_statement_when_one_statement_is_wrong() {
    let source = "echo ) oops\nls -la";
    let parsed = parse(source);
    assert!(parsed.has_errors());
    assert_eq!(
        parsed.program().statements.len(),
        2,
        "the second line must still parse: {:?}",
        parsed.program()
    );
}

#[test]
fn should_recover_at_the_next_stage_when_one_stage_is_wrong() {
    let parsed = parse("get process | | where cpu > 1 | count");
    assert!(parsed.has_errors());
    let ono_parser::Statement::Pipeline(pipeline) = &parsed.program().statements[0] else {
        panic!("expected a pipeline");
    };
    assert!(
        pipeline.head.stages.len() >= 3,
        "the stages after the error must still be parsed, got {:?}",
        pipeline.head.stages.len()
    );
}

#[test]
fn should_report_every_diagnostic_with_a_span_inside_the_source() {
    for source in [
        "echo ) ] } pwd",
        "let 5 = ) 3",
        "((((((((((",
        "where a > > > >",
        "$",
        "@@@",
        "\"\\q\"",
    ] {
        let parsed = parse(source);
        for diagnostic in parsed.diagnostics() {
            assert!(
                diagnostic.span().end() as usize <= source.len(),
                "{source:?} produced an out-of-range span {:?}",
                diagnostic.span()
            );
            assert!(
                !diagnostic.message().is_empty(),
                "{source:?} produced an empty message"
            );
        }
    }
}

#[test]
fn should_return_a_tree_without_panicking_for_every_hostile_input() {
    for source in [
        "",
        " ",
        "\n\n\n",
        "#",
        "|||",
        "&&&&",
        ";;;;",
        "((((",
        "))))",
        "[[[[",
        "]]]]",
        "{{{{",
        "}}}}",
        "\"",
        "'",
        "/",
        "$",
        "$$$",
        "@",
        "@-",
        "@-x",
        "0x",
        "0b",
        "1.",
        "1e",
        "where",
        "where where where",
        "let let let",
        "fn fn fn",
        "match match match",
        "if if if",
        "try catch catch",
        "use",
        "return return",
        "a > > b",
        "\u{1F600} \u{4E2D}\u{6587}",
        "echo \"a\\u{110000}\"",
        "echo \"a\\x\"",
        "cmd 9999999999999999999>x",
        "take 99999999999999999999999",
    ] {
        let parsed = parse(source);
        let _ = parsed.program();
        let _ = parsed.is_complete();
        let _ = parsed.has_errors();
        for diagnostic in parsed.diagnostics() {
            assert!(
                diagnostic.span().end() as usize <= source.len(),
                "{source:?}"
            );
        }
    }
}

#[test]
fn should_never_report_a_syntax_error_when_the_input_is_only_a_comment() {
    let parsed = parse("# nothing to see");
    assert!(parsed.diagnostics().is_empty());
    assert!(parsed.is_complete());
    assert!(parsed.program().statements.is_empty());
}
