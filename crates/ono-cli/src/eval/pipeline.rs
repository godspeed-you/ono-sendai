//! Pipelines: which stages are native, which are programs, and how the segments meet.
//!
//! A pipeline is cut into segments of one kind each, and the boundary of spec §12.3 between
//! objects and bytes is explicit in both directions. The native segments are run by `native`;
//! everything about assembling and running a child process is here.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use ono_core::{ErrorCode, ExitStatus, Span};
use ono_parser::{
    Argument, Pipeline, RedirectOp, RedirectTarget, Redirection, Stage, StageHead, StageList,
    Statement,
};
use ono_process::{Command, Fd, Redirect};
use ono_value::{ErrorValue, Value};

use crate::builtin;
use crate::expand;
use crate::resolve::{self, Namespace, Resolution};
use crate::session::{Mode, Session};

use super::block::each_block_stage;
use super::expression::{eval_expr, text_of};
use super::function::{call_function, called_function};
use super::materialize::captured_text;
use super::statement::{expand_alias, is_job_kill, prefix_assignments};
use super::{Eval, Flow};

/// Runs a pipeline, honouring `&&`, `||` and a trailing `&`.
pub fn run_pipeline(session: &mut Session, pipeline: &Pipeline, source: &str) -> Eval<ExitStatus> {
    // Spec §11.3: field names are checked against the declared schemas before anything runs, so
    // a typo costs one message instead of one per object.
    super::native::check(session, pipeline, source).map_err(Flow::Failed)?;

    let mut status = run_stage_list(session, &pipeline.head, source, pipeline.background)?;

    for chained in &pipeline.tail {
        let should_run = match chained.op {
            ono_parser::ChainOp::And => status.is_success(),
            ono_parser::ChainOp::Or => !status.is_success(),
        };
        if should_run {
            status = run_stage_list(session, &chained.list, source, pipeline.background)?;
        }
    }
    Ok(status)
}

