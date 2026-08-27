//! Running the object pipeline of spec §5 in the shell itself.
//!
//! A pipeline is a sequence of stages, and each one is either a child process or a native command
//! the registry declares. This module decides which is which, then runs the native ones and hands
//! the rest to [`ono_process`], threading the boundary of spec §12.3 between them.
//!
//! The boundary is explicit in both directions, as spec §12.3 requires. Bytes become objects only
//! through a command that says so — `from json` — and objects become bytes only through `to` or
//! `format`. A structured stream aimed at a child process is a type error naming `to json`
//! (ADR-0013), never a silent rendering the receiving program would have to parse back.

use std::io::Write;
use std::sync::OnceLock;

use ono_adapter::{AdapterPlan, Consumer, OutputDemand};
use ono_command::{
    BoundArguments, CommandContract, CommandRegistry, CommandTable, Invocation, Outcome, Scope,
};
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::{Stage, StageHead, StageList};
use ono_pipeline::{StreamEvent, ValueStream};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::resolve::Namespace;
use crate::session::Session;
use crate::sink::Sink;

/// The command contracts, parsed once from the copies embedded at compile time.
///
/// # Errors
///
/// The structured error the registry raises when an embedded contract cannot be read. That is a
/// build-time mistake rather than a user's, but it is reported rather than panicked over: a shell
/// that aborts on startup teaches nobody anything.
pub fn registry() -> Result<&'static CommandRegistry, ErrorValue> {
    CommandRegistry::embedded()
}

/// The native implementations, built once against the registry.
fn implementations() -> Result<&'static CommandTable, ErrorValue> {
    static TABLE: OnceLock<CommandTable> = OnceLock::new();
    if let Some(table) = TABLE.get() {
        return Ok(table);
    }
    let built = ono_command::builtin_commands(registry()?);
    Ok(TABLE.get_or_init(|| built))
}

/// One run of adjacent stages that belong on the same side of the byte boundary.
#[derive(Debug)]
enum Segment {
    /// Child processes, joined to each other by real pipes (ADR-0013).
    External(Vec<usize>),
    /// Native commands, joined by the value stream.
    Native(Vec<usize>),
}

/// Whether any stage of `list` is a native command, and so whether this module runs it at all.
///
/// Deciding this needs the whole list rather than one stage: a transform binds where structure
/// reaches it, and nowhere else (ADR-0028).
#[must_use]
pub fn claims(session: &mut Session, list: &StageList) -> bool {
    // A forced adaptation is this module's to run — or to refuse — wherever it stands
    // (spec v0.3 §1.18).
    if list.stages.iter().any(ono_command::is_adapt) {
        return true;
    }
    segments(session, list, 0, false).is_some_and(|segments| {
        segments
            .iter()
            .any(|segment| matches!(segment, Segment::Native(_)))
    })
}

/// Whether an all-external pipeline ends in a stage an adapter renders at the terminal.
///
/// Spec v0.3 §1.4: at a terminal a high-confidence adapter may produce values and let the
/// renderer display them. That is only ever the last stage, only with stdout on a terminal and
/// not redirected, and never for a background job (ADR-0057 point 2).
#[must_use]
pub fn adapts_at_terminal(session: &mut Session, list: &StageList) -> bool {
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return false;
    }
    let Some(stage) = list.stages.last() else {
        return false;
    };
    if redirects_stdout(stage) {
        return false;
    }
    negotiate_literally(session, stage, &OutputDemand::Interactive)
        .is_some_and(|negotiation| negotiation.plan().is_some())
}

/// Whether the stage sends its stdout somewhere other than the pipe or the terminal.
fn redirects_stdout(stage: &Stage) -> bool {
    stage.redirections.iter().any(|redirection| {
        matches!(redirection.fd, None | Some(1))
            && matches!(
                redirection.op,
                ono_parser::RedirectOp::Write
                    | ono_parser::RedirectOp::Append
                    | ono_parser::RedirectOp::DupWrite
            )
    })
}

/// The program a stage would run through an adapter, if it is that kind of stage at all.
///
/// `raw` never adapts (ADR-0054); `exec:` and a bare head do; a native, function or value head
/// is not a program.
fn adaptable_program(session: &Session, stage: &Stage) -> Option<(String, std::path::PathBuf)> {
    if ono_command::is_raw(stage) {
        return None;
    }
    // `adapt <program>`: the program is the word after the keyword (ADR-0064).
    if ono_command::is_adapt(stage) {
        let program = ono_command::adapt_program(stage)?;
        let path = crate::resolve::find_on_path(session, program)?;
        return Some((program.to_owned(), path));
    }
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    if !matches!(
        Namespace::from_prefix(name.namespace.as_deref()),
        Some(Namespace::Any | Namespace::External)
    ) {
        return None;
    }
    let path = crate::resolve::find_on_path(session, &name.name)?;
    Some((name.name.clone(), path))
}

