//! Statement forms of ADR-0009 and spec §19: bindings, functions, control flow, blocks.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_parser::{Argument, BinaryOp, Expr, MatchArmBody, Pattern, Statement, parse};

fn statements(source: &str) -> Vec<Statement> {
    let parsed = parse(source);
    assert!(
        parsed.diagnostics().is_empty(),
        "unexpected diagnostics for {source:?}: {:?}",
        parsed.diagnostics()
    );
    parsed.program().statements.clone()
}

fn only(source: &str) -> Statement {
    let mut statements = statements(source);
    assert_eq!(statements.len(), 1, "expected one statement in {source:?}");
    statements.remove(0)
}

#[test]
fn should_bind_a_pipeline_when_the_statement_is_a_let() {
    let Statement::Let(binding) = only("let hot = get process | where cpu > 50") else {
        panic!("expected a let statement");
    };
    assert_eq!(binding.name, "hot");
    assert!(binding.ty.is_none());
    assert_eq!(binding.value.head.stages.len(), 2);
}

#[test]
fn should_record_the_declared_type_when_a_let_is_annotated() {
    let Statement::Let(binding) = only("let n: Int = 5") else {
        panic!("expected a let statement");
    };
    let ty = binding.ty.expect("a declared type");
    assert_eq!(ty.name, "Int");
    assert!(!ty.optional);
}

#[test]
fn should_declare_a_function_with_typed_parameters_and_defaults() {
    let Statement::Fn(declaration) = only(
        "fn hot-processes(limit: Float = 20) -> Stream<Process> {\n get process | where cpu > limit\n}",
    ) else {
        panic!("expected a function declaration");
    };
    assert_eq!(declaration.name, "hot-processes");
    assert_eq!(declaration.parameters.len(), 1);
    let parameter = &declaration.parameters[0];
    assert_eq!(parameter.name, "limit");
    assert_eq!(
        parameter.ty.as_ref().map(|ty| ty.name.as_str()),
        Some("Float")
    );
    assert!(parameter.default.is_some());
    let returns = declaration.return_type.expect("a return type");
    assert_eq!(returns.name, "Stream");
    assert_eq!(returns.arguments.len(), 1);
    assert_eq!(returns.arguments[0].name, "Process");
    assert_eq!(declaration.body.statements.len(), 1);
}

#[test]
fn should_mark_a_type_as_nullable_when_it_ends_with_a_question_mark() {
    let Statement::Fn(declaration) = only("fn f(a: Int?) { }") else {
        panic!("expected a function declaration");
    };
    assert!(
        declaration.parameters[0]
            .ty
            .as_ref()
            .expect("a type")
            .optional
    );
}

#[test]
fn should_chain_branches_when_the_statement_is_an_if_else_if_else() {
    let Statement::If(conditional) = only("if a > 1 { x } else if a > 0 { y } else { z }") else {
        panic!("expected an if statement");
    };
    assert_eq!(conditional.branches.len(), 2);
    assert!(conditional.else_block.is_some());
    let Expr::Binary(condition) = &conditional.branches[0].condition else {
        panic!("expected a comparison as the condition");
    };
    assert_eq!(condition.op, BinaryOp::Gt);
}

#[test]
fn should_iterate_a_binding_when_the_statement_is_a_for() {
    let Statement::For(loop_statement) = only("for p in $hot { echo $p }") else {
        panic!("expected a for statement");
    };
    assert_eq!(loop_statement.binding, "p");
    let Expr::Variable(variable) = &loop_statement.iterable else {
        panic!("expected a variable to iterate");
    };
    assert_eq!(variable.name, "hot");
    assert_eq!(loop_statement.body.statements.len(), 1);
}

#[test]
fn should_loop_while_a_condition_holds_when_the_statement_is_a_while() {
    let Statement::While(loop_statement) = only("while a < 10 { step }") else {
        panic!("expected a while statement");
    };
    let Expr::Binary(condition) = &loop_statement.condition else {
        panic!("expected a comparison");
    };
    assert_eq!(condition.op, BinaryOp::Lt);
}

#[test]
fn should_parse_arms_when_the_statement_is_a_match() {
    let Statement::Match(matching) =
        only("match state { \"running\" => { ok }, failed => restart, _ => { skip } }")
    else {
        panic!("expected a match statement");
    };
    assert_eq!(matching.arms.len(), 3);
    assert!(matches!(
        matching.arms[0].pattern,
        Pattern::Literal(Expr::Str(_))
    ));
    assert!(matches!(matching.arms[0].body, MatchArmBody::Block(_)));
    assert!(matches!(&matching.arms[1].pattern, Pattern::Binding { name, .. } if name == "failed"));
    assert!(matches!(matching.arms[1].body, MatchArmBody::Expr(_)));
    assert!(matches!(matching.arms[2].pattern, Pattern::Wildcard(_)));
}

#[test]
fn should_bind_the_error_when_the_statement_is_a_try_catch() {
    let Statement::Try(attempt) = only("try { risky } catch err { report $err }") else {
        panic!("expected a try statement");
    };
    let catch = attempt.catch.expect("a catch clause");
    assert_eq!(catch.binding.as_deref(), Some("err"));
    assert_eq!(catch.body.statements.len(), 1);

    let Statement::Try(attempt) = only("try { risky }") else {
        panic!("expected a try statement");
    };
    assert!(attempt.catch.is_none());
}

