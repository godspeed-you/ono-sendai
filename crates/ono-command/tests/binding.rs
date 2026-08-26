//! Argument binding turns a parsed stage into resolved selectors and options, using the types the
//! command contract declares (ADR-0009, ADR-0012, spec §27).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared helper in a test binary states its preconditions the same way a #[test] \
              body does (AGENTS.md section 16)"
)]

use ono_command::CommandRegistry;
use ono_core::ErrorCode;
use ono_parser::{Stage, Statement};
use ono_value::{Duration, ErrorValue, Value};

fn registry() -> &'static CommandRegistry {
    CommandRegistry::embedded().expect("the embedded command contracts must parse")
}

/// The first stage of `source`, which must parse without diagnostics.
fn stage(source: &str) -> Stage {
    let parsed = ono_parser::parse(source);
    assert!(
        parsed.diagnostics().is_empty(),
        "`{source}` must parse cleanly, but produced {:?}",
        parsed.diagnostics()
    );
    let Some(Statement::Pipeline(pipeline)) = parsed.program().statements.first() else {
        panic!("`{source}` must parse as a pipeline");
    };
    pipeline
        .head
        .stages
        .first()
        .cloned()
        .expect("a pipeline has at least one stage")
}

/// Binds a whole stage: resolves the head against the registry, then binds the arguments.
fn bind(source: &str) -> Result<ono_command::BoundArguments, ErrorValue> {
    let stage = stage(source);
    let head = stage
        .head
        .name()
        .expect("the test sources all use a command head");
    let resolved = registry().resolve(head, &stage.arguments)?;
    resolved.contract.bind(resolved.arguments)
}

#[test]
fn should_bind_a_positional_selector_to_its_declared_type() {
    let bound = bind("get process 4419").expect("`get process 4419` binds");

    assert_eq!(bound.selector("pid"), Some(&Value::Int(4419)));
    assert_eq!(
        bound.option("tree"),
        None,
        "an option not given stays absent"
    );
}

#[test]
fn should_bind_a_long_option_written_with_an_equals_sign() {
    let bound = bind("kill process 4419 --signal=SIGHUP").expect("the stage binds");

    assert_eq!(bound.selector("pid"), Some(&Value::Int(4419)));
    assert_eq!(
        bound.option("signal"),
        Some(&Value::String("SIGHUP".into()))
    );
}

#[test]
fn should_bind_a_long_option_whose_value_is_the_next_word() {
    let bound = bind("watch process --every 5s").expect("the stage binds");

    assert_eq!(
        bound.option("every"),
        Some(&Value::Duration(
            Duration::parse("5s").expect("5s is a duration")
        ))
    );
}

#[test]
fn should_bind_a_bare_flag_as_true() {
    let bound = bind("get process --tree").expect("the stage binds");

    assert_eq!(bound.option("tree"), Some(&Value::Bool(true)));
    assert!(bound.flag("tree"));
    assert!(!bound.flag("user"), "an absent flag is not set");
}

#[test]
fn should_apply_a_declared_default_when_the_option_is_absent() {
    let bound = bind("kill process 4419").expect("the stage binds");

    assert_eq!(
        bound.option("signal"),
        Some(&Value::String("SIGKILL".into())),
        "`kill` declares `signal` with default SIGKILL, which is what distinguishes it from `stop`"
    );
}

#[test]
fn should_keep_an_expression_mode_argument_as_an_expression() {
    let bound = bind("where cpu > 20").expect("`where cpu > 20` binds");

    assert!(
        bound.selector("predicate").is_none(),
        "an expression-mode argument is not coerced to a value; the evaluator evaluates it"
    );
    assert!(
        bound.selector_expression("predicate").is_some(),
        "the predicate expression is handed back for evaluation"
    );
}

#[test]
fn should_bind_a_list_option_from_comma_separated_values() {
    let bound = bind("trace process 812 --relations \"parent,children\"").expect("the stage binds");

    assert_eq!(
        bound.option("relations"),
        Some(&Value::list([
            Value::String("parent".into()),
            Value::String("children".into()),
        ]))
    );
}

