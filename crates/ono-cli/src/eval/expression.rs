//! Expression evaluation: values, operators and the three-valued logic of spec §10.5.

use ono_core::ErrorCode;
use ono_parser::{Expr, StrPart, UnaryOp};
use ono_value::{ErrorValue, MapValue, Value};

use crate::session::Session;

use super::materialize::value_of_pipeline;
use super::{Eval, Flow};

/// A value's text form, for handing to a process that speaks bytes (spec §12.3).
///
/// A null becomes the empty string here, and only here. Spec §10.5's rule that absence must stay
/// visible is about showing data to a person: a table cell for an unknown value says `null`. An
/// interpolated argument is not a rendering, and `echo "Hello $NAME"` printing `Hello null` when
/// `NAME` is unset would be a worse lie than printing nothing (ADR-0019).
pub(crate) fn text_of(value: &Value) -> Result<String, ErrorValue> {
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
        Expr::Number(literal) => Ok(ono_command::number_literal(literal.value)),
        Expr::Unit(literal) => Ok(ono_command::unit_literal(literal.value, literal.unit)?),
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
            Ok(ono_command::index_into(&base, &key)?)
        }
        Expr::Call(call) => {
            // `language.yaml`'s `builtin_functions` is the closed list, and `ono-command` holds
            // it: `now()` since spec §6.3, `age()` and `between()` since v0.5 §28.3. Evaluating
            // one here rather than reimplementing it keeps the shell and a `where` inside a
            // provider stage answering the same thing (ADR-0693).
            let scope = ono_command::Scope::new();
            ono_command::evaluate_call(call, &Value::Null, &scope).map_err(Flow::Failed)
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

/// Evaluates an infix operator: `ono-command`'s one implementation of the operators, with each
/// operand read by this evaluator, so its variables, `$(…)` and parenthesised pipelines take part
/// (issue #137). The session has no current record, so the enum readings of ADR-0096 and
/// ADR-0222 do not apply here.
pub(super) fn eval_binary(
    session: &mut Session,
    binary: &ono_parser::BinaryExpr,
    source: &str,
) -> Eval<Value> {
    ono_command::evaluate_binary(binary, None, |side| eval_expr(session, side, source))
}

/// Whether a value counts as true where a condition is wanted.
///
/// Only `true` is true. `null` is unknown and therefore not true, which is what makes `where`
/// admit only decided matches (ADR-0014).
#[must_use]
pub fn truthy(value: &Value) -> bool {
    ono_command::is_true(value)
}

pub(super) fn equals(left: &Value, right: &Value) -> bool {
    ono_command::values_equal(left, right)
}
