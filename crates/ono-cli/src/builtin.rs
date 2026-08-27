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
/// The subject may arrive unquoted — `explain get process | where cpu > 20` — because spec §11.3
/// spells it that way: the evaluator hands the source text after `explain` over whole, pipes and
/// all, before pipeline construction would claim them. A quoted subject means the same thing.
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

    let mut source = source;
    let mut parsed = ono_parser::parse(&source);
    // Step 3 of the resolution order (ADR-0011, ADR-0070): an alias is reported as one, with
    // its expansion, and the expansion is what gets explained — it is what would run.
    let mut expanded: Vec<String> = Vec::new();
    while let Some(pipeline) = parsed
        .program()
        .statements
        .first()
        .and_then(ono_parser::Statement::as_pipeline)
        && let Some((name, text)) = crate::eval::expand_alias(session, &pipeline.head, &source)
        && !expanded.contains(&name)
    {
        print_safely(&format!(
            "  `{name}` is an alias for `{}` — step 3 of the resolution order; explaining the \
             expansion",
            session
                .alias(&name)
                .map(|alias| alias.expansion.clone())
                .unwrap_or_default()
        ));
        expanded.push(name);
        source = text;
        parsed = ono_parser::parse(&source);
    }
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
    // The last stage's consumer is whatever the shell's stdout is, and a plan that assumed a
    // terminal would promise interactive rendering to a script (spec v0.3 §1.4).
    let stdout = if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        ono_adapter::Stdout::Terminal
    } else {
        ono_adapter::Stdout::Stream
    };
    // What each program resolves to is looked up once, up front: the plan needs it to ask the
    // adapter registry (spec v0.3 §1.23), and the report below quotes it.
    let resolved: std::collections::BTreeMap<String, Option<std::path::PathBuf>> = pipeline
        .head
        .stages
        .iter()
        .filter_map(|stage| {
            ono_command::raw_program(stage)
                .or_else(|| ono_command::adapt_program(stage))
                .or_else(|| stage.head.name())
        })
        .map(|name| (name.to_owned(), crate::resolve::find_on_path(session, name)))
        .collect();
    let executables = |name: &str| resolved.get(name).cloned().flatten();
    // Inside a link frame the remote negotiates (spec v0.3 §1.54): the local registry is not
    // consulted for the plan, and the remote's answer is reported per stage below.
    let remote_host = session.link_host();
    let (providers, adapters) = session.registries();
    let adapters = remote_host.is_none().then_some(adapters);
    let plan = ono_command::plan_with(
        registry,
        Some(providers),
        pipeline,
        &source,
        &ono_command::PlanContext {
            stdout,
            adapters,
            executables: Some(&executables),
        },
    );
    // The plan quotes the source it was given and the paths it resolved, both of which are
    // attacker-controlled: a program named with an OSC sequence sitting on `PATH` would otherwise
    // retitle the terminal from inside the command that exists to tell you about it, and the
    // bytes would survive into a file (ADR-0015 T1, T9, T11).
    print_safely(&plan.render());

    if let Some(host) = remote_host {
        for (stage, planned) in pipeline.head.stages.iter().zip(plan.stages()) {
            let Some(demand) = planned.demand() else {
                continue;
            };
            if ono_command::is_raw(stage) {
                continue;
            }
            let Some(argv) = crate::native::literal_argv(stage) else {
                continue;
            };
            let state = crate::native::remote_decision(session, &argv, demand).map_or_else(
                || "raw (the remote agent cannot negotiate adapters)".to_owned(),
                |decision| decision.state,
            );
            print_safely(&format!("  adaptation on {host}: {state}"));
        }
    }

    // A stage the registry does not know is an external program, and which one it will be is the
    // half of the answer the plan cannot give (ADR-0011 T11: a shadowing binary is only
    // defensible if the shell will say which one it picked).
    for stage in &pipeline.head.stages {
        let Some(name) = ono_command::raw_program(stage)
            .or_else(|| ono_command::adapt_program(stage))
            .or_else(|| stage.head.name())
        else {
            continue;
        };
        // A head the registry knows as a verb or as a command id is native, and the plan above
        // already said everything there is to say about it.
        if registry.verb(name).is_some() || registry.get(name).is_some() {
            continue;
        }
        // A user function is step 2 of the order (ADR-0011, ADR-0070): it wins over the
        // registry and over PATH, and the report says so instead of describing what it shadows.
        if let Some(function) = session.function(name) {
            print_safely(&format!(
                "  `{name}` is a user function declared at {} — step 2 of the resolution order",
                function.declaration.span
            ));
            continue;
        }
        // The registry does not know the shell's own commands, so without this `explain cd`
        // would report that `cd` resolves to nothing — which is both false and exactly the kind
        // of thing `explain` exists to get right (ADR-0011).
        if crate::resolve::BUILTINS.contains(&name) {
            print_safely(&format!(
                "  `{name}` is a command the shell runs itself — step 4 of the resolution order"
            ));
            continue;
        }
        match resolved.get(name).cloned().flatten() {
            Some(path) => print_safely(&format!(
                "  `{name}` is an external program and resolves to {}",
                path.display()
            )),
            None => print_safely(&format!("  `{name}` resolves to nothing on PATH")),
        }
    }

    if session.mode() == Mode::Config {
        println!("  configuration mode: none of this would be allowed to run");
    }
    Ok(ExitStatus::SUCCESS)
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
