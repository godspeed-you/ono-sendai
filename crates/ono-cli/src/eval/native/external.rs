//! An adapted external program, as a stage of the object pipeline (spec v0.3, ADR-0059).
//!
//! A decoder that streams gets its own reader thread and a channel the native segment drains; a
//! decoder that needs the whole document gets the whole document. The choice is the adapter's
//! contract, not a guess about the program.

use std::io::Write;

use ono_adapter::{AdapterPlan, Consumer, OutputDemand};
use ono_command::CommandRegistry;
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::StageList;
use ono_pipeline::StreamEvent;
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::Session;

use super::Seed;
use super::foreground::run_native_segment;
use super::result::bytes_of;
use super::segment::{
    Segment, adaptable_program, native_contract, process_error_flow, redirects_stdout,
};

/// Runs an adapted stage whose decoder streams, together with the native segment that
/// consumes its records (ADR-0059).
///
/// The child runs in its own process group without the terminal; a reader thread decodes its
/// stdout line by line into a channel; the native segment — or, with none, the renderer —
/// drains the channel on the runtime. Ctrl-C therefore reaches the shell's pipeline as for any
/// native run; the child is then told to stop and waited for, and its own status stands
/// (spec v0.3 §1.20).
#[expect(
    clippy::too_many_arguments,
    reason = "one call site, and the arguments are the pipeline's actual moving parts"
)]
pub(super) fn run_streamed_segment(
    session: &mut Session,
    registry: &'static CommandRegistry,
    list: &StageList,
    indices: &[usize],
    following: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    plan: &AdapterPlan,
    argv: &[String],
    last: bool,
) -> Eval<ExitStatus> {
    let trace = ono_adapter::Trace {
        executable: plan.executable().to_path_buf(),
        version: plan.version().cloned(),
        user_invocation: argv.to_vec(),
        actual_invocation: plan.argv().to_vec(),
        host: session.link_host(),
    };
    let mut decoding =
        ono_adapter::Decoding::for_plan(plan.clone(), trace, ono_value::builtin_schemas())
            .map_err(Flow::Failed)?;
    let boundedness = if plan.invocation().plan().is_unbounded() {
        ono_pipeline::Boundedness::Unbounded
    } else {
        ono_pipeline::Boundedness::Bounded
    };

    let mut started =
        crate::eval::start_adapted_segment(session, list, indices, source, input, plan)?;
    let Some(pipe) = started.take_pipe() else {
        let outcome = session
            .executor()
            .finish_foreground(started)
            .map_err(process_error_flow)?;
        return Ok(outcome.status());
    };

    let (sender, receiver) = tokio::sync::mpsc::channel::<StreamEvent>(256);
    let reader = std::thread::spawn(move || {
        let mut file = std::fs::File::from(pipe);
        let mut buffer = vec![0u8; 64 * 1024];
        let deliver = |outcome: Result<Value, ErrorValue>| {
            let event = match outcome {
                Ok(value) => StreamEvent::Value(value),
                Err(error) => StreamEvent::Failure(error),
            };
            sender.blocking_send(event).is_ok()
        };
        loop {
            match std::io::Read::read(&mut file, &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    for outcome in decoding.feed(&buffer[..count]) {
                        if !deliver(outcome) {
                            return;
                        }
                    }
                }
            }
        }
        for outcome in decoding.finish() {
            if !deliver(outcome) {
                return;
            }
        }
    });

    let consumed = run_native_segment(
        session,
        registry,
        list,
        following,
        source,
        None,
        Seed::Stream {
            receiver,
            boundedness,
        },
        false,
        last,
    )
    .map(|_| ());
    // A reader still running means the consumer stopped before the child's output ended —
    // `take 1`, an error, Ctrl-C. Cancellation propagates to the producer (spec §18.5,
    // ADR-0059): the child is told to stop, and a status it exits with because of that is not
    // a failure of the pipeline. A reader that finished saw end of file, and the child's own
    // status stands.
    let cancelled = !reader.is_finished();
    if cancelled {
        started.terminate();
    }
    let outcome = session.executor().finish_foreground(started);
    let _ = reader.join();
    consumed?;
    let outcome = outcome.map_err(process_error_flow)?;
    let status = outcome.status();
    if status != ExitStatus::SUCCESS && !cancelled {
        let program = plan
            .executable()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err(Flow::FailedWith(
            ErrorValue::new(
                ErrorCode::ExternalExitNonzero,
                format!("{program} exited with status {}", status.code()),
            )
            .with_help(format!(
                "its stderr above says why; `raw {}` runs it as typed",
                argv.join(" ")
            )),
            status,
        ));
    }
    Ok(if cancelled {
        ExitStatus::SUCCESS
    } else {
        status
    })
}

