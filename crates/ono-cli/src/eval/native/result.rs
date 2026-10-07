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
pub(super) fn report_failures(produced: bool, failures: Vec<ErrorValue>) -> Eval<()> {
    if failures.is_empty() {
        return Ok(());
    }
    let reporter = crate::report::Reporter::new(ono_render::Presentation::choose(
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
        &[],
    ));
    if !produced {
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
        // A stage that refused because the line was interrupted ends the way an interrupted
        // foreground job ends, whoever noticed the interrupt. A long ledger scan is asked every
        // 256 rows whether anybody still wants the answer and refuses with `stream.cancelled`
        // when nobody does (§32.6, ADR-0782); reporting that as an ordinary failure would give
        // Ctrl-C the status of a command that went wrong rather than of one that was stopped.
        return Err(super::segment::interrupted_flow(first));
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
    let written = write_result(session, stage, &values, serialised, delivery.source)?;
    Ok((
        None,
        if written.is_success() {
            status
        } else {
            written
        },
    ))
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
        push_bytes(&mut bytes, value);
    }
    bytes
}

/// Appends the bytes one serialised value carries.
fn push_bytes(bytes: &mut Vec<u8>, value: &Value) {
    match value {
        // Raw bytes are written byte for byte. A document `to json` or `to text` wrote is
        // line-oriented and ends with a newline where it has none; `to bytes` is the escape
        // hatch of spec §12.2, and a byte the shell added would be a byte the file did not
        // have (ADR-0223).
        Value::Bytes(raw) => {
            bytes.extend_from_slice(raw);
            return;
        }
        Value::String(text) => bytes.extend_from_slice(text.as_bytes()),
        other => bytes.extend_from_slice(other.to_string().as_bytes()),
    }
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
}

