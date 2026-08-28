//! The remote commands the shell answers itself: the link definitions of spec §9.1 and §21
//! (`add`, `set`, `rename`, `remove`, `detach link`), the one-shot `connect host` of spec §6.1,
//! and the probe `test host`.
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
    /// `connect host <name> [--transport t]` — connect, enter, and forget on leaving.
    ConnectHost,
    /// `test host <name> [--timeout d]` — probe reachability and what the handshake negotiates.
    TestHost,
}

impl Request {
    fn verb(self) -> &'static str {
        match self {
            Self::AddLink => "add",
            Self::SetLink => "set",
            Self::RenameLink => "rename",
            Self::RemoveLink => "remove",
            Self::DetachLink => "detach",
            Self::ConnectHost => "connect",
            Self::TestHost => "test",
        }
    }

    fn target(self) -> &'static str {
        match self {
            Self::ConnectHost | Self::TestHost => "host",
            _ => "link",
        }
    }

    fn command_id(self) -> &'static str {
        match self {
            Self::AddLink => "ono.link.add",
            Self::SetLink => "ono.link.set",
            Self::RenameLink => "ono.link.rename",
            Self::RemoveLink => "ono.link.remove",
            Self::DetachLink => "ono.link.detach",
            Self::ConnectHost => "ono.host.connect",
            Self::TestHost => "ono.host.test",
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
        ("connect", Some("host")) => Some(Request::ConnectHost),
        ("test", Some("host")) => Some(Request::TestHost),
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
    let bound = bind(request, stage)?;
    let name = bound
        .require_selector("name")
        .map_err(Flow::Failed)?
        .to_string();
    act(session, request, &name, &bound)
}

/// The piped form, `get link | remove link` (ADR-0118): the links to act on arrive as
/// `ono.link/1` records and the stage holds only the options — or, for `rename link`, the new
/// name. One `ono.action-result/1` per piped link; a link that cannot be acted on is a `failed`
/// row rather than the end of the run (spec §16.5, ADR-0006).
///
/// # Errors
///
/// A type error when the command declares no stream input (`connect host`, `test host`, `add
/// link`), when a piped value is not an `ono.link/1` record, or when `rename link` is handed
/// anything but exactly one link.
pub fn answer_piped(
    session: &mut Session,
    stage: &Stage,
    request: Request,
    targets: &[Value],
) -> Eval<Vec<Value>> {
    let spelling = format!("{} {}", request.verb(), request.target());
    if matches!(
        request,
        Request::AddLink | Request::ConnectHost | Request::TestHost
    ) {
        return Err(Flow::Failed(no_stream_input(&spelling, request.target())));
    }
    let bound = bind(request, stage)?;
    let names = piped_names(&spelling, "ono.link", "name", targets).map_err(Flow::Failed)?;
    if request == Request::RenameLink && names.len() != 1 {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!(
                    "`rename link` renames one link, {} arrived through the pipe",
                    names.len()
                ),
            )
            .with_help(
                "`get link <name> | rename link <new-name>` (remote.yaml: input ono.link/1)",
            ),
        ));
    }
    let mut rows = Vec::with_capacity(names.len());
    for name in names {
        match act(session, request, &name, &bound) {
            Ok(values) => rows.extend(values),
            Err(Flow::Failed(error)) => {
                rows.push(failed_row(&name, request.command_id(), error));
            }
            Err(other) => return Err(other),
        }
    }
    Ok(rows)
}

/// The type error for a command whose contract declares `input: "null"`: the pipe cannot name
/// its target, so the answer says how the head form is spelled (ADR-0118).
pub(crate) fn no_stream_input(spelling: &str, target: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!(
            "`{spelling}` takes nothing from the pipe: name the {target} — `{spelling} <name>`"
        ),
    )
    .with_help(format!(
        "the contract declares `input: null` for `{spelling}`; `get {target} | select name` \
         shows the names to choose from"
    ))
}

/// The identities the piped records name: every value must be a `<schema>` record carrying a
/// string `<field>`, or the stage was handed something it cannot act on.
pub(crate) fn piped_names(
    spelling: &str,
    schema: &str,
    field: &str,
    targets: &[Value],
) -> Result<Vec<String>, ErrorValue> {
    targets
        .iter()
        .map(|value| {
            let name = value
                .as_record()
                .ok()
                .filter(|record| record.schema_id().name() == schema)
                .and_then(|record| record.get(field))
                .and_then(|name| name.as_str().ok().map(str::to_owned));
            name.ok_or_else(|| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!(
                        "`{spelling}` acts on {schema}/1 records, and {} arrived through the pipe",
                        value.type_name()
                    ),
                )
                .with_help(format!(
                    "`get {} | {spelling}` is the piped form",
                    schema.trim_start_matches("ono.")
                ))
            })
        })
        .collect()
}

