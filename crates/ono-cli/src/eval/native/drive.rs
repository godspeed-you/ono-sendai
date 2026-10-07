//! The block bridge of ADR-0480, and the loops that drive an assembled pipeline.
//!
//! A stage that runs a block cannot run it: the block is statements, and only the evaluator runs
//! statements, on the thread that owns the session. So the stage asks, over a bounded channel of
//! one, and the driver here answers and drains at the same time. v0.4.1 §25.3 keeps `each`
//! serial, so one request in flight is all the channel ever carries, and §65.7 forbids the
//! unbounded queue that a deeper one would be.

use ono_command::{BoundArguments, CommandContract, Invocation, Outcome, Scope};
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::{Argument, Block, Expr, Stage, StageHead, StageList};
use ono_pipeline::{StreamEvent, ValueStream};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::Session;

use super::result::action_records;
use super::segment::{
    Segment, head_name, native_contract, produces_bytes, refuse_switched_off_spatial, segments,
};
use super::{implementations, registry};

/// The block a stage runs, when the stage is `each { … }`.
///
/// A block is not an expression the transform engine can evaluate: it holds statements, and a
/// statement may run a command, bind a name or jump. Only the evaluator can run one, and only the
/// thread that owns the session may call the evaluator — which is why the stage below asks rather
/// than computes.
pub(crate) fn block_of(stage: &Stage) -> Option<&Block> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    if !matches!(name.namespace.as_deref(), None | Some("ono")) || name.name != "each" {
        return None;
    }
    match stage.arguments.as_slice() {
        [Argument::Value(Expr::Block(block))] => Some(block),
        _ => None,
    }
}

/// Where a block was written, and everything running it needs besides the item.
///
/// A block used to be identified by its position in the stage list the driver holds, which made a
/// function body's block — written in another list, in another source — impossible to drive while
/// streaming (issue #192). The site carries the block itself, so whichever driver answers the
/// request can run it (ADR-0950).
#[derive(Debug)]
pub(crate) struct BlockSite {
    /// The block, as it was parsed.
    pub(super) block: Block,
    /// The source the block's spans index: the caller's line, or the source a function was
    /// declared in.
    pub(super) source: std::sync::Arc<str>,
    /// ADR-0070 point 3: whether a later stage consumes what the block emits, so its values are
    /// captured for that stage instead of shown where they stand.
    pub(super) consumed: bool,
    /// The invocation scopes of the function calls whose bodies the block was written in,
    /// outermost first. Each is off the session while the caller's pipeline drains, and is put
    /// back on top of it for exactly the run of one item (§26.3).
    pub(super) frames: Vec<FrameCell>,
}

/// One function call's invocation scope, shared by the block sites of its body.
pub(crate) type FrameCell =
    std::sync::Arc<std::sync::Mutex<Option<crate::session::DetachedScopes>>>;

/// The channel a block stage asks its driver through.
pub(crate) type Asked = tokio::sync::mpsc::Sender<BlockRequest>;

/// One input value, and where the answer goes.
#[derive(Debug)]
pub(crate) struct BlockRequest {
    /// Which block to run.
    site: std::sync::Arc<BlockSite>,
    /// The value to bind as `@`.
    value: Value,
    /// Where the block's result goes.
    reply: tokio::sync::oneshot::Sender<BlockReply>,
}

/// What the evaluator answers a [`BlockRequest`] with.
#[derive(Debug)]
pub(super) enum BlockReply {
    /// The values the block produced for this item, and whether upstream is still wanted.
    Produced {
        /// What the block emitted for this one item — §25.4's per-invocation scope.
        values: Vec<Value>,
        /// `false` after `break`: stop reading upstream (§25.5).
        keep_going: bool,
    },
    /// The block jumped or failed: the pipeline stops, and the driver carries the reason out.
    Stop,
}

/// What the driver loop does next.
pub(super) enum Driven {
    /// A block stage is waiting for one item to be run.
    Ask(BlockRequest),
    /// No block stage will ask again.
    Asked,
    /// The pipeline produced something.
    Event(StreamEvent),
    /// The pipeline ended, or the live view was left.
    Drained,
    /// Ctrl-C reached the shell (spec §18.5).
    Interrupted,
    /// The reader of a streaming serializer's output went away (ADR-0954).
    ReaderGone,
}