/// Negotiates a stage from its source text alone, for decisions made before anything runs.
///
/// Segmentation and the terminal check cannot evaluate arguments — a `$(…)` argument would run
/// twice — so they ask with the words as written. Execution asks again with the expanded words;
/// where the two differ, the run's answer is the one that counts.
/// What the remote side of a link frame would do with an invocation (spec v0.3 §1.54).
pub(crate) struct RemoteDecision {
    pub(crate) adapted: bool,
    pub(crate) state: String,
}

/// Asks the innermost link's agent to negotiate `argv` for `demand` without running it.
///
/// `None` when the session is not inside a link frame or the agent cannot answer — an older
/// agent, a lost link — in which case the caller falls back to local semantics and says so.
pub(crate) fn remote_decision(
    session: &mut Session,
    argv: &[String],
    demand: &OutputDemand,
) -> Option<RemoteDecision> {
    let demand_name = match demand {
        OutputDemand::Structured { .. } => "structured",
        OutputDemand::Interactive => "interactive",
        _ => return None,
    };
    let handle = session.runtime()?.handle().clone();
    let link = session.remote_link()?;
    let mut stream = link.link.adapt(argv, demand_name, true).ok()?;
    handle.block_on(async {
        while let Some(message) = stream.recv().await {
            match message {
                ono_protocol::RemoteMessage::Value(Value::Map(map)) => {
                    return Some(RemoteDecision {
                        adapted: matches!(map.get("adapted"), Some(Value::Bool(true))),
                        state: map
                            .get("state")
                            .and_then(|state| state.as_str().ok())
                            .unwrap_or("unknown")
                            .to_owned(),
                    });
                }
                ono_protocol::RemoteMessage::Failure(error) => {
                    return Some(RemoteDecision {
                        adapted: false,
                        state: format!("raw ({})", error.message()),
                    });
                }
                _ => {}
            }
        }
        None
    })
}

/// The words a stage would hand its program, from the source alone, program first.
pub(crate) fn literal_argv(stage: &Stage) -> Option<Vec<String>> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    let words = literal_words(stage);
    if ono_command::is_adapt(stage) {
        let mut iter = words.into_iter();
        let program = iter.next()?;
        let mut argv = vec![program];
        argv.extend(iter);
        return Some(argv);
    }
    let mut argv = vec![name.name.clone()];
    argv.extend(words);
    Some(argv)
}

fn negotiate_literally(
    session: &mut Session,
    stage: &Stage,
    demand: &OutputDemand,
) -> Option<ono_adapter::Negotiation> {
    // Inside a link frame the remote decides (spec v0.3 §1.54); its answer is folded into the
    // two states the callers here distinguish — a plan, or not.
    if session.link_host().is_some()
        && !ono_command::is_raw(stage)
        && let Some(argv) = literal_argv(stage)
        && let Some(decision) = remote_decision(session, &argv, demand)
    {
        return Some(if decision.adapted {
            ono_adapter::Negotiation::RemoteAdapted {
                state: decision.state,
            }
        } else {
            ono_adapter::Negotiation::RawPreferred {
                reason: decision.state,
            }
        });
    }
    let (name, path) = adaptable_program(session, stage)?;
    let mut argv = vec![name];
    let words = literal_words(stage);
    argv.extend(if ono_command::is_adapt(stage) {
        words.into_iter().skip(1).collect::<Vec<String>>()
    } else {
        words
    });
    Some(session.adapters().negotiate(&path, &argv, demand))
}

/// A stage's arguments as written, where the source text is not at hand: words and options
/// are themselves; an expression is a placeholder the matcher treats as a positional.
fn literal_words(stage: &Stage) -> Vec<String> {
    stage
        .arguments
        .iter()
        .map(|argument| match argument {
            ono_parser::Argument::Word(word) => word.text.clone(),
            ono_parser::Argument::Option(option) => match &option.value {
                Some(_) => format!("--{}=", option.name),
                None => format!("--{}", option.name),
            },
            _ => "$expression".to_owned(),
        })
        .collect()
}

/// Splits `list` into runs of native and external stages.
///
/// Returns `None` when the registry itself cannot be read, which leaves the caller on the
/// external path it would have taken before native commands existed.
fn segments(
    session: &mut Session,
    list: &StageList,
    start: usize,
    seeded: bool,
) -> Option<Vec<Segment>> {
    let registry = registry().ok()?;
    let mut segments: Vec<Segment> = Vec::new();
    let mut structured = seeded;

    for (index, stage) in list.stages.iter().enumerate().skip(start) {
        let native = native_contract(session, registry, stage, structured).is_some();
        structured = if native {
            native_contract(session, registry, stage, structured)
                .is_some_and(|contract| !produces_bytes(contract))
        } else {
            // An adapted program hands structure on (spec v0.3 §1.4), so `lsblk | sort size`
            // is the transform where `printf x | sort` is the program (ADR-0028, ADR-0057).
            ono_command::is_adapt(stage)
                || negotiate_literally(session, stage, &OutputDemand::Structured { schema: None })
                    .is_some_and(|negotiation| negotiation.plan().is_some())
        };

        match segments.last_mut() {
            Some(Segment::Native(indices)) if native => indices.push(index),
            Some(Segment::External(indices)) if !native => indices.push(index),
            _ if native => segments.push(Segment::Native(vec![index])),
            _ => segments.push(Segment::External(vec![index])),
        }
    }
    Some(segments)
}

