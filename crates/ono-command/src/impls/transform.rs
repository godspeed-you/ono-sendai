//! The built-in transforms of spec §53, wired from a bound stage onto `ono-pipeline`.
//!
//! Every one of them is the same shape: read the expressions the contract declares, compile them
//! into the already-resolved functions the engine wants (ADR-0005), and hand the engine the input
//! stream. Nothing is materialised here — `sort` and `group` buffer inside the engine, where the
//! boundedness rule of spec §11.1 and the cancellation scope of spec §18.5 already are.

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_parser::Expr;
use ono_pipeline::{
    Count, Each, Group, Measure, PathSegment, Reduce, Select, SelectField, Skip, Sort, Take, Where,
};
use ono_value::{ErrorValue, Value};

use crate::bind::{Binding, BoundArguments};
use crate::expr::{Scope, evaluate, evaluate_to_value};
use crate::invoke::{CommandImpl, Invocation, Outcome};

/// Which of spec §53's transforms an implementation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Where,
    Select,
    Sort,
    Group,
    Take,
    Skip,
    Each,
    Reduce,
    Count,
    Measure,
}

/// One transform, registered against the contract that declares it.
#[derive(Debug)]
pub(crate) struct TransformCommand {
    id: String,
    kind: Kind,
}

impl TransformCommand {
    pub(crate) fn new(id: &str, kind: Kind) -> Self {
        Self {
            id: id.to_owned(),
            kind,
        }
    }
}

impl CommandImpl for TransformCommand {
    fn id(&self) -> &str {
        &self.id
    }

    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        let spelling = ctx.contract().spelling();
        let arguments = ctx.arguments();
        let scope = Arc::clone(ctx.scope());
        let input = ctx.take_input().ok_or_else(|| needs_input(&spelling))?;

        // `transform` is what raises `stream.unbounded_operation` before anything is read, so a
        // blocking transform over an endless stream costs nothing (spec §11.1).
        let output = match self.kind {
            Kind::Where => {
                let predicate = expression(arguments, "predicate", &spelling)?;
                let scope = Arc::clone(&scope);
                input.transform(Where::new(move |value: &Value| {
                    evaluate_to_value(&predicate, value, &scope)
                }))?
            }
            Kind::Select => {
                let fields = projection(arguments, &spelling, &scope)?;
                input.transform(Select::new(fields)?)?
            }
            Kind::Sort => {
                let key = key_function(arguments, "key", &spelling, &scope)?;
                let sort = Sort::new(key);
                let sort = if descending(arguments, &spelling)? {
                    sort.descending()
                } else {
                    sort
                };
                input.transform(sort)?
            }
            Kind::Group => {
                let key = key_function(arguments, "key", &spelling, &scope)?;
                input.transform(Group::new(key))?
            }
            Kind::Take => input.transform(Take::new(count(arguments, &spelling, &scope)?))?,
            Kind::Skip => input.transform(Skip::new(count(arguments, &spelling, &scope)?))?,
            Kind::Each => {
                let body = expression(arguments, "body", &spelling)?;
                let scope = Arc::clone(&scope);
                input.transform(Each::new(move |value: &Value| {
                    // One value in, one out. Spec §53 warns about accidental nesting; a body that
                    // wants many outputs builds a list and the engine keeps it as one value.
                    evaluate(&body, value, &scope).map(|produced| vec![produced])
                }))?
            }
            Kind::Reduce => {
                let body = expression(arguments, "body", &spelling)?;
                // The initial accumulator may arrive as a word (`--initial=0`) or, in expression
                // mode, as an unevaluated expression (`--initial 10`). The expression is
                // evaluated against no current value: it seeds the fold before anything flows.
                let initial = arguments.option("initial").cloned().or_else(|| {
                    arguments
                        .option_expression("initial")
                        .map(|expr| crate::expr::evaluate_to_value(expr, &Value::Null, &scope))
                });
                let scope = Arc::clone(&scope);
                let fold = move |accumulator: &Value, value: &Value| {
                    // The accumulator is `$acc`; the value in hand is the record, so its fields
                    // are in scope the way they are in every other transform (spec §10.3).
                    let scope = (*scope).clone().with_variable("acc", accumulator.clone());
                    evaluate(&body, value, &scope)
                };
                let reduce = Reduce::new(fold);
                let reduce = match initial {
                    Some(initial) => reduce.with_initial(initial),
                    None => reduce,
                };
                input.transform(reduce)?
            }
            Kind::Count => input.transform(Count::new())?,
            Kind::Measure => {
                let key = key_function(arguments, "key", &spelling, &scope)?;
                input.transform(Measure::new(key))?
            }
        };
        Ok(Outcome::Values(output))
    }
}

/// The single expression bound to a selector.
fn expression(arguments: &BoundArguments, name: &str, spelling: &str) -> Result<Expr, ErrorValue> {
    arguments.selector_expression(name).cloned().ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`{spelling}` needs an expression for `{name}`, and none was given"),
        )
        .with_help(format!("`help {spelling}` shows what it accepts"))
    })
}

/// A key function over the expression bound to `name`.
fn key_function(
    arguments: &BoundArguments,
    name: &str,
    spelling: &str,
    scope: &Arc<Scope>,
) -> Result<impl ono_pipeline::KeyFn + use<>, ErrorValue> {
    let key = expression(arguments, name, spelling)?;
    let scope = Arc::clone(scope);
    Ok(move |value: &Value| evaluate(&key, value, &scope))
}

