//! One native segment, run in the foreground: bind, assemble, drive, deliver.
//!
//! The four phases are four calls, in that order. Binding is `bind`, driving is `drive`, and what
//! becomes of the values is `result`; what is left here is the assembly between them and the
//! decisions only a whole segment can make — whether its head reads the shell's stdin, whether
//! its tail may hand objects to a child process, and whether a live stream has anybody watching.

use ono_command::{BoundArguments, CommandContract, CommandRegistry, Invocation, Outcome};
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::StageList;
use ono_pipeline::{StreamEvent, ValueStream};
use ono_value::{ActionStatus, ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::Session;

use super::bind::{bind_stage, stage_scope};
use super::drive::{BlockRequest, asking_stage, block_of, drive_segment, interrupted};
use super::result::{
    Delivery, action_records, deliver_segment, live_geometry, report_counts, report_failures,
    table_row_limit, write_failed,
};
use super::segment::{accepts_bytes, each_needs_a_stream, interrupted_flow, produces_bytes};
use super::{Seed, implementations};

/// Runs one run of native stages, answering with the bytes a following child process would read.
#[expect(
    clippy::too_many_arguments,
    reason = "one call site, and the arguments are the pipeline's actual moving parts"
)]
pub(super) fn run_native_segment(
    session: &mut Session,
    registry: &'static CommandRegistry,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    seed: Seed,
    first: bool,
    last: bool,
) -> Eval<(Option<Vec<u8>>, ExitStatus)> {
    let table = implementations(session).map_err(Flow::Failed)?;
    // Taken before the pipeline borrows the session: a live view paints with the session's
    // theme, not with whatever the default happens to be (spec §44, ADR-0332).
    let theme = ono_render::Theme::clone(session.theme());

    // Everything is bound before anything runs. A pipeline that cannot be built runs no part of
    // itself, so a typo in the third stage never leaves the first two half-done.
    let mut bound: Vec<(&'static CommandContract, BoundArguments)> = Vec::new();
    let mut structured = input.is_none() || seed.is_some();
    for index in indices {
        let stage = &list.stages[*index];
        let Some((contract, mut arguments)) = bind_stage(session, registry, stage, structured)?
        else {
            return Err(Flow::Failed(ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!("`{}` is not a native command here", stage.span),
            )));
        };
        // `format table` without `--max-rows` truncates where the sink would (spec §13.3,
        // ADR-0094 §6): the setting is the session's, so the shell hands it in here.
        if contract.id() == "ono.data.format"
            && let Some(limit) = table_row_limit(session)
        {
            arguments = arguments.with_option("max-rows", Value::Int(limit as i128));
        }
        structured = !produces_bytes(contract);
        bound.push((contract, arguments));
    }

    // A head stage that needs bytes reads the shell's own standard input, exactly as a child
    // process would have: spec §12.4's example is `curl … | ono -c 'from json | …'`, and the
    // bytes arrive on the shell's stdin, not from a stage inside the pipeline. A terminal is
    // never read implicitly — an interactive `from json` waiting silently for EOF would look
    // like a hang, and the "nothing was piped into it" error says what to do instead.
    // A seeded segment already has its input — `$hot | to json`, a function's stream — and
    // must not wait on stdin as well: with a pipe that never closes, that wait is a hang.
    let seeded = seed.is_some();
    let mut input = input;
    if first
        && input.is_none()
        && !seeded
        && let Some((head, _)) = bound.first()
        && !head.input().accepts_null()
        && accepts_bytes(head.input().text())
        && !std::io::IsTerminal::is_terminal(&std::io::stdin())
    {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::stdin().lock(), &mut bytes)
            .map_err(write_failed)?;
        input = Some(bytes);
    }

    // A streamed seed with no stage after it is the renderer's to show (ADR-0059); anything
    // else without a stage has nothing to do.
    let final_contract: Option<&'static CommandContract> =
        bound.last().map(|(contract, _)| *contract);
    if final_contract.is_none() && !matches!(seed, Seed::Stream { .. }) {
        return Ok((None, ExitStatus::SUCCESS));
    }
    let stage_has_no_redirection = list.stages[*indices.last().unwrap_or(&0)]
        .redirections
        .is_empty();

    // A structured stream cannot be handed to a child process. Spec §12.3 makes the boundary
    // explicit in both directions, and guessing a rendering the program would have to parse back
    // is exactly the text-shaped coupling the object pipeline exists to remove.
    if !last && final_contract.is_none_or(|contract| !produces_bytes(contract)) {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!(
                    "`{}` produces objects, and the next stage is a program that reads bytes",
                    final_contract
                        .map_or_else(|| "the adapter".to_owned(), CommandContract::spelling)
                ),
            )
            .with_help("choose the representation: `… | to json | …` (spec §12.3)"),
        ));
    }

    let scope = std::sync::Arc::new(stage_scope(session, &bound, source)?);
    let adapters = session.shared_adapters();
    let resolver = crate::resolve::resolver(session);
    let context = session.context();
    // A live view has nobody to watch it while its values are being bound (ADR-0069).
    let capturing = session.capturing();
    // Whether the last stage's values reach a person rather than another stage, a file or a
    // capture — the one fact a full-screen view may not decide for itself (spec v0.4 §29.1).
    let displays = last && stage_has_no_redirection && !capturing;
    let materialization = crate::eval::materialize::limits(session);
    // Which stages of this segment run a block, and where each one stands in `bound`.
    //
    // A block stage is bound like any other — same contract, same arguments, same scope — and
    // only its *execution* differs: the transform engine cannot run statements, so the evaluator
    // runs them, on this thread, one item at a time. v0.4.1 §25.1 requires that to happen while
    // the source is still open, and §25.2 forbids the alternative that used to stand here
    // (ADR-0480).
    let blocks: Vec<(usize, usize)> = indices
        .iter()
        .enumerate()
        .filter(|(_, index)| block_of(&list.stages[**index]).is_some())
        .map(|(position, index)| (position, *index))
        .collect();
    // ADR-0070 point 3: with stages after it a block's values stream into them; with nothing
    // after it the block's own statements show their results where they stand, and the stage has
    // no result of its own.
    let block_shows_itself = blocks.last().is_some_and(|(position, index)| {
        *position + 1 == bound.len() && *index + 1 == list.stages.len()
    });

    let (runtime, providers) = session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the pipeline runtime",
        ))
    })?;
    // Owned, so the borrow of the session ends with the assembly below and the evaluator can be
    // called again while this pipeline is still running — which is the whole point (ADR-0480).
    let handle = runtime.handle().clone();

    // Ctrl-C is delivered to the shell itself while a native pipeline runs — there is no child
    // for the kernel to interrupt — so whatever this thread waits on races the interrupt note and
    // loses to it (spec §18.5). Dropping the futures drops every stream receiver, which closes
    // the bounded channels and stops every producer at its next send.
    let _ = ono_process::take_interrupt();

    // One request in flight. §25.3 keeps `each` serial, so a queue of items waiting to be run
    // would buy nothing, and §65.7 forbids the shape it would take: "replacing a foreground
    // `Vec` with an unbounded background queue is not a streaming fix".
    let (asked, mut requests) = tokio::sync::mpsc::channel::<BlockRequest>(1);

    let assemble = async {
        let mut carried_failure = false;
        let mut stream: Option<ValueStream> = match seed {
            Seed::Values(values) => Some(ValueStream::from_values(values)),
            Seed::Pipe {
                stream,
                failed_rows,
            } => {
                carried_failure = failed_rows;
                Some(stream)
            }
            Seed::Stream {
                receiver,
                boundedness,
            } => Some(ValueStream::spawn(
                ono_pipeline::PipelineConfig::new(),
                boundedness,
                |sink| async move {
                    let mut receiver = receiver;
                    while let Some(event) = receiver.recv().await {
                        let delivered = match event {
                            StreamEvent::Value(value) => sink.send(value).await.is_ok(),
                            StreamEvent::Failure(error) => sink.fail(error).await.is_ok(),
                        };
                        if !delivered {
                            break;
                        }
                    }
                },
            )),
            Seed::None => input.map(|bytes| ValueStream::from_values([Value::Bytes(bytes.into())])),
        };

        let mut failed_rows = carried_failure;
        let final_stage = bound.len().saturating_sub(1);
        for (position, (contract, arguments)) in bound.iter().enumerate() {
            if let Some((_, at)) = blocks.iter().find(|(held, _)| *held == position) {
                let Some(previous) = stream.take() else {
                    return Err(each_needs_a_stream());
                };
                stream = Some(asking_stage(previous, *at, asked.clone()));
                continue;
            }
            let started = std::time::Instant::now();
            let mut invocation = Invocation::new(contract, arguments, providers)
                .with_scope(std::sync::Arc::clone(&scope))
                .with_context(context.clone())
                .with_adapters(std::sync::Arc::clone(&adapters), resolver.clone())
                .with_display(displays && position == final_stage);
            if let Some(previous) = stream.take() {
                invocation = invocation.with_input(previous);
            }
            match table.run(contract.id(), &mut invocation).await {
                // v0.4.1 §22.2: the configured materialization limits are stated once, here,
                // where the pipeline is assembled. Every stage built from this stream inherits
                // them, so a producer does not have to know they exist (ADR-0454).
                Ok(Outcome::Values(values)) => {
                    stream = Some(values.with_materialization_limits(materialization));
                }
                Ok(Outcome::Actions(outcomes)) => {
                    // Spec §11.5: one record per target, so `97 succeeded, 3 failed` stays two
                    // readable numbers rather than one ambiguous status — and a failed row
                    // fails the run, after every row has been written (spec §16.5, ADR-0006).
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
    };

    let assembled = handle.block_on(async {
        tokio::select! {
            outcome = assemble => outcome,
            () = interrupted() => Err(ErrorValue::new(ErrorCode::StreamCancelled, "interrupted")),
        }
    });
    // Every sender that remains belongs to a block stage, so the driver below learns from the
    // channel closing that no block will ask again.
    drop(asked);
    let (stream, failed_rows) = assembled.map_err(interrupted_flow)?;

    // The counters are shared by every stage of the pipeline (ADR-0014); the handle is taken
    // before the stream is drained, because the stream is consumed to do it.
    let counted = stream
        .as_ref()
        .map(|stream| stream.diagnostics().clone())
        .unwrap_or_default();

    // Taken before the stream is moved: what stops every producer of this pipeline at once. A
    // stage that runs a block may still be waiting on a source that never ends after downstream
    // has had its answer — `each { … } | take 1` over a followed file — and a shell that walked
    // away from it would leave the source running (§28.3, §28.4).
    let cancel = stream.as_ref().map(|stream| stream.cancel_token().clone());
    let mut showing = None;
    let mut draining = None;
    if let Some(stream) = stream {
        if last
            && !stream.boundedness().is_bounded()
            && stage_has_no_redirection
            && !block_shows_itself
        {
            // A live stream at a terminal renders in place (spec §18.3); anywhere else the
            // representation must be chosen, because an endless unserialised stream into a pipe
            // or file is a table that never learns its widths.
            if capturing || !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::StreamUnboundedOperation,
                        "a live stream needs a representation when nobody is watching it",
                    )
                    .with_help(
                        "pipe it through a serializer — `watch process | to json` — or bound it \
                         with `take` (spec §18.3)",
                    ),
                ));
            }
            let (width, height) = live_geometry();
            showing = Some(Box::pin(crate::live::show(stream, width, height, &theme))
                as std::pin::Pin<
                    Box<dyn std::future::Future<Output = Vec<ErrorValue>>>,
                >);
        } else {
            draining = Some(stream);
        }
    }

    let drained = drive_segment(
        session,
        &handle,
        &mut requests,
        &list.stages,
        source,
        draining,
        showing,
    )?;
    // Whatever is left is left because nobody is reading it any more: cancellation wins over
    // capacity, so a producer behind a stage that stopped does not keep enqueueing (§28.3).
    if let Some(cancel) = cancel {
        cancel.cancel();
    }
    if let Some(flow) = drained.stopped {
        return Err(flow);
    }
    let values = drained.values;
    let failures = drained.failures;

    // A failure of the provider kind — it could not answer, or not as promised — is never a
    // partial one: no object was lost, the answer was (ADR-0085). What did arrive is still
    // written; the status says the run did not get what it asked for.
    let unanswered = failures
        .iter()
        .any(|failure| failure.kind() == ono_core::ErrorKind::Provider);
    report_failures(&values, failures)?;

    // ADR-0014 counts what a pipeline dropped so that "a user who is surprised by a row count
    // has somewhere to look that is not the source code". This is where they look: one note per
    // run, on stderr, only when something was actually dropped, and only for the pipeline whose
    // result they are reading (ADR-0261).
    if last && !capturing {
        report_counts(&counted);
    }

    let status = if failed_rows || unanswered {
        ExitStatus::FAILURE
    } else {
        ExitStatus::SUCCESS
    };
    deliver_segment(
        session,
        &Delivery {
            list,
            indices,
            source,
            bound: &bound,
            last,
            block_shows_itself,
        },
        values,
        status,
    )
}
