//! `enter` and `leave`: the context stack of spec §14, run by the shell itself.
//!
//! Both change what later commands mean, which is session state no child process and no command
//! table can hold — the same reason `cd` is a builtin. The commands they implement are still the
//! registry's (`ono.service.enter`, `ono.context.leave`): the contract supplies help, completion
//! and typing, and this module supplies the effect.

use ono_command::ContextFrame;
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::{Stage, StageHead};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::{Session, ShellFrame};

/// What a stage asks the context stack to do.
pub enum Request {
    /// `enter <target> <identity>` — push a frame.
    Enter,
    /// `leave` — pop one, or with `--all` pop everything.
    Leave,
    /// `link host <name>` — create a remote link (spec §21.1).
    Link,
    /// `load plugin <id>` — negotiate and instantiate (spec §31.10).
    LoadPlugin,
}

/// Whether `stage` is a context command this module runs.
///
/// `leave` always means the stack, exactly as `cd` always means the shell. A program named
/// `enter` on `PATH` stays reachable as `exec:enter` (ADR-0011).
///
/// `enter` is shared. The v0.2 form names a target — `enter dir /etc`, `enter service nginx` —
/// and pushes a context frame; the v0.4 form names a place — `enter compute`, `enter processes`,
/// `enter 1842` — and moves the session through the spatial geography (§6.3). A first word that
/// the registry declares as an `enter <target>` is the first; anything else is the second, which
/// is what lets both spellings keep their meaning without a second vocabulary (ADR-0142).
#[must_use]
pub fn claims(stage: &Stage) -> Option<Request> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    if name.namespace.is_some() {
        return None;
    }
    match name.name.as_str() {
        "enter" if enters_a_declared_target(stage) => Some(Request::Enter),
        // v0.4 §30.2: entering a directory place changes the working directory too, and only the
        // shell owns that. A word that can only be a path is therefore the shell's; a bare name
        // stays a place selector, because §27.1 resolves a visible child before anything else and
        // a file in the working directory must not shadow a canonical domain.
        "enter" if enters_a_path(stage) => Some(Request::Enter),
        "enter" => None,
        "leave" => Some(Request::Leave),
        "link" => Some(Request::Link),
        "load"
            if stage
                .arguments
                .first()
                .and_then(ono_parser::Argument::as_word)
                == Some("plugin") =>
        {
            Some(Request::LoadPlugin)
        }
        _ => None,
    }
}

/// Whether the stage's first word is unmistakably a filesystem path (§30.2).
fn enters_a_path(stage: &Stage) -> bool {
    stage
        .arguments
        .first()
        .and_then(ono_parser::Argument::as_word)
        .is_some_and(crate::spatial::storage::looks_like_a_path)
}

/// Whether the stage's first word names a target `docs/spec/commands/` declares for `enter`.
fn enters_a_declared_target(stage: &Stage) -> bool {
    let Some(word) = stage
        .arguments
        .first()
        .and_then(ono_parser::Argument::as_word)
    else {
        return false;
    };
    crate::native::registry().is_ok_and(|registry| registry.find("enter", Some(word)).is_some())
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

    if crate::spatial::storage::looks_like_a_path(target.as_ref()) {
        return enter_place_at(session, target.as_ref());
    }
    match (target.as_ref(), identity) {
        ("dir", Some(path)) => enter_directory(session, &path),
        ("dir", None) => Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            "`enter dir` needs a directory",
        ))),
        ("link", Some(name)) => enter_link(session, name),
        (target, _) => enter_object(session, stage, target),
    }
}

/// v0.4 §30.2: `enter <path>` moves the spatial place onto the filesystem object, and — only
/// when that object is a directory — the working directory with it.
///
/// §53 settles the sharp case: "Entering a directory changes cwd; entering other object types
/// does not." A file has a path and is not a directory, so entering one leaves `cd` alone.
fn enter_place_at(session: &mut Session, word: &str) -> Eval<ExitStatus> {
    let path = crate::spatial::storage::absolute(session, word);
    let (_, is_directory) =
        crate::spatial::storage::enter_path(session, &path).map_err(Flow::Failed)?;
    if is_directory {
        let resolved = std::fs::canonicalize(&path).unwrap_or(path);
        session.set_env("PWD", resolved.as_os_str());
        session.set_cwd(resolved);
    }
    Ok(ExitStatus::SUCCESS)
}