/// Decodes what an adapted child wrote, with the provenance of spec v0.3 §1.8.
pub(super) fn decode_adapted(
    session: &Session,
    plan: &AdapterPlan,
    argv: &[String],
    bytes: &[u8],
) -> Result<Vec<Value>, ErrorValue> {
    let trace = ono_adapter::Trace {
        executable: plan.executable().to_path_buf(),
        version: plan.version().cloned(),
        user_invocation: argv.to_vec(),
        actual_invocation: plan.argv().to_vec(),
        host: session.link_host(),
    };
    ono_adapter::decode(plan.adapter(), bytes, &trace, ono_value::builtin_schemas())
}

/// The bytes a seed of values hands a child process: text and bytes pass, objects are refused.
pub(super) fn seed_bytes(values: Vec<Value>) -> Result<Vec<u8>, Flow> {
    if values
        .iter()
        .any(|value| !matches!(value, Value::String(_) | Value::Bytes(_)))
    {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                "these values are objects, and the next stage is a program that reads bytes",
            )
            .with_help("choose the representation: `… | to json | …` (spec §12.3)"),
        ));
    }
    Ok(bytes_of(&values))
}

/// The diagnostic of spec v0.3 §1.57 for a decode failure at the terminal.
pub(super) fn report_fallback(plan: &AdapterPlan, error: &ErrorValue) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "{}",
        ono_render::sanitise(&format!(
            "adapter {} failed to decode {} output",
            plan.adapter().full_id(),
            plan.executable()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        ))
    );
    let _ = writeln!(err, "{}", ono_render::sanitise(error.message()));
    let _ = writeln!(err);
    let _ = writeln!(err, "falling back to raw output");
}

/// What the last stage of an external segment is asked to produce (ADR-0052, ADR-0057 point 2).
///
/// `None` when nothing downstream could take structure: the stage then runs as it always has,
/// without even asking the registry.
pub(super) fn external_demand(
    session: &Session,
    registry: &'static CommandRegistry,
    list: &StageList,
    indices: &[usize],
    next: Option<&Segment>,
    last: bool,
) -> Option<OutputDemand> {
    let stage = &list.stages[*indices.last()?];
    if ono_command::is_adapt(stage) {
        return Some(OutputDemand::Structured { schema: None });
    }
    if redirects_stdout(stage) {
        return None;
    }
    match next {
        Some(Segment::Native(following)) => {
            let consumer = &list.stages[*following.first()?];
            let contract = native_contract(session, registry, consumer, true)?;
            let demand = OutputDemand::for_consumer(Consumer::Native {
                input: contract.input().text(),
            });
            matches!(demand, OutputDemand::Structured { .. }).then_some(demand)
        }
        Some(Segment::External(_)) => None,
        None if last && std::io::IsTerminal::is_terminal(&std::io::stdout()) => {
            Some(OutputDemand::Interactive)
        }
        None => None,
    }
}

/// Asks the registry about the segment's last stage with its arguments expanded.
///
/// # Errors
///
/// A refusal under a demand the program cannot satisfy raw — `adapter.unsupported_invocation`,
/// `adapter.version_incompatible`, `adapter.executable_mismatch`, `adapter.conflict` — with the
/// payload of spec v0.3 §1.65.
pub(super) fn negotiate_stage(
    session: &mut Session,
    list: &StageList,
    indices: &[usize],
    source: &str,
    demand: Option<OutputDemand>,
) -> Eval<Option<(AdapterPlan, Vec<String>, OutputDemand)>> {
    let Some(demand) = demand else {
        return Ok(None);
    };
    let stage = &list.stages[*indices.last().unwrap_or(&0)];
    let Some((name, path)) = adaptable_program(session, stage) else {
        return Ok(None);
    };
    let mut argv = vec![name];
    let forced = ono_command::is_adapt(stage);
    argv.extend(
        crate::eval::stage_arguments(session, stage, source)?
            .into_iter()
            .skip(usize::from(forced))
            .map(|word| word.to_string_lossy().into_owned()),
    );
    let negotiation = session.adapters().negotiate(&path, &argv, &demand);
    if let Some(error) = negotiation.refusal(&demand, &path, &argv) {
        return Err(Flow::Failed(error));
    }
    if forced && negotiation.plan().is_none() {
        // Spec v0.3 §1.18: a forced structured invocation fails rather than silently
        // downgrading to raw text.
        let invocation = argv.join(" ");
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::AdapterRequiredForStructuredPipeline,
                format!("no adapter can give `{invocation}` structured output"),
            )
            .with_help(format!(
                "`raw {invocation}` runs the program as typed; `{invocation} | from <format>` \
                 decodes its output yourself; `get command {}` lists what adapts it",
                argv.first().map_or("", String::as_str)
            ))
            .with_metadata("invocation", Value::string(&invocation))
            .with_metadata("raw_fallback_safe", Value::Bool(true))
            .with_metadata("recovery", Value::string(&format!("raw {invocation}"))),
        ));
    }
    Ok(negotiation.plan().cloned().map(|plan| (plan, argv, demand)))
}