/// The contract `stage` names, if a native command is what it means here.
///
/// `structured` says whether objects reach this stage. It is what keeps `printf … | sort` the
/// program it has always been while `get process | sort name` is the transform: a command
/// declared over a stream of records binds only where a stream of records arrives.
fn native_contract(
    session: &Session,
    registry: &'static CommandRegistry,
    stage: &Stage,
    structured: bool,
) -> Option<&'static CommandContract> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    let namespace = Namespace::from_prefix(name.namespace.as_deref())?;
    if matches!(namespace, Namespace::External | Namespace::Function) {
        return None;
    }
    // A shell builtin changes the shell, so it is never a native command: `cd` in a pipeline
    // moves a directory nobody is standing in, and the evaluator has already said so.
    if crate::resolve::BUILTINS.contains(&name.name.as_str()) {
        return None;
    }

    let contract = registry
        .resolve(&name.name, &stage.arguments)
        .ok()?
        .contract;
    if binds_here(contract, structured) || matches!(namespace, Namespace::Native) {
        return Some(contract);
    }
    // A transform reached by bytes is the program of the same name, where one exists. Where none
    // does, the transform stays and reports the type error honestly — "`count` needs objects" is
    // a better answer than "command not found: count".
    crate::resolve::find_on_path(session, &name.name)
        .is_none()
        .then_some(contract)
}

/// Whether `contract` can accept the input that reaches it.
///
/// A producer starts a pipeline and needs nothing. A serializer or parser is defined over bytes
/// and text, which is exactly what a child process hands on. A transform is defined over objects.
fn binds_here(contract: &CommandContract, structured: bool) -> bool {
    structured || accepts_bytes(contract.input().text())
}

/// Whether a declared input type admits something other than a stream of objects.
fn accepts_bytes(input: &str) -> bool {
    input.split('|').map(str::trim).any(|alternative| {
        matches!(alternative, "any" | "null" | "string" | "bytes" | "value")
            || alternative.starts_with("string")
            || alternative.starts_with("bytes")
    })
}

/// Whether a command's output is bytes or text rather than objects.
fn produces_bytes(contract: &CommandContract) -> bool {
    let output = contract.output().text();
    output.split('|').map(str::trim).all(|alternative| {
        matches!(alternative, "string" | "bytes")
            || alternative.starts_with("string")
            || alternative.starts_with("bytes")
    })
}

/// Checks every expression in `pipeline` against the schema that would reach it.
///
/// Spec §11.3: a typo in a field name is caught before process enumeration begins, because the
/// contracts declare what flows where. Everything here is declarative — nothing is enumerated and
/// nothing is spawned — so the check costs nothing when the pipeline is sound. A pipeline whose
/// schemas are unknown is not checked rather than guessed at.
///
/// # Errors
///
/// `type.unknown_field` naming the field, the schema, and the nearest declared field.
pub fn check(
    session: &mut Session,
    pipeline: &ono_parser::Pipeline,
    source: &str,
) -> Result<(), ErrorValue> {
    let Ok(registry) = registry() else {
        // An unreadable registry is reported where a native stage actually runs; the pre-flight
        // check is an optimisation of the failure path, not a second gate.
        return Ok(());
    };
    let schemas: Vec<_> = ono_value::builtin_schemas().schemas().cloned().collect();
    // A program an adapter gives a schema is a producer like any other (spec v0.3 §1.61,
    // ADR-0067): the plan says which stages those are, so the check reaches the stages after
    // them. Inside a link frame the remote decides, and nothing is known here.
    let adapted = if session.link_host().is_some() {
        Vec::new()
    } else {
        let resolve = crate::resolve::resolver(session);
        let executables = |name: &str| resolve(name);
        let (providers, adapters) = session.registries();
        ono_command::plan_with(
            registry,
            Some(providers),
            pipeline,
            source,
            &ono_command::PlanContext {
                stdout: ono_adapter::Stdout::Stream,
                adapters: Some(adapters),
                executables: Some(&executables),
            },
        )
        .adapted_schemas()
    };
    ono_command::check_pipeline_with(registry, &schemas, pipeline, &adapted)
}

