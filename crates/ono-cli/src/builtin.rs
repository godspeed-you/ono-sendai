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
        "help" => help(),
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

fn help() -> Eval<ExitStatus> {
    println!("{}", crate::usage_text());
    Ok(ExitStatus::SUCCESS)
}

/// `explain <command>` — the resolution the shell would perform, reported by the code that
/// performs it rather than described somewhere that could drift from it (ADR-0011).
fn explain(session: &mut Session, arguments: &[OsString]) -> Eval<ExitStatus> {
    let Some(name) = arguments.first() else {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            "`explain` needs something to explain",
        )));
    };
    let name = name.to_string_lossy();
    let (namespace, bare) = match name.split_once(':') {
        Some((prefix, rest)) => (crate::resolve::Namespace::from_prefix(Some(prefix)), rest),
        None => (Some(crate::resolve::Namespace::Any), name.as_ref()),
    };
    let Some(namespace) = namespace else {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            format!("unknown namespace in `{name}`"),
        )));
    };

    match crate::resolve::resolve(session, namespace, bare) {
        Ok(crate::resolve::Resolution::Builtin(builtin)) => {
            println!("{builtin}: a command the shell runs itself");
            println!("  step 4 of the resolution order (ADR-0011): native command");
        }
        Ok(crate::resolve::Resolution::External(path)) => {
            println!("{bare}: an external program");
            println!("  step 5 of the resolution order (ADR-0011): PATH");
            println!("  resolves to {}", path.display());
        }
        Err(error) => {
            println!("{bare}: not found");
            println!("  {}", error.render_terse());
            return Ok(ExitStatus::NOT_FOUND);
        }
    }
    if session.mode() == Mode::Config {
        println!("  configuration mode: this command would not be allowed to run");
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
