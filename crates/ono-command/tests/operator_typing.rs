//! Every comparison completion offers after a field of some type is one the evaluator executes on
//! a value of that type (v0.6.1 §13: "Completion MUST NOT advertise operators the evaluator
//! cannot execute"; ADR-0860).
//!
//! The relation lives in one place, `ono_command::comparisons_for`; this suite is what keeps it a
//! subset of the evaluator rather than a second opinion beside it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;
use std::sync::Arc;

use ono_command::{Scope, comparisons_for, evaluate, operator_symbol};
use ono_parser::BinaryOp;
use ono_value::{FieldType, Value};

/// The expression `source` is, read as the argument of `where`.
fn expression_of(source: &str) -> ono_parser::Expr {
    let parsed = ono_parser::parse(&format!("where {source}"));
    let stage = &parsed.program().statements[0]
        .as_pipeline()
        .expect("a pipeline")
        .head
        .stages[0];
    stage.arguments[0]
        .as_value()
        .expect("an expression")
        .clone()
}

/// The value a literal evaluates to.
fn literal(source: &str) -> Value {
    evaluate(&expression_of(source), &Value::Null, &Scope::new()).expect("a literal evaluates")
}

/// A value a field of `ty` holds, or `None` for the types whose only offered comparison is
/// equality, which the evaluator answers for any two values (`equals` never fails).
fn sample(ty: &FieldType) -> Option<Value> {
    Some(match ty {
        FieldType::Bool => Value::Bool(true),
        FieldType::Int => Value::Int(1),
        FieldType::Float => Value::Float(1.5),
        FieldType::Decimal => Value::Decimal(ono_value::Decimal::parse("1.5").expect("a decimal")),
        FieldType::String | FieldType::Any => Value::String(Arc::from("abc")),
        FieldType::Path => Value::Path(Arc::from(Path::new("/tmp"))),
        FieldType::Timestamp => literal("2000-01-01T00:00:00Z"),
        FieldType::Duration => literal("5s"),
        FieldType::ByteSize => literal("1GiB"),
        FieldType::Percent => literal("5%"),
        FieldType::Regex => literal("/a/"),
        FieldType::Uuid => Value::Uuid(
            ono_value::Uuid::parse("67e55044-10b1-426f-9247-bb680e5fe0c8").expect("a uuid"),
        ),
        FieldType::Ip => Value::Ip("127.0.0.1".parse().expect("an address")),
        FieldType::IpNetwork => {
            Value::IpNetwork(ono_value::IpNetwork::parse("10.0.0.0/8").expect("a network"))
        }
        FieldType::Port => Value::Port(443),
        // An enum field's values are its variants' names.
        FieldType::Enum(_) => Value::String(Arc::from("listen")),
        FieldType::List(_) => Value::List(Arc::from(vec![Value::Int(1)])),
        FieldType::Bytes
        | FieldType::Map
        | FieldType::Record(_)
        | FieldType::Ref(_)
        | FieldType::Error => return None,
    })
}

/// `value op value`, written so every operator has a right-hand side of the shape it takes.
fn apply(op: BinaryOp, value: &Value) -> Result<Value, ono_value::ErrorValue> {
    let source = match op {
        BinaryOp::In => "$x in [$y]".to_owned(),
        BinaryOp::NotIn => "$x not in [$y]".to_owned(),
        BinaryOp::Match => "$x ~= /a/".to_owned(),
        BinaryOp::NotMatch => "$x !~= /a/".to_owned(),
        other => format!("$x {} $y", operator_symbol(other)),
    };
    let scope = Scope::new()
        .with_variable("x", value.clone())
        .with_variable("y", value.clone());
    evaluate(&expression_of(&source), &Value::Null, &scope)
}

#[test]
fn should_offer_only_comparisons_the_evaluator_executes() {
    let types = [
        FieldType::Any,
        FieldType::Bool,
        FieldType::Int,
        FieldType::Float,
        FieldType::Decimal,
        FieldType::String,
        FieldType::Bytes,
        FieldType::Path,
        FieldType::Timestamp,
        FieldType::Duration,
        FieldType::ByteSize,
        FieldType::Percent,
        FieldType::Regex,
        FieldType::Uuid,
        FieldType::Ip,
        FieldType::IpNetwork,
        FieldType::Port,
        FieldType::enumeration(&["listen", "established"]),
        FieldType::list(FieldType::Int),
        FieldType::Map,
        FieldType::Error,
    ];
    for ty in &types {
        let offered = comparisons_for(ty);
        assert!(
            offered.contains(&BinaryOp::Eq) && offered.contains(&BinaryOp::NotEq),
            "every field can be compared for equality; `{}` offers {offered:?}",
            ty.name()
        );
        let Some(value) = sample(ty) else {
            assert_eq!(
                offered,
                [BinaryOp::Eq, BinaryOp::NotEq],
                "`{}` has no sample, so it may offer nothing beyond equality",
                ty.name()
            );
            continue;
        };
        for op in offered {
            let result = apply(*op, &value);
            assert!(
                matches!(result, Ok(Value::Bool(_))),
                "completion offers `{}` after a `{}` field, so the evaluator must execute it on \
                 {value:?}; got {result:?}",
                operator_symbol(*op),
                ty.name()
            );
        }
    }
}
