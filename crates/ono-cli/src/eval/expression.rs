//! Expression evaluation: values, operators and the three-valued logic of spec §10.5.

use ono_core::ErrorCode;
use ono_parser::{BinaryOp, Expr, NumberValue, StrPart, UnaryOp, Unit};
use ono_value::{ByteSize, Duration as OnoDuration, ErrorValue, MapValue, Percent, Value};

use crate::session::Session;

use super::materialize::value_of_pipeline;
use super::{Eval, Flow};

/// A value's text form, for handing to a process that speaks bytes (spec §12.3).
///
/// A null becomes the empty string here, and only here. Spec §10.5's rule that absence must stay
/// visible is about showing data to a person: a table cell for an unknown value says `null`. An
/// interpolated argument is not a rendering, and `echo "Hello $NAME"` printing `Hello null` when
/// `NAME` is unset would be a worse lie than printing nothing (ADR-0019).
pub(super) fn text_of(value: &Value) -> Result<String, ErrorValue> {
    if matches!(value, Value::Null) {
        return Ok(String::new());
    }
    ono_value::canonical_text(value)
}

pub(super) fn string_of(session: &mut Session, expression: &Expr, source: &str) -> Eval<String> {
    let value = eval_expr(session, expression, source)?;
    Ok(text_of(&value)?)
}

