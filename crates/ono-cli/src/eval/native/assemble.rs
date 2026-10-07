//! A pipeline with user functions in it, assembled into one stream (v0.4.1 §26.2, ADR-0950).
//!
//! A call whose body is one pipeline is not run and collected: its body is bound and assembled
//! where the call stands, so the values the body produces are what the stage after the call reads,
//! and — wherever the call stands after another stage — the stream the stage before it produces is
//! what the body's first stage reads. A block a body runs is a [`BlockSite`] the caller's driver
//! answers, in the invocation scope of the call that wrote it.
//!
//! Whether a list can be assembled is decided statically, from the contracts and the declarations
//! alone, so `explain` answers the same question without running anything (§22.4).

use std::sync::Arc;

use ono_command::{Invocation, Outcome};
use ono_core::{ErrorCode, ExitStatus, Span};
use ono_parser::{Argument, Block, Expr, MatchArmBody, Stage, StageList, Statement};
use ono_pipeline::ValueStream;
use ono_value::{ActionStatus, ErrorValue};

use crate::eval::{Eval, Flow};
use crate::session::{Function, Session};

use super::bind::{bind_stage, stage_scope};
use super::drive::{Asked, BlockRequest, BlockSite, FrameCell, asking_stage, block_of};
use super::result::action_records;
use super::segment::{admits_bytes, continuable_body, native_contract, produces_bytes};
use super::{Start, implementations, registry};

/// An assembled but undrained stream, and what its last stage means for the driver.
pub(crate) struct Assembled {
    /// The stream the last assembled stage produces.
    pub(crate) stream: ValueStream,
    /// Whether a mutation among the assembled stages reported a failed row (spec §16.5).
    pub(crate) failed_rows: bool,
    /// Whether the last assembled stage is a block that shows its own results where they stand,
    /// so the stream carries nothing to write (ADR-0070 point 3).
    pub(crate) shows_itself: bool,
}

/// Why a stage list cannot be assembled into one stream, as a sentence `explain` and a refusal
/// can both use.
pub(crate) type Shape = Result<(), String>;

/// Whether a call of `function` can be assembled where it stands, with a stream arriving at its
/// body when `input` is true.
///
/// # Errors
///
/// The reason it cannot, as a sentence about the body.
pub(crate) fn function_shape(session: &Session, function: &Function, input: bool) -> Shape {
    call_shape(session, function, input, &mut Vec::new())
}

/// The stage index of every user function call in `list`.
pub(crate) fn function_stages(session: &Session, list: &StageList) -> Vec<usize> {
    list.stages
        .iter()
        .enumerate()
        .filter(|(_, stage)| crate::eval::called_function(session, stage).is_some())
        .map(|(index, _)| index)
        .collect()
}

fn call_shape(
    session: &Session,
    function: &Function,
    input: bool,
    inside: &mut Vec<String>,
) -> Shape {
    let name = &function.declaration.name;
    if inside.contains(name) {
        return Err(format!(
            "`{name}` calls itself, so its body has no end to assemble"
        ));
    }
    let Some(body) = continuable_body(&function.declaration.body) else {
        return Err(format!("the body of `{name}` is not one pipeline"));
    };
    inside.push(name.clone());
    let shape = list_shape(session, &body.stages, input, inside).and_then(|()| {
        // A `return` ends the function, and in a streamed body its value would otherwise pass
        // through the stages after the block — which a collected body never did (§25.5).
        let last = body.stages.len().saturating_sub(1);
        match body.stages[..last]
            .iter()
            .filter_map(block_of)
            .any(block_returns)
        {
            true => Err(format!(
                "a block in the body of `{name}` returns from it before the body's last stage"
            )),
            false => Ok(()),
        }
    });
    inside.pop();
    shape
}

