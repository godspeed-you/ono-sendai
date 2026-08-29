//! Fixtures shared by the index suites: provider records built from the shipped contracts.

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared test fixture is used by some suites and not by others"
)]

use jiff::Timestamp;
use ono_spatial_core::{BootIdentity, Projection, SpatialScope};
use ono_spatial_index::{FreshnessPolicy, ProviderBridge, SpatialIndex};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

/// The instant every fixture observation is made at.
pub const NOW: Timestamp = Timestamp::UNIX_EPOCH;

/// The scope the fixtures are observed in.
pub fn scope() -> SpatialScope {
    SpatialScope::host("testbox", BootIdentity::new("testbox", "boot-a"))
}

/// A projection into that scope.
pub fn projection() -> Projection {
    Projection::new(scope(), NOW)
}

/// An empty index with the default TTL policy.
pub fn index() -> SpatialIndex {
    SpatialIndex::new(FreshnessPolicy::default())
}

/// A bridge into that scope.
pub fn bridge() -> ProviderBridge {
    ProviderBridge::new(projection())
}

/// A record of the shipped schema `schema`, carrying `fields`.
pub fn record(schema: &str, fields: &[(&str, Value)]) -> RecordValue {
    let id: SchemaId = schema.parse().expect("a well-formed schema id");
    let contract = builtin_schemas()
        .get(&id)
        .unwrap_or_else(|| panic!("{schema} is a shipped contract"));
    let mut builder = RecordValue::builder(contract, Provenance::local("test", id));
    for (name, value) in fields {
        builder = builder
            .set(name, value.clone())
            .unwrap_or_else(|error| panic!("{schema}.{name}: {}", error.message()));
    }
    builder.build()
}

/// A socket record with an inode, a state and, where it has one, a peer address.
pub fn socket_with(inode: i64, state: Option<&str>, remote: Option<&str>) -> RecordValue {
    let endpoint = |address: &str| {
        Value::Record(std::sync::Arc::new(record(
            "ono.endpoint/1",
            &[(
                "address",
                Value::Ip(address.parse().expect("a fixture address")),
            )],
        )))
    };
    let mut fields = vec![
        ("protocol", Value::string("tcp")),
        ("family", Value::string("inet")),
        ("inode", Value::Int(i128::from(inode))),
        (
            "local",
            endpoint(if remote.is_some() {
                "10.0.0.1"
            } else {
                "0.0.0.0"
            }),
        ),
    ];
    if let Some(state) = state {
        fields.push(("state", Value::string(state)));
    }
    if let Some(remote) = remote {
        fields.push(("remote", endpoint(remote)));
    }
    record("ono.socket/1", &fields)
}

/// The same record with one more field set — how a fixture adds an owner or a peer.
pub fn with(record: RecordValue, field: &str, value: Value) -> RecordValue {
    let mut builder = RecordValue::builder(record.schema().clone(), record.provenance().clone());
    for declared in record.schema().fields() {
        if declared.name() == field {
            continue;
        }
        if let Some(existing) = record.get(declared.name()) {
            builder = builder
                .set(declared.name(), existing.clone())
                .expect("a field the record already carried");
        }
    }
    builder
        .set(field, value)
        .expect("the field the fixture sets")
        .build()
}

/// A process record: the four fields `ono.process/1` needs to become a place, plus a state.
pub fn process(pid: i64, name: &str, state: &str) -> RecordValue {
    record(
        "ono.process/1",
        &[
            ("pid", Value::Int(i128::from(pid))),
            ("name", Value::string(name)),
            ("state", Value::string(state)),
            ("started", Value::string("1000")),
        ],
    )
}

/// A service record.
pub fn service(name: &str, state: &str) -> RecordValue {
    record(
        "ono.service/1",
        &[
            ("name", Value::string(name)),
            ("state", Value::string(state)),
            ("provider", Value::string("systemd")),
        ],
    )
}
