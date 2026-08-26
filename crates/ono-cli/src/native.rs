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
pub fn claims(session: &Session, list: &StageList) -> bool {
    segments(session, list).is_some_and(|segments| {
        segments
            .iter()
            .any(|segment| matches!(segment, Segment::Native(_)))
    })
}

/// Splits `list` into runs of native and external stages.
///
/// Returns `None` when the registry itself cannot be read, which leaves the caller on the
/// external path it would have taken before native commands existed.
fn segments(session: &Session, list: &StageList) -> Option<Vec<Segment>> {
    let registry = registry().ok()?;
    let mut segments: Vec<Segment> = Vec::new();
    let mut structured = false;

    for (index, stage) in list.stages.iter().enumerate() {
        let native = native_contract(session, registry, stage, structured).is_some();
        structured = native
            && native_contract(session, registry, stage, structured)
                .is_some_and(|contract| !produces_bytes(contract));

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
pub fn check(pipeline: &ono_parser::Pipeline) -> Result<(), ErrorValue> {
    let Ok(registry) = registry() else {
        // An unreadable registry is reported where a native stage actually runs; the pre-flight
        // check is an optimisation of the failure path, not a second gate.
        return Ok(());
    };
    let schemas: Vec<_> = ono_value::builtin_schemas().schemas().cloned().collect();
    ono_command::check_pipeline(registry, &schemas, pipeline)
}

/// Runs a stage list that contains at least one native command.
///
/// # Errors
///
/// The structured error of whichever stage could not be resolved, bound, or run.
pub fn run(session: &mut Session, list: &StageList, source: &str) -> Eval<ExitStatus> {
    let registry = registry().map_err(Flow::Failed)?;
    let segments = segments(session, list).ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            "the command registry could not be read",
        ))
    })?;

    let mut carried: Option<Vec<u8>> = None;
    let mut status = ExitStatus::SUCCESS;

    for (position, segment) in segments.iter().enumerate() {
        let last = position + 1 == segments.len();
        match segment {
            Segment::External(indices) => {
                let (bytes, external_status) = crate::eval::run_external_segment(
                    session, list, indices, source, carried, last,
                )?;
                carried = bytes;
                status = external_status;
            }
            Segment::Native(indices) => {
                carried =
                    run_native_segment(session, registry, list, indices, source, carried, last)?;
                status = ExitStatus::SUCCESS;
            }
        }
    }
    Ok(status)
}

/// Runs one run of native stages, answering with the bytes a following child process would read.
fn run_native_segment(
    session: &mut Session,
    registry: &'static CommandRegistry,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    last: bool,
) -> Eval<Option<Vec<u8>>> {
    let table = implementations().map_err(Flow::Failed)?;

    // Everything is bound before anything runs. A pipeline that cannot be built runs no part of
    // itself, so a typo in the third stage never leaves the first two half-done.
    let mut bound: Vec<(&'static CommandContract, BoundArguments)> = Vec::new();
    let mut structured = input.is_none();
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

    let Some((final_contract, _)) = bound.last() else {
        return Ok(None);
    };
    let final_contract = *final_contract;

    // A structured stream cannot be handed to a child process. Spec §12.3 makes the boundary
    // explicit in both directions, and guessing a rendering the program would have to parse back
    // is exactly the text-shaped coupling the object pipeline exists to remove.
    if !last && !produces_bytes(final_contract) {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!(
                    "`{}` produces objects, and the next stage is a program that reads bytes",
                    final_contract.spelling()
                ),
            )
            .with_help("choose the representation: `… | to json | …` (spec §12.3)"),
        ));
    }

    let scope = std::sync::Arc::new(Scope::new());
    let (runtime, providers) = session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the pipeline runtime",
        ))
    })?;

    let collected = runtime.block_on(async {
        let mut stream: Option<ValueStream> =
            input.map(|bytes| ValueStream::from_values([Value::Bytes(bytes.into())]));

        for (contract, arguments) in &bound {
            let started = std::time::Instant::now();
            let mut invocation = Invocation::new(contract, arguments, providers)
                .with_scope(std::sync::Arc::clone(&scope));
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
            while let Some(event) = stream.recv().await {
                match event {
                    StreamEvent::Value(value) => values.push(value),
                    StreamEvent::Failure(error) => failures.push(error),
                }
            }
        }
        Ok((values, failures))
    });

    let (values, failures) = collected.map_err(Flow::Failed)?;

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

    let stage = &list.stages[*indices.last().unwrap_or(&0)];
    write_result(
        session,
        stage,
        &values,
        produces_bytes(final_contract),
        source,
    )?;
    Ok(None)
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
