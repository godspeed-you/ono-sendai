//! Expression-mode arguments: operators, precedence, associativity (spec §6.3, ADR-0009).

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_parser::{
    ArgMode, Argument, BinaryOp, Expr, NumberValue, Stage, Statement, UnaryOp, Unit, parse,
};

fn stages(source: &str) -> Vec<Stage> {
    let parsed = parse(source);
    assert!(
        parsed.diagnostics().is_empty(),
        "unexpected diagnostics for {source:?}: {:?}",
        parsed.diagnostics()
    );
    let Some(Statement::Pipeline(pipeline)) = parsed.program().statements.first().cloned() else {
        panic!("expected a pipeline statement in {source:?}");
    };
    pipeline.head.stages
}

fn only_expression(source: &str) -> Expr {
    let stages = stages(source);
    let stage = stages.into_iter().next().expect("one stage");
    assert_eq!(stage.mode, ArgMode::Expression);
    let mut arguments = stage.arguments.into_iter();
    let Some(Argument::Value(expression)) = arguments.next() else {
        panic!("expected an expression argument in {source:?}");
    };
    assert!(
        arguments.next().is_none(),
        "expected exactly one argument in {source:?}"
    );
    expression
}

fn binary(source: &str) -> (BinaryOp, Expr, Expr) {
    match only_expression(source) {
        Expr::Binary(binary) => (binary.op, binary.lhs, binary.rhs),
        other => panic!("expected a binary expression in {source:?}, got {other:?}"),
    }
}

fn path_name(expression: &Expr) -> &str {
    match expression {
        Expr::Path(path) => &path.name,
        other => panic!("expected a field path, got {other:?}"),
    }
}

#[test]
fn should_parse_a_four_stage_pipeline_when_transforms_are_chained() {
    let stages = stages("get process | where cpu > 20 | sort cpu desc | take 10");
    assert_eq!(stages.len(), 4);
    assert_eq!(stages[0].mode, ArgMode::Words);
    assert_eq!(stages[1].mode, ArgMode::Expression);
    assert_eq!(stages[2].arguments.len(), 2, "sort takes two field paths");
    let Argument::Value(Expr::Number(number)) = &stages[3].arguments[0] else {
        panic!("expected a number for take");
    };
    assert_eq!(number.value, NumberValue::Int(10));
}

#[test]
fn should_read_a_bare_identifier_as_a_field_path_when_the_mode_is_expression() {
    let stages = stages("select pid name cpu");
    let names: Vec<&str> = stages[0]
        .arguments
        .iter()
        .map(|argument| match argument {
            Argument::Value(expression) => path_name(expression),
            other => panic!("expected a field path argument, got {other:?}"),
        })
        .collect();
    assert_eq!(names, vec!["pid", "name", "cpu"]);
}

#[test]
fn should_compare_a_field_against_a_number_when_the_argument_uses_a_relational_operator() {
    let (op, lhs, rhs) = binary("where cpu > 20");
    assert_eq!(op, BinaryOp::Gt);
    assert_eq!(path_name(&lhs), "cpu");
    let Expr::Number(number) = rhs else {
        panic!("expected a number")
    };
    assert_eq!(number.value, NumberValue::Int(20));
}

#[test]
fn should_match_a_regex_when_the_argument_uses_the_match_operator() {
    let (op, lhs, rhs) = binary("where name ~= /postgres|redis/");
    assert_eq!(op, BinaryOp::Match);
    assert_eq!(path_name(&lhs), "name");
    let Expr::Regex(regex) = rhs else {
        panic!("expected a regex")
    };
    assert_eq!(regex.pattern, "postgres|redis");
    assert_eq!(binary("where name !~= /x/").0, BinaryOp::NotMatch);
}

