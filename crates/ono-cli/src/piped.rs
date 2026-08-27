//! The piped forms of the commands the shell answers itself (ADR-0118).
//!
//! `remove link testbox` is answered by the evaluator's seams (`remote`, `plugins`, `context`),
//! which claim the *head* of a pipeline. `get link | remove link` puts the same command after
//! a pipe, and its contract says what that means: `input: "null | stream<ono.link/1>"` makes
//! the piped records the targets; `input: "null"` (`connect host`, `link host`) makes the piped
//! form a type error that names the head form. Either way the answer is the shell's, never the
//! registry's E0101 for a stage nothing implements.

use ono_core::ExitStatus;
use ono_core::Span;
use ono_parser::{Stage, StageList};

use crate::eval::{Eval, Flow};
use crate::session::Session;

/// Which seam answers the piped stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piped {
    /// A link definition or host command of `remote.rs`.
    Remote(crate::remote::Request),
    /// A KUANG/11 management command of `plugins.rs`.
    Plugin(crate::plugins::Request),
    /// `link host` — declared with no stream input.
    Link,
    /// `load plugin`.
    LoadPlugin,
}

/// The first stage after the head that a seam claims, with what claims it.
#[must_use]
pub fn claims(list: &StageList) -> Option<(usize, Piped)> {
    list.stages
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, stage)| claim(stage).map(|piped| (index, piped)))
}

fn claim(stage: &Stage) -> Option<Piped> {
    if let Some(request) = crate::remote::claims(stage) {
        return Some(Piped::Remote(request));
    }
    if let Some(request) = crate::plugins::claims(stage) {
        return Some(Piped::Plugin(request));
    }
    match crate::context::claims(stage) {
        Some(crate::context::Request::Link) => Some(Piped::Link),
        Some(crate::context::Request::LoadPlugin) => Some(Piped::LoadPlugin),
        _ => None,
    }
}

/// Runs `list`, whose stage `index` is the piped command `piped` names.
///
/// # Errors
///
/// What the stages before it report, the type error of a command with no stream input, or the
/// command's own structured refusal.
pub fn run(
    session: &mut Session,
    list: &StageList,
    source: &str,
    index: usize,
    piped: Piped,
) -> Eval<ExitStatus> {
    let stage = &list.stages[index];
    // A command that cannot take the pipe is refused before anything runs: the stages before it
    // may have effects of their own, and the user asked for something that cannot happen.
    if piped == Piped::Link {
        return Err(Flow::Failed(crate::remote::no_stream_input(
            "link host",
            "host",
        )));
    }

    let head = &list.stages[..index];
    let prefix = StageList {
        stages: head.to_vec(),
        span: Span::new(
            list.span.start(),
            head.last()
                .map_or(list.span.end(), |stage| stage.span.end()),
        ),
    };
    let targets = crate::native::run_collecting(session, &prefix, source)?;

    match piped {
        Piped::Link => unreachable!("refused above"),
        Piped::Remote(request) => {
            let values = crate::remote::answer_piped(session, stage, request, &targets)?;
            let failed = values.iter().any(is_failed_row);
            let status = crate::native::run_seeded_from(session, list, source, index + 1, values)?;
            Ok(if failed { ExitStatus::FAILURE } else { status })
        }
        Piped::Plugin(request) => {
            let words = stage_words(session, stage, source)?;
            let produced = crate::plugins::run_piped(session, request, &words, &targets)?;
            let status =
                crate::native::run_seeded_from(session, list, source, index + 1, produced.values)?;
            if let Some(failure) = produced.failure {
                crate::report::Reporter::new(ono_render::Presentation::choose(
                    std::io::IsTerminal::is_terminal(&std::io::stderr()),
                    &[],
                ))
                .error(&failure);
                return Ok(ExitStatus::FAILURE);
            }
            Ok(status)
        }
        Piped::LoadPlugin => {
            let words = stage_words(session, stage, source)?;
            let status = crate::plugins::load_piped(session, &words, &targets)?;
            if index + 1 < list.stages.len() {
                // `load plugin` prints its summary and produces no records in this build; the
                // stages after it see an empty stream rather than nothing at all.
                return crate::native::run_seeded_from(
                    session,
                    list,
                    source,
                    index + 1,
                    Vec::new(),
                );
            }
            Ok(status)
        }
    }
}

fn stage_words(session: &mut Session, stage: &Stage, source: &str) -> Eval<Vec<String>> {
    Ok(crate::eval::stage_arguments(session, stage, source)?
        .iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect())
}

/// Whether an `ono.action-result/1` row reports a failure (spec §16.5: one makes the run fail).
fn is_failed_row(value: &ono_value::Value) -> bool {
    value
        .as_record()
        .ok()
        .and_then(|record| record.get("status"))
        .and_then(|status| status.as_str().ok())
        == Some("failed")
}
