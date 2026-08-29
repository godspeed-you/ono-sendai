//! Records: schema-bound values whose fields are stored by position (spec §10.3, §25.1).
//!
//! Spec §25.1 stores a `SchemaId` in the record. A record here keeps the whole [`Schema`] behind
//! an `Arc` instead, because a field access by name must not need a registry lookup to find the
//! position it maps to; the id is still one dereference away and the `Arc` keeps a record as
//! cheap to clone as spec §34 needs it to be.

use std::sync::Arc;

use ono_core::ErrorCode;

use crate::{ErrorValue, MapValue, Provenance, Schema, SchemaId, Value};

/// What happened when a field was read (spec §10.5).
///
/// Spec §10.5 requires three outcomes to stay apart: a field the schema never declared, a
/// declared field whose value is unknown, and a field whose access failed. Collapsing them is
/// exactly the ambiguity Ono exists to remove, so they are three variants rather than one
/// nullable value.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldAccess {
    /// The schema does not declare this field and no extension carries it.
    Absent,
    /// The field exists, and its value is not known.
    Unknown,
    /// The field exists and holds this value.
    Known(Value),
    /// The field exists, and reading it failed.
    Failed(Arc<ErrorValue>),
}

impl FieldAccess {
    /// Whether the field is not part of this record at all.
    #[must_use]
    pub const fn is_absent(&self) -> bool {
        matches!(self, FieldAccess::Absent)
    }

    /// Whether the field exists with an unknown value.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, FieldAccess::Unknown)
    }

    /// Whether reading the field failed.
    #[must_use]
    pub const fn is_failed(&self) -> bool {
        matches!(self, FieldAccess::Failed(_))
    }

    /// The stored value, or `None` when the field is not part of the record.
    ///
    /// An unknown value becomes [`Value::Null`] and a failed access becomes [`Value::Error`], so
    /// the outcome survives the conversion.
    #[must_use]
    pub fn into_value(self) -> Option<Value> {
        match self {
            FieldAccess::Absent => None,
            FieldAccess::Unknown => Some(Value::Null),
            FieldAccess::Known(value) => Some(value),
            FieldAccess::Failed(error) => Some(Value::Error(error)),
        }
    }

    /// The value, treating an absent field and a failed access as errors.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeUnknownField`] for an absent field, or the recorded failure for a
    /// failed access.
    pub fn require(self, field: &str) -> Result<Value, ErrorValue> {
        match self {
            FieldAccess::Absent => Err(ErrorValue::new(
                ErrorCode::TypeUnknownField,
                format!("no field `{field}` on this record"),
            )),
            FieldAccess::Unknown => Ok(Value::Null),
            FieldAccess::Known(value) => Ok(value),
            FieldAccess::Failed(error) => Err((*error).clone()),
        }
    }
}

/// One step of a field path, as written with `.` or `?.` (spec §11.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldStep<'a> {
    name: &'a str,
    optional: bool,
}

impl<'a> FieldStep<'a> {
    /// A `.name` step: an absent field is an error.
    #[must_use]
    pub const fn required(name: &'a str) -> Self {
        Self {
            name,
            optional: false,
        }
    }

    /// A `?.name` step: an absent field, or a null receiver, yields null.
    #[must_use]
    pub const fn optional(name: &'a str) -> Self {
        Self {
            name,
            optional: true,
        }
    }

    /// The field being read.
    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }

    /// Whether the step tolerates an absent field.
    #[must_use]
    pub const fn is_optional(&self) -> bool {
        self.optional
    }
}

/// A record: a schema, its field values by position, provider extensions and provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordValue {
    schema: Arc<Schema>,
    fields: Vec<Value>,
    extra: MapValue,
    provenance: Provenance,
}

impl RecordValue {
    /// Starts building a record of `schema`, with every field unknown.
    #[must_use]
    pub fn builder(schema: Arc<Schema>, provenance: Provenance) -> RecordBuilder {
        let fields = vec![Value::Null; schema.field_count()];
        RecordBuilder {
            record: RecordValue {
                schema,
                fields,
                extra: MapValue::new(),
                provenance,
            },
        }
    }

    /// The schema this record satisfies.
    #[must_use]
    pub fn schema(&self) -> &Arc<Schema> {
        &self.schema
    }

    /// The schema's stable id.
    #[must_use]
    pub fn schema_id(&self) -> &SchemaId {
        self.schema.id()
    }

    /// Where the record came from (spec §25.2).
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The provider-specific extensions (spec §10.4).
    #[must_use]
    pub const fn extra(&self) -> &MapValue {
        &self.extra
    }

    /// The value stored at a schema position.
    #[must_use]
    pub fn field_at(&self, index: usize) -> Option<&Value> {
        self.fields.get(index)
    }

