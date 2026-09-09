//! Fixtures the recorder's outcome tests share.
//!
//! Nothing here reaches the developer's own machine: the scope, the clock domain, the instants
//! and the records are all constructed, so a test measures the recorder rather than the host it
//! runs on.

#![allow(dead_code)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_recorder::{RecorderOptions, RecorderSettings, SourceProfile};
use ono_spatial_core::{BootIdentity, SpatialScope, SpatialType};
use ono_temporal_core::{ClockDomain, EvidenceSource};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

/// The host every fixture belongs to.
pub const HOST: &str = "workstation";
/// The boot every fixture belongs to.
pub const BOOT: &str = "boot-a";

/// An instant, or the epoch where the text is not one.
#[must_use]
pub fn instant(text: &str) -> Timestamp {
    text.parse().unwrap_or(Timestamp::UNIX_EPOCH)
}

/// The scope the fixtures record about.
#[must_use]
pub fn scope() -> SpatialScope {
    SpatialScope::host(HOST, BootIdentity::new(HOST, BOOT))
}

/// The clock domain the fixtures record in.
#[must_use]
pub fn domain() -> ClockDomain {
    ClockDomain::new(HOST, Some(BOOT))
}

/// A polled process source, which is what §22.1 says procfs is.
#[must_use]
pub fn procfs_profile() -> SourceProfile {
    SourceProfile::new(
        EvidenceSource::builtin("linux.procfs").unwrap_or_else(EvidenceSource::recorder),
        "linux.procfs",
        SpatialType::Process,
    )
    .polled(ono_value::Duration::from_nanoseconds(5_000_000_000))
}

/// A subscribing source that numbers its events, which is what §22.4 says netlink is.
#[must_use]
pub fn netlink_profile() -> SourceProfile {
    SourceProfile::new(
        EvidenceSource::builtin("linux.netlink").unwrap_or_else(EvidenceSource::recorder),
        "linux.netlink",
        SpatialType::Interface,
    )
    .subscribing()
    .exhaustive(true)
}

/// Options for a recorder whose store is under `directory`.
#[must_use]
pub fn options(directory: &std::path::Path) -> RecorderOptions {
    RecorderOptions::new(scope(), domain())
        .with_store(directory.join("ledger.sqlite3"))
        .with_sources(vec![procfs_profile()])
}

/// The settings §10.4 gives a first start.
#[must_use]
pub fn settings() -> RecorderSettings {
    RecorderSettings::default()
}

/// One process record, with whatever `command` the caller wants persisted or not.
#[must_use]
pub fn process_record(pid: i64, name: &str, command: &[&str]) -> RecordValue {
    let schema_id = SchemaId::new("ono.process", 1);
    let Some(schema) = builtin_schemas().get(&schema_id) else {
        return empty_record();
    };
    let provenance = Provenance::local("linux.procfs", schema_id);
    let arguments: Vec<Value> = command.iter().map(|word| Value::string(word)).collect();
    let builder = RecordValue::builder(schema, provenance);
    let builder = set(builder, "pid", Value::Int(i128::from(pid)));
    let builder = set(builder, "name", Value::string(name));
    let builder = set(builder, "command", Value::List(Arc::from(arguments)));
    let builder = set(
        builder,
        "executable",
        Value::Path(Arc::from(std::path::Path::new("/usr/bin/nginx"))),
    );
    let builder = set(builder, "state", Value::string("running"));
    let builder = set(
        builder,
        "started",
        Value::Timestamp(instant("2026-08-31T11:00:00Z")),
    );
    builder.build()
}

fn set(builder: ono_value::RecordBuilder, name: &str, value: Value) -> ono_value::RecordBuilder {
    let fallback = builder.clone();
    builder.set(name, value).unwrap_or(fallback)
}

fn empty_record() -> RecordValue {
    let schema_id = SchemaId::new("ono.recorder-status", 1);
    let schema = builtin_schemas()
        .get(&schema_id)
        .unwrap_or_else(|| unreachable!("the recorder status contract ships with the binary"));
    RecordValue::builder(schema, Provenance::local("ono.recorder", schema_id)).build()
}