/// The `failed` row for one piped link that could not be acted on.
fn failed_row(name: &str, operation: &str, error: ErrorValue) -> Value {
    let mut identity = MapValue::new();
    identity.insert("name".into(), Value::string(name));
    let target = ValueRef::object(SchemaId::new("ono.link", 1), identity);
    ActionResult::new(target, operation, ActionStatus::Failed)
        .with_error(error)
        .into_value()
}

/// Binds the stage's arguments to the command's contract.
fn bind(request: Request, stage: &Stage) -> Eval<ono_command::BoundArguments> {
    let registry = crate::native::registry().map_err(Flow::Failed)?;
    let resolved = registry
        .resolve(request.verb(), &stage.arguments)
        .map_err(Flow::Failed)?;
    resolved
        .contract
        .bind(resolved.arguments)
        .map_err(Flow::Failed)
}

/// Acts on the link (or host) called `name` as `request` asks, with the bound options.
fn act(
    session: &mut Session,
    request: Request,
    name: &str,
    bound: &ono_command::BoundArguments,
) -> Eval<Vec<Value>> {
    let started = std::time::Instant::now();
    let option = |option: &str| {
        bound
            .option(option)
            .filter(|value| !matches!(value, Value::Null))
            .map(ToString::to_string)
    };

    let (changed, message) = match request {
        Request::ConnectHost => return connect_host(session, name, option("transport")),
        Request::TestHost => {
            let timeout = bound.option("timeout").and_then(|value| match value {
                Value::Duration(duration) => u64::try_from(duration.nanoseconds())
                    .ok()
                    .map(std::time::Duration::from_nanos),
                _ => None,
            });
            return test_host(session, name, timeout);
        }
        Request::AddLink => add_link(session, name, option("host"), option("transport"))?,
        Request::SetLink => set_link(session, name, option("host"), option("transport"))?,
        Request::RenameLink => {
            // Head form: `rename link <name> <new-name>`. Piped form: the link arrived, so the
            // one positional left is the new name and binds as `name` (ADR-0118).
            let new_name = bound
                .selector("new-name")
                .or_else(|| bound.selector("name"))
                .ok_or_else(|| {
                    Flow::Failed(
                        ErrorValue::new(
                            ErrorCode::ResolveTargetNotFound,
                            "`rename link` needs the new name",
                        )
                        .with_help("`rename link <name> <new-name>` (spec §9.1)"),
                    )
                })?
                .to_string();
            rename_link(session, name, &new_name)?
        }
        Request::RemoveLink => remove_link(session, name)?,
        Request::DetachLink => detach_link(session, name)?,
    };

    let mut identity = MapValue::new();
    identity.insert("name".into(), Value::string(name));
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
    // The link hangs up, and the process serving it is waited for (ADR-0036 §8, ADR-0161).
    session.hang_up(link);
    Ok((true, message))
}

/// `connect host`: `link host` plus the frame, minus the persistence (ADR-0104 §3). The value
/// is the link as `get link` would show it.
fn connect_host(session: &mut Session, name: &str, transport: Option<String>) -> Eval<Vec<Value>> {
    if session
        .link(name)
        .is_some_and(|link| link.connection.is_some())
    {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::IoAlreadyExists,
                format!("this session already holds an established link named `{name}`"),
            )
            .with_help(format!("`enter link {name}` stands on it")),
        ));
    }
    let transport = transport.unwrap_or_else(|| "ssh".to_owned());
    check_transport(&transport)?;
    let connection = crate::context::establish(session, name, &transport, None)?;
    let link = SessionLink {
        name: name.to_owned(),
        host: name.to_owned(),
        transport,
        agentless: false,
        persistent: false,
        connection: Some(connection),
    };
    let value = crate::session_provider::link_value(&link.row()).map_err(Flow::Failed)?;
    session.add_link(link);
    session.push_frame(crate::session::ShellFrame {
        frame: ono_command::ContextFrame::link(Value::string(name)),
        restore_cwd: None,
    });
    Ok(vec![value])
}

