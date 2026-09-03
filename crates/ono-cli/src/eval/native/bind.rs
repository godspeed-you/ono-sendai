//! Binding a native stage, and the scope its expressions read.
//!
//! One definition of "bound" for both callers — the drained segment of `foreground` and the
//! streaming continuation of v0.4.1 §26.2 below — so the two cannot drift apart in what a bound
//! stage is.

use ono_command::{BoundArguments, CommandContract, CommandRegistry, Invocation, Outcome, Scope};
use ono_core::ErrorCode;
use ono_parser::{Stage, StageList};
use ono_pipeline::ValueStream;
use ono_value::{ActionStatus, ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::Session;

use super::result::action_records;
use super::segment::{continuable_list, head_name, native_contract, refuse_switched_off_spatial};
use super::{implementations, registry};

/// Binds one stage of a native segment: the contract the registry places it at, its globs
/// expanded, its arguments resolved against that contract.
///
/// The one place a native stage is bound, so the streaming continuation of §26.2 and the drained
/// segment below cannot drift apart in what "bound" means. `None` is the registry declining to
/// place the stage here; what that means is the caller's to decide — a refusal for a segment
/// already claimed as native, a reason not to continue a stream for one that was only offered.
///
/// # Errors
///
/// The structured error of a glob that could not be expanded, an argument that does not resolve,
/// or a spatial command the settings have switched off.
pub(super) fn bind_stage(
    session: &mut Session,
    registry: &'static CommandRegistry,
    stage: &Stage,
    structured: bool,
) -> Eval<Option<(&'static CommandContract, BoundArguments)>> {
    let Some(contract) = native_contract(session, registry, stage, structured) else {
        return Ok(None);
    };
    refuse_switched_off_spatial(session, contract, stage)?;
    let arguments = crate::expand::expand_globs(session, &stage.arguments).map_err(Flow::Failed)?;
    let resolved = registry
        .resolve(head_name(stage), &arguments)
        .map_err(Flow::Failed)?;
    let arguments = contract.bind(resolved.arguments).map_err(Flow::Failed)?;
    Ok(Some((contract, arguments)))
}

/// A wholly-native pipeline, assembled into the stream it produces but not drained.
///
/// v0.4.1 §26.2: *"a function used as a pipeline stage SHOULD be able to stream values to
/// downstream stages when the function body itself streams"*, and *"the preferred v0.4.1 outcome
/// is streaming continuation rather than preservation of an accidental capture architecture"*.
/// This is that continuation: the body's stages are bound and assembled here, and the stream they
/// produce is what the caller's next stage reads, so nothing is collected in between and the
/// caller's `take 1` can answer before the body's source has ended.
///
/// §26.3 is satisfied by construction rather than by care: every expression a stage carries is
/// bound, and the `Scope` it will read is snapshotted, **while the invocation's scope is still on
/// the session**. What travels into the asynchronous producer is the snapshot, so no lexical
/// reference outlives the scope that owns it (ADR-0481).
///
/// `None` when the pipeline is not of a shape that can be continued — an external program, a
/// redirection, a serializer that ends the object stream, a `each { … }` block whose evaluator
/// belongs to another driver, or a stage the registry does not place. The caller then collects,
/// which is what it always did.
///
/// # Errors
///
/// The structured error of whichever stage could not be bound or started.
pub(crate) fn stream_segment(
    session: &mut Session,
    list: &StageList,
    source: &str,
) -> Eval<Option<(ValueStream, bool)>> {
    if !continuable_list(session, list) {
        return Ok(None);
    }
    let registry = registry().map_err(Flow::Failed)?;
    let table = implementations(session).map_err(Flow::Failed)?;

    let mut bound: Vec<(&'static CommandContract, BoundArguments)> = Vec::new();
    for stage in &list.stages {
        // `structured` is true throughout: a pipeline that can be continued is one where every
        // stage hands objects on, which `continuable_list` has already established.
        let Some(bound_stage) = bind_stage(session, registry, stage, true)? else {
            return Ok(None);
        };
        bound.push(bound_stage);
    }
    if bound.is_empty() {
        return Ok(None);
    }

    let scope = std::sync::Arc::new(stage_scope(session, &bound, source)?);
    let adapters = session.shared_adapters();
    let resolver = crate::resolve::resolver(session);
    let context = session.context();
    let materialization = crate::eval::materialize::limits(session);
    let (runtime, providers) = session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the pipeline runtime",
        ))
    })?;
    let handle = runtime.handle().clone();

    let assembled = handle.block_on(async {
        let mut stream: Option<ValueStream> = None;
        let mut failed_rows = false;
        for (contract, arguments) in &bound {
            let started = std::time::Instant::now();
            let mut invocation = Invocation::new(contract, arguments, providers)
                .with_scope(std::sync::Arc::clone(&scope))
                .with_context(context.clone())
                .with_adapters(std::sync::Arc::clone(&adapters), resolver.clone());
            if let Some(previous) = stream.take() {
                invocation = invocation.with_input(previous);
            }
            match table.run(contract.id(), &mut invocation).await {
                Ok(Outcome::Values(values)) => {
                    stream = Some(values.with_materialization_limits(materialization));
                }
                Ok(Outcome::Actions(outcomes)) => {
                    if outcomes
                        .iter()
                        .any(|outcome| outcome.status() == ActionStatus::Failed)
                    {
                        failed_rows = true;
                    }
                    stream = Some(
                        action_records(contract, outcomes, started)
                            .with_materialization_limits(materialization),
                    );
                }
                Err(error) => return Err(error),
            }
        }
        Ok((stream, failed_rows))
    });
    match assembled.map_err(Flow::Failed)? {
        (Some(stream), failed_rows) => Ok(Some((stream, failed_rows))),
        (None, _) => Ok(None),
    }
}