/// The stage a block-based `each` becomes: one that asks the evaluator, item by item.
///
/// v0.4.1 §25.4: the values a block emits for one input item are forwarded before the next input
/// item is required, subject to downstream backpressure — which is what this loop does, because
/// the next `next_value` only happens after the previous item's values have been sent. Returning
/// drops the input, which closes the upstream channel and stops the source: §25.5's "`break`
/// stops consuming upstream and cancels the remaining source where possible".
pub(super) fn asking_stage(
    input: ValueStream,
    site: std::sync::Arc<BlockSite>,
    asked: Asked,
) -> ValueStream {
    // One value in, zero or more out: a stream that ends still ends, and one that does not still
    // does not (§25.6, Appendix E's `item_transform`).
    let boundedness = input.boundedness();
    input.stage(boundedness, move |mut input, sink| async move {
        while let Some(value) = input.next_value(&sink).await {
            let (reply, answer) = tokio::sync::oneshot::channel();
            if asked
                .send(BlockRequest {
                    site: std::sync::Arc::clone(&site),
                    value,
                    reply,
                })
                .await
                .is_err()
            {
                return;
            }
            let Ok(BlockReply::Produced { values, keep_going }) = answer.await else {
                return;
            };
            for value in values {
                if sink.send(value).await.is_err() {
                    return;
                }
            }
            if !keep_going {
                return;
            }
        }
    })
}

/// Resolves when the shell has been interrupted.
///
/// Ctrl-C is delivered to the shell itself while a native pipeline runs — there is no child for
/// the kernel to interrupt — so whatever a thread waits on races this and loses to it (spec
/// §18.5). Dropping the losing future drops every stream receiver, which closes the bounded
/// channels and stops every producer at its next send.
pub(super) async fn interrupted() {
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(40));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if crate::eval::pipeline::interrupt_reached() {
            return;
        }
    }
}

/// What the driver drained out of one segment.
#[derive(Default)]
pub(super) struct Drained {
    /// The values and the per-item failures a pipeline produced, drained before they are
    /// written, rendered or retained (`docs/contracts/hardening/streaming.yaml`, §26.1).
    pub(super) values: Vec<Value>,
    pub(super) failures: Vec<ErrorValue>,
    /// The flow a block raised, which stops the drain where the block stopped.
    pub(super) stopped: Option<Flow>,
    /// How many values a streaming serializer wrote as they arrived instead of collecting them
    /// here (ADR-0954).
    pub(super) written: usize,
    /// Whether the reader of what was being written went away, which ended the drain.
    pub(super) reader_left: bool,
}

/// The driver. It is the only thing holding the session, so it is the only thing that can run a
/// block — and it is also what drains the pipeline, so the two interleave rather than take turns.
///
/// Between two answers it is inside `block_on`; while it answers one it is not, which is what
/// lets a block run a pipeline of its own (ADR-0480).
///
/// # Errors
///
/// The interrupt of spec §18.5, which unwinds rather than returning a partial drain.
pub(super) fn drive_segment(
    session: &mut Session,
    handle: &tokio::runtime::Handle,
    requests: &mut tokio::sync::mpsc::Receiver<BlockRequest>,
    mut draining: Option<ValueStream>,
    mut showing: Option<std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ErrorValue>> + '_>>>,
    mut output: Option<&mut super::result::StreamedOutput>,
) -> Eval<Drained> {
    let mut drained = Drained::default();
    let reader_gone = output
        .as_ref()
        .and_then(|output| output.reader_gone().cloned());
    let mut asking = true;
    while draining.is_some() || showing.is_some() {
        // Between two items, and before the first. `select!` below only reaches the interrupt
        // branch when nothing else is ready, and a stage running a block is never idle for long
        // enough: the driver leaves the runtime to run the item, and while it is away nothing it
        // waits on can be polled at all (spec §18.5, v0.5 §32.6). Asking here is a load and a
        // branch, and it is the only cancellation point a pipeline of many small reads has.
        if crate::eval::pipeline::interrupt_reached() {
            return Err(crate::eval::pipeline::interrupted_flow_now());
        }
        let driven = handle.block_on(async {
            tokio::select! {
                biased;
                request = requests.recv(), if asking => match request {
                    Some(request) => Driven::Ask(request),
                    None => Driven::Asked,
                },
                event = async {
                    match draining.as_mut() {
                        Some(stream) => stream.recv().await,
                        None => std::future::pending().await,
                    }
                }, if draining.is_some() => match event {
                    Some(event) => Driven::Event(event),
                    None => Driven::Drained,
                },
                reported = async {
                    match showing.as_mut() {
                        Some(shown) => shown.await,
                        None => std::future::pending().await,
                    }
                }, if showing.is_some() => {
                    drained.failures.extend(reported);
                    Driven::Drained
                }
                () = interrupted() => Driven::Interrupted,
                () = async {
                    match reader_gone.as_ref() {
                        Some(gone) => left(gone).await,
                        None => std::future::pending().await,
                    }
                }, if reader_gone.is_some() => Driven::ReaderGone,
            }
        });
        match driven {
            Driven::Ask(request) => match answer(session, &request.site, request.value) {
                Ok((produced, keep_going)) => {
                    let _ = request.reply.send(BlockReply::Produced {
                        values: produced,
                        keep_going,
                    });
                }
                Err(flow) => {
                    let _ = request.reply.send(BlockReply::Stop);
                    drained.stopped = Some(flow);
                    break;
                }
            },
            Driven::Asked => asking = false,
            // A streaming serializer's line is written now, and nothing is kept (ADR-0954). A
            // write that finds the reader gone ends the drain as the reader leaving does.
            Driven::Event(StreamEvent::Value(value)) => match output.as_deref_mut() {
                Some(output) => match output.write(&value) {
                    Ok(()) => drained.written += 1,
                    Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                        drained.reader_left = true;
                        break;
                    }
                    Err(error) => return Err(super::result::write_failed(error)),
                },
                None => drained.values.push(value),
            },
            Driven::Event(StreamEvent::Failure(error)) => drained.failures.push(error),
            Driven::Drained => {
                draining = None;
                showing = None;
            }
            Driven::Interrupted => return Err(crate::eval::pipeline::interrupted_flow_now()),
            Driven::ReaderGone => {
                drained.reader_left = true;
                break;
            }
        }
    }
    Ok(drained)
}