/// Spec §14.2: equivalent in effect to changing the working directory, with the stack's model —
/// so `leave` restores where the session stood, which plain `cd` never promised.
fn enter_directory(session: &mut Session, path: &str) -> Eval<ExitStatus> {
    let destination = session.cwd().join(path);
    let destination = destination.canonicalize().map_err(|error| {
        Flow::Failed(ErrorValue::new(
            // ADR-0191: a navigation that cannot resolve its place is `spatial.not_found`,
            // which is already what the bare `enter <path>` spelling answers.
            ErrorCode::SpatialNotFound,
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

/// Spec §14.4: entering a link makes it decide where provider calls run. The link must already
/// be held — entering is navigation, not connection.
fn enter_link(session: &mut Session, name: String) -> Eval<ExitStatus> {
    if session.link_registry(&name).is_none() {
        return Err(Flow::Failed(
            ErrorValue::new(
                // A link is a place in the §19.1 link map, so a link that is not there is a
                // destination that is not there (ADR-0191).
                ErrorCode::SpatialNotFound,
                format!("this session holds no link named `{name}`"),
            )
            .with_help(format!("`link host {name}` creates one (spec §21.1)")),
        ));
    }
    // v0.4 §19.1: entering a link is following it again, so its places stop being `stale`.
    crate::spatial::links::attach(&name);
    session.push_frame(ShellFrame {
        frame: ContextFrame::link(Value::string(&name)),
        restore_cwd: None,
    });
    Ok(ExitStatus::SUCCESS)
}

/// Spec §14.3: entering an object is a statement about a real object, so the object is resolved
/// first — a frame that narrowed every later query to nothing would be worse than an error now.
///
/// The object is named the way the `enter <target>` contract declares — `pid` for a process,
/// `path` for a file, `target` for a mount — and the provider that serves the target answers
/// (ADR-0075).
fn enter_object(session: &mut Session, stage: &Stage, target: &str) -> Eval<ExitStatus> {
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
    let resolved = registry
        .resolve("enter", &stage.arguments)
        .map_err(Flow::Failed)?;
    let bound = contract.bind(resolved.arguments).map_err(Flow::Failed)?;
    if bound.selectors().is_empty() {
        let help = match contract.selectors().first() {
            Some(selector) => format!(
                "`enter {target} <{}>` (spec §14.3), or pipe the object in: `get {target} … | \
                 enter {target}`",
                selector.name()
            ),
            None => format!("pipe the object in: `get {target} … | enter {target}` (spec §14.3)"),
        };
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`enter {target}` needs the identity of the object to enter"),
            )
            .with_help(help),
        ));
    }
    let asked = bound
        .selectors()
        .iter()
        .filter_map(|(name, binding)| binding.value().map(|value| format!("{name} {value}")))
        .collect::<Vec<_>>()
        .join(" ");

    let query = contract.query(&bound).map_err(Flow::Failed)?;
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

    let found = collected
        .values()
        .iter()
        .find_map(|value| value.as_record().ok().cloned());
    let Some(record) = found else {
        // A provider that could not read is a different answer from one that read and found
        // nothing (§35.2), and only the second is a statement about the place (§40).
        if let Some(failure) = collected.errors().iter().find(|error| !is_absence(error)) {
            return Err(Flow::Failed(failure.clone()));
        }
        return Err(Flow::Failed(missing_destination(
            session, target, &bound, &asked,
        )));
    };
    enter_record(session, target, &record)
}

/// Whether an error means the object is not there, rather than that it could not be read.
///
/// v0.4 §35.2 keeps the two apart and §40 gives the first its own name. `/proc/1842/stat: No
/// such file or directory` is what a process exiting looks like from outside — it is the answer,
/// not a failure to obtain one.
fn is_absence(error: &ErrorValue) -> bool {
    matches!(
        error.code(),
        ErrorCode::IoNotFound | ErrorCode::ResolveTargetNotFound | ErrorCode::SpatialNotFound
    )
}

/// The refusal for a movement whose destination is not there (v0.4 §40, §10.3, §20.3).
///
/// §40 names two conditions and keeps them apart: `spatial.not_found` for a place that is not
/// there, and `spatial.destination_gone` for one that was and is not — with the actionable next
/// step §40 asks for, in the words its own example uses:
///
/// ```text
/// destination no longer exists
/// process/1842 exited 12s ago
/// ```
///
/// Which of the two it is, is a question only the session can answer: a tombstone exists exactly
/// where this session watched the place go (§10.3).
fn missing_destination(
    session: &mut Session,
    target: &str,
    bound: &ono_command::BoundArguments,
    asked: &str,
) -> ErrorValue {
    let now = jiff::Timestamp::now();
    if let Some((tombstone, key)) = tombstone_for(session, target, bound, now) {
        let age = ono_value::Duration::from_nanoseconds(
            tombstone
                .age(now)
                .total(jiff::Unit::Nanosecond)
                .unwrap_or(0.0)
                .max(0.0) as i128,
        );
        return ErrorValue::new(
            ErrorCode::SpatialDestinationGone,
            format!("destination no longer exists: {target}/{key} ended {age} ago",),
        )
        .with_help(
            "the place is kept as a tombstone for as long as `spatial.tombstone.lifetime` \
             allows; `trail` still carries where it was (spec v0.4 §10.3, §20.3)",
        );
    }
    // A place this session never saw is not a place that went away: it is the second of §40's
    // two conditions. `enter` is navigation under ADR-0142 whichever grammar reached it, so its
    // refusal belongs to the spatial family and not to v0.2 §14.3's `resolve.target_not_found`
    // (ADR-0191).
    ErrorValue::new(
        ErrorCode::SpatialNotFound,
        format!("no {target} answers to `{asked}`"),
    )
    .with_help(format!(
        "`get {target}` shows what exists, and `find place <text>` what this shell can enter \
         (spec v0.4 §6.8, §40)"
    ))
}

/// The tombstone of the place a `enter <target> <identity>` names, where this session knew it.
///
/// The provider has just been asked about this one object and has said it is not there, so this
/// *is* the observation that ends its lifetime (§33.2): a place this session has seen becomes a
/// tombstone here, and a place it never saw stays unknown. That is the whole difference between
/// §40's `spatial.destination_gone` and its `spatial.not_found`.
fn tombstone_for(
    session: &mut Session,
    target: &str,
    bound: &ono_command::BoundArguments,
    now: jiff::Timestamp,
) -> Option<(ono_spatial_core::Tombstone, String)> {
    let key = bound
        .selectors()
        .iter()
        .find_map(|(_, binding)| binding.value())
        .and_then(|value| ono_value::canonical_text(value).ok())?;
    let (runtime, _) = session.pipeline_context()?;
    runtime.block_on(async {
        let mut spatial = crate::spatial::spatial_session().await;
        let known = crate::spatial::relations::types_of_target(target)
            .into_iter()
            .find_map(|object_type| spatial.reference(object_type, &key))?;
        spatial.record_removed(&known, now);
        spatial
            .tombstone_of(&known, now)
            .map(|tombstone| (tombstone.clone(), key.clone()))
    })
}

/// Runs `… | enter <target>`: the object arrives through the pipeline (spec §14.3, ADR-0075).
///
/// # Errors
///
/// `resolve.target_not_found` when nothing arrived, or the first value is not an object of the
/// named target.
pub fn enter_piped(
    session: &mut Session,
    stage: &Stage,
    source: &str,
    values: &[Value],
) -> Eval<ExitStatus> {
    let words = crate::eval::stage_arguments(session, stage, source)?;
    let Some(target) = words
        .first()
        .map(|word| word.to_string_lossy().into_owned())
    else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                "`enter` needs a target: what should be entered?",
            )
            .with_help("`get socket 443 | enter socket` (spec §14.3)"),
        ));
    };
    let registry = crate::native::registry().map_err(Flow::Failed)?;
    if registry.find("enter", Some(&target)).is_none() {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("nothing declared can be entered as `{target}`"),
            )
            .with_help("`help enter` lists the targets that carry a context"),
        ));
    }
    let expected = format!("ono.{target}");
    let record = values
        .iter()
        .find_map(|value| value.as_record().ok().cloned())
        .filter(|record| record.schema_id().name() == expected);
    let Some(record) = record else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("nothing arrived that `enter {target}` could enter"),
            )
            .with_help(format!(
                "pipe one `{expected}/1` object in, as in `get {target} … | take 1 | enter {target}`"
            )),
        ));
    };
    enter_record(session, &target, &record)
}

