//! The remote commands the shell answers itself: the link definitions of spec §9.1 and §21
//! (`add`, `set`, `rename`, `remove`, `detach link`).
//!
//! A link is session state — the definition, the connection once it is established, and the
//! frame that stands on it — and none of it can live in a provider: a provider is what a link
//! frame *swaps* (spec §14.4), so a provider that owned the link table would be answering from
//! the other side of the link the moment one is entered. The contracts are still the registry's
//! (`docs/spec/commands/remote.yaml` supplies help, completion and typing), and this module
//! supplies the effect and the `ono.action-result/1` row every mutation answers with, which then
//! seeds the rest of the pipeline exactly as a provider's outcome would (ADR-0104).

use ono_core::ErrorCode;
use ono_parser::{Stage, StageHead};
use ono_value::{ActionResult, ActionStatus, ErrorValue, MapValue, SchemaId, Value, ValueRef};

use crate::eval::{Eval, Flow};
use crate::session::{Session, SessionLink};

/// What a stage asks the shell to do with a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// `add link <name> --host <host> --transport <transport>` — record, do not connect.
    AddLink,
    /// `set link <name> --host/--transport` — change a definition.
    SetLink,
    /// `rename link <name> <new-name>`.
    RenameLink,
    /// `remove link <name>` — forget the definition, tearing the link down if established.
    RemoveLink,
    /// `detach link <name>` — pop the link's frame, keep the link.
    DetachLink,
}

impl Request {
    fn verb(self) -> &'static str {
        match self {
            Self::AddLink => "add",
            Self::SetLink => "set",
            Self::RenameLink => "rename",
            Self::RemoveLink => "remove",
            Self::DetachLink => "detach",
        }
    }

    fn command_id(self) -> &'static str {
        match self {
            Self::AddLink => "ono.link.add",
            Self::SetLink => "ono.link.set",
            Self::RenameLink => "ono.link.rename",
            Self::RemoveLink => "ono.link.remove",
            Self::DetachLink => "ono.link.detach",
        }
    }
}

/// Whether `stage` is one of the commands this module answers: by the head word and its
/// target, as `set config` is claimed (`ono:remove link` means the same; a program named
/// `remove` stays reachable as `exec:remove`).
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
        ("add", Some("link")) => Some(Request::AddLink),
        ("set", Some("link")) => Some(Request::SetLink),
        ("rename", Some("link")) => Some(Request::RenameLink),
        ("remove", Some("link")) => Some(Request::RemoveLink),
        ("detach", Some("link")) => Some(Request::DetachLink),
        _ => None,
    }
}

/// The values `stage` produces: one `ono.action-result/1` for the link named.
///
/// # Errors
///
/// A structured error when the arguments do not bind to the contract, or when the link cannot
/// be acted on as asked — a name nothing answers to, a definition that already exists, a
/// transport this build has none of. A refusal is the pipeline's failure, never a row that
/// looks like a change (spec §16.5).
pub fn answer(
    session: &mut Session,
    stage: &Stage,
    source: &str,
    request: Request,
) -> Eval<Vec<Value>> {
    let _ = source;
    let started = std::time::Instant::now();
    let registry = crate::native::registry().map_err(Flow::Failed)?;
    let resolved = registry
        .resolve(request.verb(), &stage.arguments)
        .map_err(Flow::Failed)?;
    let bound = resolved
        .contract
        .bind(resolved.arguments)
        .map_err(Flow::Failed)?;
    let name = bound
        .require_selector("name")
        .map_err(Flow::Failed)?
        .to_string();
    let option = |option: &str| {
        bound
            .option(option)
            .filter(|value| !matches!(value, Value::Null))
            .map(ToString::to_string)
    };

    let (changed, message) = match request {
        Request::AddLink => add_link(session, &name, option("host"), option("transport"))?,
        Request::SetLink => set_link(session, &name, option("host"), option("transport"))?,
        Request::RenameLink => {
            let new_name = bound
                .require_selector("new-name")
                .map_err(Flow::Failed)?
                .to_string();
            rename_link(session, &name, &new_name)?
        }
        Request::RemoveLink => remove_link(session, &name)?,
        Request::DetachLink => detach_link(session, &name)?,
    };

    let mut identity = MapValue::new();
    identity.insert("name".into(), Value::string(&name));
    let target = ValueRef::object(SchemaId::new("ono.link", 1), identity);
    let elapsed = ono_value::Duration::from_nanoseconds(
        i128::try_from(started.elapsed().as_nanos()).unwrap_or(i128::MAX),
    );
    let result = ActionResult::new(target, request.command_id(), ActionStatus::Success)
        .changed(changed)
        .with_message(&message)
        .with_duration(elapsed);
    Ok(vec![result.into_value()])
}