/// Runs a stage list that contains at least one native command.
///
/// # Errors
///
/// The structured error of whichever stage could not be resolved, bound, or run.
pub fn run(session: &mut Session, list: &StageList, source: &str) -> Eval<ExitStatus> {
    run_from(session, list, source, 0, None)
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
    let table = implementations().map_err(Flow::Failed)?;
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
        let resolved = registry
            .resolve(head_name(stage), &stage.arguments)
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
    let (runtime, providers) = session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the pipeline runtime",
        ))
    })?;
    let providers = providers.clone();

    let model = std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
    let values = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
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
                Ok(Outcome::Values(produced)) => stream = Some(produced),
                Ok(Outcome::Actions(outcomes)) => {
                    let elapsed = ono_value::Duration::from_nanoseconds(
                        i128::try_from(started.elapsed().as_nanos()).unwrap_or(i128::MAX),
                    );
                    stream = Some(ValueStream::from_values(
                        outcomes
                            .into_iter()
                            .map(|outcome| outcome.into_record(elapsed).into_value()),
                    ));
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
        handle,
    });
    Ok(ExitStatus::SUCCESS)
}

/// Runs a stage list whose head has already produced values.
///
/// A pipeline may start with a value instead of a command — `$hot | where …`, `@-1 | count` —
/// and a list splices because it *is* several values (ADR-0019). The evaluator has already
/// turned the head into `seed`; everything after it runs exactly as if a native producer had
/// streamed those values.
///
/// # Errors
///
/// The structured error of whichever stage could not be resolved, bound, or run.
pub fn run_seeded(
    session: &mut Session,
    list: &StageList,
    source: &str,
    seed: Vec<Value>,
) -> Eval<ExitStatus> {
    run_from(session, list, source, 1, Some(seed))
}

fn run_from(
    session: &mut Session,
    list: &StageList,
    source: &str,
    start: usize,
    seed: Option<Vec<Value>>,
) -> Eval<ExitStatus> {
    let registry = registry().map_err(Flow::Failed)?;
    let segments = segments(session, list, start, seed.is_some()).ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            "the command registry could not be read",
        ))
    })?;

    let mut carried: Option<Vec<u8>> = None;
    let mut seed = seed;
    let mut status = ExitStatus::SUCCESS;

    // A seeded list with nothing after the seed shows the seed itself: `@-1` alone re-renders
    // the retained result.
    if segments.is_empty() {
        if let Some(values) = seed.take()
            && let Some(stage) = list.stages.first()
        {
            write_result(session, stage, &values, false, source)?;
        }
        return Ok(status);
    }

    let mut position = 0;
    while position < segments.len() {
        let segment = &segments[position];
        let last = position + 1 == segments.len();
        match segment {
            Segment::External(indices) => {
                if let Some(values) = seed.take() {
                    // Spec §12.3: objects reach a child process only through an explicit
                    // representation. Text and bytes already are one.
                    carried = Some(seed_bytes(values)?);
                }
                let demand = external_demand(
                    session,
                    registry,
                    list,
                    indices,
                    segments.get(position + 1),
                    last,
                );
                if let Some(demand_kind) = demand.as_ref()
                    && session.link_host().is_some()
                    && matches!(
                        demand_kind,
                        OutputDemand::Structured { .. } | OutputDemand::Interactive
                    )
                    && let Some(stage) = indices.last().map(|index| &list.stages[*index])
                    && !ono_command::is_raw(stage)
                    && let Some(argv) = remote_argv(session, stage, source)?
                {
                    // Inside a link frame the adapter and the executable are the remote's
                    // (spec v0.3 §1.54, ADR-0066): the agent negotiates, runs and decodes,
                    // and the records arrive already marked with the host.
                    let following: &[usize] = match segments.get(position + 1) {
                        Some(Segment::Native(next)) => next,
                        _ => &[],
                    };
                    let consumed_next = !following.is_empty();
                    match run_remote_adapted(
                        session,
                        list,
                        following,
                        source,
                        &argv,
                        demand_kind,
                        last || (consumed_next && position + 2 == segments.len()),
                    )? {
                        RemoteRun::Adapted(status_after) => {
                            status = status_after;
                            carried = None;
                            position += if consumed_next { 2 } else { 1 };
                            continue;
                        }
                        RemoteRun::NotAdapted(reason) => {
                            // The remote has nothing for this invocation: say so, and let the
                            // program run as it always has — locally, raw.
                            if matches!(demand_kind, OutputDemand::Interactive) {
                                eprintln!(
                                    "{}",
                                    ono_render::sanitise(&format!(
                                        "{}: {reason}; running `{}` locally",
                                        session.link_host().unwrap_or_default(),
                                        argv.join(" ")
                                    ))
                                );
                            }
                        }
                    }
                }
                if let Some((plan, argv, demand)) =
                    negotiate_stage(session, list, indices, source, demand)?
                {
                    session.note_adaptation(plan.adapter().full_id(), plan.argv().join(" "));
                    if plan.adapter().decoder().kind().streams() {
                        // Records flow while the child runs (ADAPT-005, ADR-0059): the
                        // following native segment — or the renderer — consumes them as they
                        // arrive, so both segments are run here and the loop skips ahead.
                        let following: &[usize] = match segments.get(position + 1) {
                            Some(Segment::Native(next)) => next,
                            _ => &[],
                        };
                        let consumed_next = !following.is_empty();
                        status = run_streamed_segment(
                            session,
                            registry,
                            list,
                            indices,
                            following,
                            source,
                            carried.take(),
                            &plan,
                            &argv,
                            last || (consumed_next && position + 2 == segments.len()),
                        )?;
                        carried = None;
                        position += if consumed_next { 2 } else { 1 };
                        continue;
                    }
                    let (bytes, external_status) = crate::eval::run_adapted_segment(
                        session,
                        list,
                        indices,
                        source,
                        carried.take(),
                        &plan,
                    )?;
                    status = external_status;
                    let stage = &list.stages[*indices.last().unwrap_or(&0)];
                    // A child that failed has failed, whatever it wrote (spec v0.3 §1.20,
                    // ADR-0057 point 3): its output is not decoded, its status stands.
                    if external_status != ExitStatus::SUCCESS {
                        let program = plan
                            .executable()
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        return Err(Flow::FailedWith(
                            ErrorValue::new(
                                ErrorCode::ExternalExitNonzero,
                                format!("{program} exited with status {}", external_status.code()),
                            )
                            .with_help(format!(
                                "its stderr above says why; `raw {}` runs it as typed",
                                argv.join(" ")
                            )),
                            external_status,
                        ));
                    }
                    match decode_adapted(session, &plan, &argv, &bytes) {
                        Ok(values) => {
                            if last {
                                write_result(session, stage, &values, false, source)?;
                            } else {
                                seed = Some(values);
                            }
                        }
                        Err(error) => {
                            // At the terminal a decode failure falls back to the bytes the
                            // tool already wrote, never to a second run (spec v0.3 §1.57,
                            // ADR-0057 point 7). A structured consumer gets the error.
                            if demand == OutputDemand::Interactive
                                && plan.adapter().fallback() == ono_adapter::Fallback::Raw
                            {
                                report_fallback(&plan, &error);
                                let mut out = std::io::stdout().lock();
                                out.write_all(&bytes).map_err(write_failed)?;
                                out.flush().map_err(write_failed)?;
                            } else {
                                return Err(Flow::Failed(error));
                            }
                        }
                    }
                    position += 1;
                    continue;
                }
                let (bytes, external_status) = crate::eval::run_external_segment(
                    session, list, indices, source, carried, last,
                )?;
                carried = bytes;
                status = external_status;
            }
            Segment::Native(indices) => {
                carried = run_native_segment(
                    session,
                    registry,
                    list,
                    indices,
                    source,
                    carried,
                    seed.take().map_or(Seed::None, Seed::Values),
                    position == 0,
                    last,
                )?;
                status = ExitStatus::SUCCESS;
            }
        }
        position += 1;
    }
    Ok(status)
}