fn list_shape(session: &Session, stages: &[Stage], input: bool, inside: &mut Vec<String>) -> Shape {
    if stages.is_empty() {
        return Err("there is no stage to assemble".to_owned());
    }
    let registry = registry().map_err(|error| error.message().to_owned())?;
    for (position, stage) in stages.iter().enumerate() {
        let fed = input || position > 0;
        let head = super::segment::head_name(stage);
        if !stage.redirections.is_empty() {
            return Err(format!("`{head}` sends its output to a file"));
        }
        if let Some(function) = crate::eval::called_function(session, stage) {
            call_shape(session, &function, fed, inside)?;
            continue;
        }
        if block_of(stage).is_some() {
            if !fed {
                return Err("`each { … }` has no stream to run its block over".to_owned());
            }
            continue;
        }
        if session.alias(head).is_some() {
            return Err(format!("`{head}` is an alias"));
        }
        // A head the shell answers itself, or one a KUANG/11 package contributes, is not a stage
        // the native table runs: the evaluator claims it before the registry would (#130).
        if crate::resolve::builtin_for(head, super::super::pipeline::first_word(stage)).is_some()
            || crate::meta::claims(stage).is_some()
            || crate::temporal::claims(stage).is_some()
            || crate::change::claims(stage)
            || crate::remote::claims(stage).is_some()
            || crate::context::claims(stage).is_some()
            || crate::plugins::claims(stage).is_some()
            || (position == 0 && crate::plugins::contributed_command(stage).is_some())
        {
            return Err(format!("`{head}` is answered by the shell itself"));
        }
        let Some(contract) = native_contract(session, registry, stage, true) else {
            return Err(format!("`{head}` is not a native command"));
        };
        if produces_bytes(contract) || admits_bytes(contract) {
            return Err(format!("`{head}` turns the values into text"));
        }
        if position == 0 && input && contract.input().accepts_null() {
            return Err(format!(
                "`{head}` produces values of its own and reads no input stream"
            ));
        }
    }
    Ok(())
}

/// Whether a block, or a block nested in it, holds a `return`.
fn block_returns(block: &Block) -> bool {
    block.statements.iter().any(statement_returns)
}

fn statement_returns(statement: &Statement) -> bool {
    match statement {
        Statement::Return(_) => true,
        Statement::If(branch) => {
            branch
                .branches
                .iter()
                .any(|branch| block_returns(&branch.block))
                || branch.else_block.as_ref().is_some_and(block_returns)
        }
        Statement::For(loop_) => block_returns(&loop_.body),
        Statement::While(loop_) => block_returns(&loop_.body),
        Statement::Match(match_) => match_.arms.iter().any(|arm| match &arm.body {
            MatchArmBody::Block(block) => block_returns(block),
            MatchArmBody::Expr(_) => false,
        }),
        Statement::Try(try_) => {
            block_returns(&try_.body)
                || try_
                    .catch
                    .as_ref()
                    .is_some_and(|catch| block_returns(&catch.body))
        }
        Statement::Pipeline(pipeline) => std::iter::once(&pipeline.head)
            .chain(pipeline.tail.iter().map(|chained| &chained.list))
            .flat_map(|list| list.stages.iter())
            .flat_map(|stage| stage.arguments.iter())
            .any(|argument| matches!(argument, Argument::Value(Expr::Block(block)) if block_returns(block))),
        _ => false,
    }
}

/// What the stages being assembled stand inside.
struct Within<'a> {
    /// The invocation scopes of the calls whose bodies these stages are, outermost first.
    frames: &'a [FrameCell],
    /// Whether a stage after the assembled ones reads what they produce.
    consumed_after: bool,
    /// Where the block stages ask.
    asked: &'a Asked,
}

/// Assembles `stages`, which [`list_shape`] has accepted, into the stream they produce.
fn assemble(
    session: &mut Session,
    stages: &[Stage],
    source: &Arc<str>,
    input: Option<ValueStream>,
    within: &Within<'_>,
) -> Eval<Assembled> {
    let mut stream = input;
    let mut failed_rows = false;
    let mut shows_itself = false;
    for (position, stage) in stages.iter().enumerate() {
        let consumed = position + 1 < stages.len() || within.consumed_after;
        if let Some(function) = crate::eval::called_function(session, stage) {
            let assembled = assemble_call(
                session,
                &function,
                stage,
                source,
                stream.take(),
                &Within {
                    frames: within.frames,
                    consumed_after: consumed,
                    asked: within.asked,
                },
            )?;
            failed_rows |= assembled.failed_rows;
            shows_itself = assembled.shows_itself;
            stream = Some(assembled.stream);
            continue;
        }
        if let Some(block) = block_of(stage) {
            let Some(previous) = stream.take() else {
                return Err(Flow::Failed(super::segment::each_needs_a_stream()));
            };
            let site = Arc::new(BlockSite {
                block: block.clone(),
                source: Arc::clone(source),
                consumed,
                frames: within.frames.to_vec(),
            });
            // The stage is a task of the pipeline's runtime, which assembly is not inside.
            let handle = runtime_handle(session)?;
            let _entered = handle.enter();
            stream = Some(asking_stage(previous, site, within.asked.clone()));
            shows_itself = !consumed;
            continue;
        }
        let (produced, failed) = native_stage(session, stage, source, stream.take())?;
        failed_rows |= failed;
        shows_itself = false;
        stream = Some(produced);
    }
    let stream = stream.ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            "there is no stage to assemble",
        ))
    })?;
    Ok(Assembled {
        stream,
        failed_rows,
        shows_itself,
    })
}

