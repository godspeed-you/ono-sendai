//! The check spec §11.3 asks for: field names are verified against the schema flowing into a
//! stage *before* the pipeline runs.
//!
//! ```text
//! get process | where cpy > 20
//!
//! unknown field `cpy` on Process
//! perhaps: cpu
//! ```
//!
//! Spec §11.3 says outright that this "can happen before process enumeration begins because
//! `get process` advertises `Stream<Process>`", and ADR-0013 makes it the *check* step, ahead of
//! planning and running. Everything this needs is declarative — the contract's output type, and
//! the schemas the providers advertise — so nothing is enumerated, nothing is spawned, and a typo
//! costs nothing.

use std::sync::Arc;

use ono_parser::{Argument, Pipeline, Stage};
use ono_value::{ErrorValue, Schema};

use crate::contract::{ArgumentMode, IoType};
use crate::expr::check_fields;
use crate::registry::CommandRegistry;

/// Checks every expression in `pipeline` against the schema that would reach it.
///
/// `schemas` is what the providers advertise, which is where an output type such as
/// `stream<ono.process/1>` is resolved to the fields `ono.process/1` actually declares. A stage
/// whose element type is unknown — because nothing advertises it, or because an upstream stage
/// reshaped the stream — is not checked rather than guessed at: spec §11.3 asks for the check
/// "where schemas are known", and inventing an answer where they are not would reject valid
/// pipelines.
///
/// ```
/// use ono_command::{CommandRegistry, check_pipeline};
///
/// let registry = CommandRegistry::embedded()?;
/// let schemas: Vec<_> = ono_value::builtin_schemas().schemas().cloned().collect();
/// let parsed = ono_parser::parse("get process | where cpy > 20");
/// let pipeline = parsed.program().statements[0].as_pipeline().expect("a pipeline");
///
/// let error = check_pipeline(registry, &schemas, pipeline).expect_err("`cpy` is not a field");
/// assert_eq!(error.code(), ono_core::ErrorCode::TypeUnknownField);
/// assert_eq!(error.help(), Some("perhaps: cpu"));
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
///
/// # Errors
///
/// `type.unknown_field` naming the field, the schema and the nearest declared field. Only the
/// first such field is reported: spec §15.4's help is a suggestion, and a list of six of them is
/// not one.
pub fn check_pipeline(
    registry: &CommandRegistry,
    schemas: &[Arc<Schema>],
    pipeline: &Pipeline,
) -> Result<(), ErrorValue> {
    check_pipeline_with(registry, schemas, pipeline, &[])
}

/// As [`check_pipeline`], knowing which stages an adapter gives a schema (ADR-0067):
/// `adapted[i]` is the schema id of stage `i` when a program there is adapted, so the field
/// check reaches the stages after it exactly as it reaches them after a native producer.
///
/// # Errors
///
/// As [`check_pipeline`].
pub fn check_pipeline_with(
    registry: &CommandRegistry,
    schemas: &[Arc<Schema>],
    pipeline: &Pipeline,
    adapted: &[Option<String>],
) -> Result<(), ErrorValue> {
    let mut element: Vec<Arc<Schema>> = Vec::new();
    let mut index = 0;
    let lists =
        std::iter::once(&pipeline.head).chain(pipeline.tail.iter().map(|chained| &chained.list));
    for list in lists {
        for stage in &list.stages {
            let adapted_here = adapted
                .get(index)
                .and_then(Option::as_deref)
                .and_then(|id| id.parse::<ono_value::SchemaId>().ok())
                .and_then(|id| schemas.iter().find(|schema| *schema.id() == id).cloned());
            element = match adapted_here {
                Some(schema) => vec![schema],
                None => check_stage(registry, schemas, stage, &element)?,
            };
            index += 1;
        }
    }
    Ok(())
}