pub(super) fn run_stage_list(
    session: &mut Session,
    list: &StageList,
    source: &str,
    background: bool,
) -> Eval<ExitStatus> {
    // Step 2 of the resolution order (ADR-0011): a user function wins over everything but a
    // keyword, and the keywords were the parser's.
    if !background
        && let Some(stage) = list.stages.first()
        && let Some(function) = called_function(session, stage)
    {
        return call_function(session, &function, list, source);
    }

    // Spec §54: `NAME=value command …` sets the variable for this pipeline alone (ADR-0071 §2).
    if let Some((assignments, stripped)) = prefix_assignments(session, list, source)? {
        let previous: Vec<(String, Option<OsString>)> = assignments
            .iter()
            .map(|(name, _)| (name.clone(), session.env_var(name).map(OsStr::to_os_string)))
            .collect();
        for (name, value) in &assignments {
            session.set_env(name.as_str(), value.clone());
        }
        let outcome = run_stage_list(session, &stripped, source, background);
        for (name, value) in previous {
            match value {
                Some(value) => session.set_env(name, value),
                None => session.remove_env(&name),
            }
        }
        return outcome;
    }

    // Step 3: an alias is expanded exactly once and the result resolved again from the top
    // (ADR-0011, ADR-0070).
    if let Some((name, expanded)) = expand_alias(session, list, source) {
        let expanded = if background {
            format!("{expanded} &")
        } else {
            expanded
        };
        let parsed = ono_parser::parse(&expanded);
        let pipeline = parsed
            .program()
            .statements
            .first()
            .and_then(Statement::as_pipeline)
            .cloned()
            .ok_or_else(|| {
                Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::ParseSyntax,
                        format!("the expansion of alias `{name}` is not a pipeline"),
                    )
                    .with_help(format!("`{name}` expands to `{expanded}`")),
                )
            })?;
        session.begin_expanding(name);
        let outcome = run_pipeline(session, &pipeline, &expanded);
        session.finish_expanding();
        return outcome;
    }

    // `kill %N` names a job, and a job is the shell's (spec §18.1, §18.4; ADR-0071 §4). Any
    // other `kill` is the program or the native verb, untouched.
    if list.stages.len() == 1
        && let Some(stage) = list.stages.first()
        && is_job_kill(stage)
    {
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("kill")));
        }
        let arguments = stage_arguments(session, stage, source)?;
        return builtin::kill_jobs(session, &arguments);
    }

    // `resolve command`, `get config` and `set config` are answered by the shell, which alone
    // sees every stage of the order and every configuration layer (ADR-0011, ADR-0093,
    // ADR-0094). Their values seed whatever follows, as a producer's stream would.
    if !background
        && let Some(stage) = list.stages.first()
        && let Some(request) = crate::meta::claims(stage)
    {
        let alone = list.stages.len() == 1;
        // A configuration file may set a value and nothing more: `get config` in one would
        // print, and a `set config` with stages after it would run them (ADR-0010).
        if session.mode() == Mode::Config && !(request == crate::meta::Request::SetConfig && alone)
        {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        let values = crate::meta::answer(session, stage, source, request)?;
        // `set config` on its own is as quiet as `set env`: a settings line prints nothing at
        // the prompt or in a file. Its ActionResult flows when something consumes it.
        if request == crate::meta::Request::SetConfig && alone && !session.capturing() {
            return Ok(ExitStatus::SUCCESS);
        }
        return super::native::run_seeded(session, list, source, values);
    }

    // The link definitions of spec §21 are the session's too (ADR-0104): `add`, `set`, `rename`,
    // `remove` and `detach link` change the link table and the frame stack, and their
    // ActionResult seeds whatever follows.
    if !background
        && let Some(stage) = list.stages.first()
        && let Some(request) = crate::remote::claims(stage)
    {
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        let values = crate::remote::answer(session, stage, source, request)?;
        return super::native::run_seeded(session, list, source, values);
    }

    // A single builtin stage runs in the shell itself: `cd` in a child moves a directory nobody
    // is standing in.
    if list.stages.len() == 1
        && let Some(stage) = list.stages.first()
        && let Some(name) = builtin_name(session, stage)
    {
        // The configuration check comes before the command runs, and covers builtins as well as
        // external programs. Only the declarative ones are allowed: ADR-0010 says configuration
        // "sets values, defines functions and aliases", and a check that stopped `touch` while
        // letting `cd`, `exit` and `jobs` through would be a claim the code did not keep.
        if session.mode() == Mode::Config && !builtin::allowed_in_config(name) {
            return Err(Flow::Failed(config_refusal(name)));
        }
        // A builtin writes through the shell's own output, so a redirection has to be applied
        // here rather than by a child that does not exist. Silently ignoring it would send the
        // output to the terminal while the user was told it went to a file.
        if let Some(redirection) = stage.redirections.first() {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{name}` runs in the shell itself and cannot be redirected"),
                )
                .with_help(format!(
                    "`{name}` has no child process to redirect. Send it through a command that \
                     does: `{name} | to text > file`. The redirection at {} was not applied.",
                    redirection.span
                )),
            ));
        }

        let arguments = stage_arguments(session, stage, source)?;
        return builtin::run(session, name, &arguments);
    }

    // `explain` in front of a pipeline explains the whole pipeline, exactly as spec §11.3
    // spells it: `explain get process | where cpu > 20 | stop process`. The pipes belong to the
    // subject, so the subject is the source text from explain's first word to the end of the
    // list, handed over verbatim — never re-rendered from the AST, which would explain a
    // normalisation of what the user typed rather than what they typed.
    if list.stages.len() > 1
        && let Some(first) = list.stages.first()
        && builtin_name(session, first) == Some("explain")
        && let Some(end) = list.stages.last().map(|stage| stage.span.end())
    {
        let start = first
            .arguments
            .first()
            .map_or(first.span.end(), |argument| argument.span().start());
        let subject = source
            .get(start as usize..end as usize)
            .unwrap_or_default()
            .trim();
        return builtin::run(session, "explain", &[OsString::from(subject)]);
    }

    // A builtin in a longer pipeline used to be handed to `exec`, which reported it as not found
    // and then reported the pipeline as successful. Where the name also exists as a program —
    // `true`, `false` — the program is what runs, which is what every other shell does and what
    // keeps `false | true` meaningful. Where it does not, saying so plainly is the least
    // surprising answer: `cd` changes the shell, so there is no process for a pipe to attach to.
    for stage in &list.stages {
        if let Some(name) = builtin_name(session, stage)
            && stage
                .head
                .name()
                .and_then(|head| resolve::find_on_path(session, head))
                .is_none()
        {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{name}` runs in the shell itself and cannot be a pipeline stage"),
                )
                .with_help(
                    "the shell's own commands change the shell, so there is no process for a pipe \
                     to attach to",
                ),
            ));
        }
    }

    // `… | enter socket`: the object to enter arrives through the pipeline (spec §14.3). The
    // stages before it run as the native pipeline they are, with their result kept for the
    // frame instead of shown, and the last stage pushes the frame (ADR-0075).
    if list.stages.len() > 1
        && let Some(last) = list.stages.last()
        && matches!(
            crate::context::claims(last),
            Some(crate::context::Request::Enter)
        )
    {
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        let head = &list.stages[..list.stages.len() - 1];
        let prefix = StageList {
            stages: head.to_vec(),
            span: Span::new(
                list.span.start(),
                head.last()
                    .map_or(list.span.end(), |stage| stage.span.end()),
            ),
        };
        let values = super::native::run_collecting(session, &prefix, source)?;
        return crate::context::enter_piped(session, last, source, &values);
    }

    // `get link | remove link`, `get plugin | verify plugin`: a shell-answered command reached
    // through a pipe (ADR-0118). The stages before it run as the native pipeline they are, their
    // values are the command's targets, and what it produces seeds the stages after it — exactly
    // as the head form does. A command whose contract declares no stream input is refused before
    // anything runs, with the head form named.
    if !background && let Some((index, piped)) = crate::piped::claims(list) {
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        return crate::piped::run(session, list, source, index, piped);
    }

    // `enter` and `leave` change what later commands mean, which is session state: the same
    // reason `cd` runs in the shell (spec §14.1, ADR-0023).
    if list.stages.len() == 1
        && let Some(stage) = list.stages.first()
        && let Some(request) = crate::context::claims(stage)
    {
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        return match request {
            crate::context::Request::Enter => crate::context::enter(session, stage, source),
            crate::context::Request::Leave => crate::context::leave(session, stage, source),
            crate::context::Request::Link => crate::context::link(session, stage, source),
            crate::context::Request::LoadPlugin => {
                let words: Vec<String> = stage_arguments(session, stage, source)?
                    .iter()
                    .map(|word| word.to_string_lossy().into_owned())
                    .collect();
                let (id, options) = crate::plugins::LoadOptions::from_words(&words);
                match id {
                    Some(id) => crate::plugins::load_plugin_with(session, &id, &options),
                    None => Err(Flow::Failed(
                        ErrorValue::new(
                            ErrorCode::ResolveTargetNotFound,
                            "`load plugin` needs the package to load",
                        )
                        .with_help("`get plugin` lists the installed set (spec §31.8)"),
                    )),
                }
            }
        };
    }

    // `each { … }` with a block runs in the shell: a block holds statements, and a statement may
    // run a command, which the transform engine cannot (spec §19.4, ADR-0071 §1). Since v0.4.1 it
    // runs *as a stage of its own pipeline* rather than in front of one — the native path below
    // assembles it and drives it item by item (§25.1, ADR-0480) — so all that is left here is the
    // one shape that has no pipeline to be a stage of.
    if !background && each_block_stage(list) == Some(0) {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                "`each` needs a stream of values to run its block over, and none reaches it",
            )
            .with_help("put a producer in front of it: `get service | where … | each { … }`"),
        ));
    }

    // A KUANG/11 management command that must both show its record and decide the run's status
    // — `verify plugin` — runs in the shell and seeds the pipeline after it (ADR-0108 §2).
    if !background
        && let Some(stage) = list.stages.first()
        && let Some(request) = crate::plugins::claims(stage)
    {
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        let words: Vec<String> = stage_arguments(session, stage, source)?
            .iter()
            .map(|word| word.to_string_lossy().into_owned())
            .collect();
        let produced = crate::plugins::run(session, request, &words)?;
        let status = super::native::run_seeded(session, list, source, produced.values)?;
        if let Some(failure) = produced.failure {
            crate::report::Reporter::new(ono_render::Presentation::choose(
                std::io::IsTerminal::is_terminal(&std::io::stderr()),
                &[],
            ))
            .error(&failure);
            return Ok(ExitStatus::FAILURE);
        }
        return Ok(status);
    }

    // A `<package>:command` head invokes a loaded KUANG/11 package's contribution (spec §31.22,
    // ADR-0011's module namespace). The values it streams seed the rest of the pipeline exactly
    // as a native producer's would.
    if !background
        && let Some(stage) = list.stages.first()
        && let StageHead::Command(name) = &stage.head
        && let Some(namespace) = name.namespace.as_deref()
        && Namespace::from_prefix(Some(namespace)).is_none()
        && crate::plugins::loaded_package(session, namespace).is_some()
    {
        let words = stage_arguments(session, stage, source)?;
        let command = name.name.clone();
        let namespace = namespace.to_owned();
        let values = crate::plugins::invoke(session, &namespace, &command, &words)?;
        return super::native::run_seeded(session, list, source, values);
    }

    // Spec §31.68: a command an installed package *declared* is in the registry before any of
    // that package's code has run, and "invoking the command triggers policy negotiation and
    // load". Both spellings reach it — the bare `get echo-item` the registry resolves, and the
    // qualified `echo:emit` of §31.66 — and neither starts anything until it is invoked. A
    // package that declares nothing has no placeholder to invoke, so `<package>:command` is
    // still a refusal there: the shell does not start a package to discover whether a name
    // exists, which is the cost lazy loading exists to avoid.
    if !background
        && let Some(stage) = list.stages.first()
        && let StageHead::Command(name) = &stage.head
    {
        let contributed = match name.namespace.as_deref() {
            Some(namespace) if Namespace::from_prefix(Some(namespace)).is_none() => {
                crate::plugins::contributed_by_namespace(namespace, &name.name)
            }
            Some(_) => None,
            None => crate::plugins::contributed_command(stage),
        };
        if let Some(contract) = contributed {
            let mut words = stage_arguments(session, stage, source)?;
            // The target word is how the registry resolved the command, not an argument to it.
            if name.namespace.is_none()
                && contract.target().is_some_and(|target| {
                    stage
                        .arguments
                        .first()
                        .and_then(ono_parser::Argument::as_word)
                        == Some(target)
                })
                && !words.is_empty()
            {
                words.remove(0);
            }
            let values = crate::plugins::invoke_contributed(session, contract, &words)?;
            return super::native::run_seeded(session, list, source, values);
        }
    }

    // A pipeline may start with a value instead of a command: `$hot | where …`, `@-1 | count`
    // (spec §10.2, §20.2). The head is evaluated once and a list splices, because a list *is*
    // several values (ADR-0019); everything after it runs as if a producer had streamed them.
    if !background
        && let Some(stage) = list.stages.first()
        && let StageHead::Value(expression) = &stage.head
    {
        let expression = expression.clone();
        let value = eval_expr(session, &expression, source)?;
        let seed = match value {
            Value::List(items) => items.to_vec(),
            other => vec![other],
        };
        return super::native::run_seeded(session, list, source, seed);
    }

    // A pipeline with a native command in it runs through the object pipeline of spec §5, which
    // threads bytes across the boundary of spec §12.3 where a child process sits on one side.
    // So does an all-external pipeline whose last stage an adapter renders at the terminal
    // (spec v0.3 §1.4), which is what makes `lsblk` typed at the prompt a table.
    if super::native::claims(session, list)
        || (!background && !session.capturing() && super::native::adapts_at_terminal(session, list))
    {
        // A native command is as much "running something" as a child process is: `set file`
        // reaches the registry now (ADR-0068), and a configuration file that could change a
        // file's mode would be a startup script wearing a settings file's name (ADR-0010).
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        if background {
            // Spec §18.4: a backgrounded native pipeline is a job — listed, addressable,
            // stoppable — never a hidden thread (ADR-0024).
            return super::native::run_background(session, list, source);
        }
        return super::native::run(session, list, source);
    }

    if session.mode() == Mode::Config {
        return Err(Flow::Failed(config_refusal("this command")));
    }

    // A pipeline being captured hands its stdout to the capture rather than the terminal: the
    // text is the value of `(echo hi)` (ADR-0069).
    if !background && session.capturing() {
        let indices: Vec<usize> = (0..list.stages.len()).collect();
        let (_, status) = run_external_segment(session, list, &indices, source, None, true)?;
        return Ok(status);
    }

    let mut built = ono_process::Pipeline::new();
    for stage in &list.stages {
        built = built.stage(build_command(session, stage, source)?);
    }

    if background {
        let id = session
            .executor()
            .run_background(&built)
            .map_err(process_error)?;
        session.note_job_started(id.number());
        eprintln!("[{id}]");
        return Ok(ExitStatus::SUCCESS);
    }

    let outcome = session
        .executor()
        .run_foreground(&built)
        .map_err(process_error)?;

    // A stage that could not be started reports both a status and a structured reason. The
    // reason is what the user needs — "no such file" beats "exited 1" — so it is raised rather
    // than left on the outcome for nobody to read.
    if let ono_process::ForegroundOutcome::Completed(completed) = &outcome
        && let Some(failure) = completed.failure()
    {
        return Err(Flow::FailedWith(
            ErrorValue::new(failure.code(), failure.message().to_owned()),
            outcome.status(),
        ));
    }
    Ok(outcome.status())
}

