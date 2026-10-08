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
use super::drive::{
    BlockRequest, BlockSite, JobFold, answer, answering, asking_stage, block_of, drive_segment,
};
use super::result::{
    Delivery, StreamedOutput, action_records, deliver_segment, live_geometry, report_counts,
    report_failures, streams_bytes, table_row_limit, write_failed,
};
use super::segment::{accepts_bytes, each_needs_a_stream, produces_bytes};
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
    feeds: Option<&[usize]>,
) -> Eval<SegmentEnd> {
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
        && !session.is_background_job()
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
    // Nothing bound and nothing streaming into it: there is no segment to run. A seed that is
    // *already* a stream is the exception in both directions — a reader thread's values, and now
    // a package's answer that does not end (ADR-0588) — because for those the segment's work is
    // not running a stage but deciding how the stream is shown.
    if final_contract.is_none() && !matches!(seed, Seed::Stream { .. } | Seed::Pipe { .. }) {
        return Ok(SegmentEnd::answered(None, ExitStatus::SUCCESS));
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
    let shared_source: std::sync::Arc<str> = std::sync::Arc::from(source);
    let blocks: Vec<(usize, usize, std::sync::Arc<BlockSite>)> = indices
        .iter()
        .enumerate()
        .filter_map(|(position, index)| {
            block_of(&list.stages[*index]).map(|block| {
                (
                    position,
                    *index,
                    std::sync::Arc::new(BlockSite {
                        block: block.clone(),
                        source: std::sync::Arc::clone(&shared_source),
                        consumed: *index + 1 < list.stages.len(),
                        frames: Vec::new(),
                    }),
                )
            })
        })
        .collect();
    // ADR-0070 point 3: with stages after it a block's values stream into them; with nothing
    // after it the block's own statements show their results where they stand, and the stage has
    // no result of its own. A stream assembled before this segment may end in such a block too —
    // a function whose body ends in one, called last (ADR-0950).
    let block_shows_itself = blocks.last().is_some_and(|(position, index, _)| {
        *position + 1 == bound.len() && *index + 1 == list.stages.len()
    }) || (bound.is_empty()
        && matches!(
            seed,
            Seed::Pipe {
                shows_itself: true,
                ..
            }
        ));

    let (runtime, providers) = session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the pipeline runtime",
        ))
    })?;
    // Owned, so the borrow of the session ends here and the evaluator can be called again while
    // this pipeline is still being started and while it runs — which is the whole point
    // (ADR-0480).
    let handle = runtime.handle().clone();
    let providers = providers.clone();
    let providers = &providers;

    // Ctrl-C is delivered to the shell itself while a native pipeline runs — there is no child
    // for the kernel to interrupt — so whatever this thread waits on races the interrupt note and
    // loses to it (spec §18.5). Dropping the futures drops every stream receiver, which closes
    // the bounded channels and stops every producer at its next send.
    //
    // The note is dropped by entering the foreground line rather than by reading it away here: a
    // block runs one of these per item, and a bare `take_interrupt()` at each of them discards the
    // Ctrl-C aimed at the line around them, whichever item happened to start next (ADR-0782). The
    // outermost run still clears what the prompt left behind; a nested one clears nothing.
    let _running = crate::eval::pipeline::ForegroundRun::begin();

    // One request in flight. §25.3 keeps `each` serial, so a queue of items waiting to be run
    // would buy nothing, and §65.7 forbids the shape it would take: "replacing a foreground
    // `Vec` with an unbounded background queue is not a streaming fix".
    //
    // A stream assembled before this segment brings the channel its own block stages ask through,
    // and this segment's blocks ask through the same one: one driver answers every block of one
    // pipeline, wherever it was written (ADR-0950).
    let mut seed = seed;
    let channel = match &mut seed {
        Seed::Pipe { requests, .. } => requests.take(),
        _ => None,
    };
    let (asked, mut requests) =
        channel.unwrap_or_else(|| tokio::sync::mpsc::channel::<BlockRequest>(1));

    let assemble = async {
        let mut carried_failure = false;
        let mut stream: Option<ValueStream> = match seed {
            Seed::Values(values) => Some(ValueStream::from_values(values)),
            Seed::Pipe {
                stream,
                failed_rows,
                ..
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
            if let Some((_, _, site)) = blocks.iter().find(|(held, _, _)| *held == position) {
                let Some(previous) = stream.take() else {
                    return Err(each_needs_a_stream());
                };
                stream = Some(asking_stage(
                    previous,
                    std::sync::Arc::clone(site),
                    asked.clone(),
                ));
                continue;
            }
            let started = std::time::Instant::now();
            // v0.5 §4: every stage carries the session's temporal coordinate, or the one its
            // own `--at` names. `CommandTable::run` reads it there — the read-only rule of §4.7
            // and the historical evaluation of §4.5 are one seam, not two (ADR-0691).
            let temporal = crate::temporal::invocation_context(arguments).await?;
            let mut invocation = Invocation::new(contract, arguments, providers)
                .with_scope(std::sync::Arc::clone(&scope))
                .with_context(context.clone())
                .with_adapters(std::sync::Arc::clone(&adapters), resolver.clone())
                .with_temporal(temporal, registry)
                .with_display(displays && position == final_stage);
            if let Some(previous) = stream.take() {
                invocation = invocation.with_input(previous);
            }
            // v0.5 §17.2: an Ono mutation's lifecycle is recorded around the call that makes
            // it, because this is the one place the shell holds both what was asked and what came
            // back. The identity is minted before execution (§17.3), so it can travel into a
            // provider call and be joined against whatever transaction the authority returns.
            let mutating = registry
                .verb(contract.verb())
                .is_some_and(ono_command::VerbSpec::is_mutating);
            let requested_at = jiff::Timestamp::now();
            let words: Vec<String> = arguments
                .selectors()
                .iter()
                .chain(arguments.options().iter())
                .map(|(name, binding)| match binding.value() {
                    Some(value) => format!("{name}={value}"),
                    None => name.clone(),
                })
                .collect();
            match table.run(contract.id(), &mut invocation).await {
                // v0.4.1 §22.2: the configured materialization limits are stated once, here,
                // where the pipeline is assembled. Every stage built from this stream inherits
                // them, so a producer does not have to know they exist (ADR-0454).
                Ok(Outcome::Values(values)) => {
                    stream = Some(values.with_materialization_limits(materialization));
                }
                Ok(Outcome::Actions(outcomes)) => {
                    // v0.5 §17.2, §17.4: what is recorded is what the rows say. The outcomes are
                    // handed over whole rather than counted, because `action.completed` and
                    // `action.failed` are different kinds and the `ActionResult` is what tells
                    // them apart — and because §17.3's external transaction identity is carried on
                    // the outcome and nowhere else.
                    crate::temporal::record_action(
                        &contract.spelling(),
                        contract.id(),
                        &words,
                        requested_at,
                        crate::temporal::Outcome::Acted(&outcomes),
                    )
                    .await;
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
                Err(error) => {
                    if mutating {
                        crate::temporal::record_action(
                            &contract.spelling(),
                            contract.id(),
                            &words,
                            requested_at,
                            crate::temporal::Outcome::Refused(&error),
                        )
                        .await;
                    }
                    return Err(error);
                }
            }
        }
        Ok((stream, failed_rows))
    };

    // A stage that reads its whole input while it is being started — a mutation collecting its
    // targets — asks the blocks in front of it for their items now, so they are answered now.
    let assembled = answering(session, &handle, &mut requests, assemble, answer);
    // Every sender that remains belongs to a block stage, so the driver below learns from the
    // channel closing that no block will ask again.
    drop(asked);
    let (stream, failed_rows) = assembled?;

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
    // A last stage that serializes value by value is written as it goes — to the redirection's
    // file or to stdout — rather than collected for the end, which is what lets a stream that
    // never ends be written at all (ADR-0954). A capture still collects: its value is one text.
    //
    // The same lines may feed the program after this segment instead, while it runs: written into
    // its standard input as they are produced, at the pace it reads them (§28.2).
    let feeding = feeds.filter(|_| !last && !capturing && streams_bytes(&bound));
    // A redirection names a file whatever is capturing around the line, and a capture never sees
    // what goes to it — so the lines go to the file as they come there too. A background job's
    // `watch … | to jsonl > log &` is written as it runs instead of held until an end it never
    // reaches (ADR-0958).
    let streams_out = last && (!capturing || !stage_has_no_redirection) && streams_bytes(&bound);
    let mut program = None;
    let mut streamed = if let Some(program_stages) = feeding {
        let (started, output) = start_fed_program(session, list, program_stages, source)?;
        program = Some(started);
        Some(output)
    } else if streams_out {
        let stage = &list.stages[*indices.last().unwrap_or(&0)];
        Some(StreamedOutput::open(session, stage, source)?)
    } else {
        None
    };
    let mut showing = None;
    let mut draining = None;
    let mut fold = None;
    if let Some(stream) = stream {
        if last
            && !stream.boundedness().is_bounded()
            && stage_has_no_redirection
            && !block_shows_itself
            && streamed.is_none()
        {
            // A live stream at a terminal renders in place (spec §18.3); anywhere else the
            // representation must be chosen, because an endless unserialised stream into a pipe
            // or file is a table that never learns its widths. At the end of a background job's
            // own line it is the job's: folded into the table `fg` repaints, and the rest kept as
            // it arrives under the job's capture ceiling (ADR-0958).
            let job_model = session
                .job_model()
                .filter(|_| session.capturing_for_a_job())
                .cloned();
            if let Some(model) = job_model {
                let (_, height) = live_geometry();
                fold = Some(JobFold {
                    model,
                    rows: height.saturating_sub(3).max(4),
                    serialised: final_contract.is_some_and(produces_bytes),
                });
                draining = Some(stream);
            } else if capturing || !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::StreamUnboundedOperation,
                        "a live stream needs a representation when nobody is watching it",
                    )
                    .with_help(
                        "write it as it arrives with the streaming serializer — `watch process | \
                         to jsonl` — or bound it with `take` (spec §18.3, ADR-0954)",
                    ),
                ));
            } else {
                let (width, height) = live_geometry();
                showing = Some(Box::pin(crate::live::show(stream, width, height, &theme))
                    as std::pin::Pin<
                        Box<dyn std::future::Future<Output = Vec<ErrorValue>>>,
                    >);
            }
        } else {
            draining = Some(stream);
        }
    }

    // A fed program that stops — Ctrl-Z — ends the drain, so the shell can take the terminal back
    // and file it as a stopped job (ADR-0956).
    let has_stopped = program
        .as_ref()
        .map(|started: &ono_process::Foreground| move || started.has_stopped());
    let driven = drive_segment(
        session,
        &handle,
        &mut requests,
        draining,
        showing,
        streamed.as_mut(),
        has_stopped.as_ref().map(|probe| probe as &dyn Fn() -> bool),
        fold.as_ref(),
    );
    // Whatever is left is left because nobody is reading it any more: cancellation wins over
    // capacity, so a producer behind a stage that stopped does not keep enqueueing (§28.3). That
    // holds however the drain ended — an interrupt and a departed reader included.
    if let Some(cancel) = cancel {
        cancel.cancel();
    }
    let to_the_shells_own = streamed
        .as_ref()
        .is_some_and(StreamedOutput::is_the_shells_own);
    // The end of the lines is the end of the fed program's input.
    drop(streamed);
    // A fed program is waited for whatever ended the drain, so it is never left behind; its
    // status is the pipeline's, as the last program's always is (ADR-0008).
    let program_status = match program {
        Some(started) => Some(finish_fed_program(session, started)?),
        None => None,
    };
    let drained = driven?;
    if let Some(flow) = drained.stopped {
        return Err(flow);
    }
    // The reader of the shell's own output left: the shell ends as a program whose reader left
    // ends, in silence and with `SIGPIPE`'s status (ADR-0220, ADR-0954). A fed program that read
    // what it wanted and left is not that: `yes | head -1` succeeds. Nor is the reader of a file
    // the line was redirected to — a named pipe — leaving: that ends the line, with `SIGPIPE`'s
    // status, and the shell goes on.
    if drained.reader_left && program_status.is_none() {
        if to_the_shells_own {
            return Err(Flow::Exit(ExitStatus::from_signal(13)));
        }
        return Ok(SegmentEnd::answered(None, ExitStatus::from_signal(13)));
    }
    let values = drained.values;
    let failures = drained.failures;
    let wrote = drained.written > 0;

    // A failure of the provider kind — it could not answer, or not as promised — is never a
    // partial one: no object was lost, the answer was (ADR-0085). What did arrive is still
    // written; the status says the run did not get what it asked for.
    let unanswered = drained.unanswered
        || failures
            .iter()
            .any(|failure| failure.kind() == ono_core::ErrorKind::Provider);
    report_failures(wrote || !values.is_empty(), failures)?;
    // Failures a streaming drain reported as they came, with nothing written: the failure was the
    // answer, as `report_failures` makes it — already on the terminal, so only its status is
    // left to give (ADR-0221).
    let nothing_but_failures = drained
        .first_reported
        .as_ref()
        .filter(|_| !wrote)
        .map(|first| {
            if first.code() == ErrorCode::StreamCancelled {
                ExitStatus::from_signal(2)
            } else {
                crate::eval::status_for(first)
            }
        });

    // ADR-0014 counts what a pipeline dropped so that "a user who is surprised by a row count
    // has somewhere to look that is not the source code". This is where they look: one note per
    // run, on stderr, only when something was actually dropped, and only for the pipeline whose
    // result they are reading (ADR-0261).
    if last && !capturing {
        report_counts(&counted);
    }

    let status = if let Some(status) = nothing_but_failures {
        status
    } else if failed_rows || unanswered {
        ExitStatus::FAILURE
    } else {
        ExitStatus::SUCCESS
    };
    if let Some(program_status) = program_status {
        return Ok(SegmentEnd {
            bytes: None,
            status: program_status,
            fed_program: true,
        });
    }
    // What was written as it arrived is already where it was going.
    if wrote || streams_out {
        return Ok(SegmentEnd::answered(None, status));
    }
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
    .map(|(bytes, status)| SegmentEnd::answered(bytes, status))
}

