//! Where a pin lives between sessions (spec v0.4 §20.4, §26.4, §46, §46.1).
//!
//! §20.4: "`pin` marks a place as a persistent user landmark. Pins MUST store a resilient
//! selector and identity metadata rather than only a rendered path. If the target cannot be
//! resolved later, the pin remains but reports unresolved state."
//!
//! §46 makes `pins: PinRegistry` session state, and §46.1 allows it to persist where the trail
//! may not: "Trail persistence across sessions is disabled by default for privacy and
//! stale-identity reasons. Pins MAY persist." A pin is something the user chose deliberately, so
//! it persists by default and unconditionally, and it lives beside the shell's other unedited
//! state (ADR-0010) — `$XDG_STATE_HOME/ono/pins.json`, or `~/.local/state/ono/pins.json`.
//!
//! [`ono_spatial_index::PinRegistry`] holds the pins and resolves them (§45.2). This module only
//! reads and writes them, because a path is the session's business and the index has no I/O.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_spatial_core::{SpatialId, SpatialType};
use ono_spatial_index::{Pin, PinRegistry};
use ono_value::ErrorValue;
use serde::{Deserialize, Serialize};

use crate::session::Session;

/// The version of the stored document. A reader that meets a later one says so rather than
/// guessing at fields it does not know.
const VERSION: u32 = 1;

/// Where this session's pins are kept, or `None` when the session has neither
/// `XDG_STATE_HOME` nor a home directory to derive one from.
#[must_use]
pub fn pin_path(session: &Session) -> Option<PathBuf> {
    crate::config::state_dir(session).map(|directory| directory.join("pins.json"))
}

/// One stored pin: the identity it had, and the selector that found it (§20.4).
///
/// Both halves are on purpose. An identity alone breaks the moment an object's identity
/// legitimately changes — a service moved into a container, a process restarted — and a rendered
/// path alone resolves to whatever happens to be at that path now, which is worse. The type and
/// the scope are the identity metadata §20.4 asks for beside the selector: they are what keeps
/// `nginx` the service from being re-bound to `nginx` the process.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPin {
    name: String,
    spatial_id: String,
    selector: String,
    object_type: String,
    scope: String,
    pinned_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Document {
    version: u32,
    pins: Vec<StoredPin>,
}

/// The pins of one user, on disk (§46.1).
#[derive(Debug, Clone)]
pub struct PinStore {
    path: PathBuf,
}

impl PinStore {
    /// The store at `path`.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The store this session uses, or `None` where it has no state directory.
    #[must_use]
    pub fn of(session: &Session) -> Option<Self> {
        pin_path(session).map(Self::at)
    }

    /// Where the pins are kept.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The pins as they were last written.
    ///
    /// A store that does not exist yet is an empty registry, not a failure: a user who has never
    /// pinned anything has no file, and that is the ordinary case.
    ///
    /// # Errors
    ///
    /// - `provider.schema_violation` when the document is not the shape this build writes, or
    ///   carries a version it does not know. A pin file that cannot be read is reported rather
    ///   than silently replaced, because replacing it would delete what the user chose.
    /// - `io.read_failed` when the file exists and cannot be read.
    pub fn load(&self) -> Result<PinRegistry, ErrorValue> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(PinRegistry::new());
            }
            Err(error) => return Err(crate::builtin::io_error(&self.path, &error)),
        };
        let document: Document = serde_json::from_str(&text).map_err(|error| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!(
                    "the pin store at {} is not a pin store: {error}",
                    self.path.display()
                ),
            )
            .with_help("move it aside to start again; nothing else reads or writes this file")
        })?;
        if document.version != VERSION {
            return Err(ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!(
                    "the pin store at {} is version {}, and this build writes version {VERSION}",
                    self.path.display(),
                    document.version
                ),
            ));
        }

        let mut registry = PinRegistry::new();
        for stored in document.pins {
            let Some(spatial_id) = SpatialId::parse(&stored.spatial_id) else {
                return Err(ErrorValue::new(
                    ErrorCode::SpatialIdentityConflict,
                    format!(
                        "the pin `{}` carries `{}`, which is not an identity this shell produced",
                        stored.name, stored.spatial_id
                    ),
                ));
            };
            let Some(object_type) = SpatialType::from_name(&stored.object_type) else {
                return Err(ErrorValue::new(
                    ErrorCode::SpatialUnsupported,
                    format!(
                        "the pin `{}` names the type `{}`, which the geography does not have",
                        stored.name, stored.object_type
                    ),
                ));
            };
            let pinned_at = stored
                .pinned_at
                .parse::<Timestamp>()
                .unwrap_or(Timestamp::UNIX_EPOCH);
            registry.insert(Pin::new(
                stored.name,
                spatial_id,
                stored.selector,
                object_type,
                stored.scope,
                pinned_at,
            ));
        }
        Ok(registry)
    }

    /// Writes `registry` out, creating the state directory if it is not there.
    ///
    /// # Errors
    ///
    /// `io.write_failed` naming the path.
    pub fn save(&self, registry: &PinRegistry) -> Result<(), ErrorValue> {
        if let Some(parent) = self.path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            return Err(crate::builtin::io_error(parent, &error));
        }
        let document = Document {
            version: VERSION,
            pins: registry
                .pins()
                .map(|pin| StoredPin {
                    name: pin.name().to_owned(),
                    spatial_id: pin.spatial_id().to_string(),
                    selector: pin.selector().to_owned(),
                    object_type: pin.object_type().as_str().to_owned(),
                    scope: pin.scope().to_owned(),
                    pinned_at: pin.pinned_at().to_string(),
                })
                .collect(),
        };
        let text = serde_json::to_string_pretty(&document).map_err(|error| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("the pins could not be written as JSON: {error}"),
            )
        })?;
        std::fs::write(&self.path, text + "\n")
            .map_err(|error| crate::builtin::io_error(&self.path, &error))
    }
}

