//! What a drained segment becomes: reported, counted, written.
//!
//! Spec §16.5 governs the shape: what succeeded and what failed are both reported and neither is
//! collapsed into the other, and the status says which of the two the run is.

use std::io::Write;

use ono_command::{BoundArguments, CommandContract};
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::{Stage, StageList};
use ono_pipeline::ValueStream;
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::Session;
use crate::sink::Sink;

use super::segment::{admits_bytes, produces_bytes, wrote_text};

/// Reports what a segment could not produce, beside what it did.
///
/// Spec §16.5: what succeeded and what failed are both reported, and neither is collapsed into
/// the other. A process that exits between being listed and being read costs one object, not the
/// answer — so the failures are shown and the values still arrive. Only when nothing arrived at
/// all is there no answer, and that is the case the status reports (ADR-0028).
///
/// # Errors
///
/// The first failure, when nothing survived: it is then the answer rather than a note beside one,
/// and it travels as the error the run failed with (ADR-0221).
pub(super) fn report_failures(values: &[Value], failures: Vec<ErrorValue>) -> Eval<()> {
    if failures.is_empty() {
        return Ok(());
    }
    let reporter = crate::report::Reporter::new(ono_render::Presentation::choose(
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
        &[],
    ));
    if values.is_empty() {
        // Nothing survived, so the failure is the answer rather than a note beside one. It
        // travels as the error the run failed with — reported once, by the caller that
        // reports every failure — and the rest are reported here (ADR-0221).
        let mut remaining = failures.into_iter();
        let first = remaining.next().unwrap_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                "the command produced nothing",
            )
        });
        for failure in remaining {
            reporter.error(&failure);
        }
        return Err(Flow::Failed(first));
    }
    for failure in &failures {
        reporter.error(failure);
    }
    Ok(())
}

/// Where a finished segment's values go.
pub(super) struct Delivery<'a> {
    pub(super) list: &'a StageList,
    pub(super) indices: &'a [usize],
    pub(super) source: &'a str,
    pub(super) bound: &'a [(&'static CommandContract, BoundArguments)],
    pub(super) last: bool,
    pub(super) block_shows_itself: bool,
}

/// Hands a drained segment's values on: to the next segment as bytes, to the browser, to the
/// screen, or nowhere at all because a trailing block has already shown them.
///
/// # Errors
///
/// The structured error of a result that could not be written or a browser that could not run.
pub(super) fn deliver_segment(
    session: &mut Session,
    delivery: &Delivery<'_>,
    values: Vec<Value>,
    status: ExitStatus,
) -> Eval<(Option<Vec<u8>>, ExitStatus)> {
    if !delivery.last {
        return Ok((Some(bytes_of(&values)), status));
    }
    let final_contract = delivery.bound.last().map(|(contract, _)| *contract);

    // `view` consumes the terminal instead of printing (ADR-0050): the browse loop owns the
    // rows from here, and leaving it retains them and the selection.
    if final_contract.is_some_and(|contract| contract.id() == "ono.data.view") {
        let name = delivery
            .bound
            .last()
            .and_then(|(_, arguments)| arguments.selector("name"))
            .and_then(|value| value.as_str().ok())
            .unwrap_or("table")
            .to_owned();
        return match crate::view::run(session, &name, values) {
            Ok(_) => Ok((None, status)),
            Err(flow) => Err(flow),
        };
    }

    // A trailing `each { … }` with nothing after it has already shown whatever its statements
    // produced, in the caller's output context (ADR-0070 point 3). It has no result of its own,
    // and writing an empty one would retain a result the user never saw.
    if delivery.block_shows_itself {
        return Ok((None, status));
    }

    let stage = &delivery.list.stages[*delivery.indices.last().unwrap_or(&0)];
    let serialised = final_contract.is_some_and(|contract| {
        produces_bytes(contract) || (admits_bytes(contract) && wrote_text(&values))
    });
    write_result(session, stage, &values, serialised, delivery.source)?;
    Ok((None, status))
}

/// The ActionResult rows of one mutation stage, as the schema writes them: `operation` is the
/// command id that ran (`ono.process.kill`), not the verb the provider was asked in
/// (`action-result.v1.yaml`, ADR-0068 §2).
pub(super) fn action_records(
    contract: &CommandContract,
    outcomes: Vec<ono_provider_api::ActionOutcome>,
    started: std::time::Instant,
) -> ValueStream {
    let elapsed = ono_value::Duration::from_nanoseconds(
        i128::try_from(started.elapsed().as_nanos()).unwrap_or(i128::MAX),
    );
    ValueStream::from_values(outcomes.into_iter().map(move |outcome| {
        outcome
            .into_record(elapsed)
            .with_operation(contract.id())
            .into_value()
    }))
}

