//! The commands the shell must run itself.
//!
//! Every one of these changes the shell's own state, which a child process cannot do: `cd` in a
//! subprocess moves a directory nobody is standing in.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::PathBuf;

use ono_core::{ErrorCode, ExitStatus};
use ono_value::ErrorValue;

use crate::eval::{Eval, Flow};
use crate::session::Session;

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
        other => Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            format!("`{other}` is not a builtin"),
        ))),
    }
}

fn cd(session: &mut Session, arguments: &[OsString]) -> Eval<ExitStatus> {
    session.refuse_moving_a_job("cd").map_err(Flow::Failed)?;
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
    session.set_cwd(resolved.clone());
    // v0.4 §30.3: `cd` updates the spatial place only where §47's `spatial.follow_cwd` says it
    // should — by default inside the storage family, so a `cd` cannot end a process
    // investigation. §30.4 keeps `PWD` the working directory and nothing else.
    crate::spatial::storage::follow_cwd(session, &resolved);
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
        // `set config` is answered before this runs (ADR-0094), and every other target reaches
        // the registry first (ADR-0068); only a target that is not a literal word — `set $what …`
        // — can still arrive here.
        other => Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`set` has no target `{other}` in the shell itself"),
            )
            .with_help("`set env` and `set config` change the session; write the target as a word"),
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
                format!("`remove` has no target `{other}` in the shell itself"),
            )
            .with_help("`remove env` withdraws a variable; write the target as a word"),
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
    let mut lines: Vec<(u32, String)> = session
        .executor()
        .jobs()
        .iter()
        .map(|job| {
            (
                job.id.number(),
                format!("[{}] {} {}", job.id, describe(&job.state), job.command),
            )
        })
        .collect();
    for job in session.native_jobs() {
        let state = if job.handle.is_finished() {
            "done"
        } else {
            "running"
        };
        lines.push((
            job.number,
            format!("[%{}] {} {}", job.number, state, job.command),
        ));
    }
    lines.sort_by_key(|(number, _)| *number);
    for (_, line) in lines {
        print_safely(&line);
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
    // A native job answers to the same numbers (spec §18.4). Foregrounding one reattaches its
    // rendering; Ctrl-C then ends it, exactly as it ends a foreground watch.
    if let Some(number) = arguments.first().and_then(|text| {
        text.to_string_lossy()
            .trim_start_matches('%')
            .parse::<u32>()
            .ok()
    }) && session.native_jobs().iter().any(|job| job.number == number)
    {
        return crate::context_jobs::attach(session, number);
    }
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

/// `kill %N …` — ends the named jobs (spec §18.1, §18.4; ADR-0071 §4).
///
/// An external job's process group gets `SIGTERM`, exactly as `fg`/`bg` address it; a
/// backgrounded native pipeline's task is aborted, which drops every receiver and stops the
/// producers. Either way the job leaves the table.
///
/// # Errors
///
/// A structured error naming the first specifier that is not a job.
pub fn kill_jobs(session: &mut Session, arguments: &[OsString]) -> Eval<ExitStatus> {
    for argument in arguments {
        let text = argument.to_string_lossy();
        let Some(number) = text
            .strip_prefix('%')
            .and_then(|digits| digits.parse::<u32>().ok())
        else {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{text}` is not a job specifier"),
                )
                .with_help(
                    "a job is `%N`, as `jobs` lists it; `kill process <pid>` signals a process",
                ),
            ));
        };
        if session.native_jobs().iter().any(|job| job.number == number) {
            crate::context_jobs::stop(session, number)?;
            continue;
        }
        let id = job_id(session, &[OsString::from(format!("%{number}"))])?;
        session
            .executor()
            .signal_job(id, ono_process::Signal::TERM)
            .map_err(|error| {
                Flow::Failed(ErrorValue::new(error.code(), error.message().to_owned()))
            })?;
    }
    // What was signalled may already be gone; reaping now keeps the next `jobs` truthful.
    let _ = session.executor().poll_jobs();
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

    // v0.4 §38.2: "At any place: `help here` … SHOULD show spatial operations supported by that
    // place." It cannot be a registry topic like the others, because the answer is a fact about
    // the session's current place and the registry knows nothing about where anybody is standing.
    if topic == "here" {
        println!("{}", crate::spatial::here_help(session)?.render());
        return Ok(ExitStatus::SUCCESS);
    }

    // A topic naming a command of a compiled-out tier is answered with the refusal that command
    // gets, rather than as a topic nobody wrote (ADR-0911).
    let words: Vec<String> = topic.split_whitespace().map(str::to_owned).collect();
    if let Some(refusal) = crate::absent::topic(&words) {
        return Err(Flow::Failed(refusal));
    }

    let registry = match crate::eval::native::registry() {
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

/// Writes text that came from the system, with every control character neutralised.
///
/// A path, a program name and a command line are all attacker-controlled in the sense that
/// matters: anyone who can create a file can choose what a shell will later print about it. A
/// value must never be able to drive the terminal it is displayed on, and the rule holds when the
/// output is redirected too, because the file is read by something eventually (ADR-0015 T1, T9).
///
/// Line structure is preserved by sanitising each line, so a multi-line report stays a report
/// while a value inside it cannot invent a line of its own.
fn print_safely(text: &str) {
    let mut out = std::io::stdout().lock();
    for line in text.split('\n') {
        let _ = writeln!(out, "{}", ono_render::sanitise(line));
    }
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

/// Whether `name` may run while a configuration file is being read (ADR-0010).
///
/// The allowed set is exactly the declarative one: `set` records a value and `remove` withdraws
/// one. Everything else — `cd`, `exit`, `jobs`, `fg`, `bg`, `help`, `explain` — changes the
/// session or writes to the terminal, and a configuration file that could do either would be a
/// startup script wearing a settings file's name.
///
/// `exit` matters most: without this it would end the session before the shell had one, and a
/// request to leave that survived the load would replace the status of every command afterwards.
#[must_use]
pub fn allowed_in_config(name: &str) -> bool {
    matches!(name, "set" | "remove")
}
