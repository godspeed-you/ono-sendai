//! Words-mode command lines: arguments, options, redirection, chaining (ADR-0009, spec §6.1).

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_parser::{
    ArgMode, Argument, ChainOp, Expr, Pipeline, RedirectOp, RedirectTarget, Stage, Statement, parse,
};

fn pipeline(source: &str) -> Pipeline {
    let parsed = parse(source);
    assert!(
        parsed.diagnostics().is_empty(),
        "unexpected diagnostics for {source:?}: {:?}",
        parsed.diagnostics()
    );
    assert!(parsed.is_complete(), "{source:?} must parse as complete");
    assert!(!parsed.has_errors(), "{source:?} must parse without errors");
    let Some(Statement::Pipeline(pipeline)) = parsed.program().statements.first().cloned() else {
        panic!("expected a pipeline statement in {source:?}");
    };
    pipeline
}

fn stages(source: &str) -> Vec<Stage> {
    pipeline(source).head.stages
}

fn words(stage: &Stage) -> Vec<&str> {
    stage
        .arguments
        .iter()
        .filter_map(|argument| argument.as_word())
        .collect()
}

#[test]
fn should_parse_a_command_with_flags_when_the_head_is_an_external_command() {
    let stages = stages("ls -la");
    assert_eq!(stages.len(), 1);
    assert_eq!(stages[0].head.name(), Some("ls"));
    assert_eq!(stages[0].mode, ArgMode::Words);
    assert_eq!(words(&stages[0]), vec!["-la"]);
}

