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
    let mut element: Option<Arc<Schema>> = None;
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
                Some(schema) => Some(schema),
                None => check_stage(registry, schemas, stage, element)?,
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
    upstream: Option<Arc<Schema>>,
) -> Result<Option<Arc<Schema>>, ErrorValue> {
    let Some(head) = stage.head.name() else {
        // A value head — a variable, a parenthesised pipeline — carries no declared schema.
        return Ok(None);
    };
    let Ok(resolved) = registry.resolve(head, &stage.arguments) else {
        // Not a native command: an external program's output has no schema to check against, and
        // ADR-0011 puts `PATH` after the registry rather than instead of it.
        return Ok(None);
    };
    let contract = resolved.contract;

    if contract.argument_mode() == ArgumentMode::Expression
        && let Some(schema) = upstream.as_deref()
    {
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
            for (name, binding) in bound.selectors().iter().chain(bound.options()) {
                if !field_bearing(name) {
                    continue;
                }
                if let crate::Binding::Expressions(expressions) = binding {
                    for expression in expressions {
                        check_fields(expression, schema)?;
                    }
                }
            }
        } else {
            for argument in resolved.arguments {
                if let Argument::Value(expression) = argument {
                    check_fields(expression, schema)?;
                }
            }
        }
    }

    Ok(output_schema(contract.output(), schemas, upstream))
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
            element = output_schema(contract.output(), schemas, element);
        }
    }
    element
}

/// The schema flowing out of a stage: its own where it names one, the upstream's where its output
/// type is open, and nothing where it reshapes the stream into something undeclared.
fn output_schema(
    output: &IoType,
    schemas: &[Arc<Schema>],
    upstream: Option<Arc<Schema>>,
) -> Option<Arc<Schema>> {
    if output.is_open() {
        return upstream;
    }
    let id: ono_value::SchemaId = output.element_schema()?.parse().ok()?;
    schemas.iter().find(|schema| *schema.id() == id).cloned()
}
