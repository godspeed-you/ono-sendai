//! The evaluator: from a parsed program to what actually happens.
//!
//! The execution model is ADR-0013's. Phase A carries only external stages, so a pipeline becomes
//! an `ono_process::Pipeline` and runs in the foreground; the native stages of phase B slot in
//! beside them without changing the shape of anything here.

use ono_core::{ErrorCode, ExitStatus};
use ono_value::{ErrorValue, Value};

use crate::session::Session;

mod block;
mod control;
mod expression;
mod function;
mod materialize;
pub mod native;
mod pipeline;
mod statement;

pub(crate) use block::run_each_item;
pub use expression::eval_expr;
pub use expression::truthy;
pub use materialize::captured_text;
pub use pipeline::output_destination;
pub use pipeline::run_adapted_segment;
pub use pipeline::run_external_segment;
pub use pipeline::run_pipeline;
pub use pipeline::stage_arguments;
pub use pipeline::start_adapted_segment;
pub use statement::expand_alias;
pub use statement::run_statement;

/// Why evaluation of a statement stopped early.
#[derive(Debug)]
pub enum Flow {
    /// A structured error, to be reported and to set a failing status.
    Failed(ErrorValue),
    /// A structured error whose status is already known — an external command that could not be
    /// started reports both, and ADR-0008 requires its own status to be the one that survives.
    FailedWith(ErrorValue, ExitStatus),
    /// `break` inside a loop.
    Break,
    /// `continue` inside a loop.
    Continue,
    /// `return` from a function body, carrying its value.
    Return(Value),
    /// `exit`, which unwinds to the top without ending the process abruptly.
    Exit(ExitStatus),
}

impl From<ErrorValue> for Flow {
    fn from(error: ErrorValue) -> Self {
        Flow::Failed(error)
    }
}

/// The result of evaluating something that can fail or jump.
pub type Eval<T> = Result<T, Flow>;

/// Runs a whole program, returning the status of the last statement it reached.
pub fn run_program(
    session: &mut Session,
    program: &ono_parser::Program,
    source: &str,
    report: &mut dyn FnMut(&ErrorValue),
) -> ExitStatus {
    for statement in &program.statements {
        // v0.4.1 §23.4: the capture ceiling is "an upper bound on the total bytes retained by
        // simultaneous evaluator captures" of *one* shell command, so the accounting starts
        // afresh here and nowhere inside (ADR-0457).
        session.begin_command_captures();
        match run_statement(session, statement, source) {
            Ok(status) => session.set_status(status),
            Err(Flow::Failed(error)) => {
                report(&error);
                session.set_status(status_for(&error));
            }
            Err(Flow::FailedWith(error, status)) => {
                report(&error);
                session.set_status(status);
            }
            Err(Flow::Exit(status)) => {
                session.leave(status);
                return status;
            }
            // A jump outside any construct that could catch it ends the program quietly, which
            // is what a `return` at the top level of a script means.
            Err(Flow::Return(_) | Flow::Break | Flow::Continue) => return session.status(),
        }
        if let Some(status) = session.leaving() {
            return status;
        }
        // Reap whatever finished while that statement ran. Without this a script that backgrounds
        // work accumulates a zombie per job for the life of the process — the interactive loop
        // polls between prompts, and a script has no prompts. `waitpid` with `WNOHANG` costs one
        // syscall that finds nothing when there is nothing.
        let _ = session.executor().poll_jobs();
    }
    session.status()
}

/// The exit status an error implies (ADR-0008).
#[must_use]
pub fn status_for(error: &ErrorValue) -> ExitStatus {
    match error.code() {
        ErrorCode::ResolveCommandNotFound => ExitStatus::NOT_FOUND,
        ErrorCode::ParseSyntax | ErrorCode::ParseIncomplete => ExitStatus::USAGE,
        _ => ExitStatus::FAILURE,
    }
}
