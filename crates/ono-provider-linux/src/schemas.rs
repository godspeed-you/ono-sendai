//! The object contracts the Linux providers answer with.
//!
//! Every one of them is a schema of `docs/spec/schemas/`, which AGENTS.md §5 makes the public
//! contract, and every one is taken from [`ono_value::builtin_schemas`] rather than restated
//! here. That matters more than it looks: two definitions of `ono.file/1` in one process would
//! let a provider satisfy its own copy of the contract while breaking the one a command
//! type-checks against, and nothing would notice until a user's `where mode == "0644"` quietly
//! matched nothing.
//!
//! What this module adds is the small amount a provider actually needs: the ids by name, and a
//! lookup that reports a missing definition as a structured error instead of panicking.

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_value::{ErrorValue, Schema, SchemaId, SchemaRegistry};

/// `ono.process/1` — `docs/spec/schemas/process.v1.yaml`.
#[must_use]
pub fn process_id() -> SchemaId {
    SchemaId::new("ono.process", 1)
}

/// `ono.file/1` — `docs/spec/schemas/file.v1.yaml`.
#[must_use]
pub fn file_id() -> SchemaId {
    SchemaId::new("ono.file", 1)
}

/// `ono.user/1` — `docs/spec/schemas/user.v1.yaml`.
#[must_use]
pub fn user_id() -> SchemaId {
    SchemaId::new("ono.user", 1)
}

/// `ono.group/1` — `docs/spec/schemas/group.v1.yaml`.
#[must_use]
pub fn group_id() -> SchemaId {
    SchemaId::new("ono.group", 1)
}

/// `ono.mount/1` — `docs/spec/schemas/mount.v1.yaml`.
#[must_use]
pub fn mount_id() -> SchemaId {
    SchemaId::new("ono.mount", 1)
}

/// `ono.filesystem/1` — `docs/spec/schemas/filesystem.v1.yaml`.
#[must_use]
pub fn filesystem_id() -> SchemaId {
    SchemaId::new("ono.filesystem", 1)
}

/// `ono.device/1` — `docs/spec/schemas/device.v1.yaml`.
#[must_use]
pub fn device_id() -> SchemaId {
    SchemaId::new("ono.device", 1)
}

/// `ono.env-var/1` — `docs/spec/schemas/env-var.v1.yaml`.
#[must_use]
pub fn env_var_id() -> SchemaId {
    SchemaId::new("ono.env-var", 1)
}

/// Every id the Linux providers produce records of.
#[must_use]
pub fn ids() -> Vec<SchemaId> {
    vec![
        process_id(),
        file_id(),
        user_id(),
        group_id(),
        mount_id(),
        filesystem_id(),
        device_id(),
        env_var_id(),
    ]
}

/// The registry these providers resolve their contracts through.
///
/// ```
/// use ono_provider_linux::schemas;
/// let process = schemas::require(&schemas::process_id())?;
/// let identity: Vec<&str> = process.identity().iter().map(|field| &**field).collect();
/// assert_eq!(identity, ["pid", "started"]);
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[must_use]
pub fn registry() -> &'static SchemaRegistry {
    ono_value::builtin_schemas()
}

/// The schema `id` names.
///
/// # Errors
///
/// Returns [`ErrorCode::ProviderSchemaViolation`] when the contract is missing from the
/// registry, which means a file under `docs/spec/schemas/` stopped loading. Every provider entry
/// point goes through here, so a contract bug surfaces as an error a user can read rather than
/// as a panic in the middle of a pipeline.
pub fn require(id: &SchemaId) -> Result<Arc<Schema>, ErrorValue> {
    registry().get(id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("the Linux providers advertise {id} but no contract defines it"),
        )
        .with_help(
            "`docs/spec/schemas/` is where the contract lives; `cargo xtask spec-check` \
                    reports a file that stopped loading",
        )
    })
}