    /// The value stored under `name`, whether it is a declared field or an extension.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        match self.schema.position_of(name) {
            Some(index) => self.fields.get(index),
            None => self.extra.get(name),
        }
    }

    /// Reads a field, keeping the three outcomes of spec §10.5 apart.
    ///
    /// An error stored in a field the schema declares `error` is that field's *value* — spec
    /// §11.5 declares `ActionResult.error` exactly so — and is reported `Known`. An error in a
    /// field of any other type is the failure to read it (ADR-0215).
    #[must_use]
    pub fn access(&self, name: &str) -> FieldAccess {
        match self.get(name) {
            None => FieldAccess::Absent,
            Some(Value::Null) => FieldAccess::Unknown,
            Some(Value::Error(error)) if !self.declares_error(name) => {
                FieldAccess::Failed(Arc::clone(error))
            }
            Some(value) => FieldAccess::Known(value.clone()),
        }
    }

    /// Whether the schema declares `name` as holding a structured error.
    fn declares_error(&self, name: &str) -> bool {
        self.schema
            .field(name)
            .is_some_and(|field| matches!(field.ty(), crate::FieldType::Error))
    }

    /// Whether two records hold the same data, whoever observed them and when.
    ///
    /// Ordinary equality compares provenance too, which is right for asking "is this the same
    /// observation?". It is wrong for asking "did anything change?": two readings of one
    /// unchanged object differ in the instant each was observed, and that is a fact about the
    /// reading, not about the object (spec §26, §10.7, ADR-0229). Schema, declared fields and
    /// provider extensions all take part; nothing else does.
    ///
    /// ```
    /// use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};
    /// use std::sync::Arc;
    ///
    /// let schema = builtin_schemas().get(&SchemaId::new("ono.user", 1)).expect("the contract");
    /// let user = |source: &str| {
    ///     RecordValue::builder(
    ///         Arc::clone(&schema),
    ///         Provenance::local("nss", schema.id().clone()).from_source(source),
    ///     )
    ///     .set("uid", Value::Int(0))
    ///     .expect("uid is a field")
    ///     .build()
    /// };
    /// assert!(user("one reading").same_data(&user("another reading")));
    /// assert_ne!(user("one reading"), user("another reading"));
    /// ```
    #[must_use]
    pub fn same_data(&self, other: &Self) -> bool {
        self.schema.id() == other.schema.id()
            && self.fields == other.fields
            && self.extra == other.extra
    }

    /// The identity fields and their values (spec §27.3).
    #[must_use]
    pub fn identity(&self) -> MapValue {
        self.schema
            .identity()
            .iter()
            .map(|name| {
                (
                    Arc::clone(name),
                    self.get(name).cloned().unwrap_or(Value::Null),
                )
            })
            .collect()
    }

    /// A reference to this record, by schema and identity.
    #[must_use]
    pub fn to_ref(&self) -> crate::ValueRef {
        crate::ValueRef::object(self.schema_id().clone(), self.identity())
    }

    /// Checks the record against its own schema (spec §35.3).
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::ProviderSchemaViolation`] naming the first field that violates the
    /// contract.
    pub fn validate(&self) -> Result<(), ErrorValue> {
        self.schema.validate(self)
    }

    /// The record as an ordinary value.
    #[must_use]
    pub fn into_value(self) -> Value {
        Value::Record(Arc::new(self))
    }
}

/// Fills the fields of a record, refusing names the schema does not declare.
#[derive(Debug, Clone)]
pub struct RecordBuilder {
    record: RecordValue,
}

impl RecordBuilder {
    /// Stores `value` in the declared field `name`.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeUnknownField`] if the schema does not declare `name`. A provider
    /// with data the schema does not cover uses [`set_extra`](Self::set_extra) instead.
    pub fn set(mut self, name: &str, value: Value) -> Result<Self, ErrorValue> {
        let index = self.record.schema.position_of(name).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeUnknownField,
                format!("{} has no field `{name}`", self.record.schema.id()),
            )
        })?;
        if let Some(slot) = self.record.fields.get_mut(index) {
            *slot = value;
        }
        Ok(self)
    }

    /// Stores `value` under `name`, ignoring a name the schema does not declare.
    ///
    /// Only for call sites that build a record of a schema they own, where an unknown name is a
    /// bug in this crate rather than something a caller can cause.
    pub(crate) fn set_known(mut self, name: &str, value: Value) -> Self {
        if let Some(index) = self.record.schema.position_of(name)
            && let Some(slot) = self.record.fields.get_mut(index)
        {
            *slot = value;
        }
        self
    }

    /// Stores a provider-specific extension under a namespaced key (spec §10.4).
    #[must_use]
    pub fn set_extra(mut self, key: &str, value: Value) -> Self {
        self.record.extra.insert(key.into(), value);
        self
    }

    /// Replaces the provenance the record was started with.
    #[must_use]
    pub fn provenance(mut self, provenance: Provenance) -> Self {
        self.record.provenance = provenance;
        self
    }

    /// Finishes the record. Fields never set stay unknown, which is what null means (spec §10.5).
    #[must_use]
    pub fn build(self) -> RecordValue {
        self.record
    }
}
