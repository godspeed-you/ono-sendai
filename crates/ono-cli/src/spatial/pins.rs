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
