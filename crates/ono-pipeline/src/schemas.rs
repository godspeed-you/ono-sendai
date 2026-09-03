//! The schemas of the records the transforms construct.
//!
//! `select`, `group`, `measure`, `join` and `diff` all produce records that no provider defines,
//! so the shapes live here. Spec §53 leaves the record-construction syntax of `select` open and
//! declines to freeze `join` and `diff` at all; what is *not* open is that the results are
//! records rather than "preformatted headings", so each has a declared schema a renderer and
//! `inspect` can read.
//!
//! Every field is nullable and typed `any` unless the shape guarantees otherwise, because a
//! transform composes over whatever the stream carries and spec §35.3 forbids fabricating a
//! value to fit a narrower type.

use std::sync::{Arc, OnceLock};

use ono_value::{ErrorValue, FieldDef, FieldType, Provenance, Schema, SchemaId, Value};

/// The provider name recorded on every record a transform builds, so `inspect` can say where a
/// row came from even when it came from the shell itself (spec §25.2).
const PROVIDER: &str = "ono.pipeline";

/// Provenance for a record this crate constructed.
pub(crate) fn provenance(schema: &Schema) -> Provenance {
    Provenance::local(PROVIDER, schema.id().clone())
}

/// The schema `select` projects into: the chosen output names, in the order the user wrote them.
///
/// The id is shared by every projection because a projection is anonymous — it is the shape the
/// user just asked for, not a registered object type. Two different `select`s in two different
/// pipelines therefore carry the same id with different fields, which is sound as long as the
/// schema travels with the record, as it does here, and is why the projection schema is never
/// added to the [`ono_value::SchemaRegistry`].
///
/// # Errors
///
/// Returns [`ono_core::ErrorCode::TypeUnknownField`] when two projected fields would share a
/// name — two columns called `name` is a question with no answer.
pub(crate) fn selection_schema(names: &[Arc<str>]) -> Result<Arc<Schema>, ErrorValue> {
    projection_schema(names.iter().map(|name| (Arc::clone(name), None)))
}

/// The same, with the declaration each projected field was read from where there is one.
///
/// A projection of a field is still that field: `cpu` projected out of `ono.process/1` is the
/// float carrying `unit: percent` the contract declares, and a renderer reads that declaration
/// to print `2.1%` (ADR-0419, ADR-0555). Where a projected field has no single source field —
/// a computed expression, a nested path, a source that is not a record — the declaration is the
/// `any` this module's header describes, because spec §35.3 forbids asserting a type the value
/// may not have.
///
/// Nullability is not copied. A projection reads a value that may be absent from the value it
/// read and may be the error of a failed read, so every projected field is nullable whatever
/// its source said.
///
/// # Errors
///
/// Returns [`ono_core::ErrorCode::TypeUnknownField`] when two projected fields would share a
/// name.
pub(crate) fn projection_schema(
    fields: impl IntoIterator<Item = (Arc<str>, Option<FieldDef>)>,
) -> Result<Arc<Schema>, ErrorValue> {
    let mut builder = Schema::builder(SchemaId::new("ono.selection", 1), "Selection")
        .doc("The record shape a `select` projection produces.");
    let mut names: Vec<Arc<str>> = Vec::new();
    for (name, source) in fields {
        let mut field = FieldDef::new(
            &name,
            source
                .as_ref()
                .map_or(FieldType::Any, |source| source.ty().clone()),
        )
        .nullable();
        if let Some(unit) = source.as_ref().and_then(FieldDef::unit) {
            field = field.with_unit(unit);
        }
        if let Some(doc) = source.as_ref().and_then(FieldDef::doc) {
            field = field.with_doc(doc);
        }
        builder = builder.field(field);
        names.push(name);
    }
    builder
        .default_view(names.iter().map(|name| &**name))
        .build()
        .map(Arc::new)
}

/// The `Group<T>` record of spec §53: a key, how many members it has, and the members.
pub(crate) fn grouping_schema() -> Result<Arc<Schema>, ErrorValue> {
    static SCHEMA: OnceLock<Result<Arc<Schema>, ErrorValue>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            Schema::builder(SchemaId::new("ono.grouping", 1), "Grouping")
                .doc("One group of a `group` transform (spec §53).")
                .field(FieldDef::new("key", FieldType::Any).nullable())
                .field(FieldDef::new("count", FieldType::Int).required())
                .field(FieldDef::new("items", FieldType::list(FieldType::Any)).nullable())
                .default_view(["key", "count"])
                .build()
                .map(Arc::new)
        })
        .clone()
}

/// The statistics `measure` reports, as typed values rather than formatted text (spec §53).
pub(crate) fn measure_schema() -> Result<Arc<Schema>, ErrorValue> {
    static SCHEMA: OnceLock<Result<Arc<Schema>, ErrorValue>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            Schema::builder(SchemaId::new("ono.measure", 1), "Measure")
                .doc("The statistics a `measure` transform reports (spec §53).")
                .field(FieldDef::new("count", FieldType::Int).required())
                .field(FieldDef::new("skipped", FieldType::Int).required())
                .field(FieldDef::new("sum", FieldType::Any).nullable())
                .field(FieldDef::new("mean", FieldType::Any).nullable())
                .field(FieldDef::new("median", FieldType::Any).nullable())
                .field(FieldDef::new("min", FieldType::Any).nullable())
                .field(FieldDef::new("max", FieldType::Any).nullable())
                .field(FieldDef::new("stddev", FieldType::Float).nullable())
                .field(FieldDef::new("percentiles", FieldType::Map).nullable())
                .default_view(["count", "sum", "mean", "min", "max"])
                .build()
                .map(Arc::new)
        })
        .clone()
}

/// One matched pair of a `join`.
///
/// The two sides stay separate rather than being merged into one flat record: merging would
/// require inventing a rule for two fields of the same name, and spec §53 explicitly declines to
/// freeze the shape of `join` until real use cases justify one.
pub(crate) fn join_schema() -> Result<Arc<Schema>, ErrorValue> {
    static SCHEMA: OnceLock<Result<Arc<Schema>, ErrorValue>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            Schema::builder(SchemaId::new("ono.join", 1), "Join")
                .doc("One matched pair of a `join` transform (spec §53).")
                .field(FieldDef::new("key", FieldType::Any).nullable())
                .field(FieldDef::new("left", FieldType::Any).nullable())
                .field(FieldDef::new("right", FieldType::Any).nullable())
                .default_view(["key", "left", "right"])
                .build()
                .map(Arc::new)
        })
        .clone()
}

/// One difference between two snapshots.
pub(crate) fn diff_schema() -> Result<Arc<Schema>, ErrorValue> {
    static SCHEMA: OnceLock<Result<Arc<Schema>, ErrorValue>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            Schema::builder(SchemaId::new("ono.diff", 1), "Diff")
                .doc("One difference a `diff` transform found (spec §53).")
                .field(
                    FieldDef::new(
                        "change",
                        FieldType::enumeration(&["added", "removed", "changed"]),
                    )
                    .required(),
                )
                .field(FieldDef::new("key", FieldType::Any).nullable())
                .field(FieldDef::new("left", FieldType::Any).nullable())
                .field(FieldDef::new("right", FieldType::Any).nullable())
                .default_view(["change", "key"])
                .build()
                .map(Arc::new)
        })
        .clone()
}

/// Null stands in for a value the caller does not have, which is what null means (spec §10.5).
pub(crate) fn or_null(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}