/// Checks one stage and reports the schema its output carries.
fn check_stage(
    registry: &CommandRegistry,
    schemas: &[Arc<Schema>],
    stage: &Stage,
    upstream: &[Arc<Schema>],
) -> Result<Vec<Arc<Schema>>, ErrorValue> {
    let Some(head) = stage.head.name() else {
        // A value head — a variable, a parenthesised pipeline — carries no declared schema.
        return Ok(Vec::new());
    };
    let Ok(resolved) = registry.resolve(head, &stage.arguments) else {
        // Not a native command: an external program's output has no schema to check against, and
        // ADR-0011 puts `PATH` after the registry rather than instead of it.
        return Ok(Vec::new());
    };
    let contract = resolved.contract;

    if contract.argument_mode() == ArgumentMode::Expression && !upstream.is_empty() {
        // The check is type-aware, not blind: only an expression bound to a parameter that
        // carries *values* reads fields from the stream. A word bound to a string parameter —
        // `sort cpu desc`'s direction, spec §6.3's own spelling — is vocabulary, and rejecting
        // it as an unknown field would refuse the specification's own examples.
        if let Ok(bound) = contract.bind(resolved.arguments) {
            let field_bearing = |name: &str| {
                contract
                    .selector(name)
                    .or_else(|| contract.option(name))
                    .is_none_or(|spec| spec.declared_type() != &crate::DeclaredType::String)
            };
            let schema = upstream;
            for (name, binding) in bound.selectors().iter().chain(bound.options()) {
                if !field_bearing(name) {
                    continue;
                }
                // `sort desc` with no key: the word is the direction over the identity key,
                // not a field (ADR-0071 §3), exactly as the transform will read it.
                if contract.id() == "ono.data.sort" && name == "key" && bare_direction(&bound) {
                    continue;
                }
                if let crate::Binding::Expressions(expressions) = binding {
                    for expression in expressions {
                        check_against_any(expression, schema)?;
                    }
                }
            }
        } else {
            for argument in resolved.arguments {
                if let Argument::Value(expression) = argument {
                    check_against_any(expression, upstream)?;
                }
            }
        }
    }

    Ok(output_schemas(contract.output(), schemas, upstream))
}

/// Checks an expression against a stage's element type, which a union output makes a *set* of
/// schemas rather than one: `get config --problems` streams `ono.error/1` where `get config`
/// streams `ono.config-setting/1`, and the option that decides which is written on the stage
/// itself (ADR-0218).
///
/// A field is unknown only when no alternative declares it. The reported error is the first
/// alternative's, so the message still names a schema and its nearest field.
fn check_against_any(
    expression: &ono_parser::Expr,
    schemas: &[Arc<Schema>],
) -> Result<(), ErrorValue> {
    let mut first: Option<ErrorValue> = None;
    for schema in schemas {
        match check_fields(expression, schema) {
            Ok(()) => return Ok(()),
            Err(error) => {
                first.get_or_insert(error);
            }
        }
    }
    first.map_or(Ok(()), Err)
}

/// The schema flowing out of the last stage of `pipeline`, for completion (spec §15.1,
/// ADR-0074): what `get process | where cpu > 1 |` hands the stage being typed.
///
/// Nothing is checked here — a typo upstream is the check's to report when the line runs — and
/// nothing is guessed: a stage that is not a native command, or one that reshapes the stream
/// into something undeclared, leaves the schema unknown.
pub(crate) fn schema_after(
    registry: &CommandRegistry,
    schemas: &[Arc<Schema>],
    pipeline: &Pipeline,
) -> Option<Arc<Schema>> {
    let mut element: Option<Arc<Schema>> = None;
    let lists =
        std::iter::once(&pipeline.head).chain(pipeline.tail.iter().map(|chained| &chained.list));
    for list in lists {
        for stage in &list.stages {
            let head = stage.head.name()?;
            let contract = registry.resolve(head, &stage.arguments).ok()?.contract;
            let carried = element.map_or_else(Vec::new, |schema| vec![schema]);
            element = match output_schemas(contract.output(), schemas, &carried).as_slice() {
                [only] => Some(Arc::clone(only)),
                _ => None,
            };
        }
    }
    element
}

/// Whether `sort`'s key is a bare `asc`/`desc` standing in for an unwritten direction
/// (ADR-0071 §3). A written direction arrives as an expression; the default, as a value.
fn bare_direction(bound: &crate::BoundArguments) -> bool {
    let direction_written = matches!(
        bound.selector_binding("direction"),
        Some(crate::Binding::Expressions(_))
    );
    !direction_written
        && matches!(
            bound.selector_expression("key"),
            Some(ono_parser::Expr::Path(path)) if matches!(path.name.as_str(), "asc" | "desc")
        )
}

/// The schemas flowing out of a stage: its own where it names them, the upstream's where its
/// output type is open, and none where it reshapes the stream into something undeclared.
///
/// A union output — `stream<ono.config-setting/1 | ono.error/1>` — names every element type the
/// stage may produce, and every one of them is carried (ADR-0218). An alternative nothing
/// advertises makes the whole element type unknown rather than a subset that would reject a
/// field the missing schema declares.
fn output_schemas(
    output: &IoType,
    schemas: &[Arc<Schema>],
    upstream: &[Arc<Schema>],
) -> Vec<Arc<Schema>> {
    if output.is_open() {
        return upstream.to_vec();
    }
    let mut carried = Vec::new();
    for reference in output.schema_references() {
        let Ok(id) = reference.parse::<ono_value::SchemaId>() else {
            return Vec::new();
        };
        match schemas.iter().find(|schema| *schema.id() == id) {
            Some(schema) => carried.push(Arc::clone(schema)),
            None => return Vec::new(),
        }
    }
    carried
}