/// Evaluates an expression to a value.
pub fn eval_expr(session: &mut Session, expression: &Expr, source: &str) -> Eval<Value> {
    match expression {
        Expr::Number(literal) => Ok(number_value(literal.value)),
        Expr::Unit(literal) => Ok(unit_value(literal.value, literal.unit)?),
        Expr::Bool(value, _) => Ok(Value::Bool(*value)),
        Expr::Null(_) => Ok(Value::Null),
        Expr::Str(literal) => {
            let mut text = String::new();
            for part in &literal.parts {
                match part {
                    StrPart::Text { text: raw, .. } => text.push_str(raw),
                    StrPart::Expr(inner) => {
                        let value = eval_expr(session, inner, source)?;
                        text.push_str(&text_of(&value)?);
                    }
                }
            }
            Ok(Value::String(text.into()))
        }
        Expr::Ip(literal) => {
            // The lexer keeps the address as written; the value model is what knows how to read
            // one, so a malformed address is a `type.mismatch` here rather than a lexer rule.
            let address: std::net::IpAddr = literal
                .text
                .split('%')
                .next()
                .unwrap_or(&literal.text)
                .parse()
                .map_err(|_| {
                    Flow::Failed(ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("`{}` is not an IP address", literal.text),
                    ))
                })?;
            Ok(Value::Ip(address))
        }
        Expr::Timestamp(literal) => Ok(Value::parse_timestamp(&literal.text)?),
        Expr::Regex(literal) => {
            // Flags become an inline group, which is how the regex engine spells them and keeps
            // the pattern one thing rather than a pattern plus a side channel.
            let pattern = if literal.flags.is_empty() {
                literal.pattern.clone()
            } else {
                format!("(?{}){}", literal.flags, literal.pattern)
            };
            Ok(Value::Regex(std::sync::Arc::new(
                ono_value::RegexValue::new(&pattern)?,
            )))
        }
        Expr::Variable(variable) => Ok(lookup_variable(session, &variable.name)),
        Expr::Path(path) => Ok(lookup_variable(session, &path.name)),
        Expr::List(list) => {
            let mut items = Vec::with_capacity(list.items.len());
            for item in &list.items {
                items.push(eval_expr(session, item, source)?);
            }
            Ok(Value::List(items.into()))
        }
        Expr::Record(record) => {
            let mut map = MapValue::new();
            for field in &record.fields {
                let key = match &field.key {
                    ono_parser::RecordKey::Ident { name, .. } => name.clone(),
                    ono_parser::RecordKey::Str(literal) => {
                        string_of(session, &Expr::Str(literal.clone()), source)?
                    }
                };
                let value = eval_expr(session, &field.value, source)?;
                map.insert(key.into(), value);
            }
            Ok(Value::Map(std::sync::Arc::new(map)))
        }
        Expr::Paren(inner) => match &inner.inner {
            ono_parser::ParenInner::Expr(expression) => eval_expr(session, expression, source),
            ono_parser::ParenInner::Pipeline(pipeline) => {
                value_of_pipeline(session, pipeline, source)
            }
        },
        Expr::Unary(unary) => {
            let operand = eval_expr(session, &unary.operand, source)?;
            Ok(match unary.op {
                UnaryOp::Not => match operand {
                    Value::Null => Value::Null,
                    other => Value::Bool(!truthy(&other)),
                },
                UnaryOp::Neg => Value::Int(0).sub(&operand)?,
            })
        }
        Expr::Binary(binary) => eval_binary(session, binary, source),
        Expr::Field(access) => {
            let base = eval_expr(session, &access.base, source)?;
            let step = if access.optional {
                ono_value::FieldStep::optional(&access.field)
            } else {
                ono_value::FieldStep::required(&access.field)
            };
            Ok(base.follow(&[step])?)
        }
        Expr::Index(index) => {
            let base = eval_expr(session, &index.base, source)?;
            let key = eval_expr(session, &index.index, source)?;
            Ok(index_into(&base, &key)?)
        }
        Expr::Call(call) => {
            // `now()` is the one builtin function language.yaml declares (spec §6.3, ADR-0071).
            if ono_command::is_now_call(call) {
                return Ok(Value::now());
            }
            Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveCommandNotFound,
                    format!("no function to call at {}", call.span),
                )
                .with_help(
                    "`now()` is the only function an expression can call; a user function is \
                     called as a command (spec §19.3, ADR-0070)",
                ),
            ))
        }
        Expr::CurrentValue(current) => match current.selector {
            // Spec §20.2: previous structured results are reusable without screen scraping. A
            // list splices when it starts a pipeline (ADR-0019), so `@-1 | where …` streams the
            // retained rows.
            ono_parser::CurrentSelector::Previous(n) => session
                .previous_result(n)
                .map(|values| Value::list(values.to_vec()))
                .ok_or_else(|| {
                    Flow::Failed(
                        ErrorValue::new(
                            ErrorCode::ResolveTargetNotFound,
                            format!("no result to reuse at {}", current.span),
                        )
                        .with_help(
                            "`@-1` names the previous pipeline's values (spec §20.2), \
                                    and nothing has produced any yet",
                        ),
                    )
                }),
            ono_parser::CurrentSelector::Item(n) => session
                .previous_result(1)
                .and_then(|values| values.get(n.checked_sub(1)? as usize))
                .cloned()
                .ok_or_else(|| {
                    Flow::Failed(
                        ErrorValue::new(
                            ErrorCode::ResolveTargetNotFound,
                            format!("no item {n} in the current result at {}", current.span),
                        )
                        .with_help("`@N` names row N of the last shown result (spec §6.4)"),
                    )
                }),
            // The item an enclosing block is iterating shadows the interactive selection for
            // the block's duration (spec §19.4, ADR-0071 §1).
            ono_parser::CurrentSelector::Current if session.binding("@").is_some() => {
                Ok(session.binding("@").cloned().unwrap_or(Value::Null))
            }
            ono_parser::CurrentSelector::Current => session.selection().cloned().ok_or_else(|| {
                Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::ResolveTargetNotFound,
                        format!("there is no current value at {}", current.span),
                    )
                    .with_help(
                        "`@` names the item a block is iterating, or the row a view left \
                             selected; neither exists here (spec §6.4, §19.4, ADR-0050)",
                    ),
                )
            }),
        },
        Expr::Block(_) => Ok(Value::Null),
        Expr::Error(span) => Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ParseSyntax,
            format!("this expression could not be read at {span}"),
        ))),
    }
}