/// The places the user's pins currently point at, resolved against `index` (§20.4).
///
/// A pin whose identity is still a place resolves to it. A pin whose identity is gone is tried
/// again by its own selector, which is what "a resilient selector rather than only a rendered
/// path" buys: a restarted process or a service that moved keeps its pin. A pin that neither
/// answers is left out of the answer and stays in the store, because §20.4 says the pin remains
/// and reports unresolved state rather than disappearing.
#[must_use]
pub fn resolved_pins(
    registry: &PinRegistry,
    index: &ono_spatial_index::SpatialIndex,
    now: Timestamp,
) -> Vec<(String, SpatialId)> {
    use ono_spatial_index::PinResolution;
    use ono_spatial_query::{Resolution, SelectorContext, resolve};

    let mut resolved = Vec::new();
    for pin in registry.pins() {
        let outcome = registry.resolve(
            pin.name(),
            |id| index.contains(id),
            |selector, object_type| {
                match resolve(
                    index,
                    selector,
                    &SelectorContext::anywhere().of_type(object_type),
                    now,
                ) {
                    Resolution::Resolved(candidate) => Some(candidate.spatial_id().clone()),
                    // Two places answering one pin's selector is not a re-binding: §27.3 forbids
                    // an approximate answer from acting, and §29.3 forbids a silent choice
                    // between exact ones. The pin stays unresolved until the user says which.
                    _ => None,
                }
            },
        );
        match outcome {
            Some(PinResolution::Resolved(id) | PinResolution::Rebound(id)) => {
                resolved.push((pin.name().to_owned(), id));
            }
            Some(PinResolution::Unresolved) | None => {}
        }
    }
    resolved
}

// --- the commands (spec v0.4 §6, §20.4) -------------------------------------------------------

/// `pin [--name <name>]` — mark the current place as a persistent user landmark (§20.4, §26.4).
#[derive(Debug)]
pub struct PinPlace {
    store: Option<PinStore>,
}

impl PinPlace {
    /// The implementation registered against `ono.place.pin`.
    #[must_use]
    pub fn new(store: Option<PinStore>) -> Self {
        Self { store }
    }
}

impl ono_command::CommandImpl for PinPlace {
    fn id(&self) -> &str {
        "ono.place.pin"
    }

    fn invoke(
        &self,
        _ctx: &mut ono_command::Invocation<'_>,
    ) -> Result<ono_command::Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("pin"))
    }

    fn invoke_async<'a>(
        &'a self,
        ctx: &'a mut ono_command::Invocation<'_>,
    ) -> ono_command::OutcomeFuture<'a> {
        Box::pin(async move {
            let wanted = ctx
                .arguments()
                .option("name")
                .and_then(crate::spatial::commands::text_of);
            let now = Timestamp::now();
            let store = self.store.as_ref().ok_or_else(no_store)?;

            let mut session = crate::spatial::spatial_session().await;
            let here = session.current_place().clone();
            let (name, selector, object_type) = describe(&session, &here, wanted.as_deref())?;

            let mut pins = store.load()?;
            pins.insert(Pin::new(
                name,
                here.clone(),
                selector,
                object_type,
                session.scope().to_string(),
                now,
            ));
            store.save(&pins)?;
            session.set_pins(pins);

            // The answer is the place, marked: §28.1 makes a selected place a typed value, and a
            // command that changed something the user can see says what it changed.
            let record = crate::spatial::view::place_record(
                session.index(),
                &here,
                session.scope(),
                ono_spatial_core::PermissionState::Available,
                true,
                now,
            )?;
            Ok(ono_command::Outcome::Values(
                ono_pipeline::ValueStream::from_values(vec![ono_value::Value::Record(
                    std::sync::Arc::new(record),
                )]),
            ))
        })
    }
}