/// Assembles one call: its arguments read in the caller's scope, its parameters bound in a scope
/// of its own, its body assembled over `input` while that scope is on the session — and the scope
/// taken off again once the body is assembled, kept for the body's blocks (§26.3, ADR-0950).
fn assemble_call(
    session: &mut Session,
    function: &Function,
    stage: &Stage,
    source: &str,
    input: Option<ValueStream>,
    within: &Within<'_>,
) -> Eval<Assembled> {
    let declaration = &function.declaration;
    let arguments = super::super::function::call_arguments(session, stage, source)?;
    super::super::function::check_arity(declaration, arguments.len())?;
    let Some(body) = continuable_body(&declaration.body) else {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("the body of `{}` is not one pipeline", declaration.name),
        )));
    };

    let depth = session.scope_depth();
    session.push_scope();
    let cell: FrameCell = Arc::new(std::sync::Mutex::new(None));
    let mut frames = within.frames.to_vec();
    frames.push(Arc::clone(&cell));
    let assembled =
        super::super::function::bind_parameters(session, declaration, arguments, &function.source)
            .and_then(|()| {
                assemble(
                    session,
                    &body.stages,
                    &function.source,
                    input,
                    &Within {
                        frames: &frames,
                        consumed_after: within.consumed_after,
                        asked: within.asked,
                    },
                )
            });
    let invocation = session.detach_scopes(depth);
    *cell
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(invocation);
    assembled
}

/// Binds and starts one native stage over `input`, answering the stream it produces.
fn native_stage(
    session: &mut Session,
    stage: &Stage,
    source: &str,
    input: Option<ValueStream>,
) -> Eval<(ValueStream, bool)> {
    let registry = registry().map_err(Flow::Failed)?;
    let table = implementations(session).map_err(Flow::Failed)?;
    let Some(bound) = bind_stage(session, registry, stage, true)? else {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            format!("`{}` is not a native command here", stage.span.of(source)),
        )));
    };
    let bound = [bound];
    // §26.3 by construction: the scope the stage's expressions read is snapshotted now, while
    // whatever invocation scope it was written in is still on the session.
    let scope = Arc::new(stage_scope(session, &bound, source)?);
    let [(contract, arguments)] = bound;
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
    let mutating = registry
        .verb(contract.verb())
        .is_some_and(ono_command::VerbSpec::is_mutating);
    let started = handle.block_on(async {
        let started = std::time::Instant::now();
        let requested_at = jiff::Timestamp::now();
        let temporal = crate::temporal::invocation_context(&arguments).await?;
        let mut invocation = Invocation::new(contract, &arguments, providers)
            .with_scope(Arc::clone(&scope))
            .with_context(context.clone())
            .with_adapters(Arc::clone(&adapters), resolver.clone())
            .with_temporal(temporal, registry);
        if let Some(previous) = input {
            invocation = invocation.with_input(previous);
        }
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
            Ok(Outcome::Values(values)) => {
                Ok((values.with_materialization_limits(materialization), false))
            }
            Ok(Outcome::Actions(outcomes)) => {
                // v0.5 §17.2: a mutation's lifecycle is recorded where the shell holds both what
                // was asked and what came back, wherever in a pipeline the mutation stands.
                crate::temporal::record_action(
                    &contract.spelling(),
                    contract.id(),
                    &words,
                    requested_at,
                    crate::temporal::Outcome::Acted(&outcomes),
                )
                .await;
                let failed = outcomes
                    .iter()
                    .any(|outcome| outcome.status() == ActionStatus::Failed);
                Ok((
                    action_records(contract, outcomes, started)
                        .with_materialization_limits(materialization),
                    failed,
                ))
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
                Err(error)
            }
        }
    });
    started.map_err(super::segment::interrupted_flow)
}

