//! Turning a unit's D-Bus properties into an `ono.service/1` record.
//!
//! Everything systemd did not say becomes a null, and nothing becomes a zero (spec §35.3). The
//! two systemd sentinels that make that non-trivial are `MainPID = 0`, which means "this unit
//! runs no main process", and `MemoryCurrent = u64::MAX`, which means "accounting is off, I do
//! not know" — both would read as a real number if copied through.

use std::sync::{Arc, OnceLock};

use jiff::Timestamp;
use ono_value::{
    ByteSize, ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value, builtin_schemas,
};

use crate::UnitProperties;
use crate::provider::PROVIDER_ID;

/// The unit-name suffixes systemd defines. A name carrying one of these is already complete.
const UNIT_SUFFIXES: [&str; 11] = [
    ".service",
    ".socket",
    ".target",
    ".device",
    ".mount",
    ".automount",
    ".swap",
    ".timer",
    ".path",
    ".slice",
    ".scope",
];

/// The `ono.service/1` schema of spec §28.3, as `docs/spec/schemas/service.v1.yaml` fixes it.
///
/// ```
/// let schema = ono_provider_systemd::service_schema();
/// assert_eq!(schema.id().to_string(), "ono.service/1");
/// assert_eq!(schema.identity(), ["provider".into(), "name".into()]);
/// ```
#[must_use]
#[allow(
    clippy::expect_used,
    reason = "AGENTS.md section 16 admits `expect` in a provably unreachable state. `ono.service/1` is one \
              of the schemas the shell ships (spec section 28.3) and \
              crates/ono-value/tests/builtin_schemas.rs turns red the moment it is not; nothing a \
              user does can reach this branch."
)]
pub fn service_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        builtin_schemas()
            .get(&SchemaId::new("ono.service", 1))
            .expect("ono.service/1 is one of the schemas the shell ships")
    }))
}

/// The names to ask systemd about for a name a user typed.
///
/// `get service nginx` means `nginx.service`, the way `systemctl start nginx` does. A name that
/// already carries a unit suffix is asked for exactly as written.
#[must_use]
pub fn unit_name_candidates(name: &str) -> Vec<String> {
    if UNIT_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix) && name.len() > suffix.len())
    {
        return vec![name.to_owned()];
    }
    vec![name.to_owned(), format!("{name}.service")]
}

/// The `ono.service/1` record for one unit.
///
/// # Errors
///
/// Returns `provider.schema_violation` if the shipped schema no longer declares a field this
/// provider fills — the drift `cargo xtask spec-check` exists to catch (spec §36.5).
pub fn unit_record(properties: &UnitProperties) -> Result<RecordValue, ErrorValue> {
    let schema = service_schema();
    let provenance = Provenance::local(PROVIDER_ID, schema.id().clone())
        .from_source("org.freedesktop.systemd1.Manager")
        .observed_at(Timestamp::now());

    let record = RecordValue::builder(schema, provenance)
        .set("name", Value::String(properties.name.as_str().into()))?
        .set("provider", Value::String(PROVIDER_ID.into()))?
        .set("description", text(properties.description.as_deref()))?
        .set("state", Value::String(active_state(properties).into()))?
        .set("substate", text(properties.sub_state.as_deref()))?
        .set("pid", main_pid(properties.main_pid))?
        .set("enabled", enabled(properties.unit_file_state.as_deref()))?
        .set("since", since(properties.state_change_usec))?
        .set("unit_file", unit_file(properties.fragment_path.as_deref()))?
        // An empty list is what a unit with no requirements looks like; systemd has the notion,
        // so `null` here would say something else (spec §35.3, ADR-0239).
        .set(
            "dependencies",
            Value::list(
                properties
                    .dependencies
                    .iter()
                    .map(|unit| Value::String(unit.as_str().into())),
            ),
        )?
        // Provider extensions (spec §10.4): what systemd knows and `ono.service/1` does not
        // declare. Spec §33.2 shows a failed unit's `DETAIL` column, and §41.4 investigates a
        // failure — neither is answerable without the result and the exit status.
        .set_extra("systemd.load_state", text(properties.load_state.as_deref()))
        .set_extra(
            "systemd.unit_file_state",
            text(properties.unit_file_state.as_deref()),
        )
        .set_extra("systemd.memory", memory(properties.memory_current))
        .set_extra("systemd.tasks", tasks(properties.tasks_current))
        .set_extra("systemd.result", text(properties.result.as_deref()))
        .set_extra("systemd.exit_code", exit_code(properties.exec_main_status))
        .build();
    Ok(record)
}