#[test]
fn should_keep_a_quoted_argument_as_one_value_when_it_contains_spaces() {
    let stages = stages(r#"git commit -m "wip: still typing""#);
    assert_eq!(stages[0].head.name(), Some("git"));
    assert_eq!(words(&stages[0]), vec!["commit", "-m"]);
    let Some(Argument::Value(Expr::Str(literal))) = stages[0].arguments.last() else {
        panic!("expected a string argument, got {:?}", stages[0].arguments);
    };
    assert_eq!(literal.literal_text(), Some("wip: still typing"));
}

#[test]
fn should_parse_a_selector_and_a_long_option_when_a_native_command_is_invoked() {
    let stages = stages("get process 4419 --tree");
    assert_eq!(stages[0].head.name(), Some("get"));
    assert_eq!(words(&stages[0]), vec!["process", "4419"]);
    let Some(Argument::Option(option)) = stages[0].arguments.last() else {
        panic!("expected a long option, got {:?}", stages[0].arguments);
    };
    assert_eq!(option.name, "tree");
    assert!(option.value.is_none());
}

#[test]
fn should_attach_the_value_to_a_long_option_when_it_is_written_with_an_equals_sign() {
    let stages = stages("get process --user=root");
    let Some(Argument::Option(option)) = stages[0].arguments.last() else {
        panic!("expected a long option, got {:?}", stages[0].arguments);
    };
    assert_eq!(option.name, "user");
    let Some(Expr::Str(literal)) = &option.value else {
        panic!("expected the option value, got {:?}", option.value);
    };
    assert_eq!(literal.literal_text(), Some("root"));
}

#[test]
fn should_parse_a_redirection_when_a_words_mode_stage_writes_to_a_file() {
    let stages = stages("cat a.txt > out.txt");
    assert_eq!(words(&stages[0]), vec!["a.txt"]);
    assert_eq!(stages[0].redirections.len(), 1);
    let redirection = &stages[0].redirections[0];
    assert_eq!(redirection.fd, None);
    assert_eq!(redirection.op, RedirectOp::Write);
    let RedirectTarget::Word(word) = &redirection.target else {
        panic!("expected a word target, got {:?}", redirection.target);
    };
    assert_eq!(word.text, "out.txt");
}

#[test]
fn should_parse_an_appending_redirection_when_the_operator_is_doubled() {
    let stages = stages("cat a.txt >> out.txt");
    assert_eq!(stages[0].redirections[0].op, RedirectOp::Append);
}

#[test]
fn should_parse_an_input_redirection_when_the_operator_points_left() {
    let stages = stages("wc -l < in.txt");
    assert_eq!(stages[0].redirections[0].op, RedirectOp::Read);
}

#[test]
fn should_parse_a_descriptor_duplication_when_stderr_is_merged_into_stdout() {
    let stages = stages("cmd 2>&1 | grep x");
    assert_eq!(stages.len(), 2);
    let redirection = &stages[0].redirections[0];
    assert_eq!(redirection.fd, Some(2));
    assert_eq!(redirection.op, RedirectOp::DupWrite);
    assert_eq!(redirection.target, RedirectTarget::Fd(1));
    assert_eq!(stages[1].head.name(), Some("grep"));
    assert_eq!(words(&stages[1]), vec!["x"]);
}

#[test]
fn should_record_the_file_descriptor_when_a_redirection_is_prefixed_with_one() {
    let stages = stages("cmd 2> err.log");
    assert_eq!(stages[0].redirections[0].fd, Some(2));
    assert_eq!(stages[0].redirections[0].op, RedirectOp::Write);
}

#[test]
fn should_mark_a_pipeline_as_backgrounded_when_it_ends_with_an_ampersand() {
    let pipeline = pipeline("sleep 5 &");
    assert!(pipeline.background);
    assert_eq!(words(&pipeline.head.stages[0]), vec!["5"]);
}

#[test]
fn should_chain_on_exit_status_when_the_line_uses_and_or_operators() {
    let pipeline = pipeline("a && b || c");
    assert_eq!(pipeline.head.stages[0].head.name(), Some("a"));
    assert_eq!(pipeline.tail.len(), 2);
    assert_eq!(pipeline.tail[0].op, ChainOp::And);
    assert_eq!(pipeline.tail[0].list.stages[0].head.name(), Some("b"));
    assert_eq!(pipeline.tail[1].op, ChainOp::Or);
    assert_eq!(pipeline.tail[1].list.stages[0].head.name(), Some("c"));
}

#[test]
fn should_nest_a_pipeline_as_a_value_when_an_argument_is_parenthesised() {
    let stages = stages("echo (get process | count)");
    let Some(Argument::Value(Expr::Paren(paren))) = stages[0].arguments.first() else {
        panic!(
            "expected a parenthesised argument, got {:?}",
            stages[0].arguments
        );
    };
    let inner = paren.pipeline().expect("a nested pipeline");
    assert_eq!(inner.head.stages.len(), 2);
    assert_eq!(inner.head.stages[0].head.name(), Some("get"));
    assert_eq!(inner.head.stages[1].head.name(), Some("count"));
}

#[test]
fn should_parse_several_statements_when_they_are_separated_by_semicolons_or_newlines() {
    let parsed = parse("ls -la; pwd\ncat a.txt\n");
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    assert_eq!(parsed.program().statements.len(), 3);
}

#[test]
fn should_keep_the_exact_source_text_of_every_word_so_argv_is_byte_exact() {
    let source = "run --recursive ./a b-c *.tmp";
    let stages = stages(source);
    let texts: Vec<&str> = stages[0]
        .arguments
        .iter()
        .map(|argument| argument.span().of(source))
        .collect();
    assert_eq!(texts, vec!["--recursive", "./a", "b-c", "*.tmp"]);
}

#[test]
fn should_use_a_variable_as_a_stage_head_when_the_line_starts_with_one() {
    let stages = stages("$hot | count");
    let Some(variable) = stages[0].head.variable() else {
        panic!("expected a variable head, got {:?}", stages[0].head);
    };
    assert_eq!(variable.name, "hot");
}

#[test]
fn should_read_the_predicate_as_an_expression_when_a_words_mode_find_is_given_where() {
    // ADR-0138 with v0.4 §6.8: `find place --where <predicate>` is written on a words-mode line,
    // and the predicate is an expression there — otherwise `state` would be a word and `"running"`
    // a second one, and the shell would have no predicate to evaluate.
    let stages = stages(r#"find place --where state == "running" | take 5"#);
    assert_eq!(stages[0].head.name(), Some("find"));
    assert_eq!(words(&stages[0]), vec!["place"]);
    let Some(Argument::Option(option)) = stages[0].arguments.last() else {
        panic!("expected `--where`, got {:?}", stages[0].arguments);
    };
    assert_eq!(option.name, "where");
    assert!(
        matches!(option.value, Some(Expr::Binary(_))),
        "the predicate is one expression, got {:?}",
        option.value
    );
    assert_eq!(stages.len(), 2, "the pipe still ends the stage");
}

#[test]
fn should_compare_rather_than_redirect_when_a_predicate_option_contains_a_greater_than() {
    // The reason ADR-0138 exists: in words mode `>` opens a redirection, so `--where pid > 1`
    // would write the stream to a file called `1` instead of comparing.
    let stages = stages("find place --where pid > 1");
    assert!(
        stages[0].redirections.is_empty(),
        "`>` inside a predicate compares, got {:?}",
        stages[0].redirections
    );
    let Some(Argument::Option(option)) = stages[0].arguments.last() else {
        panic!("expected `--where`, got {:?}", stages[0].arguments);
    };
    assert!(
        matches!(option.value, Some(Expr::Binary(_))),
        "`pid > 1` is one comparison, got {:?}",
        option.value
    );
}

#[test]
fn should_leave_an_unrelated_option_a_bare_flag_when_its_head_declares_no_predicate() {
    // The table is per head and per option (ADR-0138): nothing else changes meaning, and an
    // external program still receives the word that follows its own `--where`.
    let stages = stages("grep --where state");
    let Some(Argument::Option(option)) = stages[0].arguments.first() else {
        panic!("expected `--where`, got {:?}", stages[0].arguments);
    };
    assert!(option.value.is_none());
    assert_eq!(words(&stages[0]), vec!["state"]);
}
