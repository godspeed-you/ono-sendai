//! Binding a native stage, and the scope its expressions read.
//!
//! One definition of "bound" for both callers — the drained segment of `foreground` and the
//! assembled stream of v0.4.1 §26.2 in `assemble` — so the two cannot drift apart in what a bound
//! stage is.

use ono_command::{BoundArguments, CommandContract, CommandRegistry, Scope};
use ono_parser::Stage;
use ono_value::Value;

use crate::eval::{Eval, Flow};
use crate::session::Session;

use super::segment::{head_name, native_contract, refuse_switched_off_spatial};

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
    // Issue #302: a job's relative paths mean the directory it was started in, whatever the
    // foreground has done to the process's directory since (ADR-0957).
    // Inside a link frame the remote answers what the stages ask, and a directory of this
    // machine means nothing there: their paths travel as written. What stays on this side — a
    // redirection, a plugin reference — is still anchored (review S3).
    if let Some(directory) = session
        .anchoring_directory()
        .filter(|_| session.link_host().is_none())
    {
        scope = scope.with_working_directory(directory);
    }
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