pub(super) fn lookup_variable(session: &Session, name: &str) -> Value {
    // The status of the last statement, under the name every shell user already knows and under
    // a name they can discover (ADR-0019).
    if name == "?" || name == "status" {
        return Value::Int(i128::from(session.status().code()));
    }
    if let Some(variable) = name.strip_prefix("env.") {
        return session.env_var(variable).map_or(Value::Null, |value| {
            Value::String(value.to_string_lossy().into_owned().into())
        });
    }
    // `$env` is the environment as a record, so `$env.PATH` is an ordinary field access and the
    // environment is inspectable as data rather than only readable one name at a time (ADR-0010).
    if name == "env" && session.binding("env").is_none() {
        let mut map = MapValue::new();
        for (key, value) in session.env() {
            map.insert(
                key.to_string_lossy().into_owned().into(),
                Value::String(value.to_string_lossy().into_owned().into()),
            );
        }
        return Value::Map(std::sync::Arc::new(map));
    }
    if let Some(value) = session.binding(name) {
        return value.clone();
    }
    session.env_var(name).map_or(Value::Null, |value| {
        Value::String(value.to_string_lossy().into_owned().into())
    })
}

pub(super) fn number_value(value: NumberValue) -> Value {
    match value {
        NumberValue::Int(int) => Value::Int(i128::from(int)),
        NumberValue::Float(float) => Value::Float(float),
    }
}

pub(super) fn unit_value(value: NumberValue, unit: Unit) -> Result<Value, ErrorValue> {
    let magnitude = match value {
        NumberValue::Int(int) => int as f64,
        NumberValue::Float(float) => float,
    };
    let suffix = unit.as_str();
    if let Some(byte_unit) = ono_value::ByteUnit::from_suffix(suffix) {
        let bytes = magnitude * byte_unit.factor() as f64;
        if !bytes.is_finite() || bytes < 0.0 {
            return Err(ErrorValue::new(
                ErrorCode::TypeInvalidUnit,
                format!("{magnitude}{suffix} is not a byte size"),
            ));
        }
        return Ok(Value::ByteSize(ByteSize::from_bytes(bytes as u128)));
    }
    if let Some(time_unit) = ono_value::DurationUnit::from_suffix(suffix) {
        let nanoseconds = magnitude * time_unit.nanoseconds() as f64;
        if !nanoseconds.is_finite() {
            return Err(ErrorValue::new(
                ErrorCode::TypeInvalidUnit,
                format!("{magnitude}{suffix} is not a duration"),
            ));
        }
        return Ok(Value::Duration(OnoDuration::from_nanoseconds(
            nanoseconds as i128,
        )));
    }
    Ok(Value::Percent(Percent::new(magnitude)))
}

/// Evaluates an infix operator with the three-valued semantics ADR-0014 freezes.
pub(super) fn eval_binary(
    session: &mut Session,
    binary: &ono_parser::BinaryExpr,
    source: &str,
) -> Eval<Value> {
    // `and` and `or` short-circuit, so a decided result is not made unknown by the other operand.
    match binary.op {
        BinaryOp::And => {
            let left = eval_expr(session, &binary.lhs, source)?;
            if matches!(left, Value::Bool(false)) {
                return Ok(Value::Bool(false));
            }
            let right = eval_expr(session, &binary.rhs, source)?;
            return Ok(kleene_and(&left, &right));
        }
        BinaryOp::Or => {
            let left = eval_expr(session, &binary.lhs, source)?;
            if matches!(left, Value::Bool(true)) {
                return Ok(Value::Bool(true));
            }
            let right = eval_expr(session, &binary.rhs, source)?;
            return Ok(kleene_or(&left, &right));
        }
        _ => {}
    }

    let left = eval_expr(session, &binary.lhs, source)?;
    let right = eval_expr(session, &binary.rhs, source)?;

    // `x == null` is an identity test, not a three-valued comparison (ADR-0014): without the
    // exception the commonest question anyone asks would silently match nothing.
    if matches!(binary.op, BinaryOp::Eq | BinaryOp::NotEq)
        && (matches!(&binary.lhs, Expr::Null(_)) || matches!(&binary.rhs, Expr::Null(_)))
    {
        // One side is the literal `null`, so the question is only whether the other side is.
        let other = if matches!(&binary.lhs, Expr::Null(_)) {
            &right
        } else {
            &left
        };
        let is_null = matches!(other, Value::Null);
        return Ok(Value::Bool(match binary.op {
            BinaryOp::Eq => is_null,
            _ => !is_null,
        }));
    }

    if matches!(left, Value::Null) || matches!(right, Value::Null) {
        return Ok(Value::Null);
    }

    Ok(match binary.op {
        BinaryOp::Eq => Value::Bool(equals(&left, &right)),
        BinaryOp::NotEq => Value::Bool(!equals(&left, &right)),
        BinaryOp::Lt => Value::Bool(left.compare_to(&right)?.is_lt()),
        BinaryOp::LtEq => Value::Bool(left.compare_to(&right)?.is_le()),
        BinaryOp::Gt => Value::Bool(left.compare_to(&right)?.is_gt()),
        BinaryOp::GtEq => Value::Bool(left.compare_to(&right)?.is_ge()),
        BinaryOp::In => Value::Bool(contains(&right, &left)),
        BinaryOp::NotIn => Value::Bool(!contains(&right, &left)),
        BinaryOp::Match => Value::Bool(regex_matches(&right, &left)?),
        BinaryOp::NotMatch => Value::Bool(!regex_matches(&right, &left)?),
        BinaryOp::Add => left.add(&right)?,
        BinaryOp::Sub => left.sub(&right)?,
        BinaryOp::Mul => left.mul(&right)?,
        BinaryOp::Div => left.div(&right)?,
        BinaryOp::Rem => remainder(&left, &right)?,
        BinaryOp::And | BinaryOp::Or => Value::Null,
    })
}

