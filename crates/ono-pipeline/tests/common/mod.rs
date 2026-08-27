//! Fixtures shared by the pipeline test suites.
//!
//! Every helper here asserts an observable outcome or builds input data. None of them knows how
//! the engine is structured internally (AGENTS.md §11).

#![allow(
    dead_code,
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared test fixture states preconditions the same way a #[test] body does, and \
              each test binary uses a different subset of the helpers"
)]

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use ono_value::{
    ErrorValue, FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, Value,
};

/// A test that waits must fail rather than stall the suite, so every await is bounded.
pub const LIMIT: Duration = Duration::from_secs(20);

/// Runs `future` under a hard timeout.
pub async fn within<F: Future>(future: F) -> F::Output {
    match tokio::time::timeout(LIMIT, future).await {
        Ok(output) => output,
        Err(_) => panic!("the pipeline did not finish within {LIMIT:?}: it hung"),
    }
}

/// The schema the transform tests project, filter, sort and group over.
pub fn demo_schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.test.demo", 1), "Demo")
            .field(FieldDef::new("pid", FieldType::Int).required())
            .field(FieldDef::new("name", FieldType::String).nullable())
            .field(FieldDef::new("cpu", FieldType::Float).nullable())
            .field(FieldDef::new("owner", FieldType::Map).nullable())
            .identity(["pid"])
            .build()
            .expect("the demo schema is well formed"),
    )
}

/// A demo record. `cpu` is `None` when the value is unknown, exactly as a provider reports it.
pub fn demo(pid: i128, name: &str, cpu: Option<f64>) -> Value {
    let schema = demo_schema();
    let provenance = Provenance::local("test", schema.id().clone());
    let mut builder = RecordValue::builder(schema, provenance)
        .set("pid", Value::Int(pid))
        .expect("pid is declared")
        .set("name", Value::string(name))
        .expect("name is declared");
    if let Some(cpu) = cpu {
        builder = builder
            .set("cpu", Value::Float(cpu))
            .expect("cpu is declared");
    }
    builder.build().into_value()
}

/// A demo record whose `cpu` field could not be read, which is neither absent nor unknown.
pub fn demo_unreadable(pid: i128, name: &str, error: ErrorValue) -> Value {
    let schema = demo_schema();
    let provenance = Provenance::local("test", schema.id().clone());
    RecordValue::builder(schema, provenance)
        .set("pid", Value::Int(pid))
        .expect("pid is declared")
        .set("name", Value::string(name))
        .expect("name is declared")
        .set("cpu", error.into_value())
        .expect("cpu is declared")
        .build()
        .into_value()
}

/// A demo record carrying a nested `owner` map, for nested-path projection.
pub fn demo_owned(pid: i128, owner: &str) -> Value {
    let schema = demo_schema();
    let provenance = Provenance::local("test", schema.id().clone());
    let mut owner_map = ono_value::MapValue::new();
    owner_map.insert("name".into(), Value::string(owner));
    RecordValue::builder(schema, provenance)
        .set("pid", Value::Int(pid))
        .expect("pid is declared")
        .set("owner", Value::Map(Arc::new(owner_map)))
        .expect("owner is declared")
        .build()
        .into_value()
}

/// The integers `0..count` as values.
pub fn ints(count: i128) -> Vec<Value> {
    (0..count).map(Value::Int).collect()
}

/// Reads the `field` of every record in `values`, for asserting on an outcome rather than a shape.
pub fn field_of(values: &[Value], field: &str) -> Vec<Value> {
    values
        .iter()
        .map(|value| match value {
            Value::Record(record) => record.get(field).cloned().unwrap_or(Value::Null),
            other => other.clone(),
        })
        .collect()
}