/// Resolves once a watched reader has gone away.
async fn left(gone: &std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(40));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if gone.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
    }
}

/// Runs one item through the block a site names, in the scope the block was written in.
///
/// A block of the caller's own pipeline runs in the caller's scope, which is the session as it
/// stands. A block of a streamed function body runs in that call's invocation scope: it is put back
/// on top of the session for this one item and taken off again afterwards, so the parameter the
/// call bound is what the block reads, a `let` that advances it is what the next item reads, and
/// the caller's later stages never see it (§26.3, ADR-0950).
///
/// `return` inside such a body ends the function, not the caller: the function's stream closes
/// with the returned value, and its source is read no further — what a collected body does with
/// the same `return` (§25.5).
fn answer(session: &mut Session, site: &BlockSite, item: Value) -> Eval<(Vec<Value>, bool)> {
    let mut marks = Vec::with_capacity(site.frames.len());
    for cell in &site.frames {
        marks.push(session.scope_depth());
        let detached = cell
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        session.attach_scopes(detached.unwrap_or_default());
    }
    let (mut produced, outcome) =
        crate::eval::run_each_item(session, &site.block, &site.source, item, site.consumed);
    for (cell, mark) in site.frames.iter().zip(marks).rev() {
        let detached = session.detach_scopes(mark);
        *cell
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(detached);
    }
    match outcome {
        Ok(keep_going) => Ok((produced, keep_going)),
        Err(Flow::Return(value)) if !site.frames.is_empty() => {
            if !matches!(value, Value::Null) {
                produced.push(value);
            }
            Ok((produced, false))
        }
        Err(flow) => Err(flow),
    }
}