/// The bytes a serialised stream carries into a child process.
pub(super) fn bytes_of(values: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in values {
        match value {
            // Raw bytes are written byte for byte. A document `to json` or `to text` wrote is
            // line-oriented and ends with a newline where it has none; `to bytes` is the escape
            // hatch of spec §12.2, and a byte the shell added would be a byte the file did not
            // have (ADR-0223).
            Value::Bytes(raw) => {
                bytes.extend_from_slice(raw);
                continue;
            }
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
pub(super) fn write_result(
    session: &mut Session,
    stage: &Stage,
    values: &[Value],
    serialised: bool,
    source: &str,
) -> Eval<()> {
    let destination = crate::eval::output_destination(session, stage, source)?;
    // A pipeline run for its value hands on what it would have shown instead of showing it
    // (spec §19.2, ADR-0069; ADR-0072 §4): the values themselves, or the one document a
    // serializer made of them. Nothing is rendered and nothing is retained for `@-1`, because
    // nothing was shown. A redirection still means the file.
    if destination.is_none() && session.capturing() {
        if serialised {
            session.capture(&[crate::eval::captured_text(&bytes_of(values))])?;
        } else {
            session.capture(values)?;
        }
        return Ok(());
    }
    // What is about to be shown is what `@-1` and `@N` reuse (spec §20.2). Serialised output is
    // not retained: its values are one rendered document, and reusing the objects it was made
    // from is what the retention of the *previous* result is for.
    if !serialised {
        crate::report::retention_notice(session.retain(values));
    }
    match destination {
        Some(mut file) => {
            let bytes = if serialised {
                bytes_of(values)
            } else {
                rendered_bytes(values, table_row_limit(session))
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
            let mut sink = Sink::for_stdout(&borrowed).with_theme(session.theme());
            if let Some(limit) = table_row_limit(session) {
                sink = sink.with_max_rows(limit);
            }
            sink.write(values);
            Ok(())
        }
    }
}

/// The row limit a rendered table honours: `render.table.max_rows`, where 0 means every row
/// (ADR-0094 §6).
pub(super) fn table_row_limit(session: &Session) -> Option<usize> {
    session
        .settings()
        .int("render.table.max_rows")
        .and_then(|rows| usize::try_from(rows).ok())
        .filter(|rows| *rows > 0)
}

/// The rendered form, laid out at the fixed width a file gets (spec §4.6).
pub(super) fn rendered_bytes(values: &[Value], max_rows: Option<usize>) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut sink = Sink::for_file();
    if let Some(limit) = max_rows {
        sink = sink.with_max_rows(limit);
    }
    for line in sink.render(values) {
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
    }
    bytes
}

/// Says what the pipeline dropped, when it dropped anything.
///
/// A predicate that could not be decided excludes a row (ADR-0014, spec §10.5) and an aggregate
/// skips a null rather than counting it as a zero (spec §35.3). Both are right, and both make a
/// row count smaller than a user expects for a reason no output shows. One line on stderr, in
/// the terms the language uses, is that reason (ADR-0261).
pub(super) fn report_counts(counted: &ono_pipeline::Diagnostics) {
    let excluded = counted.excluded_unknown();
    let skipped = counted.skipped_null();
    if excluded == 0 && skipped == 0 {
        return;
    }
    let reporter = crate::report::Reporter::new(ono_render::Presentation::choose(
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
        &[],
    ));
    if excluded > 0 {
        reporter.note(&format!(
            "{excluded} {} excluded because the condition could not be decided on {} \
             (spec §10.5)",
            plural(excluded, "value", "values"),
            plural(excluded, "it", "them"),
        ));
    }
    if skipped > 0 {
        reporter.note(&format!(
            "{skipped} unknown {} skipped, so the result is over the rest (spec §35.3)",
            plural(skipped, "value was", "values were"),
        ));
    }
}

/// `one` for a count of one, `many` for anything else.
pub(super) fn plural(count: u64, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

/// Reports a failed write on the closed taxonomy of spec §43.
///
/// The taxonomy has no generic I/O code, so anything the specific codes do not describe is
/// reported the way `ono-process` reports it: the operating system refused the operation, with
/// the real reason in the message.
pub(super) fn write_failed(error: std::io::Error) -> Flow {
    // A reader that closed the pipe is not a failure to report. `… | head` is how a Unix user
    // asks for the first page, and every other shell stops there in silence; a diagnostic on the
    // terminal would be the shell complaining about being used correctly. The process stops with
    // the status a program killed by `SIGPIPE` reports (ADR-0220).
    if error.kind() == std::io::ErrorKind::BrokenPipe {
        return Flow::Exit(ExitStatus::from_signal(13));
    }
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
