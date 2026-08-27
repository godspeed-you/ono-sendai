//! `view` (spec §13.5, §31.55, ADR-0050): the interactive browser over a stream of values.
//!
//! The view owns the terminal until dismissed and mutates nothing: selection can never change
//! pipeline data, and leaving keeps the selected row as the `@` reference, so the next command
//! acts on what was picked. Where stdout is not a terminal, the same values render plainly and
//! deterministically instead — §31.55's own fallback, and §17.4's law that nothing interactive
//! hides in a pipeline.

use std::io::{IsTerminal, Write};

use ono_core::{ErrorCode, ExitStatus};
use ono_editor::{KeyCode, RawMode, read_key};
use ono_render::{Layout, Presentation, Renderer, Theme, Token, View};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::Session;

/// Runs the view over already-collected values, answering when the user leaves it.
///
/// # Errors
///
/// A structured error for an unknown view name or a terminal that cannot enter raw mode.
pub fn run(session: &mut Session, name: &str, values: Vec<Value>) -> Eval<ExitStatus> {
    let tree = match name {
        "table" => false,
        "tree" => true,
        other => {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveTargetNotFound,
                    format!("no view answers to `{other}`"),
                )
                .with_help("the built-in views are `table` and `tree` (spec §13.6)"),
            ));
        }
    };

    if !std::io::stdout().is_terminal() {
        // §31.55: on redirection the view falls back to the plain rendering of the same values.
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
        let sink = if tree {
            crate::sink::Sink::for_stdout(&borrowed).with_view(View::Tree)
        } else {
            crate::sink::Sink::for_stdout(&borrowed)
        };
        sink.write(&values);
        session.retain_result(values);
        return Ok(ExitStatus::SUCCESS);
    }

    let selected = browse(&values, tree).map_err(|error| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            format!("the terminal refused the view: {error}"),
        ))
    })?;

    // Leaving the view keeps what was picked: bare `@` now names it (§6.4, ADR-0033), and the
    // rows join the retained results exactly as if they had been printed.
    if let Some(value) = selected {
        session.select(value);
    }
    session.retain_result(values);
    Ok(ExitStatus::SUCCESS)
}

/// The interactive loop: a cursor, a pane, and three keys' worth of contract.
fn browse(values: &[Value], tree: bool) -> std::io::Result<Option<Value>> {
    let theme = Theme::default();
    let renderer = Renderer::new();
    let (width, height) = crate::native::live_geometry();
    let page = height.saturating_sub(6).max(4);
    let layout = Layout::new(width);

    let raw = RawMode::enter()?;
    let mut out = std::io::stdout().lock();
    let mut cursor = 0usize;
    let mut offset = 0usize;
    let mut pane = false;
    let mut painted = 0usize;

    let outcome = loop {
        // The frame: a viewport of rows, the cursor marked, the pane below when open.
        let mut lines = Vec::new();
        if tree {
            for value in values {
                for line in layout.render_view_styled(
                    &renderer,
                    std::slice::from_ref(value),
                    View::Tree,
                    &theme,
                    Presentation::Terminal,
                ) {
                    lines.push(line);
                }
            }
        } else {
            let visible = &values[offset..(offset + page).min(values.len())];
            let table = layout.render_view_styled(
                &renderer,
                visible,
                View::Table,
                &theme,
                Presentation::Terminal,
            );
            for (index, line) in table.into_iter().enumerate() {
                // Line 0 is the header; row i sits on line i+1 while the table stays unwrapped.
                let marked = if index == cursor - offset + 1 {
                    format!(
                        "{} {line}",
                        theme.paint(">", Token::Accent, Presentation::Terminal)
                    )
                } else {
                    format!("  {line}")
                };
                lines.push(marked);
            }
        }
        if pane && let Some(value) = values.get(cursor) {
            lines.push(theme.paint("--- inspect", Token::Dim, Presentation::Terminal));
            for line in layout.render_view_styled(
                &renderer,
                std::slice::from_ref(value),
                View::List,
                &theme,
                Presentation::Terminal,
            ) {
                lines.push(line);
            }
        }
        lines.push(theme.paint(
            "[up/down] move   [enter] inspect   [q] keep selection and leave",
            Token::Dim,
            Presentation::Terminal,
        ));

        if painted > 0 {
            write!(out, "\x1b[{painted}A\x1b[0J")?;
        }
        for line in &lines {
            writeln!(out, "{line}")?;
        }
        out.flush()?;
        painted = lines.len();

        match read_key()?.code() {
            KeyCode::Up => {
                cursor = cursor.saturating_sub(1);
                if cursor < offset {
                    offset = cursor;
                }
            }
            KeyCode::Down => {
                cursor = (cursor + 1).min(values.len().saturating_sub(1));
                if cursor >= offset + page {
                    offset = cursor + 1 - page;
                }
            }
            KeyCode::Enter => pane = !pane,
            KeyCode::Char('q') | KeyCode::Esc => break values.get(cursor).cloned(),
            KeyCode::Char('c') if false => break None,
            _ => {}
        }
    };
    drop(raw);
    Ok(outcome)
}