/// Runs a run of adjacent external stages, threading bytes into and out of it.
///
/// Adjacent child processes are joined to each other by real pipes, exactly as before: ADR-0013
/// keeps `yes | head -1` a genuine `SIGPIPE` rather than a buffer the shell drains. Bytes only
/// pass through the shell where a native stage sits on one side of the boundary.
///
/// # Errors
///
/// The structured error of whichever stage could not be built or started.
pub fn run_external_segment(
    session: &mut Session,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    last: bool,
) -> Eval<(Option<Vec<u8>>, ExitStatus)> {
    if session.mode() == Mode::Config {
        return Err(Flow::Failed(config_refusal("this command")));
    }

    let captured = last && session.capturing();
    let capture = !last || captured;
    let mut built = ono_process::Pipeline::new();
    for (position, index) in indices.iter().enumerate() {
        let mut command = build_command(session, &list.stages[*index], source)?;
        if position == 0
            && let Some(bytes) = input.clone()
        {
            command = command.stdin(ono_process::Input::Bytes(bytes));
        }
        if position + 1 == indices.len() && capture {
            command = command.stdout(ono_process::Output::Capture);
        }
        built = built.stage(command);
    }

    let outcome = session
        .executor()
        .run_foreground(&built)
        .map_err(process_error)?;

    if let ono_process::ForegroundOutcome::Completed(completed) = &outcome
        && let Some(failure) = completed.failure()
    {
        return Err(Flow::FailedWith(
            ErrorValue::new(failure.code(), failure.message().to_owned()),
            outcome.status(),
        ));
    }

    let bytes = capture.then(|| {
        outcome
            .completed()
            .and_then(|completed| completed.stages().last())
            .map(|stage| stage.stdout.clone())
            .unwrap_or_default()
    });
    if captured {
        session.capture(&[captured_text(bytes.as_deref().unwrap_or_default())])?;
        return Ok((None, outcome.status()));
    }
    Ok((bytes, outcome.status()))
}