/// What starts a native segment's stream besides carried bytes.
enum Seed {
    /// Nothing: the head stage is a producer, or reads the carried bytes.
    None,
    /// Values already in hand — a retained result, a plugin's answer, a decoded document.
    Values(Vec<Value>),
    /// Values arriving from a reader thread while the child that produces them still runs.
    Stream {
        receiver: tokio::sync::mpsc::Receiver<StreamEvent>,
        boundedness: ono_pipeline::Boundedness,
    },
}

impl Seed {
    fn is_some(&self) -> bool {
        !matches!(self, Self::None)
    }
}

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
fn run_streamed_segment(
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

fn process_error_flow(error: ono_process::Error) -> Flow {
    Flow::Failed(ErrorValue::new(error.code(), error.message().to_owned()))
}

/// What a remote adaptation came to.
enum RemoteRun {
    /// The remote adapted the invocation and its records were consumed; the status stands.
    Adapted(ExitStatus),
    /// The remote has no adapter for it, with the reason it gave.
    NotAdapted(String),
}

/// The invocation a stage would run, with its arguments expanded, program first; `None` for a
/// stage that is not a program.
fn remote_argv(session: &mut Session, stage: &Stage, source: &str) -> Eval<Option<Vec<String>>> {
    let StageHead::Command(name) = &stage.head else {
        return Ok(None);
    };
    if !matches!(
        Namespace::from_prefix(name.namespace.as_deref()),
        Some(Namespace::Any | Namespace::External)
    ) {
        return Ok(None);
    }
    let forced = ono_command::is_adapt(stage);
    let words: Vec<String> = crate::eval::stage_arguments(session, stage, source)?
        .into_iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    let mut argv = Vec::new();
    if forced {
        let Some(program) = words.first() else {
            return Ok(None);
        };
        argv.push(program.clone());
        argv.extend(words.into_iter().skip(1));
    } else {
        argv.push(name.name.clone());
        argv.extend(words);
    }
    Ok(Some(argv))
}

/// Runs an invocation through the remote's adapter (spec v0.3 §1.54): the records stream from
/// the agent into the following native segment — or the renderer — exactly as a locally
/// streamed adaptation would (ADR-0059), already marked with the host.
///
/// # Errors
///
/// The remote's refusal under a structured demand, as the structured error it sent.
fn run_remote_adapted(
    session: &mut Session,
    list: &StageList,
    following: &[usize],
    source: &str,
    argv: &[String],
    demand: &OutputDemand,
    last: bool,
) -> Eval<RemoteRun> {
    let registry = registry().map_err(Flow::Failed)?;
    let demand_name = match demand {
        OutputDemand::Interactive => "interactive",
        _ => "structured",
    };
    // Ask first without running: the remote's own decision is what decides whether the
    // program runs there adapted or here raw.
    let Some(decision) = remote_decision(session, argv, demand) else {
        return Ok(RemoteRun::NotAdapted(
            "the remote agent cannot negotiate adapters".to_owned(),
        ));
    };
    if !decision.adapted {
        if matches!(demand, OutputDemand::Structured { .. }) {
            let host = session.link_host().unwrap_or_default();
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::AdapterNotAvailable,
                    format!("{host}: {}", decision.state),
                )
                .with_help(format!(
                    "`raw {}` runs the program as typed; a structured consumer needs an \
                     adapter on the host the frame stands on (spec v0.3 §1.54)",
                    argv.join(" ")
                ))
                .with_metadata("invocation", Value::string(&argv.join(" ")))
                .with_metadata("raw_fallback_safe", Value::Bool(true)),
            ));
        }
        return Ok(RemoteRun::NotAdapted(decision.state));
    }
    let Some(runtime_handle) = session
        .runtime()
        .map(tokio::runtime::Runtime::handle)
        .cloned()
    else {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the pipeline runtime",
        )));
    };
    let (sender, receiver) = tokio::sync::mpsc::channel::<StreamEvent>(256);
    let link = session.remote_link().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            "the link is gone",
        ))
    })?;
    let mut stream = link
        .link
        .adapt(argv, demand_name, false)
        .map_err(Flow::Failed)?;
    let host: std::sync::Arc<str> = std::sync::Arc::from(link.link.host());
    runtime_handle.spawn(async move {
        while let Some(message) = stream.recv().await {
            let event = match message {
                ono_protocol::RemoteMessage::Value(value) => {
                    StreamEvent::Value(ono_remote::retag_value(value, &host))
                }
                ono_protocol::RemoteMessage::Failure(error) => StreamEvent::Failure(error),
                ono_protocol::RemoteMessage::Event(_) => continue,
            };
            if sender.send(event).await.is_err() {
                break;
            }
        }
    });
    run_native_segment(
        session,
        registry,
        list,
        following,
        source,
        None,
        Seed::Stream {
            receiver,
            boundedness: ono_pipeline::Boundedness::Bounded,
        },
        false,
        last,
    )?;
    let host = session.link_host().unwrap_or_default();
    session.note_adaptation(format!("{} on {host}", decision.state), argv.join(" "));
    Ok(RemoteRun::Adapted(ExitStatus::SUCCESS))
}

