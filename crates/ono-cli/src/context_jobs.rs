//! Foregrounding a native job: its rendering comes back, and Ctrl-C ends it (ADR-0024).

use ono_core::ExitStatus;
use ono_render::{Layout, Presentation, Renderer, Theme, View};
use ono_value::Value;

use crate::eval::{Eval, Flow};
use crate::session::Session;

/// Stops native job `number` without reattaching it (`kill %N`, ADR-0071 §4).
///
/// The job's evaluator is cancelled and the process group it waits on, if any, is signalled —
/// the same cancellation Ctrl-C performs on a foreground run. The job leaves the table once it
/// has ended; a job whose evaluator is still waiting on a child that has not stopped stays
/// listed, so the thread and the process are never left running untracked (review C2).
///
/// # Errors
///
/// A structured error when no such job exists.
pub fn stop(session: &mut Session, number: u32) -> Eval<()> {
    let Some(job) = session.native_job(number) else {
        return Err(no_such_job(number));
    };
    job.handle.stop(ono_process::Signal::TERM);
    // The job may be waiting on a child; it is given a moment to reap it, so `kill %1; jobs` is
    // not a race (v0.4.1 §28.4, ADR-0952).
    job.handle
        .wait_until_finished(std::time::Duration::from_secs(2));
    if job.handle.is_finished() {
        session.take_native_job(number);
    } else {
        still_stopping(number);
    }
    Ok(())
}

/// Brings native job `number` to the foreground and collects it.
///
/// Foregrounding changes the job's attachment, not its result (issue #301, ADR-0958). `fg` waits
/// for the job, as it waits for a program: the shell holds the terminal meanwhile, and a job
/// whose line ends in a live stream has its table repainted in place while it runs (ADR-0024).
/// Ctrl-C stops the job and its children. When the job has ended, what it produced is shown —
/// once, as the foreground would have shown it — its failures are reported (spec §43), and `fg`
/// returns the status the line ended with (ADR-0008), or `130` when Ctrl-C ended it. Without a
/// terminal nothing changes but the repainting: the job is waited for, never cut short. The job
/// leaves the table, unless Ctrl-C could not end it, and then it stays listed until it has ended
/// (review C2).
///
/// # Errors
///
/// A structured error when no such job exists.
pub fn attach(session: &mut Session, number: u32) -> Eval<ExitStatus> {
    let Some(job) = session.native_job(number) else {
        return Err(no_such_job(number));
    };

    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let renderer = Renderer::new();
    let theme = Theme::clone(session.theme());
    let (width, height) = crate::eval::native::live_geometry();
    let layout = Layout::new(width).max_rows(height.saturating_sub(3).max(4));
    let mut painted = 0usize;
    let mut shown: Option<Vec<Value>> = None;
    let mut next_frame = std::time::Instant::now();
    let mut interrupted = false;

    let _ = ono_process::take_interrupt();
    while !job.handle.is_finished() {
        if ono_process::take_interrupt() {
            interrupted = true;
            job.handle.stop(ono_process::Signal::INT);
            job.handle
                .wait_until_finished(std::time::Duration::from_secs(2));
            break;
        }
        // A live table is a presentation over the job's rows (ADR-0024): repainted at most once
        // a frame, and only where a terminal shows it.
        if terminal && std::time::Instant::now() >= next_frame {
            let rows = rows_of(&job.model);
            if !rows.is_empty() && shown.as_ref() != Some(&rows) {
                painted = repaint(&layout, &renderer, &theme, &rows, painted);
                shown = Some(rows);
            }
            next_frame = std::time::Instant::now() + std::time::Duration::from_millis(250);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if interrupted && !job.handle.is_finished() {
        still_stopping(number);
        return Ok(ExitStatus::from_signal(2));
    }
    let Some(job) = session.take_native_job(number) else {
        return Err(no_such_job(number));
    };

    // The table's last state, where it was not already on the screen.
    let rows = rows_of(&job.model);
    if terminal && !rows.is_empty() && shown.as_ref() != Some(&rows) {
        repaint(&layout, &renderer, &theme, &rows, painted);
        shown = Some(rows.clone());
    }
    let mut values = std::mem::take(
        &mut *job
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    if shown.is_none() {
        values.extend(rows);
    }
    if !values.is_empty() {
        show(session, &values);
    }
    // Spec §43: what a job could not do is reported as the foreground would report it — code,
    // name and help — when the job is collected (ADR-0952).
    let reporter = crate::report::Reporter::new(Presentation::choose(
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
        &[],
    ));
    for failure in job
        .failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
    {
        reporter.error(failure);
    }
    if interrupted {
        return Ok(ExitStatus::from_signal(2));
    }
    // A job that ended has a status; one whose evaluator could not record it did not finish
    // its line, which is a failure and never a success (ADR-0958).
    Ok(job.handle.status().unwrap_or(ExitStatus::FAILURE))
}

/// The job's table rows, in identity order.
fn rows_of(model: &crate::session::LiveModel) -> Vec<Value> {
    model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .cloned()
        .collect()
}

/// Repaints the job's table over its previous frame, answering the new line count.
fn repaint(
    layout: &Layout,
    renderer: &Renderer,
    theme: &Theme,
    rows: &[Value],
    painted: usize,
) -> usize {
    use std::io::Write as _;
    let lines =
        layout.render_view_styled(renderer, rows, View::Table, theme, Presentation::Terminal);
    let mut out = std::io::stdout().lock();
    if painted > 0 {
        let _ = write!(out, "\x1b[{painted}A\x1b[0J");
    }
    for line in &lines {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
    lines.len()
}

/// Shows what a collected job produced, as the foreground shows a result: the bytes a
/// serializer or a program wrote are written as they were, and values are rendered — in the
/// order the job produced them — and retained for `@-1` (spec §20.2).
fn show(session: &mut Session, values: &[Value]) {
    let objects: Vec<Value> = values
        .iter()
        .filter(|value| !matches!(value, Value::Bytes(_)))
        .cloned()
        .collect();
    if !objects.is_empty() {
        crate::report::retention_notice(session.retain(&objects));
    }
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
    let sink = crate::sink::Sink::for_stdout(&borrowed).with_theme(session.theme());
    let mut pending: Vec<Value> = Vec::new();
    for value in values {
        if let Value::Bytes(bytes) = value {
            if !pending.is_empty() {
                sink.write(&std::mem::take(&mut pending));
            }
            use std::io::Write as _;
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(bytes).and_then(|()| out.flush());
        } else {
            pending.push(value.clone());
        }
    }
    if !pending.is_empty() {
        sink.write(&pending);
    }
}

fn no_such_job(number: u32) -> Flow {
    Flow::Failed(ono_value::ErrorValue::new(
        ono_core::ErrorCode::ResolveTargetNotFound,
        format!("no job %{number}"),
    ))
}

/// Says that a signalled job has not ended yet, and stays listed until it has.
fn still_stopping(number: u32) {
    ono_core::diagnostic!(
        "ono: job %{number} has not stopped yet — a program it runs did not end on the signal; \
         it stays listed until it has (`jobs`, `get job`)"
    );
}