/// Runs an external segment whose last stage is adapted: the same pipeline, with the last
/// command replaced by the adapter's plan and its stdout captured for decoding (ADR-0057).
///
/// # Errors
///
/// As [`run_external_segment`]: the child's own failure, with its status (spec v0.3 §1.20).
pub fn run_adapted_segment(
    session: &mut Session,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    plan: &ono_adapter::AdapterPlan,
) -> Eval<(Vec<u8>, ExitStatus)> {
    let built = adapted_pipeline(
        session,
        list,
        indices,
        source,
        input,
        plan,
        ono_process::Output::Capture,
    )?;
    let outcome = session
        .executor()
        .run_foreground(&built)
        .map_err(process_error)?;
    if let ono_process::ForegroundOutcome::Completed(completed) = &outcome
        && let Some(failure) = completed.failure()
    {
        return Err(Flow::FailedWith(
            ErrorValue::new(failure.code(), failure.message().to_owned()),
            outcome.status(),
        ));
    }
    let bytes = outcome
        .completed()
        .and_then(|completed| completed.stages().last())
        .map(|stage| stage.stdout.clone())
        .unwrap_or_default();
    Ok((bytes, outcome.status()))
}

/// Starts an adapted segment whose records are decoded while it runs (ADR-0059): the last
/// stage's stdout is handed back as a pipe, the terminal stays with the shell.
///
/// # Errors
///
/// A stage that could not be started, with its status.
pub fn start_adapted_segment(
    session: &mut Session,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    plan: &ono_adapter::AdapterPlan,
) -> Eval<ono_process::Foreground> {
    let built = adapted_pipeline(
        session,
        list,
        indices,
        source,
        input,
        plan,
        ono_process::Output::Pipe,
    )?;
    let started = session
        .executor()
        .start_piped(&built)
        .map_err(process_error)?;
    if let Some(failure) = started.failure() {
        let error = ErrorValue::new(failure.code(), failure.message().to_owned());
        let outcome = session
            .executor()
            .finish_foreground(started)
            .map_err(process_error)?;
        return Err(Flow::FailedWith(error, outcome.status()));
    }
    Ok(started)
}