/// How a native segment ended.
pub(super) struct SegmentEnd {
    /// The bytes a following program reads, when the segment is not the last and did not feed it.
    pub(super) bytes: Option<Vec<u8>>,
    /// The segment's status — or the fed program's, when it fed one.
    pub(super) status: ExitStatus,
    /// Whether the segment ran the program after it, feeding it as it went (ADR-0954).
    pub(super) fed_program: bool,
}

impl SegmentEnd {
    fn answered(bytes: Option<Vec<u8>>, status: ExitStatus) -> Self {
        Self {
            bytes,
            status,
            fed_program: false,
        }
    }
}

/// Starts the programs after a streaming serializer with a pipe for the first one's standard
/// input, and the output the serializer's lines are written into (ADR-0954).
///
/// The programs are the foreground job, exactly as they would be after any other stage: they get
/// the terminal, so Ctrl-C reaches them, and their leaving is what ends the stream — the pipe's
/// write end reports it as soon as no reader is left.
fn start_fed_program(
    session: &mut Session,
    list: &StageList,
    program_stages: &[usize],
    source: &str,
) -> Eval<(ono_process::Foreground, StreamedOutput)> {
    let mut built = ono_process::Pipeline::new();
    for (position, index) in program_stages.iter().enumerate() {
        let mut command =
            crate::eval::pipeline::build_command(session, &list.stages[*index], source)?;
        if position == 0 {
            command = command.stdin(ono_process::Input::Pipe);
        }
        built = built.stage(command);
    }
    let mut started = session
        .executor()
        .start_foreground(&built)
        .map_err(super::segment::process_error_flow)?;
    if let Some(failure) = started.failure() {
        let error = ErrorValue::new(failure.code(), failure.message().to_owned());
        let outcome = session
            .executor()
            .finish_foreground(started)
            .map_err(super::segment::process_error_flow)?;
        return Err(Flow::FailedWith(error, outcome.status()));
    }
    let Some(input) = started.take_stdin() else {
        let outcome = session
            .executor()
            .finish_foreground(started)
            .map_err(super::segment::process_error_flow)?;
        return Err(Flow::FailedWith(
            ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                "the program's input could not be opened",
            ),
            outcome.status(),
        ));
    };
    Ok((started, StreamedOutput::into_program(input)))
}

/// Waits for a fed program and answers its status.
fn finish_fed_program(session: &mut Session, started: ono_process::Foreground) -> Eval<ExitStatus> {
    let outcome = session
        .executor()
        .finish_foreground(started)
        .map_err(super::segment::process_error_flow)?;
    if let ono_process::ForegroundOutcome::Completed(completed) = &outcome
        && let Some(failure) = completed.failure()
    {
        return Err(Flow::FailedWith(
            ErrorValue::new(failure.code(), failure.message().to_owned()),
            outcome.status(),
        ));
    }
    Ok(outcome.status())
}