/// What the last stage of an external segment is asked to produce (ADR-0052, ADR-0057 point 2).
///
/// `None` when nothing downstream could take structure: the stage then runs as it always has,
/// without even asking the registry.
fn external_demand(
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
fn negotiate_stage(
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

/// Decodes what an adapted child wrote, with the provenance of spec v0.3 §1.8.
fn decode_adapted(
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

/// The diagnostic of spec v0.3 §1.57 for a decode failure at the terminal.
fn report_fallback(plan: &AdapterPlan, error: &ErrorValue) {
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

/// The bytes a seed of values hands a child process: text and bytes pass, objects are refused.
fn seed_bytes(values: Vec<Value>) -> Result<Vec<u8>, Flow> {
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

/// Runs one run of native stages, answering with the bytes a following child process would read.
#[expect(
    clippy::too_many_arguments,
    reason = "one call site, and the arguments are the pipeline's actual moving parts"
)]
fn run_native_segment(
    session: &mut Session,
    registry: &'static CommandRegistry,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    seed: Seed,
    first: bool,
    last: bool,
) -> Eval<Option<Vec<u8>>> {
    let table = implementations().map_err(Flow::Failed)?;

    // Everything is bound before anything runs. A pipeline that cannot be built runs no part of
    // itself, so a typo in the third stage never leaves the first two half-done.
    let mut bound: Vec<(&'static CommandContract, BoundArguments)> = Vec::new();
    let mut structured = input.is_none() || seed.is_some();
    for index in indices {
        let stage = &list.stages[*index];
        let contract = native_contract(session, registry, stage, structured).ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!("`{}` is not a native command here", stage.span),
            ))
        })?;
        let resolved = registry
            .resolve(head_name(stage), &stage.arguments)
            .map_err(Flow::Failed)?;
        let arguments = contract.bind(resolved.arguments).map_err(Flow::Failed)?;
        structured = !produces_bytes(contract);
        bound.push((contract, arguments));
    }

    // A head stage that needs bytes reads the shell's own standard input, exactly as a child
    // process would have: spec §12.4's example is `curl … | ono -c 'from json | …'`, and the
    // bytes arrive on the shell's stdin, not from a stage inside the pipeline. A terminal is
    // never read implicitly — an interactive `from json` waiting silently for EOF would look
    // like a hang, and the "nothing was piped into it" error says what to do instead.
    let mut input = input;
    if first
        && input.is_none()
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
        return Ok(None);
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
    let (runtime, providers) = session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the pipeline runtime",
        ))
    })?;

    // Ctrl-C is delivered to the shell itself while a native pipeline runs — there is no child
    // for the kernel to interrupt — so the pipeline future races the interrupt note and loses
    // to it (spec §18.5). Dropping the futures drops every stream receiver, which closes the
    // bounded channels and stops every producer at its next send.
    let _ = ono_process::take_interrupt();
    let interrupted = async {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(40));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if ono_process::take_interrupt() {
                return;
            }
        }
    };

    let pipeline = async {
        let mut stream: Option<ValueStream> = match seed {
            Seed::Values(values) => Some(ValueStream::from_values(values)),
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
                Ok(Outcome::Values(values)) => stream = Some(values),
                Ok(Outcome::Actions(outcomes)) => {
                    // Spec §11.5: one record per target, so `97 succeeded, 3 failed` stays two
                    // readable numbers rather than one ambiguous status.
                    let elapsed = ono_value::Duration::from_nanoseconds(
                        i128::try_from(started.elapsed().as_nanos()).unwrap_or(i128::MAX),
                    );
                    stream = Some(ValueStream::from_values(
                        outcomes
                            .into_iter()
                            .map(|outcome| outcome.into_record(elapsed).into_value()),
                    ));
                }
                Err(error) => return Err(error),
            }
        }

        let mut values = Vec::new();
        let mut failures = Vec::new();
        if let Some(mut stream) = stream {
            if last && !stream.boundedness().is_bounded() && stage_has_no_redirection {
                // A live stream at a terminal renders in place (spec §18.3); anywhere else the
                // representation must be chosen, because an endless unserialised stream into a
                // pipe or file is a table that never learns its widths.
                if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
                    let (width, height) = live_geometry();
                    failures.extend(crate::live::show(stream, width, height).await);
                    return Ok((Vec::new(), failures));
                }
                return Err(ErrorValue::new(
                    ErrorCode::StreamUnboundedOperation,
                    "a live stream needs a representation when nobody is watching it",
                )
                .with_help(
                    "pipe it through a serializer — `watch process | to json` — or bound it \
                     with `take` (spec §18.3)",
                ));
            }
            while let Some(event) = stream.recv().await {
                match event {
                    StreamEvent::Value(value) => values.push(value),
                    StreamEvent::Failure(error) => failures.push(error),
                }
            }
        }
        Ok((values, failures))
    };

    let collected = runtime.block_on(async {
        tokio::select! {
            outcome = pipeline => outcome,
            () = interrupted => Err(ErrorValue::new(
                ErrorCode::StreamCancelled,
                "interrupted",
            )),
        }
    });

    let (values, failures) = collected.map_err(|error| {
        if error.code() == ErrorCode::StreamCancelled {
            // 128 + SIGINT, the status every shell reports for an interrupted foreground job
            // (ADR-0008); the message would only repeat what the ^C on the terminal already
            // says.
            Flow::FailedWith(error, ExitStatus::from_signal(2))
        } else {
            Flow::Failed(error)
        }
    })?;

    // Spec §16.5: what succeeded and what failed are both reported, and neither is collapsed into
    // the other. A process that exits between being listed and being read costs one object, not
    // the answer — so the failures are shown and the values still arrive. Only when nothing
    // arrived at all is there no answer, and that is the case the status reports (ADR-0028).
    if !failures.is_empty() {
        let reporter = crate::report::Reporter::new(ono_render::Presentation::choose(
            std::io::IsTerminal::is_terminal(&std::io::stderr()),
            &[],
        ));
        for failure in &failures {
            reporter.error(failure);
        }
        if values.is_empty() {
            let first = failures.into_iter().next().unwrap_or_else(|| {
                ErrorValue::new(
                    ErrorCode::ProviderUnavailable,
                    "the command produced nothing",
                )
            });
            return Err(Flow::Failed(first));
        }
    }

    if !last {
        return Ok(Some(bytes_of(&values)));
    }

    // `view` consumes the terminal instead of printing (ADR-0050): the browse loop owns the
    // rows from here, and leaving it retains them and the selection.
    if final_contract.is_some_and(|contract| contract.id() == "ono.data.view") {
        let name = bound
            .last()
            .and_then(|(_, arguments)| arguments.selector("name"))
            .and_then(|value| value.as_str().ok())
            .unwrap_or("table")
            .to_owned();
        return match crate::view::run(session, &name, values) {
            Ok(_) => Ok(None),
            Err(flow) => Err(flow),
        };
    }

    let stage = &list.stages[*indices.last().unwrap_or(&0)];
    write_result(
        session,
        stage,
        &values,
        final_contract.is_some_and(produces_bytes),
        source,
    )?;
    Ok(None)
}

