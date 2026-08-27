//! The commands the shell answers with values of its own: `resolve command`, `get config` and
//! `set config`.
//!
//! None of these has a provider and none can be answered by the registry alone: resolution runs
//! through functions, aliases and `PATH`, which only the session sees (ADR-0011), and the
//! configuration layers are the session's (ADR-0010). The contracts are still the registry's —
//! `ono.command.resolve`, `ono.config.get`, `ono.config.set` in `docs/spec/commands/meta.yaml`
//! supply help, completion and typing — and this module supplies the values, which then seed the
//! rest of the pipeline exactly as a producer's stream would (ADR-0093).

use std::ffi::OsString;

use ono_core::ErrorCode;
use ono_parser::{Stage, StageHead};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::resolve::Namespace;
use crate::session::Session;

/// What a stage asks the shell to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// `resolve command <word>` — what the word resolves to (spec §6.5).
    ResolveCommand,
}

/// Whether `stage` is one of the commands this module answers.
///
/// The decision is by the head word and its target, exactly as `enter` is claimed by its head:
/// `ono:resolve command` means the same, and a program named `resolve` stays reachable as
/// `exec:resolve`.
#[must_use]
pub fn claims(stage: &Stage) -> Option<Request> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    if !matches!(name.namespace.as_deref(), None | Some("ono")) {
        return None;
    }
    let target = stage
        .arguments
        .first()
        .and_then(ono_parser::Argument::as_word);
    match (name.name.as_str(), target) {
        ("resolve", Some("command")) => Some(Request::ResolveCommand),
        _ => None,
    }
}

/// The values `stage` produces.
///
/// # Errors
///
/// The structured error of the command: `resolve.command_not_found` for a word nothing answers
/// to, or a type error for arguments the command cannot use.
pub fn answer(
    session: &mut Session,
    stage: &Stage,
    source: &str,
    request: Request,
) -> Eval<Vec<Value>> {
    match request {
        Request::ResolveCommand => {
            let words = crate::eval::stage_arguments(session, stage, source)?;
            resolve_command(session, &words).map_err(Flow::Failed)
        }
    }
}

/// `resolve command <word>`: the one record for what the head word resolves to (ADR-0093).
fn resolve_command(session: &Session, words: &[OsString]) -> Result<Vec<Value>, ErrorValue> {
    // The first word is the target, `command`.
    let word = words
        .get(1)
        .map(|word| word.to_string_lossy())
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                "`resolve command` needs the head word to resolve",
            )
            .with_help("`resolve command ls` says which `ls` would run (spec §6.5)")
        })?;
    let (namespace, name) = match word.split_once(':') {
        Some((prefix, rest)) if Namespace::from_prefix(Some(prefix)).is_some() => (
            Namespace::from_prefix(Some(prefix)).unwrap_or(Namespace::Any),
            rest,
        ),
        _ => (Namespace::Any, word.as_ref()),
    };
    crate::resolve::describe(session, namespace, name).map(|record| vec![record])
}