/// Pushes the frame for an object that exists.
///
/// The identity kept is the one the object itself reports for the handle `enter` takes —
/// `nginx` normalises to `nginx.service` — so the prompt and the implicit selector agree with
/// the provider. A target entered only through the pipeline is shown by its first identity
/// field (ADR-0075).
fn enter_record(
    session: &mut Session,
    target: &str,
    record: &ono_value::RecordValue,
) -> Eval<ExitStatus> {
    // v0.4 §30.2: "`enter` changes the spatial place." The context frame of §14.3 narrows later
    // commands; the place of v0.4 §46 says where the session is standing. One `enter` sets both,
    // and §30.4 keeps them separate pieces of state.
    // §47 switches the place off with the rest of the layer: the context frame still narrows
    // later commands, and nothing moves a place no spatial verb will answer for.
    if !crate::spatial::disabled(session)
        && let Some((runtime, _)) = session.pipeline_context()
    {
        runtime.block_on(crate::spatial::enter_observed(record));
    }
    let registry = crate::native::registry().map_err(Flow::Failed)?;
    let handle = registry.find("enter", Some(target)).and_then(|contract| {
        contract
            .selectors()
            .first()
            .map(|spec| spec.name().to_owned())
    });
    let frame = ContextFrame::of_record(target, Value::Null, record);
    let identity = handle
        .as_deref()
        .and_then(|field| {
            // A handle that lives inside a structural sub-record — a socket's port inside its
            // local endpoint — names the whole endpoint: `127.0.0.1:443` is how the prompt
            // shows the socket, and the port alone is not.
            if record.get(field).is_none()
                && let Some(endpoint) = record.schema().fields().iter().find_map(|top| match record
                    .get(top.name())
                {
                    Some(Value::Record(nested)) if nested.get(field).is_some() => Some(nested),
                    _ => None,
                })
            {
                let rendered: Vec<String> = endpoint
                    .schema()
                    .fields()
                    .iter()
                    .filter_map(|nested| match endpoint.get(nested.name()) {
                        Some(Value::Null) | None => None,
                        Some(value) => Some(value.to_string()),
                    })
                    .collect();
                return Some(Value::string(&rendered.join(":")));
            }
            frame.handle(field).cloned()
        })
        .or_else(|| {
            record
                .schema()
                .identity()
                .iter()
                .find_map(|field| frame.handle(field).cloned())
        })
        .unwrap_or_else(|| Value::string(&record.identity().to_string()));
    let frame = ContextFrame::of_record(target, identity, record);
    session.push_frame(ShellFrame {
        frame,
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
        // A one-shot connection (`connect host`) exists for its frame and goes with it
        // (ADR-0104 §3): leaving hangs up.
        if matches!(popped.frame.kind(), ono_command::FrameKind::Link) {
            let name = popped.frame.identity().to_string();
            let one_shot = session.link(&name).is_some_and(|link| !link.persistent);
            if one_shot
                && session.link_frames(&name) == 0
                && let Some(link) = session.remove_link(&name)
            {
                session.hang_up(link);
            }
        }
        if !all {
            break;
        }
    }
    Ok(ExitStatus::SUCCESS)
}

/// Runs `link host <name>` (spec §21.1): connect, negotiate, mount, remember.
///
/// # Errors
///
/// The structured refusals of the handshake and the trust decision — `remote.host_key_changed`,
/// `safety.policy_denied`, `remote.unreachable` — exactly as the protocol raises them.
pub fn link(session: &mut Session, stage: &Stage, source: &str) -> Eval<ExitStatus> {
    let words = crate::eval::stage_arguments(session, stage, source)?;
    let mut host = None;
    let mut transport = "ssh".to_owned();
    let mut take_transport = false;
    let mut agentless = false;
    for word in &words {
        let text = word.to_string_lossy();
        if take_transport {
            transport = text.into_owned();
            take_transport = false;
        } else if text == "--transport" {
            take_transport = true;
        } else if let Some(value) = text.strip_prefix("--transport=") {
            transport = value.to_owned();
        } else if text == "--agentless" {
            agentless = true;
        } else if text == "host" {
            // the target word of `link host <name>`
        } else if !text.starts_with("--") {
            host = Some(text.into_owned());
        }
    }
    let Some(host) = host else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                "`link host` needs the host to link to",
            )
            .with_help("`link host prod-db` (spec §21.1)"),
        ));
    };

    let connection = establish(session, &host, &transport, None)?;
    let targets = connection.targets();

    // Spec §21.3: the agentless fallback MUST be visible. This build has no agentless provider
    // set yet, so the mode is recorded and reported wherever the link is described, and the
    // summary says who actually answers (ADR-0106).
    println!(
        "linked {host} ({transport}{}): {}",
        if agentless {
            ", agentless requested — served by the agent until the fallback exists"
        } else {
            ""
        },
        if targets.is_empty() {
            "no targets negotiated".to_owned()
        } else {
            targets.join(" ")
        }
    );
    // v0.4 §19.1: a link just negotiated is one this session follows, whatever a `detach link`
    // of an earlier link by the same name left behind.
    crate::spatial::links::attach(&host);
    session.add_link(crate::session::SessionLink {
        name: host.clone(),
        host,
        transport,
        agentless,
        persistent: true,
        connection: Some(connection),
    });
    Ok(ExitStatus::SUCCESS)
}

