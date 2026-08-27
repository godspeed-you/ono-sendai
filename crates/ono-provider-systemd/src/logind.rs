//! The `session` target: login sessions, read from systemd-logind over D-Bus (ADR-0100).
//!
//! Spec §9.1 gives `get session` one sentence — "enumerate local/login/session objects" — and
//! names no source. On a systemd machine the sessions are logind's: `org.freedesktop.login1`
//! tracks every login the PAM stack opens, whether at a seat, on a tty or over SSH, and publishes
//! each as an object with typed properties. Everything here is `Manager.ListSessions` plus
//! `org.freedesktop.DBus.Properties.GetAll` on `org.freedesktop.login1.Session`; nothing runs
//! `loginctl` or `who`, and nothing parses text (spec §50).
//!
//! Where no login manager answers — a container, a machine without systemd — the provider is
//! [`Availability::Unavailable`] with the reason, exactly as the service provider is. An empty
//! stream would claim "nobody is logged in", which is a different and unverifiable statement
//! (spec §10.5, §35.3).

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value, builtin_schemas};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Connection, Proxy};

use crate::BusError;
use crate::dbus::{budgeted, number, open_system_bus, text};

/// The id this provider signs its records with.
pub const SESSION_PROVIDER_ID: &str = "systemd-logind";

const DESTINATION: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER_INTERFACE: &str = "org.freedesktop.login1.Manager";
const SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// One row of `org.freedesktop.login1.Manager.ListSessions`: `(susso)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionListing {
    /// The session id.
    pub id: String,
    /// The uid of the user holding it.
    pub uid: u32,
    /// The login name logind recorded for that uid.
    pub user_name: String,
    /// The seat, where the session has one.
    pub seat: Option<String>,
    /// The session's object path, for the property read that follows.
    pub path: OwnedObjectPath,
}

/// The properties of one session, from `org.freedesktop.login1.Session`.
///
/// Every field but the id is optional because logind genuinely leaves them empty: an SSH login
/// has no seat and no display, a `background` session has no tty. `None` is "logind did not
/// say", which becomes a null rather than a placeholder (spec §35.3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionProperties {
    /// `Id`.
    pub id: String,
    /// `TTY`.
    pub tty: Option<String>,
    /// `Display`.
    pub display: Option<String>,
    /// `Type`: `tty`, `x11`, `wayland`, `mir`, `web`, `unspecified`.
    pub kind: Option<String>,
    /// `Class`: `user`, `greeter`, `lock-screen`, `background`, `manager`, …
    pub class: Option<String>,
    /// `State`: `online`, `active`, `closing`.
    pub state: Option<String>,
    /// `Remote`.
    pub remote: Option<bool>,
    /// `RemoteHost`.
    pub remote_host: Option<String>,
    /// `Service` — the PAM service that opened the session.
    pub service: Option<String>,
    /// `Leader` — the pid of the session leader; logind reports 0 for none.
    pub leader: Option<u32>,
    /// `Scope` — the systemd scope unit.
    pub scope: Option<String>,
    /// `Timestamp`, in microseconds since the Unix epoch: when the session was opened.
    pub timestamp_usec: Option<u64>,
}

/// The part of `org.freedesktop.login1` this provider speaks.
///
/// A trait for the same reason [`crate::SystemdBus`] is one: logind is the outside world, and a
/// recorded implementation is the fixture that lets the positive path be tested on a machine
/// where nobody is logged in through logind at all.
#[async_trait::async_trait]
pub trait LoginBus: Send + Sync + fmt::Debug {
    /// The availability probe: a property read on the `Manager` interface.
    ///
    /// # Errors
    ///
    /// [`BusError::Unavailable`] when no login manager answers.
    async fn manager_reachable(&self) -> Result<(), BusError>;

    /// Calls `Manager.ListSessions`.
    ///
    /// # Errors
    ///
    /// Whatever the bus reported.
    async fn list_sessions(&self) -> Result<Vec<SessionListing>, BusError>;