pub(super) fn adapted_pipeline(
    session: &mut Session,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    plan: &ono_adapter::AdapterPlan,
    stdout: ono_process::Output,
) -> Eval<ono_process::Pipeline> {
    if session.mode() == Mode::Config {
        return Err(Flow::Failed(config_refusal("this command")));
    }
    let mut built = ono_process::Pipeline::new();
    for (position, index) in indices.iter().enumerate() {
        let stage = &list.stages[*index];
        let adapted = position + 1 == indices.len();
        let mut command = if adapted {
            adapted_command(session, stage, plan, source)?
        } else {
            build_command(session, stage, source)?
        };
        if position == 0 {
            if let Some(bytes) = input.clone() {
                command = command.stdin(ono_process::Input::Bytes(bytes));
            } else if adapted && plan.stdin() == ono_adapter::StdinMode::Null {
                command = command.stdin(ono_process::Input::Null);
            }
        }
        if adapted {
            command = command.stdout(stdout);
        }
        built = built.stage(command);
    }
    Ok(built)
}

/// The command an adapter's plan describes: the pinned executable, the plan's argv, the plan's
/// environment on top of the session's, and the stage's own redirections (spec v0.3 §1.7).
pub(super) fn adapted_command(
    session: &mut Session,
    stage: &Stage,
    plan: &ono_adapter::AdapterPlan,
    source: &str,
) -> Eval<Command> {
    let mut command = Command::new(plan.executable().as_os_str())
        .args(plan.argv().iter().skip(1).map(OsString::from))
        .current_dir(session.cwd())
        .env_clear();
    for (name, value) in session.env() {
        command = command.env(name, value);
    }
    for (name, value) in plan.env() {
        command = command.env(OsString::from(name), OsString::from(value));
    }
    for redirection in &stage.redirections {
        command = command.redirect(build_redirect(session, redirection, source)?);
    }
    Ok(command)
}