pub(super) fn kleene_and(left: &Value, right: &Value) -> Value {
    match (left, right) {
        (Value::Bool(false), _) | (_, Value::Bool(false)) => Value::Bool(false),
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        _ => Value::Bool(truthy(left) && truthy(right)),
    }
}

pub(super) fn kleene_or(left: &Value, right: &Value) -> Value {
    match (left, right) {
        (Value::Bool(true), _) | (_, Value::Bool(true)) => Value::Bool(true),
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        _ => Value::Bool(truthy(left) || truthy(right)),
    }
}

/// Whether a value counts as true where a condition is wanted.
///
/// Only `true` is true. `null` is unknown and therefore not true, which is what makes `where`
/// admit only decided matches (ADR-0014).
#[must_use]
pub fn truthy(value: &Value) -> bool {
    matches!(value, Value::Bool(true))
}

pub(super) fn equals(left: &Value, right: &Value) -> bool {
    if left == right {
        return true;
    }
    left.compare_to(right).is_ok_and(std::cmp::Ordering::is_eq)
}

pub(super) fn contains(haystack: &Value, needle: &Value) -> bool {
    match haystack {
        Value::List(items) => items.iter().any(|item| equals(item, needle)),
        Value::String(text) => {
            ono_value::canonical_text(needle).is_ok_and(|needle| text.contains(&needle))
        }
        _ => false,
    }
}

pub(super) fn regex_matches(pattern: &Value, subject: &Value) -> Result<bool, ErrorValue> {
    let regex = pattern.as_regex()?;
    let text = ono_value::canonical_text(subject)?;
    Ok(regex.is_match(&text))
}

pub(super) fn remainder(left: &Value, right: &Value) -> Result<Value, ErrorValue> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) if *b != 0 => Ok(Value::Int(a % b)),
        (Value::Int(_), Value::Int(_)) => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            "the remainder of a division by zero is undefined",
        )),
        _ => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "`%` needs two integers, got {} and {}",
                left.type_name(),
                right.type_name()
            ),
        )),
    }
}

pub(super) fn index_into(base: &Value, key: &Value) -> Result<Value, ErrorValue> {
    match (base, key) {
        (Value::List(items), Value::Int(index)) => {
            let index = usize::try_from(*index).map_err(|_| {
                ErrorValue::new(ErrorCode::TypeMismatch, "a list index cannot be negative")
            })?;
            Ok(items.get(index).cloned().unwrap_or(Value::Null))
        }
        (Value::Map(map), Value::String(key)) => Ok(map.get(key).cloned().unwrap_or(Value::Null)),
        (Value::Record(record), Value::String(field)) => {
            Ok(record.get(field).cloned().unwrap_or(Value::Null))
        }
        _ => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "{} cannot be indexed by {}",
                base.type_name(),
                key.type_name()
            ),
        )),
    }
}
