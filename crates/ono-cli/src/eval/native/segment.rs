//! Which stages of a pipeline are this module's, and what a contract admits and produces.
//!
//! Spec §12.3's boundary is decided here and nowhere else: a stage that produces bytes ends the
//! object stream, a stage that admits them begins one, and a segment is a run of stages of one
//! kind. Every predicate reads a published contract, so `explain` can answer the same questions
//! without running anything (v0.4.1 §22.4).

use ono_adapter::OutputDemand;
use ono_command::{CommandContract, CommandRegistry};
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::{Block, Stage, StageHead, StageList};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::resolve::Namespace;
use crate::session::Session;

use super::drive::block_of;
use super::registry;
use super::remote::negotiate_literally;

/// One run of adjacent stages that belong on the same side of the byte boundary.
#[derive(Debug)]
pub(super) enum Segment {
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
pub(super) fn redirects_stdout(stage: &Stage) -> bool {
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
pub(super) fn adaptable_program(
    session: &Session,
    stage: &Stage,
) -> Option<(String, std::path::PathBuf)> {
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

/// Splits `list` into runs of native and external stages.
///
/// Returns `None` when the registry itself cannot be read, which leaves the caller on the
/// external path it would have taken before native commands existed.
pub(super) fn segments(
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
pub(super) fn native_contract(
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
    let first_word = stage
        .arguments
        .first()
        .and_then(ono_parser::Argument::as_word);
    if crate::resolve::builtin_for(&name.name, first_word).is_some() {
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
pub(super) fn binds_here(contract: &CommandContract, structured: bool) -> bool {
    structured || accepts_bytes(contract.input().text())
}

/// Whether a declared input type admits something other than a stream of objects.
pub(super) fn accepts_bytes(input: &str) -> bool {
    input.split('|').map(str::trim).any(|alternative| {
        matches!(alternative, "any" | "null" | "string" | "bytes" | "value")
            || alternative.starts_with("string")
            || alternative.starts_with("bytes")
    })
}

/// Whether a command's output *may* be text, among the alternatives it declares.
///
/// `look` answers with a `PlaceView` and, when `--json` asked for it, with the one document
/// §29.1 requires it to write without a terminal (v0.4 §6.1). Both are its output, so the
/// contract declares both, and what it actually produced decides how the result is written.
pub(super) fn admits_bytes(contract: &CommandContract) -> bool {
    contract
        .output()
        .text()
        .split('|')
        .map(str::trim)
        .any(|alternative| {
            matches!(alternative, "string" | "bytes")
                || alternative.starts_with("string")
                || alternative.starts_with("bytes")
        })
}

/// Whether these values are the text such a command wrote rather than the objects it returned.
pub(super) fn wrote_text(values: &[Value]) -> bool {
    !values.is_empty()
        && values
            .iter()
            .all(|value| matches!(value, Value::String(_) | Value::Bytes(_)))
}

/// Whether a command's output is bytes or text rather than objects.
pub(super) fn produces_bytes(contract: &CommandContract) -> bool {
    let output = contract.output().text();
    output.split('|').map(str::trim).all(|alternative| {
        matches!(alternative, "string" | "bytes")
            || alternative.starts_with("string")
            || alternative.starts_with("bytes")
    })
}

/// The one pipeline a function body is, when it is one pipeline and nothing else.
///
/// v0.4.1 §26.2's continuation applies to a body of this shape and no other: several statements,
/// a chained `&&`, or a backgrounded pipeline each mean the call has a result of its own that the
/// stages after it read, rather than a stream they can be attached to.
pub(crate) fn continuable_body(body: &Block) -> Option<&StageList> {
    let [ono_parser::Statement::Pipeline(pipeline)] = body.statements.as_slice() else {
        return None;
    };
    (pipeline.tail.is_empty() && !pipeline.background).then_some(&pipeline.head)
}

/// Whether every stage of `list` hands objects to the next one, so the whole of it can become a
/// stage of the caller's pipeline (v0.4.1 §26.2).
///
/// Decided from the contracts alone, so `explain` can answer the same question without running
/// anything (§22.4). A serializer ends the object stream, an external program is not this
/// module's to continue, a redirection sends the values somewhere else, and a `each { … }` block
/// belongs to the driver of the pipeline it was written in.
pub(crate) fn continuable_list(session: &Session, list: &StageList) -> bool {
    let Ok(registry) = registry() else {
        return false;
    };
    !list.stages.is_empty()
        && list.stages.iter().all(|stage| {
            stage.redirections.is_empty()
                && block_of(stage).is_none()
                && native_contract(session, registry, stage, true)
                    .is_some_and(|contract| !produces_bytes(contract) && !admits_bytes(contract))
        })
}

/// The head word of a stage, or the empty string for a stage that has none.
pub(super) fn head_name(stage: &Stage) -> &str {
    match &stage.head {
        StageHead::Command(name) => &name.name,
        _ => "",
    }
}

/// Refuses a spatial verb while `spatial.enabled` is false (spec v0.4 §47, §40).
///
/// §47: "Disabling `spatial.enabled` MUST leave the typed shell and ordinary commands
/// functional." Off switches off the verbs of §6 and nothing else, and they refuse by name
/// rather than disappearing — `try { look } catch e { $e.name }` reads `spatial.unsupported`,
/// which a script can branch on, where a missing command could only be guessed at.
pub(super) fn refuse_switched_off_spatial(
    session: &Session,
    contract: &'static CommandContract,
    stage: &Stage,
) -> Eval<()> {
    if contract.id().starts_with("ono.place.") && crate::spatial::disabled(session) {
        return Err(Flow::Failed(crate::spatial::switched_off(head_name(stage))));
    }
    Ok(())
}

/// The refusal for `each { … }` with no stream in front of it (spec §19.4).
pub(super) fn each_needs_a_stream() -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        "`each` needs a stream of values to run its block over, and none reaches it",
    )
    .with_help("put a producer in front of it: `get service | where … | each { … }`")
}

/// The flow a pipeline error becomes, with an interrupted run reported as every shell reports one.
pub(super) fn interrupted_flow(error: ErrorValue) -> Flow {
    if error.code() == ErrorCode::StreamCancelled {
        // 128 + SIGINT, the status every shell reports for an interrupted foreground job
        // (ADR-0008); the message would only repeat what the ^C on the terminal already says.
        Flow::FailedWith(error, ExitStatus::from_signal(2))
    } else {
        Flow::Failed(error)
    }
}

pub(super) fn process_error_flow(error: ono_process::Error) -> Flow {
    Flow::Failed(ErrorValue::new(error.code(), error.message().to_owned()))
}

/// The stage that would receive this external run's bytes and cannot use them.
///
/// A native command declares what reaches it. One that admits bytes — `from`, `to`, `format` —
/// is the user decoding the program's output themselves and is handed the bytes. One declared
/// over a stream of objects has nothing to work with, and that is knowable from the contract
/// alone, before the program runs.
pub(super) fn consumer_needing_objects(
    session: &Session,
    registry: &'static CommandRegistry,
    list: &StageList,
    next: Option<&Segment>,
) -> Option<&'static CommandContract> {
    let Some(Segment::Native(following)) = next else {
        return None;
    };
    let consumer = &list.stages[*following.first()?];
    let contract = native_contract(session, registry, consumer, true)?;
    (!accepts_bytes(contract.input().text())).then_some(contract)
}

/// The refusal for bytes that cannot become the objects the next stage is declared over.
///
/// The invocation is quoted as the user wrote it, so every route out of the refusal is a line
/// they can run: the program raw, the output decoded explicitly, or the adapters that exist.
pub(super) fn unstructured_refusal(
    list: &StageList,
    indices: &[usize],
    source: &str,
    consumer: &'static CommandContract,
) -> ErrorValue {
    let stage = &list.stages[*indices.last().unwrap_or(&0)];
    let invocation = source
        .get(stage.span.start() as usize..stage.span.end() as usize)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let program = head_name(stage).to_owned();
    let spelling = consumer.spelling();
    ErrorValue::new(
        ErrorCode::AdapterRequiredForStructuredPipeline,
        format!(
            "`{spelling}` is defined over objects, and no adapter can give `{invocation}` \
             structured output"
        ),
    )
    .with_help(format!(
        "`raw {invocation}` runs the program as typed; `{invocation} | from <format>` decodes \
         its output yourself; `get command {program}` lists what adapts it"
    ))
    .with_metadata("invocation", Value::string(&invocation))
    .with_metadata("consumer", Value::string(&spelling))
    .with_metadata("raw_fallback_safe", Value::Bool(true))
    .with_metadata("recovery", Value::string(&format!("raw {invocation}")))
}