#[test]
fn should_reject_an_option_the_command_does_not_declare() {
    let error = bind("get process --recursive").expect_err("`--recursive` is not declared");

    assert_eq!(error.code(), ErrorCode::TypeUnknownField);
    assert!(
        error.message().contains("recursive"),
        "the message must name the offending option, was: {}",
        error.message()
    );
    let help = error.help().unwrap_or_default();
    assert!(
        help.contains("tree") || help.contains("user"),
        "the help must name the closest declared option, was: {help}"
    );
}

#[test]
fn should_reject_an_option_value_of_the_wrong_type() {
    let error =
        bind("trace process 812 --depth=deep").expect_err("`--depth` is declared as an int");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(
        error.message().contains("int"),
        "the message must name the expected type, was: {}",
        error.message()
    );
}

#[test]
fn should_reject_a_selector_that_does_not_parse_as_its_declared_type() {
    let error = bind("stop process notapid").expect_err("`pid` is declared as an int");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(
        error.message().contains("pid"),
        "the message must name the offending selector, was: {}",
        error.message()
    );
    assert!(
        error.message().contains("notapid"),
        "the message must quote what was given, was: {}",
        error.message()
    );
}

#[test]
fn should_report_a_required_selector_that_was_never_given() {
    let bound = bind("inspect process").expect("the stage binds with no selector");
    let error = bound
        .require_selector("pid")
        .expect_err("`inspect process` cannot run without a pid or a piped process");

    assert!(
        error.message().contains("pid"),
        "the message must name the missing selector, was: {}",
        error.message()
    );
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[test]
fn should_reject_a_unit_that_does_not_fit_the_declared_dimension() {
    let error = bind("watch process --every 5MiB").expect_err("a byte size is not a duration");

    assert_eq!(error.code(), ErrorCode::TypeInvalidUnit);
    assert!(
        error.message().contains("every") || error.message().contains("5MiB"),
        "the message must name the offending option or value, was: {}",
        error.message()
    );
}

#[test]
fn should_reject_a_surplus_positional_argument() {
    let error = bind("stop process 4419 4420").expect_err("`stop process` declares one selector");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(
        error.message().contains("4420"),
        "the message must quote the surplus argument, was: {}",
        error.message()
    );
}

#[test]
fn should_report_an_option_left_without_its_value() {
    let error = bind("watch process --every").expect_err("`--every` needs a duration");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(
        error.message().contains("every"),
        "the message must name the option, was: {}",
        error.message()
    );
}

#[test]
fn should_report_an_unknown_target_for_a_known_verb() {
    let stage = stage("get nonesuch");
    let error = registry()
        .resolve("get", &stage.arguments)
        .expect_err("`get nonesuch` names no command");

    assert_eq!(error.code(), ErrorCode::ResolveTargetNotFound);
    assert!(error.message().contains("nonesuch"));
}

#[test]
fn should_report_an_unknown_head_as_command_not_found() {
    let stage = stage("frobnicate process");
    let error = registry()
        .resolve("frobnicate", &stage.arguments)
        .expect_err("`frobnicate` is not a native command");

    assert_eq!(error.code(), ErrorCode::ResolveCommandNotFound);
    assert!(error.message().contains("frobnicate"));
}

#[test]
fn should_build_a_provider_query_from_the_bound_arguments() {
    let stage = stage("get process 4419 --tree");
    let resolved = registry()
        .resolve("get", &stage.arguments)
        .expect("`get process` resolves");
    let bound = resolved
        .contract
        .bind(resolved.arguments)
        .expect("the stage binds");

    let query = resolved
        .contract
        .query(&bound)
        .expect("a words-mode producer builds a query");

    assert_eq!(query.target_name(), "process");
    assert!(query.flag("tree"));
    assert!(
        query
            .selectors()
            .iter()
            .any(|selector| selector.field_name() == Some("pid")),
        "the pid selector is pushed down to the provider"
    );
}