#[test]
fn should_test_membership_when_the_argument_uses_in() {
    let (op, lhs, rhs) = binary(r#"where user in ["root","postgres"]"#);
    assert_eq!(op, BinaryOp::In);
    assert_eq!(path_name(&lhs), "user");
    let Expr::List(list) = rhs else {
        panic!("expected a list")
    };
    assert_eq!(list.items.len(), 2);
}

#[test]
fn should_test_non_membership_when_the_argument_uses_not_in() {
    assert_eq!(binary("where user not in [1, 2]").0, BinaryOp::NotIn);
}

#[test]
fn should_bind_and_tighter_than_or_when_both_appear() {
    let (op, lhs, rhs) = binary("where a and b or c");
    assert_eq!(op, BinaryOp::Or);
    let Expr::Binary(left) = lhs else {
        panic!("expected the and-expression on the left")
    };
    assert_eq!(left.op, BinaryOp::And);
    assert_eq!(path_name(&rhs), "c");
}

#[test]
fn should_bind_comparison_tighter_than_and_when_units_and_calls_are_mixed() {
    let (op, lhs, rhs) = binary("where memory >= 512MiB and modified < now() - 7d");
    assert_eq!(op, BinaryOp::And);

    let Expr::Binary(left) = lhs else {
        panic!("expected a comparison on the left")
    };
    assert_eq!(left.op, BinaryOp::GtEq);
    let Expr::Unit(size) = left.rhs else {
        panic!("expected a byte size")
    };
    assert_eq!(size.unit, Unit::MiB);
    assert_eq!(size.value, NumberValue::Int(512));

    let Expr::Binary(right) = rhs else {
        panic!("expected a comparison on the right")
    };
    assert_eq!(right.op, BinaryOp::Lt);
    let Expr::Binary(difference) = right.rhs else {
        panic!("expected a subtraction on the right of the comparison")
    };
    assert_eq!(difference.op, BinaryOp::Sub);
    let Expr::Call(call) = difference.lhs else {
        panic!("expected a call to now()")
    };
    assert!(call.arguments.is_empty());
    let Expr::Unit(duration) = difference.rhs else {
        panic!("expected a duration")
    };
    assert_eq!(duration.unit, Unit::D);
}

#[test]
fn should_read_a_dotted_name_as_nested_field_access_when_the_mode_is_expression() {
    let (op, lhs, _) = binary("where remote.port == 443");
    assert_eq!(op, BinaryOp::Eq);
    let Expr::Field(access) = lhs else {
        panic!("expected a field access")
    };
    assert_eq!(access.field, "port");
    assert!(!access.optional);
    assert_eq!(path_name(&access.base), "remote");
}

#[test]
fn should_read_an_optional_chain_when_the_dot_is_preceded_by_a_question_mark() {
    let (_, lhs, _) = binary("where remote?.port == 443");
    let Expr::Field(access) = lhs else {
        panic!("expected a field access")
    };
    assert!(access.optional);
}

#[test]
fn should_index_a_value_when_the_postfix_is_a_bracket() {
    let expression = only_expression("where names[0]");
    let Expr::Index(index) = expression else {
        panic!("expected an index expression")
    };
    assert_eq!(path_name(&index.base), "names");
}

#[test]
fn should_group_arithmetic_left_to_right_when_operators_have_the_same_precedence() {
    let (op, lhs, rhs) = binary("where a - b - c");
    assert_eq!(op, BinaryOp::Sub);
    assert_eq!(path_name(&rhs), "c");
    let Expr::Binary(left) = lhs else {
        panic!("expected the left subtraction")
    };
    assert_eq!(left.op, BinaryOp::Sub);
}

#[test]
fn should_bind_multiplication_tighter_than_addition_when_both_appear() {
    let (op, _, rhs) = binary("where a + b * c");
    assert_eq!(op, BinaryOp::Add);
    let Expr::Binary(right) = rhs else {
        panic!("expected the multiplication on the right")
    };
    assert_eq!(right.op, BinaryOp::Mul);
}

#[test]
fn should_override_precedence_when_a_subexpression_is_parenthesised() {
    let (op, lhs, _) = binary("where (a + b) * c");
    assert_eq!(op, BinaryOp::Mul);
    let Expr::Paren(paren) = lhs else {
        panic!("expected a parenthesised expression")
    };
    let Some(Expr::Binary(sum)) = paren.expression().cloned() else {
        panic!("expected an expression inside the parentheses")
    };
    assert_eq!(sum.op, BinaryOp::Add);
}

#[test]
fn should_apply_a_prefix_operator_when_the_expression_is_negated() {
    let expression = only_expression("where not a");
    let Expr::Unary(unary) = expression else {
        panic!("expected a unary expression")
    };
    assert_eq!(unary.op, UnaryOp::Not);

    let expression = only_expression("where -a");
    let Expr::Unary(unary) = expression else {
        panic!("expected a unary expression")
    };
    assert_eq!(unary.op, UnaryOp::Neg);
}

#[test]
fn should_read_the_boolean_and_null_keywords_as_literals_when_they_appear() {
    let (_, _, rhs) = binary("where a == true");
    assert_eq!(rhs, Expr::Bool(true, rhs.span()));
    let (_, _, rhs) = binary("where a == false");
    assert_eq!(rhs, Expr::Bool(false, rhs.span()));
    let (_, _, rhs) = binary("where a == null");
    assert_eq!(rhs, Expr::Null(rhs.span()));
}

#[test]
fn should_build_a_record_when_the_brace_starts_with_a_key_and_a_colon() {
    let expression = only_expression(r#"where {name: "x", port: 80}"#);
    let Expr::Record(record) = expression else {
        panic!("expected a record, got a different node")
    };
    assert_eq!(record.fields.len(), 2);
    assert_eq!(record.fields[0].key.name(), Some("name"));
    assert_eq!(record.fields[1].key.name(), Some("port"));
}

#[test]
fn should_span_the_whole_expression_when_a_comparison_is_parsed() {
    let source = "where cpu > 20";
    let expression = only_expression(source);
    assert_eq!(expression.span().of(source), "cpu > 20");
}

#[test]
fn should_read_a_long_option_between_expressions_when_the_mode_is_expression() {
    // `reduce $acc + @ --initial 10`: the option is an argument boundary, not a double unary
    // minus applied to a field named `initial` (ADR-0032). The value pairs with the option at
    // binding, exactly as in words mode.
    let stage = stages("reduce $acc + @ --initial 10").remove(0);
    assert_eq!(stage.arguments.len(), 3, "got {:?}", stage.arguments);
    assert!(matches!(stage.arguments[0], Argument::Value(_)));
    let Argument::Option(option) = &stage.arguments[1] else {
        panic!("expected an option, got {:?}", stage.arguments[1]);
    };
    assert_eq!(option.name, "initial");
    assert!(option.value.is_none());
    assert!(matches!(stage.arguments[2], Argument::Value(_)));
}

#[test]
fn should_keep_a_spaced_double_negation_meaning_negation() {
    // `- -x` is still double negation; only the adjacent `--name` spelling is an option.
    let expression = only_expression("where - -cpu");
    assert!(matches!(expression, Expr::Unary(_)), "got {expression:?}");
}
