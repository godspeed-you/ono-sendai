//! The commands the shell must run itself.
//!
//! Every one of these changes the shell's own state, which a child process cannot do: `cd` in a
//! subprocess moves a directory nobody is standing in.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use ono_core::{ErrorCode, ExitStatus};
use ono_value::ErrorValue;

use crate::eval::{Eval, Flow};
use crate::session::{Mode, Session};

/// Runs the builtin `name` with already-expanded arguments.
pub fn run(session: &mut Session, name: &str, arguments: &[OsString]) -> Eval<ExitStatus> {
    match name {
        "cd" => cd(session, arguments),
        "exit" => exit(arguments),
        "set" => set(session, arguments),
        "remove" => remove(session, arguments),
        "jobs" => jobs(session),
        "fg" => foreground(session, arguments),
        "bg" => background(session, arguments),
        "true" => Ok(ExitStatus::SUCCESS),
        "false" => Ok(ExitStatus::FAILURE),
        "help" => help(session, arguments),
        "explain" => explain(session, arguments),
        other => Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            format!("`{other}` is not a builtin"),
        ))),
    }
}

fn cd(session: &mut Session, arguments: &[OsString]) -> Eval<ExitStatus> {
    let target = match arguments.first() {
        Some(path) => PathBuf::from(path),
        None => session.home().ok_or_else(|| {
            Flow::Failed(
                ErrorValue::new(
                    ErrorCode::IoNotFound,
                    "there is no home directory to return to",
                )
                .with_help("`HOME` is unset; name a directory instead"),
            )
        })?,
    };
    let absolute = if target.is_absolute() {
        target
    } else {
        session.cwd().join(target)
    };

    let resolved = std::fs::canonicalize(&absolute).map_err(|error| {
        Flow::Failed(
            io_error(&absolute, &error)
                .with_help("the shell stays where it was; nothing has changed"),
        )
    })?;
    if !resolved.is_dir() {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::IoNotDirectory,
            format!("{} is not a directory", resolved.display()),
        )));
    }

    session.set_env("PWD", resolved.as_os_str());
    session.set_cwd(resolved);
    Ok(ExitStatus::SUCCESS)
}

fn exit(arguments: &[OsString]) -> Eval<ExitStatus> {
    let status = match arguments.first() {
        None => ExitStatus::SUCCESS,
        Some(text) => {
            let text = text.to_string_lossy();
            match text.trim().parse::<u8>() {
                Ok(code) => ExitStatus::from_code(code),
                Err(_) => {
                    return Err(Flow::Failed(
                        ErrorValue::new(
                            ErrorCode::TypeMismatch,
                            format!("`{text}` is not an exit status"),
                        )
                        .with_help("an exit status is a number from 0 to 255"),
                    ));
                }
            }
        }
    };
    Err(Flow::Exit(status))
}

/// `set env NAME = value`, and `set config path = value` (spec §30).
fn set(session: &mut Session, arguments: &[OsString]) -> Eval<ExitStatus> {
    let words: Vec<String> = arguments
        .iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    let mut rest: Vec<&str> = words.iter().map(String::as_str).collect();

    let target = rest.first().copied().ok_or_else(|| {
        Flow::Failed(
            ErrorValue::new(ErrorCode::ResolveTargetNotFound, "`set` needs a target")
                .with_help("`set env NAME = value`, or `set config path = value` (spec §30)"),
        )
    })?;
    rest.remove(0);
    // The `=` is punctuation, not a value.
    rest.retain(|word| *word != "=");

    match target {
        "env" => {
            let name = rest.first().copied().ok_or_else(|| {
                Flow::Failed(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "`set env` needs a variable name",
                ))
            })?;
            let value = rest.get(1).copied().unwrap_or_default();
            session.set_env(name, value);
            Ok(ExitStatus::SUCCESS)
        }
        "config" => {
            // Configuration is read into the session in ADR-0010's layers; a `set config` in a
            // running shell records the value at the invocation layer.
            let name = rest.first().copied().unwrap_or_default();
            let value = rest.get(1).copied().unwrap_or_default();
            session.bind(
                format!("config.{name}"),
                ono_value::Value::String(value.into()),
            );
            Ok(ExitStatus::SUCCESS)
        }
        other => Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`set` has no target `{other}`"),
            )
            .with_help("`set env` and `set config` are the targets phase A carries"),
        )),
    }
}

fn remove(session: &mut Session, arguments: &[OsString]) -> Eval<ExitStatus> {
    let words: Vec<String> = arguments
        .iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    match words.first().map(String::as_str) {
        Some("env") => {
            let Some(name) = words.get(1) else {
                return Err(Flow::Failed(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "`remove env` needs a variable name",
                )));
            };
            session.remove_env(name);
            Ok(ExitStatus::SUCCESS)
        }
        Some(other) => Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`remove` has no target `{other}` yet"),
            )
            .with_help("`remove env` is the target phase A carries; `remove file` arrives with the file provider"),
        )),
        None => Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            "`remove` needs a target",
        ))),
    }
}

fn jobs(session: &mut Session) -> Eval<ExitStatus> {
    // Reaping first, so what is printed is what is true now rather than what was true last time
    // the prompt was drawn.
    let _ = session.executor().poll_jobs();
    for job in session.executor().jobs() {
        println!("[{}] {} {}", job.id, describe(&job.state), job.command);
    }
    Ok(ExitStatus::SUCCESS)
}

fn describe(state: &ono_process::JobState) -> &'static str {
    match state {
        ono_process::JobState::Running => "running",
        ono_process::JobState::Stopped(_) => "stopped",
        ono_process::JobState::Exited(_) => "done",
    }
}