/// Opens the file a stage's redirections send its output to, if any.
///
/// A native stage writes through the shell rather than through a child, so its redirection has to
/// be applied here. Only the output forms are meaningful: a native producer reads no bytes.
///
/// # Errors
///
/// The structured error of a redirection that cannot be understood or a file that cannot be
/// opened.
pub fn output_destination(
    session: &mut Session,
    stage: &Stage,
    source: &str,
) -> Eval<Option<std::fs::File>> {
    let Some(redirection) = stage.redirections.last() else {
        return Ok(None);
    };
    // The same reading of a redirection a child process gets, so `> f` means one thing in the
    // shell however the stage on its left is run.
    let (path, append) = match build_redirect(session, redirection, source)? {
        Redirect::Write { path, .. } => (path, false),
        Redirect::Append { path, .. } => (path, true),
        Redirect::Read { .. } | Redirect::Duplicate { .. } => {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "a native command's output can only be sent to a file",
                )
                .with_help("send it through `to json` into a program to redirect it any other way"),
            ));
        }
    };

    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    options.open(&path).map(Some).map_err(|error| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoNotFound,
            format!("cannot open {}: {error}", path.display()),
        ))
    })
}

/// A redirection with no descriptor written means the obvious one: 0 for input, 1 for output.
pub(super) fn descriptor_for(redirection: &Redirection) -> u16 {
    match redirection.fd {
        Some(written) => narrow_fd(written),
        None => match redirection.op {
            RedirectOp::Read | RedirectOp::DupRead => 0,
            _ => 1,
        },
    }
}

/// Descriptor numbers above the process limit cannot exist; saturating keeps a nonsensical one
/// from wrapping into a real descriptor.
pub(super) fn narrow_fd(number: u32) -> u16 {
    u16::try_from(number).unwrap_or(u16::MAX)
}

/// Whether `name` spells one of the value model's type names.
pub(super) fn is_type_name(name: &str) -> bool {
    matches!(
        name,
        "null"
            | "bool"
            | "int"
            | "float"
            | "decimal"
            | "string"
            | "bytes"
            | "path"
            | "timestamp"
            | "duration"
            | "bytesize"
            | "percent"
            | "regex"
            | "uuid"
            | "ip"
            | "ipnetwork"
            | "port"
            | "list"
            | "map"
            | "record"
            | "error"
    )
}

/// Why a configuration file may not run `what` (ADR-0010).
pub(super) fn config_refusal(what: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::SafetyPolicyDenied,
        format!("a configuration file may not run {what}"),
    )
    .with_help(
        "configuration is declarative: it sets values with `set`, removes them with `remove`, and \
         defines functions and aliases. It runs nothing else — not an external command, and not \
         one of the shell's own (ADR-0010).",
    )
}

pub(super) fn process_error(error: ono_process::Error) -> Flow {
    Flow::Failed(ErrorValue::new(error.code(), error.message().to_owned()))
}

pub(super) fn builtin_name(session: &Session, stage: &Stage) -> Option<&'static str> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    let namespace = Namespace::from_prefix(name.namespace.as_deref())?;
    if matches!(namespace, Namespace::External) {
        return None;
    }
    match resolve::resolve(session, namespace, &name.name) {
        Ok(Resolution::Builtin(builtin)) => resolve::builtin_for(builtin, first_word(stage)),
        _ => None,
    }
}