    /// Reads every property of one session.
    ///
    /// Returns `Ok(None)` when the session is gone — a snapshot of a moving system, not a
    /// failure.
    ///
    /// # Errors
    ///
    /// Whatever the bus reported, other than "no such object".
    async fn session_properties(
        &self,
        path: &OwnedObjectPath,
    ) -> Result<Option<SessionProperties>, BusError>;
}

/// The one implementation of [`LoginBus`] that talks to a real login manager.
pub struct LoginSystemBus {
    connection: Connection,
    manager: Proxy<'static>,
}

impl fmt::Debug for LoginSystemBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginSystemBus")
            .field("destination", &DESTINATION)
            .finish()
    }
}

impl LoginSystemBus {
    /// Opens the system bus and builds the `org.freedesktop.login1.Manager` proxy.
    ///
    /// # Errors
    ///
    /// [`BusError::Unavailable`], naming what was missing, when there is no bus socket or the
    /// connection cannot be established.
    pub async fn connect() -> Result<Self, BusError> {
        let connection = open_system_bus().await?;
        let manager = budgeted(
            "building the org.freedesktop.login1.Manager proxy",
            Proxy::new(&connection, DESTINATION, MANAGER_PATH, MANAGER_INTERFACE),
        )
        .await?;
        Ok(Self {
            connection,
            manager,
        })
    }
}

#[async_trait::async_trait]
impl LoginBus for LoginSystemBus {
    async fn manager_reachable(&self) -> Result<(), BusError> {
        budgeted(
            "reading org.freedesktop.login1.Manager.NAutoVTs",
            self.manager.get_property::<u32>("NAutoVTs"),
        )
        .await
        .map(|_| ())
        .map_err(|error| {
            BusError::Unavailable(format!(
                "the D-Bus system bus answered but org.freedesktop.login1.Manager did not: {}",
                error.message()
            ))
        })
    }

