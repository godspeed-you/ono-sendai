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
    /// `link host <name>` — create a remote link (spec §21.1).
    Link,
    /// `get link` — the links this session holds.
    GetLink,
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
        "link" => Some(Request::Link),
        "get"
            if stage
                .arguments
                .first()
                .and_then(ono_parser::Argument::as_word)
                == Some("link") =>
        {
            Some(Request::GetLink)
        }
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
        ("link", Some(name)) => enter_link(session, name),
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

/// Spec §14.4: entering a link makes it decide where provider calls run. The link must already
/// be held — entering is navigation, not connection.
fn enter_link(session: &mut Session, name: String) -> Eval<ExitStatus> {
    if session.link_registry(&name).is_none() {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("this session holds no link named `{name}`"),
            )
            .with_help(format!("`link host {name}` creates one (spec §21.1)")),
        ));
    }
    session.push_frame(ShellFrame {
        frame: ContextFrame::link(Value::string(&name)),
        restore_cwd: None,
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
    for word in &words {
        let text = word.to_string_lossy();
        if take_transport {
            transport = text.into_owned();
            take_transport = false;
        } else if text == "--transport" {
            take_transport = true;
        } else if let Some(value) = text.strip_prefix("--transport=") {
            transport = value.to_owned();
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

    let command = match transport.as_str() {
        // The agent over OpenSSH, which also did the authenticating (ADR-0037).
        "ssh" => ono_remote::ssh_command(&ono_remote::SshTarget::new(&host)),
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

    let (runtime, _) = session.pipeline_context().ok_or_else(|| {
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
    let config = ono_protocol::ClientConfig::new(&host)
        .with_schemas(schemas)
        .with_trust_policy(ono_protocol::TrustPolicy::Unauthenticated)
        .with_identity(ono_protocol::Identity::new(whoami()));
    let connected = runtime.block_on(async {
        let transport = ono_remote::SubprocessTransport::spawn(command)?;
        ono_remote::RemoteLink::connect(transport, config).await
    });
    let link = connected.map_err(Flow::Failed)?;

    let mut registry = ono_provider_api::ProviderRegistry::new();
    link.register_into(&mut registry);
    let targets: Vec<String> = registry
        .providers()
        .iter()
        .flat_map(|provider| provider.targets().iter().map(|target| (*target).to_owned()))
        .collect();

    println!(
        "linked {host} ({transport}): {}",
        if targets.is_empty() {
            "no targets negotiated".to_owned()
        } else {
            targets.join(" ")
        }
    );
    session.add_link(crate::session::SessionLink {
        name: host,
        transport,
        link,
        registry: std::sync::Arc::new(registry),
    });
    Ok(ExitStatus::SUCCESS)
}

/// Runs `get link`: the session's link table, one row per link (ono.link/1).
///
/// # Errors
///
/// None in practice; the signature matches its callers.
pub fn get_link(session: &mut Session) -> Eval<ExitStatus> {
    for held in session.links() {
        let targets: Vec<String> = held
            .registry
            .providers()
            .iter()
            .flat_map(|provider| provider.targets().iter().map(|target| (*target).to_owned()))
            .collect();
        println!(
            "{}  {}  connected  {}",
            held.name,
            held.transport,
            targets.join(" ")
        );
    }
    Ok(ExitStatus::SUCCESS)
}

/// The user this side identifies as, for the handshake.
fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| "ono".to_owned())
}