/// The count `take` and `skip` were given, evaluated with no record in hand.
fn count(
    arguments: &BoundArguments,
    spelling: &str,
    scope: &Arc<Scope>,
) -> Result<usize, ErrorValue> {
    let value = match arguments.selector_binding("count") {
        Some(Binding::Value(value)) => value.clone(),
        Some(Binding::Expressions(expressions)) => {
            let first = expressions.first().ok_or_else(|| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{spelling}` needs a count"),
                )
            })?;
            evaluate(first, &Value::Null, scope)?
        }
        None => {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{spelling}` needs a count, and none was given"),
            )
            .with_help(format!("write it as `{spelling} 10`")));
        }
    };
    let count = value.as_int()?;
    usize::try_from(count).map_err(|_| {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`{spelling}` cannot take {count} values"),
        )
        .with_help("a count is zero or more")
    })
}

/// Whether `sort` was asked for the other end of the order.
fn descending(arguments: &BoundArguments, spelling: &str) -> Result<bool, ErrorValue> {
    let written = match arguments.selector_binding("direction") {
        Some(Binding::Value(Value::String(text))) => text.to_string(),
        Some(Binding::Expressions(expressions)) => match expressions.first() {
            Some(Expr::Path(path)) => path.name.clone(),
            Some(Expr::Str(literal)) => literal.literal_text().unwrap_or_default().to_owned(),
            _ => {
                return Err(direction_error(spelling, "an expression"));
            }
        },
        Some(Binding::Value(other)) => ono_value::canonical_text(other)?,
        None => "asc".to_owned(),
    };
    match written.as_str() {
        "asc" => Ok(false),
        "desc" => Ok(true),
        other => Err(direction_error(spelling, other)),
    }
}

fn direction_error(spelling: &str, written: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!("`{spelling}` orders `asc` or `desc`, not `{written}`"),
    )
}

/// The fields `select` projects, in the order they were written.
fn projection(
    arguments: &BoundArguments,
    spelling: &str,
    scope: &Arc<Scope>,
) -> Result<Vec<SelectField>, ErrorValue> {
    let mut fields = Vec::new();
    match arguments.selector_binding("fields") {
        Some(Binding::Expressions(expressions)) => {
            for expression in expressions {
                push_projection(&mut fields, expression, spelling, scope)?;
            }
        }
        // A quoted or defaulted list of names, which is the same projection written as data.
        Some(Binding::Value(Value::List(names))) => {
            for name in names.iter() {
                fields.push(SelectField::field(name.as_str()?));
            }
        }
        Some(Binding::Value(value)) => fields.push(SelectField::field(value.as_str()?)),
        None => {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{spelling}` needs at least one field"),
            )
            .with_help("write it as `select pid name memory`"));
        }
    }
    Ok(fields)
}

fn push_projection(
    fields: &mut Vec<SelectField>,
    expression: &Expr,
    spelling: &str,
    scope: &Arc<Scope>,
) -> Result<(), ErrorValue> {
    // `select {mem_mb: memory / 1MiB}` — a record literal names its own computed fields, which is
    // the one spelling spec §53 shows for them.
    if let Expr::Record(record) = expression {
        for field in &record.fields {
            let name = match &field.key {
                ono_parser::RecordKey::Ident { name, .. } => name.clone(),
                ono_parser::RecordKey::Str(literal) => literal
                    .literal_text()
                    .ok_or_else(|| {
                        ErrorValue::new(
                            ErrorCode::TypeMismatch,
                            format!("`{spelling}` needs a literal name for a computed field"),
                        )
                    })?
                    .to_owned(),
            };
            let body = field.value.clone();
            let scope = Arc::clone(scope);
            fields.push(SelectField::computed(&name, move |value: &Value| {
                evaluate(&body, value, &scope)
            }));
        }
        return Ok(());
    }

    match path_of(expression) {
        Some(segments) => fields.push(SelectField::path(segments)),
        None => {
            return Err(ErrorValue::new(
                ErrorCode::TypeUnknownField,
                format!("`{spelling}` projects field paths, and this argument is not one"),
            )
            .with_help("name a computed field, as in `select {mem_mb: memory / 1MiB}`"));
        }
    }
    Ok(())
}

/// The field path an expression writes, when it writes one.
fn path_of(expression: &Expr) -> Option<Vec<PathSegment>> {
    match expression {
        Expr::Path(path) => Some(vec![PathSegment::required(&path.name)]),
        Expr::Field(access) => {
            let mut segments = path_of(&access.base)?;
            segments.push(if access.optional {
                PathSegment::optional(&access.field)
            } else {
                PathSegment::required(&access.field)
            });
            Some(segments)
        }
        // `select @` projects the value itself, which `SelectField::path` names `value`.
        Expr::CurrentValue(reference) => {
            matches!(reference.selector, ono_parser::CurrentSelector::Current).then(Vec::new)
        }
        _ => None,
    }
}

fn needs_input(spelling: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!("`{spelling}` transforms a stream, and nothing was piped into it"),
    )
    .with_help("put a producer in front of it, as in `get process | ...`")
}