/// Backgrounds a native pipeline as a job (spec §18.4, ADR-0024).
///
/// The stream chain is built exactly as a foreground run builds it, then driven by a task on the
/// session runtime instead of being awaited: events fold into a row model the way the live view
/// folds them, other values collect, and `fg` later repaints or prints whichever the pipeline
/// produced. Aborting the task drops every receiver, which stops the producers — the same
/// cancellation the foreground path uses.
///
/// # Errors
///
/// The structured error of whichever stage could not be resolved or bound, or a refusal when
/// the pipeline mixes in external stages, which a job with no process group cannot carry yet.
pub fn run_background(session: &mut Session, list: &StageList, source: &str) -> Eval<ExitStatus> {
    let registry = registry().map_err(Flow::Failed)?;
    let table = implementations(session).map_err(Flow::Failed)?;
    let segments = segments(session, list, 0, false).ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            "the command registry could not be read",
        ))
    })?;
    // A stream chain carries native stages and nothing else. A block, a function call or a
    // program needs an evaluator, and gets one of its own (ADR-0952).
    let [Segment::Native(indices)] = segments.as_slice() else {
        return run_evaluated_job(session, list, source);
    };
    if indices
        .iter()
        .any(|index| block_of(&list.stages[*index]).is_some())
    {
        return run_evaluated_job(session, list, source);
    }

    let mut bound: Vec<(&'static CommandContract, BoundArguments)> = Vec::new();
    let mut structured = true;
    for index in indices {
        let stage = &list.stages[*index];
        let contract = native_contract(session, registry, stage, structured).ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!("`{}` is not a native command here", stage.span),
            ))
        })?;
        refuse_switched_off_spatial(session, contract, stage)?;
        let arguments =
            crate::expand::expand_globs(session, &stage.arguments).map_err(Flow::Failed)?;
        let resolved = registry
            .resolve(head_name(stage), &arguments)
            .map_err(Flow::Failed)?;
        let arguments = contract.bind(resolved.arguments).map_err(Flow::Failed)?;
        structured = !produces_bytes(contract);
        bound.push((contract, arguments));
    }

    let command_text = source
        .get(list.span.start() as usize..list.span.end() as usize)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let scope = std::sync::Arc::new(Scope::new());
    let context = session.context();
    let adapters = session.shared_adapters();
    let resolver = crate::resolve::resolver(session);
    let materialization = crate::eval::materialize::limits(session);
    let (runtime, providers) = session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the pipeline runtime",
        ))
    })?;
    let providers = providers.clone();

    let model = std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
    let values: std::sync::Arc<std::sync::Mutex<Vec<Value>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let failures = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    let task_model = std::sync::Arc::clone(&model);
    let task_values = std::sync::Arc::clone(&values);
    let task_failures = std::sync::Arc::clone(&failures);
    let handle = runtime.spawn(async move {
        let mut stream: Option<ValueStream> = None;
        for (contract, arguments) in &bound {
            let started = std::time::Instant::now();
            let temporal = match crate::temporal::invocation_context(arguments).await {
                Ok(temporal) => temporal,
                Err(error) => {
                    let _ = task_failures.lock().map(|mut held| held.push(error));
                    return;
                }
            };
            let mut invocation = Invocation::new(contract, arguments, &providers)
                .with_scope(std::sync::Arc::clone(&scope))
                .with_context(context.clone())
                .with_adapters(std::sync::Arc::clone(&adapters), resolver.clone())
                .with_temporal(temporal, registry);
            if let Some(previous) = stream.take() {
                invocation = invocation.with_input(previous);
            }
            match table.run(contract.id(), &mut invocation).await {
                // v0.4.1 §22.2, as in the foreground loop: the configured limits are stated where
                // the pipeline is assembled, and every stage below inherits them (ADR-0454).
                Ok(Outcome::Values(produced)) => {
                    stream = Some(produced.with_materialization_limits(materialization));
                }
                Ok(Outcome::Actions(outcomes)) => {
                    stream = Some(
                        action_records(contract, outcomes, started)
                            .with_materialization_limits(materialization),
                    );
                }
                Err(error) => {
                    task_failures
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(error);
                    return;
                }
            }
        }
        let Some(mut stream) = stream else {
            return;
        };
        while let Some(event) = stream.recv().await {
            match event {
                StreamEvent::Value(value) => {
                    if !crate::live::apply(
                        &mut task_model
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner),
                        &value,
                    ) {
                        task_values
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(value);
                    }
                }
                StreamEvent::Failure(error) => {
                    task_failures
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(error);
                }
            }
        }
    });

    let number = session.executor().reserve_job_number();
    ono_core::diagnostic!("[%{number}]");
    session.push_native_job(crate::session::NativeJob {
        number,
        command: command_text,
        model,
        values,
        failures,
        started: Value::now(),
        handle: crate::session::JobRun::Task(handle),
    });
    Ok(ExitStatus::SUCCESS)
}

