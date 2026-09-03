//! Budget-aware finite collection: the one place a pipeline is run for its values.
//!
//! v0.4.1 §30.2 asks that this module own the helpers "so no caller recreates them ad hoc", and
//! §23 is why it matters: every capture opened here is measured and charged to the session's
//! shared `Budget` (§21.1, ADR-0453, ADR-0457). A caller that wants a pipeline's values asks
//! here; nobody else calls `begin_capture`/`end_capture` in the evaluator.

use ono_core::ExitStatus;
use ono_parser::{Argument, Expr, Pipeline, StageHead};
use ono_value::Value;

use crate::session::Session;

use super::Eval;
use super::expression::eval_expr;
use super::pipeline::run_pipeline;

/// What `let` binds, with the status of the pipeline that produced it.
pub(super) fn binding_value(
    session: &mut Session,
    pipeline: &Pipeline,
    source: &str,
) -> Eval<(Value, ExitStatus)> {
    if let Some(expression) = bare_value(pipeline) {
        return Ok((eval_expr(session, expression, source)?, ExitStatus::SUCCESS));
    }
    captured_value(session, pipeline, source)
}

/// The expression a one-stage pipeline is, when it is one: `let name = "world"`.
pub(super) fn bare_value(pipeline: &Pipeline) -> Option<&Expr> {
    if !pipeline.tail.is_empty() || pipeline.head.stages.len() != 1 {
        return None;
    }
    let stage = pipeline.head.stages.first()?;
    if !stage.redirections.is_empty() {
        return None;
    }
    match (&stage.head, stage.arguments.as_slice()) {
        (StageHead::Value(expression), []) => Some(expression),
        // An expression-mode stage with no arguments and a bare head is a field path or a
        // literal read as a command; `let n = 3` arrives this way.
        (StageHead::Error(_), [Argument::Value(expression)]) => Some(expression),
        _ => None,
    }
}

/// The value a pipeline produces when it is used as one, as in `( … )`.
pub(super) fn value_of_pipeline(
    session: &mut Session,
    pipeline: &Pipeline,
    source: &str,
) -> Eval<Value> {
    if let Some(expression) = bare_value(pipeline) {
        return eval_expr(session, expression, source);
    }
    Ok(captured_value(session, pipeline, source)?.0)
}

/// Runs `pipeline` for its values rather than for the screen.
///
/// The pipeline runs through the ordinary evaluator — checked, planned, resolved as always —
/// with its final values diverted from the sink. A pipeline that ends in a program rather than
/// a native stage writes bytes, and bytes are not captured here: capturing a program's output
/// as a value is the language's `(…)` substitution, which is a separate increment.
pub(super) fn capture_pipeline(
    session: &mut Session,
    pipeline: &ono_parser::Pipeline,
    source: &str,
) -> Eval<Vec<Value>> {
    session.begin_capture();
    let outcome = run_pipeline(session, pipeline, source);
    let values = session.end_capture();
    outcome?;
    Ok(values)
}

/// What one materializing stage of this evaluator may collect (§22.2, Appendix A).
///
/// The evaluator's one door to the materialization budget, so no caller reads it ad hoc. The
/// numbers themselves stay in [`crate::limits`], which derives every one of them from the
/// settings catalogue: §52.2 forbids a limit being "independently typed into five files if one
/// contract can generate the others", and that is a stronger claim than file locality.
pub(super) fn limits(session: &Session) -> ono_pipeline::MaterializationLimits {
    crate::limits::materialization(session.settings())
}

/// Runs a pipeline for its value rather than its display (spec §19.2, ADR-0069).
///
/// Everything the pipeline would have shown is collected instead: a native pipeline's values, or
/// the text a program wrote to its stdout. One value is that value; several are a list, because
/// a list splices back into several values when it starts a pipeline (ADR-0019); none is the
/// empty list — the pipeline is known to have produced nothing, which is not the same as not
/// knowing. The status is the pipeline's own, so `$?` after `let x = …` says whether it worked.
pub(super) fn captured_value(
    session: &mut Session,
    pipeline: &Pipeline,
    source: &str,
) -> Eval<(Value, ExitStatus)> {
    session.begin_capture();
    let outcome = run_pipeline(session, pipeline, source);
    let mut captured = session.end_capture();
    let status = outcome?;
    let value = match captured.len() {
        1 => captured.remove(0),
        _ => Value::list(captured),
    };
    Ok((value, status))
}

/// The value a program's captured stdout stands for: its text, without the newline every
/// line-oriented program ends its output with — `(echo hi)` is `hi`, exactly as `$(echo hi)`
/// has always been.
#[must_use]
pub fn captured_text(bytes: &[u8]) -> Value {
    let text = String::from_utf8_lossy(bytes);
    Value::String(text.trim_end_matches(['\n', '\r']).into())
}
