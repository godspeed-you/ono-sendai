//! The expression compiler: a parsed expression, a record and a scope become a value.
//!
//! `ono-pipeline` deliberately knows nothing about syntax — [`Where`](ono_pipeline::Where) holds a
//! predicate, [`Sort`](ono_pipeline::Sort) holds a key function (ADR-0005). This module is what
//! turns `cpu > 20` into one of those, and it is where three rules of the language live:
//!
//! - **A bare identifier is a field of the current record.** Spec §10.3 allows a pipeline
//!   predicate to expose the current record's fields directly, "syntactic sugar for a
//!   current-value binding", and `where cpu > 20` is unreadable any other way.
//! - **`@` is the current value**, `@-1` an earlier pipeline's result and `@3` an item of the
//!   current one (spec §6.4). Those come from the [`Scope`] the invocation carries.
//! - **`$name` is a shell binding**, also from the [`Scope`]. Unbound is `null`, as it is
//!   everywhere else in the shell.
//!
//! ADR-0014's three-valued semantics are implemented here exactly: a comparison with an unknown
//! is unknown, `and`/`or`/`not` are Kleene, `x == null` is a total identity test, and a field read
//! that *failed* stays an error rather than degrading into an unknown.

use std::collections::BTreeMap;
use std::sync::Arc;

use ono_core::{ErrorCode, Span};
use ono_parser::{
    BinaryOp, CurrentSelector, Expr, FieldAccess, NumberValue, ParenValue, RecordKey, StrPart,
    UnaryOp, Unit,
};
use ono_value::{
    ByteSize, Duration, ErrorValue, FieldStep, MapValue, Percent, RegexValue, Schema, Value,
};

use crate::suggest::closest;

/// What an expression can see besides the record in front of it.
///
/// The evaluator owns the session; this is the part of it a command needs. It is passed in rather
/// than reached for, so a transform's behaviour is a function of its inputs and a test can state
/// the whole world an expression runs in.
///
/// ```
/// use ono_command::Scope;
/// use ono_value::Value;
///
/// let scope = Scope::new().with_variable("limit", Value::Int(20));
/// assert_eq!(scope.variable("limit"), Some(&Value::Int(20)));
/// assert_eq!(scope.variable("nothing"), None);
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scope {
    variables: BTreeMap<String, Value>,
    current: Option<Value>,
    previous: Vec<Value>,
    items: Vec<Value>,
    pipelines: BTreeMap<Span, Value>,
}

impl Scope {
    /// An empty scope: no bindings, no current value, no history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `$name`.
    #[must_use]
    pub fn with_variable(mut self, name: &str, value: Value) -> Self {
        self.variables.insert(name.to_owned(), value);
        self
    }

    /// Binds `@` explicitly, for a scope that has a current value of its own.
    ///
    /// Inside a transform `@` is the record being processed, and this is not needed; it matters
    /// for a nested block, where `@` names the item the block iterates (spec §19.4).
    #[must_use]
    pub fn with_current(mut self, value: Value) -> Self {
        self.current = Some(value);
        self
    }

    /// Records the results of earlier pipelines, most recent first, so `@-1` names one.
    #[must_use]
    pub fn with_previous(mut self, results: impl IntoIterator<Item = Value>) -> Self {
        self.previous = results.into_iter().collect();
        self
    }

    /// Records the items of the current result, so `@3` names one (spec §6.4).
    #[must_use]
    pub fn with_items(mut self, items: impl IntoIterator<Item = Value>) -> Self {
        self.items = items.into_iter().collect();
        self
    }

    /// Records the value a parenthesised pipeline produced, keyed by the span of the
    /// parentheses, so an expression that names `(get socket)` reads the values the evaluator
    /// already collected (ADR-0072 §4). Running a pipeline is the evaluator's job, not this
    /// crate's (ADR-0005); the evaluator finds them with [`nested_pipelines`].
    #[must_use]
    pub fn with_pipeline_result(mut self, span: Span, value: Value) -> Self {
        self.pipelines.insert(span, value);
        self
    }

