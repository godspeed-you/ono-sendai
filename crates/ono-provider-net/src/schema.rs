//! The object contracts these providers answer against, taken from
//! [`ono_value::builtin_schemas`] so that one definition of each serves the whole process.

use std::sync::Arc;

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value};

/// `ono.dns-record/1` — `docs/spec/schemas/dns-record.v1.yaml`.
#[must_use]
pub fn dns_record_id() -> SchemaId {
    SchemaId::new("ono.dns-record", 1)
}

/// The schema `id` names.
///
/// # Errors
///
/// `provider.schema_violation` when the contract is missing from the registry, which means a
/// file under `docs/spec/schemas/` stopped loading.
pub fn require(id: &SchemaId) -> Result<Arc<Schema>, ErrorValue> {
    ono_value::builtin_schemas().get(id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("the network providers advertise {id} but no contract defines it"),
        )
        .with_help("`docs/spec/schemas/` is where the contract lives")
    })
}

/// Assembles one record with the provenance spec §25.2 requires on every observation.
pub(crate) fn build(
    schema: &Arc<Schema>,
    provider: &str,
    source: &str,
    fields: Vec<(&str, Value)>,
) -> Result<RecordValue, ErrorValue> {
    let provenance = Provenance::local(provider, schema.id().clone())
        .from_source(source)
        .observed_at(Timestamp::now());
    let mut builder = RecordValue::builder(Arc::clone(schema), provenance);
    for (name, value) in fields {
        builder = builder.set(name, value)?;
    }
    Ok(builder.build())
}