/// The literal word after the head, which decides whether `set`/`remove` are the shell's.
/// The registry's refusal when `head` is a verb it knows and only the target word is wrong.
///
/// `None` when the registry does not know the verb at all, or when it would answer something
/// else: nothing here invents a reason the registry did not give (ADR-0217).
pub(super) fn registry_target_refusal(head: &str, arguments: &[Argument]) -> Option<ErrorValue> {
    let registry = super::native::registry().ok()?;
    let error = registry.resolve(head, arguments).err()?;
    (error.code() == ErrorCode::ResolveTargetNotFound).then_some(error)
}

pub(super) fn first_word(stage: &Stage) -> Option<&str> {
    stage
        .arguments
        .first()
        .and_then(ono_parser::Argument::as_word)
}

/// Builds the external command one stage describes.
///
/// Everything that reaches here becomes a child process — a stage the shell runs itself has
/// already been handled — so a name that is both a builtin and a program on `PATH` resolves to
/// the program. That is what keeps `false | true` meaningful.
pub(super) fn build_command(session: &mut Session, stage: &Stage, source: &str) -> Eval<Command> {
    let StageHead::Command(name) = &stage.head else {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            "this stage has no command to run",
        )));
    };

    // `raw <program> …` runs the program on PATH and nothing else, with the arguments exactly as
    // typed (spec v0.3 §1.17, ADR-0054). The keyword wins over a program called `raw`, as
    // `explain` does; `exec:raw` reaches such a program.
    if name.namespace.is_none() && name.name == ono_adapter::ADAPT {
        // The native runner claims every `adapt` stage; reaching here means the stage had
        // nothing to adapt — `adapt` alone — or a form the runner does not take (a
        // background job), and a forced adaptation never runs as a plain program.
        let arguments = stage_arguments(session, stage, source)?;
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                if arguments.is_empty() {
                    "`adapt` needs a program to adapt".to_owned()
                } else {
                    "`adapt` cannot run here".to_owned()
                },
            )
            .with_help(
                "`adapt <program> [arguments]` forces the program's output into values \
                 (spec v0.3 §1.18); run it in the foreground",
            ),
        ));
    }
    if name.namespace.is_none() && name.name == ono_adapter::RAW {
        let mut arguments = stage_arguments(session, stage, source)?;
        if arguments.is_empty() {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveCommandNotFound,
                    "`raw` needs a program to run",
                )
                .with_help(
                    "`raw <program> [arguments]` runs the program with nothing between it and \
                     the terminal: no argv rewrite, no decoder, no renderer (spec v0.3 §1.17)",
                ),
            ));
        }
        let program = arguments.remove(0).to_string_lossy().into_owned();
        let resolution =
            resolve::resolve(session, Namespace::External, &program).map_err(|error| {
                Flow::Failed(error.with_help(format!(
                    "`raw` runs programs only; `{program}` is not one on PATH"
                )))
            })?;
        return assemble_command(session, &resolution, arguments, stage, source);
    }

    let namespace = Namespace::from_prefix(name.namespace.as_deref()).ok_or_else(|| {
        Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!(
                    "unknown namespace `{}`",
                    name.namespace.as_deref().unwrap_or_default()
                ),
            )
            .with_help(
                "the namespaces are `ono:`, `exec:`, `fn:` and a loaded package's name \
                 (ADR-0011, spec §31.22)",
            ),
        )
    })?;

    let namespace = if namespace == Namespace::Any
        && resolve::builtin_for(&name.name, first_word(stage)).is_some()
    {
        Namespace::External
    } else {
        namespace
    };

    let resolution = resolve::resolve(session, namespace, &name.name).map_err(|error| {
        // A head word the registry knows as a verb was refused for its target word, not for
        // itself: `trace group root` is `trace` with a target it has no command for. Reporting
        // the verb as missing after the search reached `PATH` hides what was wrong, so the
        // registry's own refusal — which names the target and its near misses — is the answer
        // (spec §15.4, ADR-0217).
        if let Some(refusal) = registry_target_refusal(&name.name, &stage.arguments) {
            return Flow::Failed(refusal);
        }
        let suggestions = resolve::suggestions(session, &name.name);
        let error = if suggestions.is_empty() {
            error
        } else {
            error.with_help(format!("did you mean: {}", suggestions.join(", ")))
        };
        Flow::Failed(error)
    })?;

    let arguments = stage_arguments(session, stage, source)?;
    assemble_command(session, &resolution, arguments, stage, source)
}