fn foreground(session: &mut Session, arguments: &[OsString]) -> Eval<ExitStatus> {
    let id = job_id(session, arguments)?;
    let outcome = session
        .executor()
        .foreground(id)
        .map_err(|error| Flow::Failed(ErrorValue::new(error.code(), error.message().to_owned())))?;
    Ok(outcome.status())
}

fn background(session: &mut Session, arguments: &[OsString]) -> Eval<ExitStatus> {
    let id = job_id(session, arguments)?;
    session
        .executor()
        .background(id)
        .map_err(|error| Flow::Failed(ErrorValue::new(error.code(), error.message().to_owned())))?;
    Ok(ExitStatus::SUCCESS)
}

fn job_id(session: &mut Session, arguments: &[OsString]) -> Eval<ono_process::JobId> {
    if let Some(text) = arguments.first() {
        let text = text.to_string_lossy();
        let digits = text.trim_start_matches('%');
        let number: u32 = digits.parse().map_err(|_| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{text}` is not a job"),
            ))
        })?;
        // The id is looked up rather than constructed, so `fg %9` reports a job that does not
        // exist instead of signalling one that does.
        return session
            .executor()
            .jobs()
            .into_iter()
            .find(|job| job.id.number() == number)
            .map(|job| job.id)
            .ok_or_else(|| {
                Flow::Failed(ErrorValue::new(
                    ErrorCode::ResolveTargetNotFound,
                    format!("there is no job %{number}"),
                ))
            });
    }
    // No argument means the most recent job, which is what a user means by `fg`.
    session
        .executor()
        .jobs()
        .last()
        .map(|job| job.id)
        .ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                "there are no jobs",
            ))
        })
}

/// `help [topic]` — generated from the command registry, never hand-written (spec §15.2).
///
/// A help page assembled by hand is one that stops matching the command the first time either
/// changes. The registry is the contract, so the page is derived from it and `spec-check` fails
/// if a command's contract loses the summary, the documentation or the example the page needs.
fn help(session: &mut Session, arguments: &[OsString]) -> Eval<ExitStatus> {
    let topic = arguments
        .iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");

    let registry = match ono_command::CommandRegistry::embedded() {
        Ok(registry) => registry,
        Err(error) => return Err(Flow::Failed(error)),
    };
    // The provider registry is consulted so a page can say whether the provider a command needs
    // is actually available here — but only when a topic was named, so bare `help` stays as cheap
    // as spec §34's startup budget expects.
    let page = if topic.is_empty() {
        ono_command::help(registry, None, "")
    } else {
        ono_command::help(registry, Some(session.providers()), &topic)
    };

    match page {
        Ok(page) => {
            println!("{}", page.render());
            Ok(ExitStatus::SUCCESS)
        }
        Err(error) => Err(Flow::Failed(error)),
    }
}

/// `explain <pipeline>` — what would happen, reported without anything happening.
///
/// Spec §15.3 and §42 want the resolution and the execution plan of a whole pipeline, not of one
/// name: which command each stage resolves to, which provider and capability it will use, its
/// input and output schemas, whether it streams, and what it would change. ADR-0011 requires the
/// order to be reported by the code that performs it rather than described somewhere that could
/// drift from it.
///
/// A pipeline has to arrive quoted — `explain "get process | where cpu > 20"` — because an
/// unquoted `|` would send `explain` itself into a pipeline, which is the grammar working
/// correctly rather than a limitation to work around.
fn explain(session: &mut Session, arguments: &[OsString]) -> Eval<ExitStatus> {
    let source = arguments
        .iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    if source.trim().is_empty() {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                "`explain` needs something to explain",
            )
            .with_help("`explain \"get process | where cpu > 20\"` — quote the pipeline"),
        ));
    }

    let parsed = ono_parser::parse(&source);
    let Some(pipeline) = parsed
        .program()
        .statements
        .first()
        .and_then(ono_parser::Statement::as_pipeline)
    else {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ParseSyntax,
            format!("`{source}` is not a pipeline"),
        )));
    };

    let registry = ono_command::CommandRegistry::embedded().map_err(Flow::Failed)?;
    let plan = ono_command::plan(registry, Some(session.providers()), pipeline, &source);
    println!("{}", plan.render());

    // A stage the registry does not know is an external program, and which one it will be is the
    // half of the answer the plan cannot give (ADR-0011 T11: a shadowing binary is only
    // defensible if the shell will say which one it picked).
    for stage in &pipeline.head.stages {
        let Some(name) = stage.head.name() else {
            continue;
        };
        // A head the registry knows as a verb or as a command id is native, and the plan above
        // already said everything there is to say about it.
        if registry.verb(name).is_some() || registry.get(name).is_some() {
            continue;
        }
        match crate::resolve::find_on_path(session, name) {
            Some(path) => println!("  `{name}` is external and resolves to {}", path.display()),
            None => println!("  `{name}` resolves to nothing on PATH"),
        }
    }

    if session.mode() == Mode::Config {
        println!("  configuration mode: none of this would be allowed to run");
    }
    Ok(ExitStatus::SUCCESS)
}

/// Turns an I/O failure into the coded error of spec §43 that matches it.
pub fn io_error(path: &std::path::Path, error: &std::io::Error) -> ErrorValue {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::IoNotFound,
        std::io::ErrorKind::PermissionDenied => ErrorCode::IoPermissionDenied,
        std::io::ErrorKind::AlreadyExists => ErrorCode::IoAlreadyExists,
        std::io::ErrorKind::NotADirectory => ErrorCode::IoNotDirectory,
        _ => ErrorCode::IoPermissionDenied,
    };
    ErrorValue::new(code, format!("{}: {error}", path.display()))
}

/// Whether `name` is a command the shell runs itself.
#[must_use]
pub fn is_builtin(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| crate::resolve::BUILTINS.contains(&name))
}