/// What the expressions of a native segment can see: the session's `$variables`, and the values
/// of every parenthesised pipeline written in an argument, run here and now (ADR-0072 §4).
///
/// `ono-command` evaluates expressions but never runs pipelines (ADR-0005), so
/// `join (get socket) --on pid` needs the evaluator to run `(get socket)` first and hand the
/// records in. They are keyed by the parentheses' span, which is unique within one source.
pub(super) fn stage_scope(
    session: &mut Session,
    bound: &[(&'static CommandContract, BoundArguments)],
    source: &str,
) -> Eval<Scope> {
    let mut scope = Scope::new();
    // v0.2 §20.2: `@-1` and `@N` name the results this session retained. A command argument that
    // writes one — v0.4 §28.2's `enter @-1` — reads the same values the pipeline head does, or
    // the reference would mean two different things in two positions of one language.
    let mut previous: Vec<Value> = Vec::new();
    for back in 1..=crate::session::DEEPEST_REFERENCE {
        match session.previous_result(back) {
            Some(values) => previous.push(Value::list(values.to_vec())),
            None => break,
        }
    }
    scope = scope.with_previous(previous);
    for (name, value) in session.bindings() {
        // `each { … }` binds the item it iterates as `@` (spec §19.4, ADR-0071 §1). Inside the
        // block that is the current value, not merely a variable spelled `@`, or the
        // specification's own `each { restart service @ }` would reach a native stage with
        // nothing bound (ADR-0219).
        if name == "@" {
            scope = scope.with_current(value.clone());
        }
        scope = scope.with_variable(&name, value);
    }
    // The session's effective settings travel as `config.<key>` bindings, so a command that
    // reads configuration — the bulk threshold of spec §11.6 (ADR-0082 §5) — sees the value
    // `set config` and the layers of ADR-0094 resolved, at its declared type.
    for (key, value) in session.settings().effective_values() {
        scope = scope.with_variable(&format!("config.{key}"), value.clone());
    }
    for (_, arguments) in bound {
        for (_, binding) in arguments.selectors().iter().chain(arguments.options()) {
            for expression in binding.expressions() {
                for nested in ono_command::nested_pipelines(expression) {
                    let Some(pipeline) = nested.pipeline() else {
                        continue;
                    };
                    let values =
                        crate::eval::materialize::capture_pipeline(session, pipeline, source)?;
                    scope = scope.with_pipeline_result(nested.span, Value::list(values));
                }
            }
        }
    }
    Ok(scope)
}