/// Whether a segment's output is lines a serializer wrote value by value, so it is written as it
/// arrives rather than when the stream ends (ADR-0954).
///
/// That is a segment whose last serializer is `to jsonl` and whose stages after it only pass
/// lines on as they come — `take 2`, `where …` — rather than wait for the end.
pub(super) fn streams_bytes(bound: &[(&'static CommandContract, BoundArguments)]) -> bool {
    let Some(serializer) = bound
        .iter()
        .rposition(|(contract, _)| produces_bytes(contract))
    else {
        return false;
    };
    let (contract, arguments) = &bound[serializer];
    contract.id() == "ono.data.to"
        && arguments
            .selector("format")
            .and_then(|format| format.as_str().ok())
            == Some("jsonl")
        && bound[serializer + 1..].iter().all(|(after, _)| {
            after.is_streaming()
                && !after
                    .execution()
                    .is_some_and(ono_command::ExecutionClass::may_materialize)
        })
}

/// Where a streaming serializer's lines go as they are produced, and whether whoever reads them
/// is still there (ADR-0954).
pub(super) struct StreamedOutput {
    writer: Box<dyn Write>,
    /// Set once the reader of a pipe has gone away, by a watcher that notices it without a write.
    reader_gone: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// The watcher, stopped — and its copy of the descriptor closed — when the output is done with.
    watching: Option<Watcher>,
    /// Whether the lines go to the shell's own standard output, whose reader leaving ends the
    /// shell — rather than to a file or a program, whose reader leaving ends the line (ADR-0220).
    own: bool,
}

impl StreamedOutput {
    /// The output a stage's lines go to: the file its redirection names, or the shell's stdout.
    pub(super) fn open(session: &mut Session, stage: &Stage, source: &str) -> Eval<Self> {
        if let Some(file) = crate::eval::output_destination(session, stage, source)? {
            // A named pipe's reader can leave while nothing is being written, as a pipe's can.
            let watched = std::os::fd::AsFd::as_fd(&file)
                .try_clone_to_owned()
                .ok()
                .filter(is_pipe)
                .map(watch_reader);
            let (reader_gone, watching) = watched.unzip();
            return Ok(Self {
                writer: Box::new(file),
                reader_gone,
                watching,
                own: false,
            });
        }
        let watched = std::os::fd::AsFd::as_fd(&std::io::stdout())
            .try_clone_to_owned()
            .ok()
            .filter(is_pipe)
            .map(watch_reader);
        let (reader_gone, watching) = watched.unzip();
        Ok(Self {
            writer: Box::new(std::io::stdout()),
            reader_gone,
            watching,
            own: true,
        })
    }

    /// The output that writes into a program's standard input, watching for the program leaving.
    pub(super) fn into_program(input: std::os::fd::OwnedFd) -> Self {
        let watched = input.try_clone().ok().map(watch_reader);
        let (reader_gone, watching) = watched.unzip();
        Self {
            writer: Box::new(std::fs::File::from(input)),
            reader_gone,
            watching,
            own: false,
        }
    }

    /// Whether the lines go to the shell's own standard output.
    pub(super) const fn is_the_shells_own(&self) -> bool {
        self.own
    }

    /// Writes one value's line and flushes it, so it is where its reader can see it now.
    ///
    /// # Errors
    ///
    /// The write's own error; a broken pipe is the reader having gone, which the caller decides
    /// the meaning of.
    pub(super) fn write(&mut self, value: &Value) -> std::io::Result<()> {
        let mut bytes = Vec::new();
        push_bytes(&mut bytes, value);
        self.writer.write_all(&bytes)?;
        self.writer.flush()
    }

    /// The flag a watcher sets when the reader of the pipe has gone, if one is watching.
    pub(super) fn reader_gone(&self) -> Option<&std::sync::Arc<std::sync::atomic::AtomicBool>> {
        self.reader_gone.as_ref()
    }
}

impl Drop for StreamedOutput {
    fn drop(&mut self) {
        // The watcher holds a copy of the write end: it is closed here, before the drop returns,
        // so the end of the lines reaches a fed program the moment the output is done with, not
        // on the watcher's next look (review C7d).
        drop(self.watching.take());
    }
}

/// A thread watching a pipe's write end for its reader leaving, and how to stop it at once.
struct Watcher {
    /// Closing this wakes the watcher.
    stop: Option<std::os::fd::OwnedFd>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Whether a descriptor is a pipe, whose reader can go away while nothing is being written.
fn is_pipe(descriptor: &std::os::fd::OwnedFd) -> bool {
    nix::sys::stat::fstat(descriptor).is_ok_and(|status| {
        nix::sys::stat::SFlag::from_bits_truncate(status.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFIFO)
    })
}

/// Watches the write end of a pipe for its reader going away.
///
/// A pipe's write end reports an error condition as soon as no reader is left, without anything
/// being written — so `… | to jsonl | head -1` ends when `head` leaves, not when the source next
/// produces a value it would have failed to write (v0.4.1 §28.3). The watcher holds its own copy
/// of the descriptor and ends when told to, or when it has seen the reader go.
fn watch_reader(
    descriptor: std::os::fd::OwnedFd,
) -> (std::sync::Arc<std::sync::atomic::AtomicBool>, Watcher) {
    let gone = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let report = std::sync::Arc::clone(&gone);
    let Ok((woken, stop)) = nix::unistd::pipe() else {
        return (
            gone,
            Watcher {
                stop: None,
                thread: None,
            },
        );
    };
    let thread = std::thread::Builder::new()
        .name("ono-reader-watch".to_owned())
        .spawn(move || {
            use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
            loop {
                let mut descriptors = [
                    PollFd::new(std::os::fd::AsFd::as_fd(&descriptor), PollFlags::empty()),
                    PollFd::new(std::os::fd::AsFd::as_fd(&woken), PollFlags::POLLIN),
                ];
                match poll(&mut descriptors, PollTimeout::NONE) {
                    Ok(_) => {}
                    Err(nix::errno::Errno::EINTR) => continue,
                    Err(_) => return,
                }
                if descriptors[0].revents().is_some_and(|events| {
                    events.intersects(PollFlags::POLLERR | PollFlags::POLLHUP)
                }) {
                    report.store(true, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
                if descriptors[1]
                    .revents()
                    .is_some_and(|events| !events.is_empty())
                {
                    return;
                }
            }
        })
        .ok();
    (
        gone,
        Watcher {
            stop: Some(stop),
            thread,
        },
    )
}

/// Writes the last segment's result where the stage's redirections say it goes, and answers the
/// status the writing gives the line: success, or `SIGPIPE`'s when the reader of the file it was
/// redirected to — a named pipe — left before it had everything (ADR-0220). Only the shell's own
/// output's reader leaving ends the shell.
pub(super) fn write_result(
    session: &mut Session,
    stage: &Stage,
    values: &[Value],
    serialised: bool,
    source: &str,
) -> Eval<ExitStatus> {
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
        return Ok(ExitStatus::SUCCESS);
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
            match file.write_all(&bytes).and_then(|()| file.flush()) {
                Ok(()) => Ok(ExitStatus::SUCCESS),
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                    Ok(ExitStatus::from_signal(13))
                }
                Err(error) => Err(write_failed(error)),
            }
        }
        None if serialised => {
            let mut out = std::io::stdout().lock();
            out.write_all(&bytes_of(values)).map_err(write_failed)?;
            out.flush().map_err(write_failed)?;
            Ok(ExitStatus::SUCCESS)
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
            Ok(ExitStatus::SUCCESS)
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
