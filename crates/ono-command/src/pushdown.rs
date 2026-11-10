//! Which comparisons of a `where` a provider may be handed (ADR-0980).
//!
//! A provider can declare that its source evaluates comparisons on some of its fields
//! ([`Provider::pushdown`](ono_provider_api::Provider::pushdown)). This module reads what a
//! predicate offers: the comparisons it *starts* with. It knows no provider, no target and no
//! field; whether a comparison is one a provider declared is decided where the query is built,
//! against the declaration.
//!
//! The rule is the smallest one that cannot change an answer:
//!
//! - only a stage that filters its input by a predicate offers anything, and only the stage that
//!   directly follows the producer is asked. A stage between the two — `take`, `sort`, another
//!   `where` — sees the records the provider produced, so what the provider withholds would
//!   change what that stage sees;
//! - the predicate is read as a conjunction, left to right. A conjunct is offered while it is a
//!   field compared with a literal, and the reading stops at the first one that is anything else.
//!   `and` evaluates its left side first (ADR-0014), so a conjunct after the stopping point is
//!   only ever asked about records the earlier ones did not already reject — and one of the
//!   earlier ones may *fail* on a record a later comparison would have withheld. Withholding it
//!   would swallow the failure;
//! - a disjunction, a negation, arithmetic, a variable or a call is not a comparison with a
//!   literal, and offers nothing.
//!
//! The predicate itself is never rewritten. Whatever is offered here, the stage still decides
//! every record it is given.

use ono_parser::{BinaryOp, Expr, NumberValue, StrPart};
use ono_provider_api::{Comparison, ComparisonOperator};
use ono_value::Value;

use crate::bind::BoundArguments;
use crate::contract::CommandContract;

/// The command that keeps the values of a stream its predicate holds for.
const FILTER: &str = "ono.data.where";

/// The comparisons the stage `contract`, bound to `arguments`, would hand a producer directly
/// before it: the field-against-literal conjuncts its predicate starts with, in the order they
/// are written, each with the field on the left.
///
/// Empty for every stage that is not a filter, and for a filter whose predicate does not start
/// with such a comparison.
///
/// ```
/// use ono_command::{CommandRegistry, pushable_comparisons};
/// use ono_provider_api::ComparisonOperator;
///
/// let registry = CommandRegistry::embedded().expect("the embedded contracts parse");
/// let parsed = ono_parser::parse("where 3 >= depth and name == \"mail\" and size > $n");
/// let stage = &parsed.program().statements[0]
///     .as_pipeline()
///     .expect("a pipeline")
///     .head
///     .stages[0];
/// let resolved = registry.resolve(stage.head.name().expect("a command"), &stage.arguments)?;
/// let arguments = resolved.contract.bind(resolved.arguments)?;
///
/// let offered = pushable_comparisons(resolved.contract, &arguments);
/// assert_eq!(offered.len(), 2, "the comparison with a variable ends the reading");
/// assert_eq!(offered[0].field(), "depth");
/// assert_eq!(offered[0].operator(), ComparisonOperator::LessOrEqual);
/// assert_eq!(offered[1].field(), "name");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[must_use]
pub fn pushable_comparisons(
    contract: &CommandContract,
    arguments: &BoundArguments,
) -> Vec<Comparison> {
    if contract.id() != FILTER {
        return Vec::new();
    }
    let Some(predicate) = arguments.selector_expression("predicate") else {
        return Vec::new();
    };
    let mut conjuncts = Vec::new();
    collect_conjuncts(predicate, &mut conjuncts);
    conjuncts.into_iter().map_while(comparison_of).collect()
}

/// The operands of a conjunction, left to right, with grouping parentheses removed.
fn collect_conjuncts<'a>(expression: &'a Expr, found: &mut Vec<&'a Expr>) {
    match ungrouped(expression) {
        Expr::Binary(binary) if binary.op == BinaryOp::And => {
            collect_conjuncts(&binary.lhs, found);
            collect_conjuncts(&binary.rhs, found);
        }
        other => found.push(other),
    }
}

/// `expression` without the parentheses that only group it. A parenthesised pipeline stays what
/// it is: it has to be run, and is no comparison.
fn ungrouped(expression: &Expr) -> &Expr {
    let mut current = expression;
    while let Expr::Paren(inner) = current {
        match inner.expression() {
            Some(expression) => current = expression,
            None => break,
        }
    }
    current
}

/// `expression` as a field compared with a literal, where it is one.
fn comparison_of(expression: &Expr) -> Option<Comparison> {
    let Expr::Binary(binary) = ungrouped(expression) else {
        return None;
    };
    let operator = match binary.op {
        BinaryOp::Eq => ComparisonOperator::Equal,
        BinaryOp::NotEq => ComparisonOperator::NotEqual,
        BinaryOp::Lt => ComparisonOperator::Less,
        BinaryOp::LtEq => ComparisonOperator::LessOrEqual,
        BinaryOp::Gt => ComparisonOperator::Greater,
        BinaryOp::GtEq => ComparisonOperator::GreaterOrEqual,
        _ => return None,
    };
    match (ungrouped(&binary.lhs), ungrouped(&binary.rhs)) {
        (Expr::Path(field), literal) => {
            Some(Comparison::new(&field.name, operator, literal_of(literal)?))
        }
        (literal, Expr::Path(field)) => Some(Comparison::new(
            &field.name,
            operator.mirrored(),
            literal_of(literal)?,
        )),
        _ => None,
    }
}

/// The value a literal denotes, for the literals a comparison may carry: an integer, a boolean,
/// and a string with nothing interpolated into it.
fn literal_of(expression: &Expr) -> Option<Value> {
    match expression {
        Expr::Number(literal) => match literal.value {
            NumberValue::Int(_) => Some(crate::expr::number_literal(literal.value)),
            NumberValue::Float(_) => None,
        },
        Expr::Bool(value, _) => Some(Value::Bool(*value)),
        Expr::Str(literal) => {
            let mut text = String::new();
            for part in &literal.parts {
                match part {
                    StrPart::Text { text: raw, .. } => text.push_str(raw),
                    StrPart::Expr(_) => return None,
                }
            }
            Some(Value::string(&text))
        }
        _ => None,
    }
}