    async fn list_sessions(&self) -> Result<Vec<SessionListing>, BusError> {
        /// The five columns of `ListSessions`: id, uid, user name, seat, object path.
        type Row = (String, u32, String, String, OwnedObjectPath);

        let rows: Vec<Row> = budgeted(
            "org.freedesktop.login1.Manager.ListSessions",
            self.manager.call::<_, _, Vec<Row>>("ListSessions", &()),
        )
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, uid, user_name, seat, path)| SessionListing {
                id,
                uid,
                user_name,
                seat: non_empty(seat),
                path,
            })
            .collect())
    }

    async fn session_properties(
        &self,
        path: &OwnedObjectPath,
    ) -> Result<Option<SessionProperties>, BusError> {
        let proxy = budgeted(
            "building the D-Bus properties proxy",
            Proxy::new(
                &self.connection,
                DESTINATION,
                path.clone(),
                PROPERTIES_INTERFACE,
            ),
        )
        .await?;
        let properties: HashMap<String, OwnedValue> = match budgeted(
            "reading org.freedesktop.login1.Session properties",
            proxy.call::<_, _, HashMap<String, OwnedValue>>("GetAll", &(SESSION_INTERFACE,)),
        )
        .await
        {
            Ok(properties) => properties,
            Err(BusError::NoSuchUnit(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(Some(SessionProperties {
            id: text(&properties, "Id").unwrap_or_default(),
            tty: text(&properties, "TTY"),
            display: text(&properties, "Display"),
            kind: text(&properties, "Type"),
            class: text(&properties, "Class"),
            state: text(&properties, "State"),
            remote: properties
                .get("Remote")
                .and_then(|value| bool::try_from(value.clone()).ok()),
            remote_host: text(&properties, "RemoteHost"),
            service: text(&properties, "Service"),
            leader: number::<u32>(&properties, "Leader").filter(|pid| *pid != 0),
            scope: text(&properties, "Scope"),
            timestamp_usec: number::<u64>(&properties, "Timestamp").filter(|usec| *usec != 0),
        }))
    }
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// Whether a login manager could be reached, and what to ask if it could.
#[derive(Debug)]
enum Backing {
    Ready(Arc<dyn LoginBus>),
    Missing(String),
}

/// The logind session provider: `ono.session/1` records read over D-Bus.
#[derive(Debug)]
pub struct SessionProvider {
    backing: Backing,
}

impl SessionProvider {
    /// Connects to the system bus and probes `org.freedesktop.login1.Manager`.
    ///
    /// Never fails: not reaching a login manager is a *state* of this provider, reported through
    /// [`Provider::availability`], not an error of construction.
    pub async fn connect() -> Self {
        match LoginSystemBus::connect().await {
            Ok(bus) => Self::over(Arc::new(bus)).await,
            Err(error) => Self {
                backing: Backing::Missing(error.message().to_owned()),
            },
        }
    }

    /// A provider over any implementation of the logind surface, probed the same way.
    pub async fn over(bus: Arc<dyn LoginBus>) -> Self {
        let backing = match bus.manager_reachable().await {
            Ok(()) => Backing::Ready(bus),
            Err(error) => Backing::Missing(error.message().to_owned()),
        };
        Self { backing }
    }

    fn bus(&self) -> Result<Arc<dyn LoginBus>, ErrorValue> {
        match &self.backing {
            Backing::Ready(bus) => Ok(Arc::clone(bus)),
            Backing::Missing(reason) => Err(ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("no login manager answers here — {reason}"),
            )
            .with_help(
                "`session` needs systemd-logind. Having none is not the same as nobody being \
                 logged in, so this is a refusal to answer rather than an empty answer.",
            )),
        }
    }
}

/// `ono.session/1`, as `docs/spec/schemas/session.v1.yaml` declares it.
fn session_schema() -> Result<Arc<Schema>, ErrorValue> {
    static SCHEMA: OnceLock<Option<Arc<Schema>>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| builtin_schemas().get(&SchemaId::new("ono.session", 1)))
        .clone()
        .ok_or_else(|| missing_contract("ono.session/1"))
}

/// `ono.user/1`, for the `user` reference of every session.
fn user_schema() -> Result<Arc<Schema>, ErrorValue> {
    static SCHEMA: OnceLock<Option<Arc<Schema>>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| builtin_schemas().get(&SchemaId::new("ono.user", 1)))
        .clone()
        .ok_or_else(|| missing_contract("ono.user/1"))
}

fn missing_contract(id: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderSchemaViolation,
        format!("{SESSION_PROVIDER_ID} advertises {id} but no contract defines it"),
    )
    .with_help("`docs/spec/schemas/` is where the contract lives; `cargo xtask spec-check` reports a file that stopped loading")
}

/// Whether a session belongs to the user `--user` names, by name or by uid.
///
/// The option is typed `ref<ono.user/1>`; what arrives is the name or the number the user wrote,
/// and either identifies the account (spec §23.6 keeps the number authoritative).
fn holder_matches(wanted: Option<&Value>, listing: &SessionListing) -> bool {
    match wanted {
        None => true,
        Some(Value::Int(uid)) => i128::from(listing.uid) == *uid,
        Some(Value::String(text)) => {
            **text == *listing.user_name || text.parse::<u32>().is_ok_and(|uid| uid == listing.uid)
        }
        Some(_) => false,
    }
}