/// How long `test host` waits when `--timeout` is not written.
const DEFAULT_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// What spec §21.2 says a handshake negotiates, as `test host` reports it: the transport, the
/// protocol version, the far side's agent, the provider ids.
fn negotiated_facts(
    transport: &str,
    connection: &crate::session::LinkConnection,
) -> (String, u16, String, Vec<Value>) {
    let negotiated = connection.link.negotiated();
    let providers: Vec<Value> = negotiated
        .providers()
        .iter()
        .map(|descriptor| Value::string(descriptor.id()))
        .collect();
    (
        transport.to_owned(),
        negotiated.version(),
        negotiated.peer().agent().to_owned(),
        providers,
    )
}

/// `test host`: the handshake's facts for a held link, or one connection made and hung up
/// (ADR-0104 §4).
fn test_host(
    session: &mut Session,
    name: &str,
    timeout: Option<std::time::Duration>,
) -> Eval<Vec<Value>> {
    let started = std::time::Instant::now();
    let held = session.link(name).and_then(|link| {
        link.connection
            .as_ref()
            .map(|connection| negotiated_facts(&link.transport, connection))
    });
    let (transport, version, agent, providers) = match held {
        Some(facts) => facts,
        None => {
            // A definition says how the host is reached; otherwise the host is reached the way
            // `link host` reaches it by default.
            let (host, transport) = session.link(name).map_or_else(
                || (name.to_owned(), "ssh".to_owned()),
                |link| (link.host.clone(), link.transport.clone()),
            );
            let connection = crate::context::establish(
                session,
                &host,
                &transport,
                Some(timeout.unwrap_or(DEFAULT_PROBE_TIMEOUT)),
            )
            .map_err(|flow| match flow {
                Flow::Failed(error) if error.code() != ErrorCode::RemoteUnreachable => {
                    Flow::Failed(
                        ErrorValue::new(
                            ErrorCode::RemoteUnreachable,
                            format!("{name} is not reachable: {}", error.message()),
                        )
                        .with_help(error.help().unwrap_or_default()),
                    )
                }
                other => other,
            })?;
            let facts = negotiated_facts(&transport, &connection);
            // Hanging up is what a probe does with the link it made (ADR-0036 §8).
            drop(connection);
            facts
        }
    };

    let elapsed = ono_value::Duration::from_nanoseconds(
        i128::try_from(started.elapsed().as_nanos()).unwrap_or(i128::MAX),
    );
    let schema = ono_value::builtin_schemas()
        .get(&SchemaId::new("ono.probe-result", 1))
        .ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                "`test host` advertises ono.probe-result/1 but no contract defines it",
            ))
        })?;
    let provenance =
        ono_value::Provenance::local(crate::session_provider::PROVIDER_ID, schema.id().clone());
    let record = ono_value::RecordValue::builder(schema, provenance)
        .set("host", Value::string(name))
        .and_then(|builder| builder.set("port", Value::Null))
        .and_then(|builder| builder.set("protocol", Value::string("ono")))
        .and_then(|builder| builder.set("reachable", Value::Bool(true)))
        .and_then(|builder| builder.set("duration", Value::Duration(elapsed)))
        .and_then(|builder| builder.set("error", Value::Null))
        .and_then(|builder| builder.set("transport", Value::string(&transport)))
        .and_then(|builder| builder.set("protocol_version", Value::Int(i128::from(version))))
        .and_then(|builder| builder.set("agent", Value::string(&agent)))
        .and_then(|builder| builder.set("providers", Value::list(providers)))
        .map_err(Flow::Failed)?
        .build();
    Ok(vec![record.into_value()])
}

fn detach_link(session: &mut Session, name: &str) -> Eval<(bool, String)> {
    let Some(link) = session.link(name) else {
        return Err(unknown_link(name));
    };
    let persistent = link.persistent;
    // v0.4 §19.1/§35.2: detaching leaves the attachment. The link is not torn down — `changed`
    // below reports only whether a frame was popped, exactly as v0.2 §9.1 defines it — but this
    // session has stopped following the host's space, so the places behind it are `stale` rather
    // than answered as if they were still being kept current (ADR-0171).
    crate::spatial::links::detach(name);
    let frames = session.pop_link_frames(name);
    if frames == 0 {
        return Ok((
            false,
            format!("{name} was not entered; nothing to detach from"),
        ));
    }
    // A one-shot connection (`connect host`) exists for its frame and goes with it (ADR-0104).
    if !persistent {
        if let Some(link) = session.remove_link(name) {
            session.hang_up(link);
        }
        return Ok((true, format!("{name} detached and hung up")));
    }
    Ok((true, format!("{name} detached; the link is kept")))
}
