//! `enter` and `leave`: the context stack of spec §14, run by the shell itself.
//!
//! Both change what later commands mean, which is session state no child process and no command
//! table can hold — the same reason `cd` is a builtin. The commands they implement are still the
//! registry's (`ono.service.enter`, `ono.context.leave`): the contract supplies help, completion
//! and typing, and this module supplies the effect.

use ono_command::ContextFrame;
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::{Stage, StageHead};
use ono_provider_api::{Query, Selector};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::{Session, ShellFrame};

/// What a stage asks the context stack to do.
pub enum Request {
    /// `enter <target> <identity>` — push a frame.
    Enter,
    /// `leave` — pop one, or with `--all` pop everything.
    Leave,
}

/// Whether `stage` is a context command this module runs.
///
/// The decision is by head word alone: `enter` and `leave` always mean the stack, exactly as
/// `cd` always means the shell. A program named `enter` on `PATH` stays reachable as
/// `exec:enter` (ADR-0011).
#[must_use]
pub fn claims(stage: &Stage) -> Option<Request> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    if name.namespace.is_some() {
        return None;
    }
    match name.name.as_str() {
        "enter" => Some(Request::Enter),
        "leave" => Some(Request::Leave),
        _ => None,
    }
}

/// Runs `enter`, validating that the entered object exists before anything narrows to it.
///
/// # Errors
///
/// A structured error when the target is unknown, the object does not exist, or the provider
/// that would know cannot answer.
pub fn enter(session: &mut Session, stage: &Stage, source: &str) -> Eval<ExitStatus> {
    let words = crate::eval::stage_arguments(session, stage, source)?;
    let mut words = words.iter().map(|word| word.to_string_lossy());
    let Some(target) = words.next() else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                "`enter` needs a target: what should be entered?",
            )
            .with_help("`enter dir /etc`, `enter service nginx` (spec §14.1)"),
        ));
    };
    let identity = words.next().map(|word| word.into_owned());

    match (target.as_ref(), identity) {
        ("dir", Some(path)) => enter_directory(session, &path),
        ("dir", None) => Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            "`enter dir` needs a directory",
        ))),
        (target, Some(identity)) => enter_object(session, target, identity),
        (target, None) => Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`enter {target}` needs the identity of the object to enter"),
            )
            .with_help(format!("`enter {target} <name>` (spec §14.3)")),
        )),
    }
}

/// Spec §14.2: equivalent in effect to changing the working directory, with the stack's model —
/// so `leave` restores where the session stood, which plain `cd` never promised.
fn enter_directory(session: &mut Session, path: &str) -> Eval<ExitStatus> {
    let destination = session.cwd().join(path);
    let destination = destination.canonicalize().map_err(|error| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoNotFound,
            format!("cannot enter {}: {error}", destination.display()),
        ))
    })?;
    if !destination.is_dir() {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::IoNotDirectory,
            format!("{} is not a directory", destination.display()),
        )));
    }

    let previous = session.cwd().to_path_buf();
    let frame = ContextFrame::filesystem(Value::Path(destination.clone().into()));
    session.set_cwd(destination);
    session.push_frame(ShellFrame {
        frame,
        restore_cwd: Some(previous),
    });
    Ok(ExitStatus::SUCCESS)
}

/// Spec §14.3: entering an object is a statement about a real object, so the object is resolved
/// first — a frame that narrowed every later query to nothing would be worse than an error now.
fn enter_object(session: &mut Session, target: &str, identity: String) -> Eval<ExitStatus> {
    let registry = crate::native::registry().map_err(Flow::Failed)?;
    let contract = registry.find("enter", Some(target)).ok_or_else(|| {
        Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("nothing declared can be entered as `{target}`"),
            )
            .with_help("`help enter` lists the targets that carry a context"),
        )
    })?;
    if contract.stability() != ono_command::Stability::Stable {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!(
                    "`enter {target}` is declared but not delivered ({})",
                    contract.stability()
                ),
            )
            .with_help("spec §52 asks for its usefulness to be validated first"),
        ));
    }

    let query = Query::target(target).with(Selector::field("name", Value::string(&identity)));
    let (runtime, providers) = session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the runtime",
        ))
    })?;
    // The snapshot spawns its producer, so it must be created inside the runtime it runs on.
    let collected = runtime
        .block_on(async { Ok::<_, ErrorValue>(providers.snapshot(&query)?.collect().await) })
        .map_err(Flow::Failed)?;

    let Some(found) = collected.values().first() else {
        if let Some(failure) = collected.errors().first() {
            return Err(Flow::Failed(failure.clone()));
        }
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("no {target} answers to `{identity}`"),
            )
            .with_help(format!("`get {target}` shows what exists")),
        ));
    };

    // The identity kept is the one the object itself reports — `nginx` normalises to
    // `nginx.service` — so the prompt and the implicit selector agree with the provider.
    let identity = found
        .as_record()
        .ok()
        .and_then(|record| record.get("name").cloned())
        .unwrap_or_else(|| Value::string(&identity));
    session.push_frame(ShellFrame {
        frame: ContextFrame::new(target, identity),
        restore_cwd: None,
    });
    Ok(ExitStatus::SUCCESS)
}

/// Runs `leave` (spec §14.1: pops a frame; ADR-0023: the ground cannot be left).
///
/// # Errors
///
/// Structured errors only for arguments that cannot be read; an empty stack is a diagnostic,
/// never a failure.
pub fn leave(session: &mut Session, stage: &Stage, source: &str) -> Eval<ExitStatus> {
    let words = crate::eval::stage_arguments(session, stage, source)?;
    let all = words.iter().any(|word| word == "--all");

    if session.frames().is_empty() {
        // ADR-0023: a stack that can be popped past its base is a stack that will be.
        eprintln!("ono: nothing to leave: the session stands on its ground context");
        return Ok(ExitStatus::SUCCESS);
    }

    loop {
        let Some(popped) = session.pop_frame() else {
            break;
        };
        if let Some(previous) = popped.restore_cwd {
            session.set_cwd(previous);
        }
        if !all {
            break;
        }
    }
    Ok(ExitStatus::SUCCESS)
}