/// Runs `list`, whose stages up to and including `through` are assembled into one stream with
/// every user function among them, and whose remaining stages read that stream.
///
/// The stages before the first call after the head are assembled when they can be; when they
/// cannot — a value at the head, a program, a body of several statements — they run first and
/// their values seed the rest, which then has to be finite (§26.2's guard). From the first such
/// call on, every stage up to `through` must be assembled: a call between two stages reads its
/// input as a stream or not at all, and the refusal says why before anything has run.
///
/// # Errors
///
/// `type.mismatch` naming the call and the shape of its body when a call cannot take the stream
/// in front of it; otherwise whatever a stage reports.
pub(crate) fn run_assembled(
    session: &mut Session,
    list: &StageList,
    source: &str,
    through: usize,
) -> Eval<ExitStatus> {
    let calls = function_stages(session, list);
    let first_inner = calls
        .iter()
        .copied()
        .find(|index| *index >= 1)
        .unwrap_or(through + 1)
        .min(through + 1);
    let head = &list.stages[..first_inner];
    let inner = &list.stages[first_inner..=through];

    // Everything is decided before anything runs: a call that cannot take its input is refused
    // while nothing has been spawned, bound or collected.
    if !inner.is_empty()
        && let Err(reason) = list_shape(session, inner, true, &mut Vec::new())
    {
        return Err(Flow::Failed(cannot_take_input(
            session, inner, source, &reason,
        )));
    }
    let head_streams = list_shape(session, head, false, &mut Vec::new()).is_ok();

    let shared: Arc<str> = Arc::from(source);
    let (asked, requests) = tokio::sync::mpsc::channel::<BlockRequest>(1);
    let head_assembled = if head_streams {
        assemble(
            session,
            head,
            &shared,
            None,
            &Within {
                frames: &[],
                consumed_after: first_inner < list.stages.len(),
                asked: &asked,
            },
        )?
    } else {
        let prefix = StageList {
            stages: head.to_vec(),
            span: Span::new(
                list.span.start(),
                head.last()
                    .map_or(list.span.end(), |stage| stage.span.end()),
            ),
        };
        session.begin_capture();
        let outcome = super::super::pipeline::run_stage_list(session, &prefix, source, false);
        let values = session.end_capture();
        outcome?;
        let handle = runtime_handle(session)?;
        let _entered = handle.enter();
        Assembled {
            stream: ValueStream::from_values(values),
            failed_rows: false,
            shows_itself: false,
        }
    };
    let assembled = if inner.is_empty() {
        head_assembled
    } else {
        let continued = assemble(
            session,
            inner,
            &shared,
            Some(head_assembled.stream),
            &Within {
                frames: &[],
                consumed_after: through + 1 < list.stages.len(),
                asked: &asked,
            },
        )?;
        Assembled {
            failed_rows: continued.failed_rows || head_assembled.failed_rows,
            ..continued
        }
    };
    super::run_from(
        session,
        list,
        source,
        through + 1,
        Start::Pipe {
            stream: assembled.stream,
            failed_rows: assembled.failed_rows,
            shows_itself: assembled.shows_itself,
            requests: Some((asked, requests)),
        },
    )
}

/// The session's pipeline runtime, which every stage of an assembled stream is a task of.
fn runtime_handle(session: &mut Session) -> Eval<tokio::runtime::Handle> {
    session
        .pipeline_context()
        .map(|(runtime, _)| runtime.handle().clone())
        .ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                "the operating system refused to start the pipeline runtime",
            ))
        })
}

/// The refusal for a call that cannot read the stream in front of it.
fn cannot_take_input(session: &Session, inner: &[Stage], source: &str, reason: &str) -> ErrorValue {
    let call = inner
        .iter()
        .find(|stage| crate::eval::called_function(session, stage).is_some())
        .map_or("the call", |stage| super::segment::head_name(stage));
    let written = inner
        .first()
        .map(|stage| stage.span.of(source).trim().to_owned())
        .unwrap_or_default();
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!("`{call}` cannot read the stream in front of it: {reason}"),
    )
    .with_help(format!(
        "a function between two stages reads its input through its body, which must be one \
         pipeline whose first stage reads a stream — `fn {call}() {{ where … }}` (ADR-0951); \
         `{written}` is where the stream arrives"
    ))
    .with_metadata("function", ono_value::Value::string(call))
}
