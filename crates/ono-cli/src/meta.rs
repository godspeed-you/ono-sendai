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
use ono_parser::{Argument, Stage, StageHead};
use ono_value::{ActionResult, ActionStatus, ErrorValue, MapValue, SchemaId, Value, ValueRef};

use crate::eval::{Eval, Flow};
use crate::resolve::Namespace;
use crate::session::Session;
use crate::settings::{Given, Layer};

/// What a stage asks the shell to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// `resolve command <word>` — what the word resolves to (spec §6.5).
    ResolveCommand,
    /// `get config [key]` — the settings with their provenance (spec §30).
    GetConfig,
    /// `set config key = value` — one typed assignment (spec §30).
    SetConfig,
    /// `inspect limits [key]` — the effective runtime limits (v0.4.1 §54.3).
    InspectLimits,
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
        ("get", Some("config")) => Some(Request::GetConfig),
        ("set", Some("config")) => Some(Request::SetConfig),
        ("inspect", Some("limits")) => Some(Request::InspectLimits),
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
        Request::GetConfig => {
            let words = crate::eval::stage_arguments(session, stage, source)?;
            get_config(session, &words).map_err(Flow::Failed)
        }
        Request::SetConfig => set_config(session, stage, source),
        Request::InspectLimits => {
            let words = crate::eval::stage_arguments(session, stage, source)?;
            inspect_limits(session, &words).map_err(Flow::Failed)
        }
    }
}

/// `inspect limits [key|prefix.]`: the effective runtime limits (v0.4.1 §54.3, §12.4).
///
/// The figures come from the settings catalogue, which is what the shell enforces, so this is a
/// view of the limits in force rather than a second table of the same numbers (§52.2). Nothing
/// here is secret: a limit is a ceiling, and §53.3's fingerprints and keys are not settings.
fn inspect_limits(session: &Session, words: &[OsString]) -> Result<Vec<Value>, ErrorValue> {
    // The first word is the target, `limits`.
    let mut selector: Option<String> = None;
    for word in words.iter().skip(1) {
        let word = word.to_string_lossy();
        if let Some(option) = word.strip_prefix("--") {
            return Err(ErrorValue::new(
                ErrorCode::TypeUnknownField,
                format!("`inspect limits` has no option `--{option}`"),
            )
            .with_help("`inspect limits` takes one key or dotted prefix, and no options"));
        }
        if selector.replace(word.into_owned()).is_some() {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                "`inspect limits` takes one key or prefix",
            ));
        }
    }
    let rows = crate::limits::rows(session.settings());
    let Some(selector) = selector else {
        return Ok(rows);
    };
    let matched: Vec<Value> = rows
        .into_iter()
        .filter(|row| match row {
            Value::Map(map) => match map.get("key") {
                Some(Value::String(key)) => {
                    if let Some(prefix) = selector.strip_suffix('.') {
                        key.starts_with(prefix)
                    } else {
                        **key == *selector
                    }
                }
                _ => false,
            },
            _ => false,
        })
        .collect();
    if matched.is_empty() {
        return Err(ErrorValue::new(
            ErrorCode::TypeUnknownField,
            format!("there is no limit `{selector}`"),
        )
        .with_help("`inspect limits` with no argument lists every limit in force"));
    }
    Ok(matched)
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

/// `get config [key|prefix.] [--problems] [--overridden]` (spec §30, ADR-0094).
fn get_config(session: &Session, words: &[OsString]) -> Result<Vec<Value>, ErrorValue> {
    let mut selector: Option<String> = None;
    let mut problems = false;
    let mut overridden = false;
    // The first word is the target, `config`.
    for word in words.iter().skip(1) {
        let word = word.to_string_lossy();
        match word.as_ref() {
            "--problems" => problems = true,
            "--overridden" => overridden = true,
            option if option.starts_with("--") => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeUnknownField,
                    format!("`get config` has no option `{option}`"),
                )
                .with_help("`get config` takes `--problems` and `--overridden`"));
            }
            key => {
                if selector.replace(key.to_owned()).is_some() {
                    return Err(ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        "`get config` takes one key or prefix",
                    ));
                }
            }
        }
    }
    if problems {
        return Ok(session.settings().problems().to_vec());
    }
    session.settings().records(selector.as_deref(), overridden)
}

/// `set config <key> = <value>`: one typed assignment at the layer being read — the file's
/// while a configuration file loads, the invocation's at the prompt (ADR-0010, ADR-0094).
fn set_config(session: &mut Session, stage: &Stage, source: &str) -> Eval<Vec<Value>> {
    let started = std::time::Instant::now();
    let usage = |what: &str| {
        Flow::Failed(
            ErrorValue::new(ErrorCode::TypeMismatch, format!("`set config` {what}"))
                .with_help("`set config render.table.max_rows = 200` (spec §30)"),
        )
    };
    // The first argument is the target, `config`.
    let mut arguments = stage.arguments.iter().skip(1);
    let key = match arguments.next() {
        Some(Argument::Word(word)) => word.text.clone(),
        _ => return Err(usage("needs the dotted key of a setting")),
    };
    let mut argument = arguments.next();
    // The `=` is punctuation, not a value.
    if let Some(Argument::Word(word)) = argument
        && word.text == "="
    {
        argument = arguments.next();
    }
    let given = match argument {
        Some(Argument::Word(word)) => {
            let expanded = crate::expand::expand_word(session, &word.text)?;
            let text = expanded
                .iter()
                .map(|word| word.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            Given::Word(text)
        }
        Some(Argument::Value(expression)) => {
            Given::Value(crate::eval::eval_expr(session, expression, source)?)
        }
        Some(Argument::Option(_) | Argument::Error(_)) | None => {
            return Err(usage("needs a value after the key"));
        }
    };
    if arguments.next().is_some() {
        return Err(usage("takes one value"));
    }

    let (layer, file, line) = match session.settings().reading() {
        Some(reading) => (
            reading.layer,
            Some(reading.path.clone()),
            Some(stage.span.line_column(source).0),
        ),
        None => (Layer::Invocation, None, None),
    };
    let in_file = file.is_some();
    let changed = match session
        .settings_mut()
        .assign(&key, given, layer, file, line)
    {
        Ok(changed) => changed,
        Err(error) => {
            // A bad setting never stops the shell from starting: the failure is reported by the
            // loader and kept for `get config --problems` (ADR-0010).
            if in_file {
                session.settings_mut().note_problem(&error);
            }
            return Err(Flow::Failed(error));
        }
    };

    let mut identity = MapValue::new();
    identity.insert("key".into(), Value::string(&key));
    let target = ValueRef::object(SchemaId::new("ono.config-setting", 1), identity);
    let elapsed = ono_value::Duration::from_nanoseconds(
        i128::try_from(started.elapsed().as_nanos()).unwrap_or(i128::MAX),
    );
    let result = ActionResult::new(target, "ono.config.set", ActionStatus::Success)
        .changed(changed)
        .with_message(&format!("{key} set at the {} layer", layer.name()))
        .with_duration(elapsed);
    Ok(vec![result.into_value()])
}
