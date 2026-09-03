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
    Segment, head_name, interrupted_flow, native_contract, produces_bytes,
    refuse_switched_off_spatial, segments,
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

/// One input value, and where the answer goes.
#[derive(Debug)]
pub(super) struct BlockRequest {
    /// Which stage of the list asked, so the driver knows which block to run.
    stage: usize,
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
    stage: usize,
    asked: tokio::sync::mpsc::Sender<BlockRequest>,
) -> ValueStream {
    // One value in, zero or more out: a stream that ends still ends, and one that does not still
    // does not (§25.6, Appendix E's `item_transform`).
    let boundedness = input.boundedness();
    input.stage(boundedness, move |mut input, sink| async move {
        while let Some(value) = input.next_value(&sink).await {
            let (reply, answer) = tokio::sync::oneshot::channel();
            if asked
                .send(BlockRequest {
                    stage,
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
        if ono_process::take_interrupt() {
            return;
        }
    }
}

/// What the driver drained out of one segment.
#[derive(Default)]
pub(super) struct Drained {
    /// The values and the per-item failures a pipeline produced, drained before they are
    /// written, rendered or retained (`docs/spec/hardening/streaming.yaml`, §26.1).
    pub(super) values: Vec<Value>,
    pub(super) failures: Vec<ErrorValue>,
    /// The flow a block raised, which stops the drain where the block stopped.
    pub(super) stopped: Option<Flow>,
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
    stages: &[Stage],
    source: &str,
    mut draining: Option<ValueStream>,
    mut showing: Option<std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ErrorValue>> + '_>>>,
) -> Eval<Drained> {
    let mut drained = Drained::default();
    let mut asking = true;
    while draining.is_some() || showing.is_some() {
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
            }
        });
        match driven {
            Driven::Ask(request) => {
                let Some(block) = block_of(&stages[request.stage]) else {
                    continue;
                };
                // ADR-0070 point 3 again, per item: the block's values are captured only where
                // a later stage consumes them. §25.4 permits this scope because it is one item's
                // result, and forbids the collection over all items that used to stand here.
                let consumed = request.stage + 1 < stages.len();
                match crate::eval::run_each_item(session, block, source, request.value, consumed) {
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
                }
            }
            Driven::Asked => asking = false,
            Driven::Event(StreamEvent::Value(value)) => drained.values.push(value),
            Driven::Event(StreamEvent::Failure(error)) => drained.failures.push(error),
            Driven::Drained => {
                draining = None;
                showing = None;
            }
            Driven::Interrupted => {
                return Err(interrupted_flow(ErrorValue::new(
                    ErrorCode::StreamCancelled,
                    "interrupted",
                )));
            }
        }
    }
    Ok(drained)
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
    let [Segment::Native(indices)] = segments.as_slice() else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                "a background job cannot mix native stages with external programs yet",
            )
            .with_help("background the native part alone, or serialise into a file"),
        ));
    };

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
            let mut invocation = Invocation::new(contract, arguments, &providers)
                .with_scope(std::sync::Arc::clone(&scope))
                .with_context(context.clone())
                .with_adapters(std::sync::Arc::clone(&adapters), resolver.clone());
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
    eprintln!("[%{number}]");
    session.push_native_job(crate::session::NativeJob {
        number,
        command: command_text,
        model,
        values,
        failures,
        started: Value::now(),
        handle,
    });
    Ok(ExitStatus::SUCCESS)
}