/// The transports this build has (ono.link/1 `transport`).
fn check_transport(transport: &str) -> Eval<()> {
    if matches!(transport, "ssh" | "local") {
        return Ok(());
    }
    Err(Flow::Failed(
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("no transport answers to `{transport}`"),
        )
        .with_help("the transports are `ssh` and `local` (ono.link/1)"),
    ))
}

fn unknown_link(name: &str) -> Flow {
    Flow::Failed(
        ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            format!("this session holds no link named `{name}`"),
        )
        .with_help("`get link` lists the links this session knows"),
    )
}

fn add_link(
    session: &mut Session,
    name: &str,
    host: Option<String>,
    transport: Option<String>,
) -> Eval<(bool, String)> {
    if session.link(name).is_some() {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::IoAlreadyExists,
                format!("this session already holds a link named `{name}`"),
            )
            .with_help(format!(
                "`set link {name} …` changes it, `remove link {name}` forgets it"
            )),
        ));
    }
    let transport = transport.unwrap_or_else(|| "ssh".to_owned());
    check_transport(&transport)?;
    let host = host.unwrap_or_else(|| name.to_owned());
    let message = format!("{name} defined: {host} over {transport}, not established");
    session.add_link(SessionLink {
        name: name.to_owned(),
        host,
        transport,
        agentless: false,
        persistent: true,
        connection: None,
    });
    Ok((true, message))
}

fn set_link(
    session: &mut Session,
    name: &str,
    host: Option<String>,
    transport: Option<String>,
) -> Eval<(bool, String)> {
    if host.is_none() && transport.is_none() {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                "`set link` needs a property to set, and none was given",
            )
            .with_help("name what should change: --host, --transport"),
        ));
    }
    if let Some(transport) = &transport {
        check_transport(transport)?;
    }
    let link = session.link_mut(name).ok_or_else(|| unknown_link(name))?;
    if link.connection.is_some() {
        // A definition describes how a connection is made; changing it under an established
        // connection would make the table describe a link that does not exist.
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!(
                    "`{name}` is established, and an established link's definition cannot change"
                ),
            )
            .with_help(format!(
                "`remove link {name}` tears it down; then define it again"
            )),
        ));
    }
    let mut changed = false;
    if let Some(host) = host
        && link.host != host
    {
        link.host = host;
        changed = true;
    }
    if let Some(transport) = transport
        && link.transport != transport
    {
        link.transport = transport;
        changed = true;
    }
    let message = format!("{name}: {} over {}", link.host, link.transport);
    Ok((changed, message))
}

fn rename_link(session: &mut Session, name: &str, new_name: &str) -> Eval<(bool, String)> {
    if session.link(name).is_none() {
        return Err(unknown_link(name));
    }
    if name == new_name {
        return Ok((false, format!("{name} keeps its name")));
    }
    if session.link(new_name).is_some() {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::IoAlreadyExists,
                format!("this session already holds a link named `{new_name}`"),
            )
            .with_help("`get link` lists the names in use"),
        ));
    }
    if session.link_frames(name) > 0 {
        // The frames on the stack name the link by its name (spec §14.4); a rename underneath
        // an active frame would leave the prompt standing on a link that no longer exists.
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("`{name}` is entered, and an entered link cannot be renamed"),
            )
            .with_help(format!("`detach link {name}` first")),
        ));
    }
    if let Some(link) = session.link_mut(name) {
        link.name = new_name.to_owned();
    }
    Ok((true, format!("{name} renamed to {new_name}")))
}

fn remove_link(session: &mut Session, name: &str) -> Eval<(bool, String)> {
    let Some(link) = session.remove_link(name) else {
        return Err(unknown_link(name));
    };
    let frames = session.pop_link_frames(name);
    let message = match (link.connection.is_some(), frames) {
        (true, 0) => format!("{name} torn down and forgotten"),
        (true, _) => format!("{name} detached, torn down and forgotten"),
        (false, _) => format!("{name} forgotten; it was never established"),
    };
    // Dropping the connection hangs up (ADR-0036 §8).
    drop(link);
    Ok((true, message))
}

fn detach_link(session: &mut Session, name: &str) -> Eval<(bool, String)> {
    let Some(link) = session.link(name) else {
        return Err(unknown_link(name));
    };
    let persistent = link.persistent;
    let frames = session.pop_link_frames(name);
    if frames == 0 {
        return Ok((
            false,
            format!("{name} was not entered; nothing to detach from"),
        ));
    }
    // A one-shot connection (`connect host`) exists for its frame and goes with it (ADR-0104).
    if !persistent {
        drop(session.remove_link(name));
        return Ok((true, format!("{name} detached and hung up")));
    }
    Ok((true, format!("{name} detached; the link is kept")))
}