/// Backgrounds a line that needs an evaluator — a block, a function call, a program — as a job
/// with an evaluator of its own (spec §18.4, ADR-0952).
///
/// A block holds statements, and only an evaluator runs statements: the foreground's is busy with
/// the next line, so the job gets one of its own, on a thread of its own, built from a copy of the
/// session (`Session::fork_for_job`). It runs the line exactly as the foreground would, with its
/// results captured for `fg` instead of shown, and with three differences that make it a job:
///
/// - its programs run in process groups of their own, are never handed the terminal and read an
///   empty input — a background job does not read the terminal;
/// - its interrupt is its own: `kill %N` and Ctrl-C under `fg` stop it, and a Ctrl-C aimed at the
///   foreground never reaches it;
/// - what its blocks bind or rebind stays in the job's copy of the session.
///
/// # Errors
///
/// A refusal inside a link frame, or the operating system's refusal to start a thread.
pub(crate) fn run_evaluated_job(
    session: &mut Session,
    list: &StageList,
    source: &str,
) -> Eval<ExitStatus> {
    let snapshot = session.fork_for_job().map_err(Flow::Failed)?;
    let command_text = source
        .get(list.span.start() as usize..list.span.end() as usize)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let values: std::sync::Arc<std::sync::Mutex<Vec<Value>>> = std::sync::Arc::default();
    let failures: std::sync::Arc<std::sync::Mutex<Vec<ErrorValue>>> = std::sync::Arc::default();
    let status: std::sync::Arc<std::sync::Mutex<Option<ExitStatus>>> = std::sync::Arc::default();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let canceller = snapshot.canceller();

    let number = session.executor().reserve_job_number();
    let job = EvaluatorRun {
        snapshot,
        list: list.clone(),
        source: source.to_owned(),
        values: std::sync::Arc::clone(&values),
        failures: std::sync::Arc::clone(&failures),
        status: std::sync::Arc::clone(&status),
        cancel: std::sync::Arc::clone(&cancel),
    };
    let thread = match std::thread::Builder::new()
        .name(format!("ono-job-{number}"))
        .spawn(move || job.run())
    {
        Ok(thread) => thread,
        Err(error) => {
            session.executor().release_job_number(number);
            return Err(Flow::Failed(ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                format!("the operating system refused to start the job: {error}"),
            )));
        }
    };

    ono_core::diagnostic!("[%{number}]");
    session.push_native_job(crate::session::NativeJob {
        number,
        command: command_text,
        model: std::sync::Arc::default(),
        values,
        failures,
        started: Value::now(),
        handle: crate::session::JobRun::Evaluator(crate::session::EvaluatorJob {
            cancel,
            canceller,
            thread,
            status,
        }),
    });
    Ok(ExitStatus::SUCCESS)
}

/// Everything a job's evaluator thread owns.
struct EvaluatorRun {
    snapshot: crate::session::JobSnapshot,
    list: StageList,
    source: String,
    values: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    failures: std::sync::Arc<std::sync::Mutex<Vec<ErrorValue>>>,
    status: std::sync::Arc<std::sync::Mutex<Option<ExitStatus>>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl EvaluatorRun {
    /// Runs the line on this thread, as the job's evaluator, and records how it ended.
    fn run(self) {
        let EvaluatorRun {
            snapshot,
            list,
            source,
            values,
            failures,
            status,
            cancel,
        } = self;
        crate::eval::pipeline::enter_background_job(std::sync::Arc::clone(&cancel));
        let mut session = snapshot.into_session();
        // One shell command, captured for `fg`: the job's results are what it hands over, and
        // §23.4's ceiling bounds what it may hold while nobody collects them (ADR-0457).
        session.begin_command_captures();
        session.begin_capture();
        let outcome = crate::eval::pipeline::run_stage_list(&mut session, &list, &source, false);
        let produced = session.end_capture();
        let _ = session.executor().poll_jobs();
        values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(produced);
        // The interrupt a stopped job unwinds with is how it was told to stop, not something that
        // went wrong, so it is not kept for `fg` to report.
        let stopped = cancel.load(std::sync::atomic::Ordering::SeqCst);
        let record = |error: ErrorValue| {
            if stopped && error.code() == ErrorCode::StreamCancelled {
                return;
            }
            failures
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(error);
        };
        let ended = match outcome {
            Ok(status) => status,
            Err(Flow::Failed(error)) => {
                let status = crate::eval::status_for(&error);
                record(error);
                status
            }
            Err(Flow::FailedWith(error, status)) => {
                record(error);
                status
            }
            Err(Flow::Exit(status)) => status,
            Err(Flow::Return(_) | Flow::Break | Flow::Continue) => ExitStatus::SUCCESS,
        };
        *status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(ended);
    }
}