pub(super) fn assemble_command(
    session: &mut Session,
    resolution: &resolve::Resolution,
    arguments: Vec<OsString>,
    stage: &Stage,
    source: &str,
) -> Eval<Command> {
    let mut command = Command::new(resolve::program_of(resolution))
        .args(arguments)
        .current_dir(session.cwd())
        .env_clear();
    for (name, value) in session.env() {
        command = command.env(name, value);
    }
    for redirection in &stage.redirections {
        command = command.redirect(build_redirect(session, redirection, source)?);
    }
    Ok(command)
}

pub(super) fn build_redirect(
    session: &mut Session,
    redirection: &Redirection,
    source: &str,
) -> Eval<Redirect> {
    let target = match &redirection.target {
        RedirectTarget::Word(word) => {
            let expanded = expand::expand_to_one(session, &word.text)?;
            PathBuf::from(expanded)
        }
        RedirectTarget::Value(expression) => {
            let value = eval_expr(session, expression, source)?;
            PathBuf::from(OsString::from(text_of(&value)?))
        }
        RedirectTarget::Fd(descriptor) => {
            let fd = descriptor_for(redirection);
            return Ok(Redirect::Duplicate {
                fd: Fd::new(fd),
                from: Fd::new(narrow_fd(*descriptor)),
            });
        }
        RedirectTarget::Error(_) => {
            return Err(Flow::Failed(ErrorValue::new(
                ErrorCode::ParseSyntax,
                "this redirection has no target",
            )));
        }
    };

    let descriptor = descriptor_for(redirection);

    Ok(match redirection.op {
        RedirectOp::Read => Redirect::read_from(Fd::new(descriptor), target),
        RedirectOp::Write => Redirect::write_to(Fd::new(descriptor), target),
        RedirectOp::Append => Redirect::append_to(Fd::new(descriptor), target),
        RedirectOp::DupRead | RedirectOp::DupWrite => {
            return Err(Flow::Failed(ErrorValue::new(
                ErrorCode::ParseSyntax,
                "a duplicating redirection needs a descriptor on its right",
            )));
        }
    })
}

/// A stage's argument text and its words-mode reading, for a stage handed to a program.
pub(super) fn argument_region<'a>(
    stage: &Stage,
    source: &'a str,
) -> Option<(Vec<Argument>, &'a str)> {
    let first = stage.arguments.first()?.span();
    let last = stage.arguments.last()?.span();
    let region = source.get(first.start() as usize..last.end() as usize)?;
    Some((ono_parser::words_arguments(region), region))
}

/// Expands a stage's arguments into the argv an external command receives.
///
/// A list contributes one argument per element, because it *is* several values; nothing else
/// contributes more than one (ADR-0019).
///
/// A stage the parser read in expression mode — because the registry declares a native command
/// of that name — and that resolution then handed to a program is read back as the words the
/// user typed: `printf … | sort -r` is coreutils `sort` with the flag `-r`, not a native sort
/// negating a field called `r` (ADR-0028, ADR-0260).
pub fn stage_arguments(session: &mut Session, stage: &Stage, source: &str) -> Eval<Vec<OsString>> {
    // The whole argument region, re-read in words mode, when the parse mode was decided by a
    // native head the resolution did not choose (ADR-0260). Nothing else is re-read: a
    // words-mode stage was already read as words.
    let rewritten = (stage.mode == ono_parser::ArgMode::Expression)
        .then(|| argument_region(stage, source))
        .flatten();
    let (arguments, source) = match &rewritten {
        Some((arguments, region)) => (arguments.as_slice(), *region),
        None => (stage.arguments.as_slice(), source),
    };
    let mut argv = Vec::new();
    for argument in arguments {
        match argument {
            Argument::Word(word) => argv.extend(expand::expand_word(session, &word.text)?),
            Argument::Option(option) => match &option.value {
                Some(value) => {
                    let text = text_of(&eval_expr(session, value, source)?)?;
                    argv.push(OsString::from(format!("--{}={text}", option.name)));
                }
                None => argv.push(OsString::from(format!("--{}", option.name))),
            },
            Argument::Value(expression) => {
                let value = eval_expr(session, expression, source)?;
                match value {
                    Value::List(items) => {
                        for item in items.iter() {
                            argv.push(OsString::from(text_of(item)?));
                        }
                    }
                    single => argv.push(OsString::from(text_of(&single)?)),
                }
            }
            Argument::Error(_) => {
                return Err(Flow::Failed(ErrorValue::new(
                    ErrorCode::ParseSyntax,
                    "this argument could not be read",
                )));
            }
        }
    }
    Ok(argv)
}