    /// The value bound to `$name`, if anything is.
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }

    /// The pre-run value of the parenthesised pipeline at `span`, if the evaluator ran it.
    #[must_use]
    pub fn pipeline_result(&self, span: Span) -> Option<&Value> {
        self.pipelines.get(&span)
    }

    /// The value bound to `@`, if the scope carries one of its own.
    #[must_use]
    pub fn current(&self) -> Option<&Value> {
        self.current.as_ref()
    }

    /// The result of the pipeline `back` runs ago; `@-1` asks for `back == 1`.
    #[must_use]
    pub fn previous(&self, back: u32) -> Option<&Value> {
        let index = usize::try_from(back).ok()?.checked_sub(1)?;
        self.previous.get(index)
    }

    /// Item `index` of the current result, numbered from one as `@1` is.
    #[must_use]
    pub fn item(&self, index: u32) -> Option<&Value> {
        let index = usize::try_from(index).ok()?.checked_sub(1)?;
        self.items.get(index)
    }
}

/// Evaluates `expression` with `current` as the record its bare identifiers name.
///
/// ```
/// use ono_command::{Scope, evaluate};
/// use ono_value::Value;
///
/// let parsed = ono_parser::parse("where 40 > 20");
/// let stage = &parsed.program().statements[0]
///     .as_pipeline()
///     .expect("a pipeline")
///     .head
///     .stages[0];
/// let expression = stage.arguments[0].as_value().expect("an expression");
/// assert_eq!(
///     evaluate(expression, &Value::Null, &Scope::new())?,
///     Value::Bool(true)
/// );
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
///
/// # Errors
///
/// Returns the structured error of whatever went wrong: `type.unknown_field` for a field the
/// record does not have, `type.mismatch` for an operation the operands do not support,
/// `type.invalid_unit` for two dimensions that do not meet, and the recorded failure when a
/// field's access failed.
pub fn evaluate(expression: &Expr, current: &Value, scope: &Scope) -> Result<Value, ErrorValue> {
    match expression {
        Expr::Number(literal) => Ok(number(literal.value)),
        Expr::Unit(literal) => quantity(literal.value, literal.unit),
        Expr::Bool(value, _) => Ok(Value::Bool(*value)),
        Expr::Null(_) => Ok(Value::Null),
        Expr::Str(literal) => {
            let mut text = String::new();
            for part in &literal.parts {
                match part {
                    StrPart::Text { text: raw, .. } => text.push_str(raw),
                    StrPart::Expr(inner) => {
                        let value = evaluate(inner, current, scope)?;
                        text.push_str(&ono_value::canonical_text(&value)?);
                    }
                }
            }
            Ok(Value::string(&text))
        }
        Expr::Timestamp(literal) => Value::parse_timestamp(&literal.text),
        Expr::Regex(literal) => {
            // Flags become an inline group, which is how the engine spells them and keeps the
            // pattern one thing rather than a pattern plus a side channel.
            let pattern = if literal.flags.is_empty() {
                literal.pattern.clone()
            } else {
                format!("(?{}){}", literal.flags, literal.pattern)
            };
            Ok(Value::Regex(Arc::new(RegexValue::new(&pattern)?)))
        }
        Expr::Ip(literal) => literal
            .text
            .split('%')
            .next()
            .unwrap_or(&literal.text)
            .parse()
            .map(Value::Ip)
            .map_err(|_| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{}` is not an IP address", literal.text),
                )
            }),
        // A binding nobody made is unknown, which is what the shell answers everywhere else.
        Expr::Variable(variable) => Ok(scope
            .variable(&variable.name)
            .cloned()
            .unwrap_or(Value::Null)),
        Expr::CurrentValue(reference) => match reference.selector {
            CurrentSelector::Current => Ok(scope.current().unwrap_or(current).clone()),
            CurrentSelector::Previous(back) => {
                Ok(scope.previous(back).cloned().unwrap_or_default())
            }
            CurrentSelector::Item(index) => Ok(scope.item(index).cloned().unwrap_or_default()),
        },
        // Spec §10.3: within a pipeline predicate the current record exposes its fields directly.
        Expr::Path(path) => current.follow(&[FieldStep::required(&path.name)]),
        // A chain `local.port` is followed as one path from its receiver, so that a null a
        // schema-known field answered with stays unknown through the steps beneath it instead
        // of becoming a type error at the next one (spec §10.5, ADR-0089).
        Expr::Field(access) => {
            let mut steps = vec![field_step(access)];
            let mut receiver = &access.base;
            while let Expr::Field(inner) = receiver {
                steps.push(field_step(inner));
                receiver = &inner.base;
            }
            let base = match receiver {
                Expr::Path(path) => {
                    steps.push(FieldStep::required(&path.name));
                    current.clone()
                }
                other => evaluate(other, current, scope)?,
            };
            steps.reverse();
            base.follow(&steps)
        }
        Expr::Index(index) => {
            let base = evaluate(&index.base, current, scope)?;
            let key = evaluate(&index.index, current, scope)?;
            index_into(&base, &key)
        }
        Expr::List(list) => list
            .items
            .iter()
            .map(|item| evaluate(item, current, scope))
            .collect::<Result<Vec<Value>, ErrorValue>>()
            .map(Value::list),
        Expr::Record(record) => {
            let mut map = MapValue::new();
            for field in &record.fields {
                let key = record_key(&field.key, current, scope)?;
                map.insert(key.into(), evaluate(&field.value, current, scope)?);
            }
            Ok(Value::Map(Arc::new(map)))
        }
        Expr::Paren(inner) => match inner.expression() {
            Some(expression) => evaluate(expression, current, scope),
            // A parenthesised pipeline has to be run, and running one is the evaluator's job:
            // it hands the result in through the scope, or this is an honest refusal.
            None => scope
                .pipeline_result(inner.span)
                .cloned()
                .ok_or_else(|| needs_evaluator("a parenthesised pipeline")),
        },
        Expr::Block(_) => Err(needs_evaluator("a block")),
        Expr::Unary(unary) => {
            let operand = evaluate(&unary.operand, current, scope)?;
            match unary.op {
                // `not null` is null: negating an unknown leaves it unknown (ADR-0014).
                UnaryOp::Not => Ok(match operand {
                    Value::Null => Value::Null,
                    other => Value::Bool(!is_true(&other)),
                }),
                UnaryOp::Neg => match operand {
                    Value::Null => Ok(Value::Null),
                    other => Value::Int(0).sub(&other),
                },
            }
        }
        Expr::Binary(binary) => binary_op(binary, current, scope),
        Expr::Call(call) if is_now_call(call) => Ok(Value::now()),
        Expr::Call(call) => Err(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            format!("no function to call at {}", call.span),
        )
        .with_help(
            "`now()` is the only function an expression can call; a user function is called as \
             a command (spec §19.3, ADR-0070)",
        )),
        Expr::Error(span) => Err(ErrorValue::new(
            ErrorCode::ParseSyntax,
            format!("this expression could not be read at {span}"),
        )),
    }
}

/// Whether a call is `now()`, the one builtin function `language.yaml` declares (spec §6.3,
/// ADR-0071).
#[must_use]
pub fn is_now_call(call: &ono_parser::CallExpr) -> bool {
    matches!(&call.callee, Expr::Path(path) if path.name == "now") && call.arguments.is_empty()
}

/// Evaluates `expression` and reports a failure as the error *value* it is.
///
/// This is the form a predicate needs: ADR-0014 gives `where` four answers, and an error is one of
/// them rather than a reason to stop the pipeline (spec §16.5).
#[must_use]
pub fn evaluate_to_value(expression: &Expr, current: &Value, scope: &Scope) -> Value {
    match evaluate(expression, current, scope) {
        Ok(value) => value,
        Err(error) => error.into_value(),
    }
}

/// Whether a value counts as true where a condition is wanted.
///
/// Only `true` is true: `null` is unknown and therefore not a match, which is what makes `where`
/// admit only decided rows (ADR-0014).
#[must_use]
pub fn is_true(value: &Value) -> bool {
    matches!(value, Value::Bool(true))
}

/// Every parenthesised pipeline `expression` contains, outermost first.
///
/// The evaluator runs these before the stage that names them and hands the results in through
/// [`Scope::with_pipeline_result`] (ADR-0072 §4). A pipeline nested inside another one's
/// arguments is that pipeline's to run, so the walk stops at the parentheses.
#[must_use]
pub fn nested_pipelines(expression: &Expr) -> Vec<&ParenValue> {
    let mut found = Vec::new();
    collect_pipelines(expression, &mut found);
    found
}

fn collect_pipelines<'a>(expression: &'a Expr, found: &mut Vec<&'a ParenValue>) {
    match expression {
        Expr::Paren(inner) => match inner.expression() {
            Some(expression) => collect_pipelines(expression, found),
            None => found.push(inner),
        },
        Expr::Unary(unary) => collect_pipelines(&unary.operand, found),
        Expr::Binary(binary) => {
            collect_pipelines(&binary.lhs, found);
            collect_pipelines(&binary.rhs, found);
        }
        Expr::Field(access) => collect_pipelines(&access.base, found),
        Expr::Index(index) => {
            collect_pipelines(&index.base, found);
            collect_pipelines(&index.index, found);
        }
        Expr::List(list) => list
            .items
            .iter()
            .for_each(|item| collect_pipelines(item, found)),
        Expr::Record(record) => record
            .fields
            .iter()
            .for_each(|field| collect_pipelines(&field.value, found)),
        Expr::Call(call) => {
            collect_pipelines(&call.callee, found);
            call.arguments
                .iter()
                .for_each(|argument| collect_pipelines(argument, found));
        }
        Expr::Str(literal) => literal.parts.iter().for_each(|part| {
            if let StrPart::Expr(inner) = part {
                collect_pipelines(inner, found);
            }
        }),
        Expr::Number(_)
        | Expr::Unit(_)
        | Expr::Regex(_)
        | Expr::Timestamp(_)
        | Expr::Ip(_)
        | Expr::Bool(_, _)
        | Expr::Null(_)
        | Expr::Variable(_)
        | Expr::CurrentValue(_)
        | Expr::Path(_)
        | Expr::Block(_)
        | Expr::Error(_) => {}
    }
}

/// Checks every bare field name `expression` reads against `schema`, before anything runs.
///
/// This is the check spec §11.3 asks for: `get process | where cpy > 20` reports
/// `type.unknown_field` with a suggestion, and nothing is enumerated. Only root-level identifiers
/// are checked, because a schema declares its own fields and not the shape of a nested value; and
/// a step written with `?.` is skipped, because `?.` is exactly the opt-in to a runtime lookup
/// (spec §11.4, ADR-0014).
///
/// # Errors
///
/// `type.unknown_field` naming the field, the schema and the nearest declared field.
pub fn check_fields(expression: &Expr, schema: &Schema) -> Result<(), ErrorValue> {
    match expression {
        Expr::Path(path) => match schema.field(&path.name) {
            Some(_) => Ok(()),
            None => Err(unknown_field(&path.name, schema)),
        },
        Expr::Field(access) => {
            if access.optional {
                // `?.` opts the whole access into a runtime lookup, which is the point of it.
                return Ok(());
            }
            check_fields(&access.base, schema)
        }
        Expr::Index(index) => {
            check_fields(&index.base, schema)?;
            check_fields(&index.index, schema)
        }
        Expr::Unary(unary) => check_fields(&unary.operand, schema),
        Expr::Binary(binary) => {
            // `where state == failed`: a bare word that is one of the other side's declared
            // enum values is that value, not a field (ADR-0096).
            if let Some((_, word)) = enum_word(binary, schema) {
                let field = if std::ptr::eq(word, &binary.lhs) {
                    &binary.rhs
                } else {
                    &binary.lhs
                };
                return check_fields(field, schema);
            }
            check_fields(&binary.lhs, schema)?;
            check_fields(&binary.rhs, schema)
        }
        Expr::List(list) => list
            .items
            .iter()
            .try_for_each(|item| check_fields(item, schema)),
        Expr::Record(record) => record
            .fields
            .iter()
            .try_for_each(|field| check_fields(&field.value, schema)),
        Expr::Str(literal) => literal.parts.iter().try_for_each(|part| match part {
            StrPart::Expr(inner) => check_fields(inner, schema),
            StrPart::Text { .. } => Ok(()),
        }),
        Expr::Paren(inner) => match inner.expression() {
            Some(expression) => check_fields(expression, schema),
            None => Ok(()),
        },
        Expr::Call(call) => call
            .arguments
            .iter()
            .try_for_each(|argument| check_fields(argument, schema)),
        Expr::Number(_)
        | Expr::Unit(_)
        | Expr::Regex(_)
        | Expr::Timestamp(_)
        | Expr::Ip(_)
        | Expr::Bool(_, _)
        | Expr::Null(_)
        | Expr::Variable(_)
        | Expr::CurrentValue(_)
        | Expr::Block(_)
        | Expr::Error(_) => Ok(()),
    }
}

/// The bare word of a comparison that names one of the other side's declared enum values, with
/// that value — `failed` in `state == failed` over a schema whose `state` is an enum with
/// `failed` among its values (ADR-0096).
///
/// Only a comparison qualifies, only when the word is not itself a field of the schema (a field
/// wins, as spec §10.3 says), and only for a field declared `enum`: a string field's comparand
/// stays a field lookup, so `where name == foo` still reports `foo` as unknown.
fn enum_word<'e>(
    binary: &'e ono_parser::BinaryExpr,
    schema: &Schema,
) -> Option<(&'e str, &'e Expr)> {
    if !matches!(
        binary.op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq
    ) {
        return None;
    }
    let variants_of = |expression: &Expr| match expression {
        Expr::Path(path) => match schema.field(&path.name).map(ono_value::FieldDef::ty) {
            Some(ono_value::FieldType::Enum(variants)) => Some(variants.clone()),
            _ => None,
        },
        _ => None,
    };
    let word_of = |expression: &'e Expr| match expression {
        Expr::Path(path) if schema.field(&path.name).is_none() => Some(path.name.as_str()),
        _ => None,
    };
    let candidate = |field: &Expr, word: &'e Expr| {
        let variants = variants_of(field)?;
        let name = word_of(word)?;
        variants
            .iter()
            .any(|variant| &**variant == name)
            .then_some((name, word))
    };
    candidate(&binary.lhs, &binary.rhs).or_else(|| candidate(&binary.rhs, &binary.lhs))
}

/// The variants of the enum field an ordering comparison names, in declared order.
///
/// `None` unless the operator orders — `<`, `<=`, `>`, `>=` — and one side is a path to a field
/// the schema declares as an enum. Equality never needs this: two names are equal or they are
/// not, whatever their positions (ADR-0222).
fn ordered_enum(binary: &ono_parser::BinaryExpr, schema: &Schema) -> Option<Arc<[Arc<str>]>> {
    if !matches!(
        binary.op,
        BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq
    ) {
        return None;
    }
    let variants_of = |expression: &Expr| match expression {
        Expr::Path(path) => match schema.field(&path.name).map(ono_value::FieldDef::ty) {
            Some(ono_value::FieldType::Enum(variants)) => Some(variants.clone()),
            _ => None,
        },
        _ => None,
    };
    variants_of(&binary.lhs).or_else(|| variants_of(&binary.rhs))
}

/// A value's position among `variants`, or `None` when it names none of them.
fn rank(value: &Value, variants: &[Arc<str>]) -> Option<usize> {
    let name = value.as_str().ok()?;
    variants.iter().position(|variant| &**variant == name)
}

/// One step of a field chain, optional where it was written `?.`.
fn field_step(access: &FieldAccess) -> FieldStep<'_> {
    if access.optional {
        FieldStep::optional(&access.field)
    } else {
        FieldStep::required(&access.field)
    }
}

fn unknown_field(name: &str, schema: &Schema) -> ErrorValue {
    let error = ErrorValue::new(
        ErrorCode::TypeUnknownField,
        format!("unknown field `{name}` on {}", schema.name()),
    );
    match closest(name, schema.fields().iter().map(ono_value::FieldDef::name)) {
        Some(near) => error.with_help(format!("perhaps: {near}")),
        None => error.with_help(format!("`{}` declares no field `{name}`", schema.id())),
    }
}

fn needs_evaluator(what: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderUnsupported,
        format!("{what} has to be run, and running one is the evaluator's job"),
    )
    .with_help("write the value out, or run the sub-pipeline into a variable first")
}

fn record_key(key: &RecordKey, current: &Value, scope: &Scope) -> Result<String, ErrorValue> {
    match key {
        RecordKey::Ident { name, .. } => Ok(name.clone()),
        RecordKey::Str(literal) => {
            let value = evaluate(&Expr::Str(literal.clone()), current, scope)?;
            ono_value::canonical_text(&value)
        }
    }
}

fn number(value: NumberValue) -> Value {
    match value {
        NumberValue::Int(int) => Value::Int(i128::from(int)),
        NumberValue::Float(float) => Value::Float(float),
    }
}

/// A numeric literal with a unit suffix becomes the semantic scalar of spec §10.6.
fn quantity(value: NumberValue, unit: Unit) -> Result<Value, ErrorValue> {
    let magnitude = match value {
        NumberValue::Int(int) => int as f64,
        NumberValue::Float(float) => float,
    };
    let suffix = unit.as_str();
    if let Some(bytes) = ono_value::ByteUnit::from_suffix(suffix) {
        let total = magnitude * bytes.factor() as f64;
        if !total.is_finite() || total < 0.0 {
            return Err(ErrorValue::new(
                ErrorCode::TypeInvalidUnit,
                format!("{magnitude}{suffix} is not a byte size"),
            ));
        }
        return Ok(Value::ByteSize(ByteSize::from_bytes(total as u128)));
    }
    if let Some(time) = ono_value::DurationUnit::from_suffix(suffix) {
        let nanoseconds = magnitude * time.nanoseconds() as f64;
        if !nanoseconds.is_finite() {
            return Err(ErrorValue::new(
                ErrorCode::TypeInvalidUnit,
                format!("{magnitude}{suffix} is not a duration"),
            ));
        }
        return Ok(Value::Duration(Duration::from_nanoseconds(
            nanoseconds as i128,
        )));
    }
    Ok(Value::Percent(Percent::new(magnitude)))
}

fn binary_op(
    binary: &ono_parser::BinaryExpr,
    current: &Value,
    scope: &Scope,
) -> Result<Value, ErrorValue> {
    // `and` and `or` short-circuit, so a question one operand already decided is not made unknown
    // by the other, and an operand that would fail is not evaluated at all.
    match binary.op {
        BinaryOp::And => {
            let left = evaluate(&binary.lhs, current, scope)?;
            if matches!(left, Value::Bool(false)) {
                return Ok(Value::Bool(false));
            }
            let right = evaluate(&binary.rhs, current, scope)?;
            return Ok(kleene_and(&left, &right));
        }
        BinaryOp::Or => {
            let left = evaluate(&binary.lhs, current, scope)?;
            if matches!(left, Value::Bool(true)) {
                return Ok(Value::Bool(true));
            }
            let right = evaluate(&binary.rhs, current, scope)?;
            return Ok(kleene_or(&left, &right));
        }
        _ => {}
    }

    // The record's own schema decides whether a bare word is one of an enum field's values
    // (ADR-0096), exactly as the pre-flight check decided it against the advertised schema.
    let enum_value = match current {
        Value::Record(record) => enum_word(binary, record.schema()),
        _ => None,
    };
    let operand = |side: &Expr| -> Result<Value, ErrorValue> {
        match enum_value {
            Some((value, word)) if std::ptr::eq(word, side) => Ok(Value::string(value)),
            _ => evaluate(side, current, scope),
        }
    };
    let left = operand(&binary.lhs)?;
    let right = operand(&binary.rhs)?;

    // `x == null` is an identity test rather than a three-valued comparison (ADR-0014): without
    // the exception the commonest question anyone asks would silently match nothing.
    if matches!(binary.op, BinaryOp::Eq | BinaryOp::NotEq)
        && (matches!(&binary.lhs, Expr::Null(_)) || matches!(&binary.rhs, Expr::Null(_)))
    {
        let other = if matches!(&binary.lhs, Expr::Null(_)) {
            &right
        } else {
            &left
        };
        let unknown = other.is_null();
        return Ok(Value::Bool(match binary.op {
            BinaryOp::Eq => unknown,
            _ => !unknown,
        }));
    }

    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }

    // An enum is a closed set written from least to greatest, so `where level >= error` orders by
    // severity rather than by spelling — the comparison spec §41.4 writes and the one alphabet
    // gets backwards (ADR-0222). Only an ordering comparison over a field the schema declares as
    // an enum takes this reading; equality is equality either way.
    if let Value::Record(record) = current
        && let Some(variants) = ordered_enum(binary, record.schema())
        && let (Some(left), Some(right)) = (rank(&left, &variants), rank(&right, &variants))
    {
        let ordering = left.cmp(&right);
        return Ok(Value::Bool(match binary.op {
            BinaryOp::Lt => ordering.is_lt(),
            BinaryOp::LtEq => ordering.is_le(),
            BinaryOp::Gt => ordering.is_gt(),
            // `ordered_enum` answers for no other operator.
            _ => ordering.is_ge(),
        }));
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

fn kleene_and(left: &Value, right: &Value) -> Value {
    match (left, right) {
        (Value::Bool(false), _) | (_, Value::Bool(false)) => Value::Bool(false),
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        _ => Value::Bool(is_true(left) && is_true(right)),
    }
}

fn kleene_or(left: &Value, right: &Value) -> Value {
    match (left, right) {
        (Value::Bool(true), _) | (_, Value::Bool(true)) => Value::Bool(true),
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        _ => Value::Bool(is_true(left) || is_true(right)),
    }
}

fn equals(left: &Value, right: &Value) -> bool {
    if left == right {
        return true;
    }
    left.compare_to(right).is_ok_and(std::cmp::Ordering::is_eq)
}

fn contains(haystack: &Value, needle: &Value) -> bool {
    match haystack {
        Value::List(items) => items.iter().any(|item| equals(item, needle)),
        Value::String(text) => {
            ono_value::canonical_text(needle).is_ok_and(|needle| text.contains(&needle))
        }
        _ => false,
    }
}

fn regex_matches(pattern: &Value, subject: &Value) -> Result<bool, ErrorValue> {
    let regex = pattern.as_regex()?;
    let text = ono_value::canonical_text(subject)?;
    Ok(regex.is_match(&text))
}

fn remainder(left: &Value, right: &Value) -> Result<Value, ErrorValue> {
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

fn index_into(base: &Value, key: &Value) -> Result<Value, ErrorValue> {
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