/// `unpin [<name>]` — remove a user landmark (§6, §20.4).
#[derive(Debug)]
pub struct UnpinPlace {
    store: Option<PinStore>,
}

impl UnpinPlace {
    /// The implementation registered against `ono.place.unpin`.
    #[must_use]
    pub fn new(store: Option<PinStore>) -> Self {
        Self { store }
    }
}

impl ono_command::CommandImpl for UnpinPlace {
    fn id(&self) -> &str {
        "ono.place.unpin"
    }

    fn invoke(
        &self,
        _ctx: &mut ono_command::Invocation<'_>,
    ) -> Result<ono_command::Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("unpin"))
    }

    fn invoke_async<'a>(
        &'a self,
        ctx: &'a mut ono_command::Invocation<'_>,
    ) -> ono_command::OutcomeFuture<'a> {
        Box::pin(async move {
            let named = ctx
                .arguments()
                .selector("name")
                .and_then(crate::spatial::commands::text_of);
            let store = self.store.as_ref().ok_or_else(no_store)?;

            let mut session = crate::spatial::spatial_session().await;
            let here = session.current_place().clone();
            let mut pins = store.load()?;

            // Without a name, `unpin` removes the pin on the place the user is standing on, which
            // is the mirror of the `pin` that has no name either (§20.4).
            let name = match named {
                Some(name) => name,
                None => pins
                    .pins()
                    .find(|pin| pin.spatial_id() == &here)
                    .map(|pin| pin.name().to_owned())
                    .ok_or_else(|| {
                        ErrorValue::new(
                            ErrorCode::SpatialNotFound,
                            "this place is not pinned, so there is nothing to remove",
                        )
                        .with_help(
                            "`unpin <name>` removes a pin on another place (spec v0.4 §20.4)",
                        )
                    })?,
            };
            if pins.remove(&name).is_none() {
                return Err(ErrorValue::new(
                    ErrorCode::SpatialNotFound,
                    format!("no pin is called `{name}`"),
                ));
            }
            store.save(&pins)?;
            session.set_pins(pins);
            Ok(ono_command::Outcome::Values(
                ono_pipeline::ValueStream::from_values(Vec::new()),
            ))
        })
    }
}

/// Pins `place` if it is not pinned, unpins it if it is, and says which happened (§20.4, §26.4).
///
/// The full-screen view binds `p` to exactly this (§23.3), and the two commands above are the
/// same operation with a name argument — so the store is written the same way from both, and a
/// pin made from the map is a pin `jump @name` reaches.
///
/// # Errors
///
/// `spatial.not_found` when the session no longer knows the place, and the store's own refusal
/// when the pin could not be written.
pub fn toggle_pin(
    store: &PinStore,
    session: &mut crate::spatial::SpatialSessionState,
    place: &SpatialId,
    now: Timestamp,
) -> Result<String, ErrorValue> {
    let mut pins = store.load()?;
    let existing = pins
        .pins()
        .find(|pin| pin.spatial_id() == place)
        .map(|pin| pin.name().to_owned());
    let said = match existing {
        Some(name) => {
            pins.remove(&name);
            format!("unpinned {name}")
        }
        None => {
            let (name, selector, object_type) = describe(session, place, None)?;
            pins.insert(Pin::new(
                name.clone(),
                place.clone(),
                selector,
                object_type,
                session.scope().to_string(),
                now,
            ));
            format!("pinned {name}")
        }
    };
    store.save(&pins)?;
    session.set_pins(pins);
    Ok(said)
}

/// The name, the resilient selector and the identity metadata a pin on `here` stores (§20.4).
///
/// The selector is the name the place answers to rather than the identity it has now: that is the
/// half that survives a restart, and the object type beside it is what keeps `nginx` the service
/// from being re-bound to `nginx` the process.
fn describe(
    session: &crate::spatial::SpatialSessionState,
    here: &SpatialId,
    wanted: Option<&str>,
) -> Result<(String, String, SpatialType), ErrorValue> {
    if let Some(space) = ono_spatial_query::resolve::space_of(here) {
        let name = wanted.unwrap_or(space.label).to_owned();
        return Ok((name, space.id.to_owned(), space.object_type));
    }
    let entry = session.index().get(here).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::SpatialNotFound,
            "this session no longer knows the place it is standing on",
        )
    })?;
    let selector = entry.object().display_name().to_owned();
    let name = wanted.unwrap_or(&selector).to_owned();
    Ok((name, selector, entry.object().object_type()))
}

/// The refusal for a session that has nowhere to keep a pin (§46.1).
fn no_store() -> ErrorValue {
    ErrorValue::new(
        ErrorCode::IoPermissionDenied,
        "this session has no state directory, so a pin could not outlive it",
    )
    .with_help("set `XDG_STATE_HOME` or `HOME` (spec v0.4 §46.1)")
}