/// Whether the unit is in the state `job` would put it in already, and the sentence saying so.
#[must_use]
pub fn already_in_state(properties: &UnitProperties, job: crate::JobKind) -> Option<String> {
    let state = properties.active_state.as_deref();
    match job {
        crate::JobKind::Start if state == Some("active") => {
            Some(format!("`{}` is already active", properties.name))
        }
        crate::JobKind::Stop if state == Some("inactive") => {
            Some(format!("`{}` is already inactive", properties.name))
        }
        // A restart and a reload always do something, whatever state the unit was in.
        _ => None,
    }
}

fn text(value: Option<&str>) -> Value {
    match value {
        Some(text) if !text.is_empty() => Value::String(text.into()),
        _ => Value::Null,
    }
}

/// The `state` field. It is required, so a state this provider does not model becomes the
/// schema's own `unknown` member rather than a guess (spec §10.5).
fn active_state(properties: &UnitProperties) -> &'static str {
    match properties.active_state.as_deref() {
        Some("active") => "active",
        Some("reloading") => "reloading",
        Some("inactive") => "inactive",
        Some("failed") => "failed",
        Some("activating") => "activating",
        Some("deactivating") => "deactivating",
        _ => "unknown",
    }
}

/// systemd reports `MainPID = 0` for a unit with no main process. Zero is not a pid.
fn main_pid(pid: Option<u32>) -> Value {
    match pid {
        Some(0) | None => Value::Null,
        Some(pid) => Value::Int(i128::from(pid)),
    }
}

/// Whether the unit starts at boot, from `UnitFileState`.
///
/// Only the states that answer the question answer it. `static`, `indirect`, `generated`,
/// `transient` and `linked` describe units that can be neither enabled nor disabled: for those
/// the honest answer is that it is not known, not that it is false (ADR-0014).
fn enabled(unit_file_state: Option<&str>) -> Value {
    match unit_file_state {
        Some("enabled" | "enabled-runtime") => Value::Bool(true),
        Some("disabled" | "masked" | "masked-runtime" | "bad") => Value::Bool(false),
        _ => Value::Null,
    }
}

/// systemd reports a zero timestamp for a unit that has not changed state. That is not 1970.
fn since(usec: Option<u64>) -> Value {
    let Some(usec) = usec.filter(|usec| *usec > 0) else {
        return Value::Null;
    };
    i64::try_from(usec)
        .ok()
        .and_then(|usec| Timestamp::from_microsecond(usec).ok())
        .map_or(Value::Null, Value::Timestamp)
}

/// `FragmentPath` is empty for a transient or generated unit, which has no unit file at all.
fn unit_file(fragment_path: Option<&str>) -> Value {
    match fragment_path {
        Some(path) if !path.is_empty() => Value::Path(std::path::PathBuf::from(path).into()),
        _ => Value::Null,
    }
}

/// `u64::MAX` is systemd's "accounting is off"; it must not become a byte count.
fn memory(bytes: Option<u64>) -> Value {
    match bytes {
        Some(u64::MAX) | None => Value::Null,
        Some(bytes) => Value::ByteSize(ByteSize::from_bytes(u128::from(bytes))),
    }
}

fn tasks(count: Option<u64>) -> Value {
    match count {
        Some(u64::MAX) | None => Value::Null,
        Some(count) => Value::Int(i128::from(count)),
    }
}

fn exit_code(status: Option<i32>) -> Value {
    status.map_or(Value::Null, |status| Value::Int(i128::from(status)))
}
