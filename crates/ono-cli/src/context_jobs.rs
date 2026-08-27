//! Foregrounding a native job: its rendering comes back, and Ctrl-C ends it (ADR-0024).

use ono_core::ExitStatus;
use ono_render::{Layout, Presentation, Renderer, Theme, View};
use ono_value::Value;

use crate::eval::{Eval, Flow};
use crate::session::Session;

/// Stops native job `number` without reattaching it (`kill %N`, ADR-0071 §4).
///
/// Aborting the task drops every stream receiver, which stops the producers — the same
/// cancellation Ctrl-C performs on a foreground run. The job leaves the table.
///
/// # Errors
///
/// A structured error when no such job exists.
pub fn stop(session: &mut Session, number: u32) -> Eval<()> {
    let Some(job) = session.take_native_job(number) else {
        return Err(Flow::Failed(ono_value::ErrorValue::new(
            ono_core::ErrorCode::ResolveTargetNotFound,
            format!("no job %{number}"),
        )));
    };
    job.handle.abort();
    Ok(())
}

/// Reattaches native job `number` to the terminal.
///
/// A live job repaints its rows in place until Ctrl-C ends it; a finished one prints what it
/// produced. Either way the job leaves the table — foregrounding is how a native job is
/// collected, exactly as `fg` collects an external one.
///
/// # Errors
///
/// A structured error when no such job exists.
pub fn attach(session: &mut Session, number: u32) -> Eval<ExitStatus> {
    let Some(job) = session.take_native_job(number) else {
        return Err(Flow::Failed(ono_value::ErrorValue::new(
            ono_core::ErrorCode::ResolveTargetNotFound,
            format!("no job %{number}"),
        )));
    };

    let live = !job.handle.is_finished();
    if live && std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        let _ = ono_process::take_interrupt();
        let renderer = Renderer::new();
        let theme = Theme::default();
        let (width, height) = crate::native::live_geometry();
        let layout = Layout::new(width).max_rows(height.saturating_sub(3).max(4));
        let mut painted = 0usize;
        while !ono_process::take_interrupt() {
            if job.handle.is_finished() {
                break;
            }
            let rows: Vec<Value> = job
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .values()
                .cloned()
                .collect();
            let lines = layout.render_view_styled(
                &renderer,
                &rows,
                View::Table,
                &theme,
                Presentation::Terminal,
            );
            use std::io::Write as _;
            let mut out = std::io::stdout().lock();
            if painted > 0 {
                let _ = write!(out, "\x1b[{painted}A\x1b[0J");
            }
            for line in &lines {
                let _ = writeln!(out, "{line}");
            }
            let _ = out.flush();
            drop(out);
            painted = lines.len();
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        job.handle.abort();
        return Ok(ExitStatus::from_signal(2));
    }

    // A finished job hands over what it made; an unfinished one without a terminal is stopped —
    // there is nothing to reattach it to.
    job.handle.abort();
    let values = std::mem::take(
        &mut *job
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    let model_rows: Vec<Value> = job
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .cloned()
        .collect();
    let shown = if values.is_empty() {
        model_rows
    } else {
        values
    };
    if !shown.is_empty() {
        session.retain_result(shown.clone());
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
        crate::sink::Sink::for_stdout(&borrowed).write(&shown);
    }
    for failure in job
        .failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
    {
        eprintln!("ono: {failure}");
    }
    Ok(ExitStatus::SUCCESS)
}