/// What the expressions of a native segment can see: the session's `$variables`, and the values
/// of every parenthesised pipeline written in an argument, run here and now (ADR-0072 §4).
///
/// `ono-command` evaluates expressions but never runs pipelines (ADR-0005), so
/// `join (get socket) --on pid` needs the evaluator to run `(get socket)` first and hand the
/// records in. They are keyed by the parentheses' span, which is unique within one source.
fn stage_scope(
    session: &mut Session,
    bound: &[(&'static CommandContract, BoundArguments)],
    source: &str,
) -> Eval<Scope> {
    let mut scope = Scope::new();
    for (name, value) in session.bindings() {
        scope = scope.with_variable(&name, value);
    }
    for (_, arguments) in bound {
        for (_, binding) in arguments.selectors().iter().chain(arguments.options()) {
            for expression in binding.expressions() {
                for nested in ono_command::nested_pipelines(expression) {
                    let Some(pipeline) = nested.pipeline() else {
                        continue;
                    };
                    let values = capture_pipeline(session, pipeline, source)?;
                    scope = scope.with_pipeline_result(nested.span, Value::list(values));
                }
            }
        }
    }
    Ok(scope)
}

/// Runs `pipeline` for its values rather than for the screen.
///
/// The pipeline runs through the ordinary evaluator — checked, planned, resolved as always —
/// with its final values diverted from the sink. A pipeline that ends in a program rather than
/// a native stage writes bytes, and bytes are not captured here: capturing a program's output
/// as a value is the language's `(…)` substitution, which is a separate increment.
pub(crate) fn capture_pipeline(
    session: &mut Session,
    pipeline: &ono_parser::Pipeline,
    source: &str,
) -> Eval<Vec<Value>> {
    session.begin_capture();
    let outcome = crate::eval::run_pipeline(session, pipeline, source);
    let values = session.end_capture();
    outcome?;
    Ok(values)
}

/// The head word of a stage, or the empty string for a stage that has none.
fn head_name(stage: &Stage) -> &str {
    match &stage.head {
        StageHead::Command(name) => &name.name,
        _ => "",
    }
}

/// The bytes a serialised stream carries into a child process.
fn bytes_of(values: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in values {
        match value {
            Value::Bytes(raw) => bytes.extend_from_slice(raw),
            Value::String(text) => bytes.extend_from_slice(text.as_bytes()),
            other => bytes.extend_from_slice(other.to_string().as_bytes()),
        }
        if !bytes.ends_with(b"\n") {
            bytes.push(b'\n');
        }
    }
    bytes
}

/// Writes the last segment's result where the stage's redirections say it goes.
fn write_result(
    session: &mut Session,
    stage: &Stage,
    values: &[Value],
    serialised: bool,
    source: &str,
) -> Eval<()> {
    // A pipeline run for its value hands the values on instead of showing them (ADR-0072 §4);
    // it is neither rendered nor retained, because nothing was shown.
    if session.capture(values) {
        return Ok(());
    }
    // What is about to be shown is what `@-1` and `@N` reuse (spec §20.2). Serialised output is
    // not retained: its values are one rendered document, and reusing the objects it was made
    // from is what the retention of the *previous* result is for.
    if !serialised {
        session.retain_result(values.to_vec());
    }
    let destination = crate::eval::output_destination(session, stage, source)?;
    match destination {
        Some(mut file) => {
            let bytes = if serialised {
                bytes_of(values)
            } else {
                rendered_bytes(values)
            };
            file.write_all(&bytes).map_err(write_failed)?;
            file.flush().map_err(write_failed)
        }
        None if serialised => {
            let mut out = std::io::stdout().lock();
            out.write_all(&bytes_of(values)).map_err(write_failed)?;
            out.flush().map_err(write_failed)
        }
        None => {
            let environment: Vec<(String, String)> = session
                .env()
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
                .collect();
            let borrowed: Vec<(&str, &str)> = environment
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            Sink::for_stdout(&borrowed).write(values);
            Ok(())
        }
    }
}

/// The rendered form, laid out at the fixed width a file gets (spec §4.6).
fn rendered_bytes(values: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for line in Sink::for_file().render(values) {
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
    }
    bytes
}

/// Reports a failed write on the closed taxonomy of spec §43.
///
/// The taxonomy has no generic I/O code, so anything the specific codes do not describe is
/// reported the way `ono-process` reports it: the operating system refused the operation, with
/// the real reason in the message.
fn write_failed(error: std::io::Error) -> Flow {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::IoNotFound,
        std::io::ErrorKind::AlreadyExists => ErrorCode::IoAlreadyExists,
        std::io::ErrorKind::NotADirectory => ErrorCode::IoNotDirectory,
        _ => ErrorCode::IoPermissionDenied,
    };
    Flow::Failed(ErrorValue::new(
        code,
        format!("the output could not be written: {error}"),
    ))
}

/// The terminal's size for a live view, with the fallbacks the sink already uses.
pub(crate) fn live_geometry() -> (usize, usize) {
    let (width, height) = ono_editor::terminal_size().unwrap_or((0, 0));
    (width.max(20), if height == 0 { 24 } else { height })
}