#[test]
fn should_parse_jump_statements_when_they_appear_in_a_block() {
    let Statement::Fn(declaration) = only("fn f() { return 1\n break\n continue }") else {
        panic!("expected a function declaration");
    };
    assert_eq!(declaration.body.statements.len(), 3);
    assert!(matches!(&declaration.body.statements[0], Statement::Return(r) if r.value.is_some()));
    assert!(matches!(
        declaration.body.statements[1],
        Statement::Break(_)
    ));
    assert!(matches!(
        declaration.body.statements[2],
        Statement::Continue(_)
    ));
}

#[test]
fn should_declare_an_alias_for_a_pipeline_when_the_statement_is_an_alias() {
    let Statement::Alias(alias) = only("alias failed = get service | where state == failed") else {
        panic!("expected an alias statement");
    };
    assert_eq!(alias.name, "failed");
    assert_eq!(
        alias.value.head.stages.len(),
        2,
        "the alias stands for the whole pipeline after the `=` (ADR-0070)"
    );
    let source = "alias failed = get service | where state == failed";
    assert_eq!(
        alias.value.span.of(source),
        "get service | where state == failed",
        "the pipeline's span is the text an expansion substitutes"
    );
}

#[test]
fn should_report_a_missing_equals_sign_when_an_alias_has_none() {
    let parsed = parse("alias ll ls -la");
    assert!(
        parsed.has_errors(),
        "an alias without `=` is a syntax error, got {:?}",
        parsed.diagnostics()
    );
}

#[test]
fn should_import_a_module_when_the_statement_is_a_use() {
    let Statement::Use(import) = only("use ono:process") else {
        panic!("expected a use statement");
    };
    assert_eq!(import.module.namespace.as_deref(), Some("ono"));
    assert_eq!(import.module.name, "process");
}

#[test]
fn should_read_a_brace_as_a_block_when_the_first_token_is_not_a_key() {
    let statements = statements("get service | each { restart service @ }");
    let Statement::Pipeline(pipeline) = &statements[0] else {
        panic!("expected a pipeline");
    };
    let Argument::Value(Expr::Block(block)) = &pipeline.head.stages[1].arguments[0] else {
        panic!(
            "expected a block argument, got {:?}",
            pipeline.head.stages[1].arguments
        );
    };
    assert_eq!(block.statements.len(), 1);
}

#[test]
fn should_read_a_value_followed_by_an_operator_as_one_expression_statement() {
    let statements = statements("each { @ * 2 }");
    let Statement::Pipeline(pipeline) = &statements[0] else {
        panic!("expected a pipeline");
    };
    let Argument::Value(Expr::Block(block)) = &pipeline.head.stages[0].arguments[0] else {
        panic!("expected a block argument");
    };
    let Some(Statement::Pipeline(inner)) = block.statements.first() else {
        panic!(
            "expected a statement in the block, got {:?}",
            block.statements
        );
    };
    let stage = &inner.head.stages[0];
    assert!(
        matches!(stage.head, ono_parser::StageHead::Value(Expr::Binary(_))),
        "spec §19.4 / ADR-0071 §1: `@ * 2` is one expression, not `@` with two words, got {:?}",
        stage.head
    );
    assert!(stage.arguments.is_empty());
}

#[test]
fn should_keep_a_variable_head_followed_by_a_pipe_as_a_pipeline_seed() {
    let statements = statements("$hot | select pid");
    let Statement::Pipeline(pipeline) = &statements[0] else {
        panic!("expected a pipeline");
    };
    assert_eq!(pipeline.head.stages.len(), 2);
    assert!(
        matches!(
            pipeline.head.stages[0].head,
            ono_parser::StageHead::Value(Expr::Variable(_))
        ),
        "`$hot | …` seeds the pipeline with the variable (spec §19.2), got {:?}",
        pipeline.head.stages[0].head
    );
}

#[test]
fn should_read_a_brace_as_a_record_when_the_first_token_is_a_key_and_a_colon() {
    let statements = statements(r#"each {name: "x", port: 80}"#);
    let Statement::Pipeline(pipeline) = &statements[0] else {
        panic!("expected a pipeline");
    };
    let Argument::Value(Expr::Record(record)) = &pipeline.head.stages[0].arguments[0] else {
        panic!("expected a record argument");
    };
    assert_eq!(record.fields.len(), 2);
}

#[test]
fn should_read_an_empty_brace_pair_as_the_empty_record() {
    let statements = statements("each {}");
    let Statement::Pipeline(pipeline) = &statements[0] else {
        panic!("expected a pipeline");
    };
    let Argument::Value(Expr::Record(record)) = &pipeline.head.stages[0].arguments[0] else {
        panic!("expected an empty record");
    };
    assert!(record.fields.is_empty());
}

#[test]
fn should_accept_a_quoted_key_when_a_record_field_needs_one() {
    let statements = statements(r#"each {"a-b": 1}"#);
    let Statement::Pipeline(pipeline) = &statements[0] else {
        panic!("expected a pipeline");
    };
    let Argument::Value(Expr::Record(record)) = &pipeline.head.stages[0].arguments[0] else {
        panic!("expected a record argument");
    };
    assert_eq!(record.fields[0].key.name(), Some("a-b"));
}

#[test]
fn should_skip_blank_lines_and_comments_when_a_script_is_parsed() {
    let statements = statements("# a note\n\nls\n\n# another\npwd\n");
    assert_eq!(statements.len(), 2);
}