/// One `ono.session/1` record from a listing row and the session's properties.
fn session_record(
    listing: &SessionListing,
    properties: &SessionProperties,
    schema: &Arc<Schema>,
    user_schema: &Arc<Schema>,
) -> Result<RecordValue, ErrorValue> {
    let user = RecordValue::builder(
        Arc::clone(user_schema),
        Provenance::local(SESSION_PROVIDER_ID, user_schema.id().clone()),
    )
    .set("uid", Value::Int(i128::from(listing.uid)))?
    .set("name", optional_text(Some(listing.user_name.clone())))?
    .build();
    let provenance = Provenance::local(SESSION_PROVIDER_ID, schema.id().clone())
        .from_source("org.freedesktop.login1.Manager.ListSessions + Session properties")
        .observed_at(Timestamp::now());
    let since = properties
        .timestamp_usec
        .and_then(|usec| i64::try_from(usec).ok())
        .and_then(|usec| Timestamp::from_microsecond(usec).ok())
        .map_or(Value::Null, Value::Timestamp);
    Ok(RecordValue::builder(Arc::clone(schema), provenance)
        .set("id", Value::string(&listing.id))?
        .set("user", user.into_value())?
        .set("seat", optional_text(listing.seat.clone()))?
        .set("tty", optional_text(properties.tty.clone()))?
        .set("display", optional_text(properties.display.clone()))?
        .set(
            "type",
            enumerated(
                properties.kind.as_deref(),
                &["tty", "x11", "wayland", "mir", "web", "unspecified"],
                None,
            ),
        )?
        .set("class", optional_text(properties.class.clone()))?
        .set(
            "state",
            enumerated(
                properties.state.as_deref(),
                &["online", "active", "closing"],
                Some("unknown"),
            ),
        )?
        .set("remote", properties.remote.map_or(Value::Null, Value::Bool))?
        .set("remote_host", optional_text(properties.remote_host.clone()))?
        .set("service", optional_text(properties.service.clone()))?
        .set(
            "leader",
            properties
                .leader
                .map_or(Value::Null, |pid| Value::Int(i128::from(pid))),
        )?
        .set("scope", optional_text(properties.scope.clone()))?
        .set("since", since)?
        .build())
}

fn optional_text(text: Option<String>) -> Value {
    match text {
        Some(text) if !text.is_empty() => Value::string(&text),
        _ => Value::Null,
    }
}

/// An enum field: the value where it is one the contract declares, the fallback where the
/// contract has one for "not modelled", and null where logind said nothing.
fn enumerated(value: Option<&str>, declared: &[&str], fallback: Option<&str>) -> Value {
    match value {
        None => Value::Null,
        Some(value) if declared.contains(&value) => Value::string(value),
        Some(_) => fallback.map_or(Value::Null, Value::string),
    }
}

#[async_trait::async_trait]
impl Provider for SessionProvider {
    fn id(&self) -> &str {
        SESSION_PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["session"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        session_schema().into_iter().collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("session.list", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        match &self.backing {
            Backing::Ready(_) => Availability::Available,
            Backing::Missing(reason) => Availability::unavailable(reason.clone()),
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let bus = self.bus()?;
        let schema = session_schema()?;
        let user_schema = user_schema()?;
        let wanted_user = query.option_value("user").cloned();
        let selectors: Vec<Selector> = query.selectors().to_vec();
        let limit = query.max();

        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let listings = match bus.list_sessions().await {
                    Ok(listings) => listings,
                    Err(error) => {
                        let _ = sink.fail(error.into_error()).await;
                        return;
                    }
                };
                let mut emitted = 0usize;
                for listing in listings
                    .iter()
                    .filter(|listing| holder_matches(wanted_user.as_ref(), listing))
                {
                    if limit.is_some_and(|limit| emitted >= limit) {
                        return;
                    }
                    let properties = match bus.session_properties(&listing.path).await {
                        Ok(Some(properties)) => properties,
                        // Gone between the listing and the read: a moving system, not a failure.
                        Ok(None) => continue,
                        Err(error) => {
                            // One unreadable session must not cost the others (spec §16.5).
                            if sink.fail(error.into_error()).await.is_err() {
                                return;
                            }
                            continue;
                        }
                    };
                    match session_record(listing, &properties, &schema, &user_schema) {
                        Ok(record) => {
                            if !selectors.iter().all(|selector| selector.matches(&record)) {
                                continue;
                            }
                            emitted += 1;
                            if sink.send(record.into_value()).await.is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            if sink.fail(error).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let query = Query::target("session").with(selector.clone());
        let collected = self.snapshot(&query)?.collect().await;
        Ok(collected
            .values()
            .iter()
            .filter_map(|value| match value {
                Value::Record(record) => ObjectRef::of(record),
                _ => None,
            })
            .collect())
    }
}