/// Connects to `host` over `transport`, negotiates and mounts (spec §21.2): the connection
/// behind `link host`, `connect host` and an unlinked `test host` (ADR-0104). `timeout` bounds
/// the whole of it; without one the transport's own limits apply.
///
/// # Errors
///
/// The structured refusals of the handshake and the trust decision — `remote.host_key_changed`,
/// `safety.policy_denied`, `remote.unreachable` — exactly as the protocol raises them; a
/// timeout is `remote.unreachable` naming the bound.
pub fn establish(
    session: &mut Session,
    host: &str,
    transport: &str,
    timeout: Option<std::time::Duration>,
) -> Eval<crate::session::LinkConnection> {
    let command = match transport {
        // The agent over OpenSSH, which also did the authenticating (ADR-0037).
        "ssh" => {
            let mut target = ono_remote::SshTarget::new(host);
            // The file `get host` lists hosts from is the file ssh resolves them with
            // (ADR-0103, ADR-0104): the shell's `~` is `HOME`, and ssh's own is the account's.
            if let Some(config) = session
                .host_sources()
                .ssh_config
                .filter(|path| path.is_file())
            {
                target = target.with_config(config);
            }
            ono_remote::ssh_command(&target)
        }
        // This very binary as a child: the whole path over a pipe pair, no network. It is how
        // a link is exercised in a test, a container, or on the machine itself.
        "local" => {
            let mut command = tokio::process::Command::new(
                std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("ono")),
            );
            command.arg("--agent");
            command
        }
        other => {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveTargetNotFound,
                    format!("no transport answers to `{other}`"),
                )
                .with_help("the transports are `ssh` and `local` (ono.link/1)"),
            ));
        }
    };

    let runtime = session.runtime().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the runtime",
        ))
    })?;

    let schemas = std::sync::Arc::new(ono_value::builtin_schemas().clone());
    // Both transports carry their own authentication story (ADR-0037): over `ssh`, OpenSSH
    // verified the host before the agent ever spoke; `local` is a child of this very process.
    // The protocol-level policy is therefore unauthenticated *by name* — an explicit statement,
    // never a silent default, and a future TCP transport gets `Required` with the trust store.
    let config = ono_protocol::ClientConfig::new(host)
        .with_schemas(schemas)
        .with_trust_policy(ono_protocol::TrustPolicy::Unauthenticated)
        .with_identity(ono_protocol::Identity::new(whoami()));
    let mut agent = None;
    let connected = runtime.block_on(async {
        let connect = async {
            let transport = ono_remote::SubprocessTransport::spawn(command)?;
            // Kept before the transport is handed over: from here on the shell is responsible
            // for ending the process it just started (spec §21.4, ADR-0161).
            agent = Some(transport.child());
            ono_remote::RemoteLink::connect(transport, config).await
        };
        match timeout {
            Some(bound) => match tokio::time::timeout(bound, connect).await {
                Ok(outcome) => outcome,
                Err(_) => Err(ErrorValue::new(
                    ErrorCode::RemoteUnreachable,
                    format!("{host} did not answer the handshake within {bound:?}"),
                )
                .with_help("`--timeout` bounds how long a probe waits (spec §21.2)")),
            },
            None => connect.await,
        }
    });
    let link = match connected {
        Ok(link) => link,
        Err(refusal) => {
            // The child was already started; a handshake that failed must not leave it behind.
            if let Some(agent) = &agent {
                runtime.block_on(agent.end(crate::session::AGENT_GRACE));
            }
            return Err(Flow::Failed(refusal));
        }
    };

    let mut registry = ono_provider_api::ProviderRegistry::new();
    link.register_into(&mut registry);
    Ok(crate::session::LinkConnection {
        link,
        registry: std::sync::Arc::new(registry),
        agent,
    })
}

/// The user this side identifies as, for the handshake.
fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| "ono".to_owned())
}
