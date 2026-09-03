//! Object schemas, the registry that holds them, and the evolution rules of spec §10.4.
//!
//! A schema is the public contract of an object type (spec §27.3): an identity, an ordered list
//! of fields and the columns a table shows by default. Records are indexed by schema position
//! (spec §25.1), so the schema is what turns a position back into a name.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use ono_core::ErrorCode;

use crate::{ErrorValue, RecordValue, Value};

/// The stable identity of a schema, such as `ono.process/1` (spec §27.3).
///
/// ```
/// use ono_value::SchemaId;
/// let id: SchemaId = "ono.process/1".parse()?;
/// assert_eq!(id.name(), "ono.process");
/// assert_eq!(id.version(), 1);
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaId {
    name: Arc<str>,
    version: u32,
}

impl SchemaId {
    /// Creates an id from its namespaced name and its version.
    #[must_use]
    pub fn new(name: &str, version: u32) -> Self {
        Self {
            name: name.into(),
            version,
        }
    }

    /// The namespaced name, without the version.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The schema version. A breaking change takes the next version (spec §10.4).
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }
}

impl FromStr for SchemaId {
    type Err = ErrorValue;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || {
            ErrorValue::new(
                ErrorCode::ParseSyntax,
                format!("`{text}` is not a schema id; expected `name/version`"),
            )
        };
        let (name, version) = text.rsplit_once('/').ok_or_else(error)?;
        if name.is_empty() {
            return Err(error());
        }
        Ok(Self::new(
            name,
            version.parse::<u32>().map_err(|_| error())?,
        ))
    }
}

impl fmt::Display for SchemaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.name, self.version)
    }
}

/// The unit a numeric field carries, where the field's own type does not already say it.
///
/// Changing a field's unit changes its meaning, which spec §10.4 makes a breaking change; the
/// unit is therefore part of the contract rather than documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    /// Percent of one, as spec §27.3 declares for `cpu`.
    Percent,
    /// Bytes.
    Bytes,
    /// Seconds.
    Seconds,
    /// A plain count of things.
    Count,
}

impl Unit {
    /// The unit's name as the machine-readable registries spell it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Unit::Percent => "percent",
            Unit::Bytes => "bytes",
            Unit::Seconds => "seconds",
            Unit::Count => "count",
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The declared type of a record field.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    /// Any value at all, for fields whose shape is genuinely open.
    Any,
    /// A boolean.
    Bool,
    /// An integer.
    Int,
    /// A binary floating-point number.
    Float,
    /// An exact decimal number.
    Decimal,
    /// Text.
    String,
    /// Raw bytes.
    Bytes,
    /// A filesystem path.
    Path,
    /// An instant in time.
    Timestamp,
    /// A span of time.
    Duration,
    /// A quantity of information.
    ByteSize,
    /// A percentage.
    Percent,
    /// A regular expression.
    Regex,
    /// A universally unique identifier.
    Uuid,
    /// An IP address.
    Ip,
    /// An IP address with a prefix length.
    IpNetwork,
    /// A TCP or UDP port number.
    Port,
    /// One of a closed set of names.
    Enum(Arc<[Arc<str>]>),
    /// A homogeneous list.
    List(Arc<FieldType>),
    /// A map with string keys.
    Map,
    /// A nested record of a named schema.
    Record(SchemaId),
    /// A reference to an object of a named schema, carrying its identity rather than the object.
    Ref(SchemaId),
    /// A structured error.
    Error,
}

impl FieldType {
    /// A list of `inner`.
    #[must_use]
    pub fn list(inner: FieldType) -> Self {
        Self::List(Arc::new(inner))
    }

    /// A closed set of names.
    #[must_use]
    pub fn enumeration(variants: &[&str]) -> Self {
        Self::Enum(variants.iter().map(|name| Arc::from(*name)).collect())
    }

    /// Whether `value` is an acceptable inhabitant of this type.
    ///
    /// `Value::Null` is never accepted here: whether a field may be unknown is the field's
    /// nullability, not its type (spec §10.5).
    #[must_use]
    pub fn accepts(&self, value: &Value) -> bool {
        match (self, value) {
            (FieldType::Any, _) => !matches!(value, Value::Null),
            (FieldType::Bool, Value::Bool(_))
            | (FieldType::Int, Value::Int(_))
            | (FieldType::Float, Value::Float(_))
            | (FieldType::Decimal, Value::Decimal(_))
            | (FieldType::String, Value::String(_))
            | (FieldType::Bytes, Value::Bytes(_))
            | (FieldType::Path, Value::Path(_))
            | (FieldType::Timestamp, Value::Timestamp(_))
            | (FieldType::Duration, Value::Duration(_))
            | (FieldType::ByteSize, Value::ByteSize(_))
            | (FieldType::Percent, Value::Percent(_))
            | (FieldType::Regex, Value::Regex(_))
            | (FieldType::Uuid, Value::Uuid(_))
            | (FieldType::Ip, Value::Ip(_))
            | (FieldType::IpNetwork, Value::IpNetwork(_))
            | (FieldType::Port, Value::Port(_))
            | (FieldType::Map, Value::Map(_))
            | (FieldType::Error, Value::Error(_)) => true,
            (FieldType::Enum(variants), Value::String(text)) => {
                variants.iter().any(|variant| **variant == **text)
            }
            (FieldType::List(inner), Value::List(items)) => {
                items.iter().all(|item| inner.accepts(item))
            }
            (FieldType::Record(id), Value::Record(record)) => record.schema_id() == id,
            // A reference is an identity, and providers legitimately carry it as a name, a
            // number, an identity map or the resolved object itself.
            (FieldType::Ref(_), value) => !matches!(value, Value::Null),
            _ => false,
        }
    }

    /// Whether a field of this type can become `other` without losing information.
    ///
    /// Spec §10.4 calls widening a numeric representation "usually compatible if lossless"; the
    /// widenings recognised here are the ones that are.
    #[must_use]
    pub fn widens_to(&self, other: &FieldType) -> bool {
        matches!(
            (self, other),
            (FieldType::Int, FieldType::Float) | (FieldType::Int, FieldType::Decimal)
        )
    }

    /// The type's name as the machine-readable registries spell it.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            FieldType::Any => "any".to_owned(),
            FieldType::Bool => "bool".to_owned(),
            FieldType::Int => "int".to_owned(),
            FieldType::Float => "float".to_owned(),
            FieldType::Decimal => "decimal".to_owned(),
            FieldType::String => "string".to_owned(),
            FieldType::Bytes => "bytes".to_owned(),
            FieldType::Path => "path".to_owned(),
            FieldType::Timestamp => "timestamp".to_owned(),
            FieldType::Duration => "duration".to_owned(),
            FieldType::ByteSize => "bytesize".to_owned(),
            FieldType::Percent => "percent".to_owned(),
            FieldType::Regex => "regex".to_owned(),
            FieldType::Uuid => "uuid".to_owned(),
            FieldType::Ip => "ip".to_owned(),
            FieldType::IpNetwork => "ipnetwork".to_owned(),
            FieldType::Port => "port".to_owned(),
            FieldType::Enum(variants) => {
                let names: Vec<&str> = variants.iter().map(|name| &**name).collect();
                format!("enum<{}>", names.join("|"))
            }
            FieldType::List(inner) => format!("list<{}>", inner.name()),
            FieldType::Map => "map".to_owned(),
            FieldType::Record(id) => format!("record<{id}>"),
            FieldType::Ref(id) => format!("ref<{id}>"),
            FieldType::Error => "error".to_owned(),
        }
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

/// One field of a schema (spec §27.3).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    name: Arc<str>,
    ty: FieldType,
    required: bool,
    nullable: bool,
    unit: Option<Unit>,
    doc: Option<Arc<str>>,
}

impl FieldDef {
    /// Declares a field. It is neither required nor nullable until said otherwise.
    #[must_use]
    pub fn new(name: &str, ty: FieldType) -> Self {
        Self {
            name: name.into(),
            ty,
            required: false,
            nullable: false,
            unit: None,
            doc: None,
        }
    }

    /// Marks the field as one every record must carry with a known value.
    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Marks the field as one whose value may be unknown (spec §10.5).
    #[must_use]
    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }

    /// Declares the unit the field's numbers are in.
    #[must_use]
    pub fn with_unit(mut self, unit: Unit) -> Self {
        self.unit = Some(unit);
        self
    }

    /// Documents the field, for `help` and the generated reference.
    #[must_use]
    pub fn with_doc(mut self, doc: &str) -> Self {
        self.doc = Some(doc.into());
        self
    }

    /// The field's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The field's declared type.
    #[must_use]
    pub const fn ty(&self) -> &FieldType {
        &self.ty
    }

    /// Whether every record must carry a known value here.
    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.required
    }

    /// Whether the value may be unknown.
    #[must_use]
    pub const fn is_nullable(&self) -> bool {
        self.nullable
    }

    /// The unit the field's numbers are in, if it carries one.
    #[must_use]
    pub const fn unit(&self) -> Option<Unit> {
        self.unit
    }

    /// The field's documentation, if it has any.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }
}

/// The contract of an object type (spec §10.3, §27.3).
#[derive(Debug, Clone)]
pub struct Schema {
    id: SchemaId,
    name: Arc<str>,
    fields: Arc<[FieldDef]>,
    positions: HashMap<Arc<str>, usize>,
    identity: Arc<[Arc<str>]>,
    identity_fallback: Arc<[Arc<str>]>,
    /// `identity` followed by `identity_fallback`, assembled once so
    /// [`Schema::identity_for`] can hand back a slice rather than build one per record.
    identity_extended: Arc<[Arc<str>]>,
    default_view: Arc<[Arc<str>]>,
    doc: Option<Arc<str>>,
}

impl Schema {
    /// Starts building a schema.
    #[must_use]
    pub fn builder(id: SchemaId, name: &str) -> SchemaBuilder {
        SchemaBuilder {
            id,
            name: name.into(),
            fields: Vec::new(),
            identity: Vec::new(),
            identity_fallback: Vec::new(),
            default_view: Vec::new(),
            doc: None,
        }
    }

    /// A schema with no fields at all, used only as a last resort when a built-in definition
    /// fails to build (see `builtin.rs`).
    pub(crate) fn empty(id: SchemaId, name: &str) -> Self {
        Self {
            id,
            name: name.into(),
            fields: Arc::from(Vec::new()),
            positions: HashMap::new(),
            identity: Arc::from(Vec::new()),
            identity_fallback: Arc::from(Vec::new()),
            identity_extended: Arc::from(Vec::new()),
            default_view: Arc::from(Vec::new()),
            doc: None,
        }
    }

    /// The schema's stable id.
    #[must_use]
    pub const fn id(&self) -> &SchemaId {
        &self.id
    }

    /// The schema's semantic type name, such as `Process`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every field, in declaration order. A record's fields are stored in this order.
    #[must_use]
    pub fn fields(&self) -> &[FieldDef] {
        &self.fields
    }

    /// The number of fields a record of this schema holds.
    #[must_use]
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// The field declared under `name`, if the schema declares one.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&FieldDef> {
        self.position_of(name)
            .and_then(|index| self.fields.get(index))
    }

    /// The storage position of `name`, if the schema declares it.
    #[must_use]
    pub fn position_of(&self, name: &str) -> Option<usize> {
        self.positions.get(name).copied()
    }

    /// The fields that identify an object of this type (spec §27.3).
    #[must_use]
    pub fn identity(&self) -> &[Arc<str>] {
        &self.identity
    }

    /// The fields that join the identity when the declared one is incomplete (ADR-0553).
    ///
    /// A declared identity can be right about the objects that carry it and silent about the
    /// ones that do not: a filesystem with no UUID, a `TIME_WAIT` connection with no inode. The
    /// fallback names what tells those apart, and it is used only for them.
    #[must_use]
    pub fn identity_fallback(&self) -> &[Arc<str>] {
        &self.identity_fallback
    }

    /// The fields that identify `record`.
    ///
    /// The declared identity, unless one of its components is null in this record — then the
    /// fallback fields join it, because a null is the absence of a value rather than a value
    /// (spec §10.5), and an identity with a hole in it cannot tell two objects apart. The
    /// declared components stay in the identity either way: what the record does say about which
    /// object it is remains part of the answer.
    #[must_use]
    pub fn identity_for(&self, record: &RecordValue) -> &[Arc<str>] {
        if self.identity_fallback.is_empty() {
            return &self.identity;
        }
        let complete = self
            .identity
            .iter()
            .all(|name| record.get(name).is_some_and(|value| !value.is_null()));
        if complete {
            &self.identity
        } else {
            &self.identity_extended
        }
    }

    /// The columns a table shows unless the user asks for others (spec §27.3).
    ///
    /// A view is a rendering strategy, never part of the data (spec §10.7).
    #[must_use]
    pub fn default_view(&self) -> &[Arc<str>] {
        &self.default_view
    }

    /// Whether `name` is one of the default view's columns.
    #[must_use]
    pub fn is_default_view_column(&self, name: &str) -> bool {
        self.default_view.iter().any(|column| &**column == name)
    }

    /// The schema's documentation, if it has any.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }

    /// Checks a record against this schema (spec §35.3).
    ///
    /// A field carrying an [`ErrorValue`] is accepted whatever its declared type is: spec §10.5
    /// requires a failed access to stay visible, and calling that a schema violation would push
    /// providers back towards fabricating data.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::ProviderSchemaViolation`] naming the first field that violates the
    /// contract.
    pub fn validate(&self, record: &RecordValue) -> Result<(), ErrorValue> {
        if record.schema_id() != &self.id {
            return Err(self.violation(format!(
                "record claims schema {} but was checked against {}",
                record.schema_id(),
                self.id
            )));
        }
        for (index, field) in self.fields.iter().enumerate() {
            let value = record.field_at(index).unwrap_or(&Value::Null);
            if matches!(value, Value::Error(_)) {
                continue;
            }
            if matches!(value, Value::Null) {
                if field.required || !field.nullable {
                    return Err(self.violation(format!(
                        "required field `{}` of {} is null",
                        field.name, self.id
                    )));
                }
                continue;
            }
            if !field.ty.accepts(value) {
                return Err(self.violation(format!(
                    "field `{}` of {} must be {} but holds {}",
                    field.name,
                    self.id,
                    field.ty,
                    value.type_name()
                )));
            }
        }
        for key in record.extra().keys() {
            if self.positions.contains_key(key) {
                return Err(self.violation(format!(
                    "extension `{key}` shadows the declared field of the same name on {}",
                    self.id
                )));
            }
        }
        Ok(())
    }

    fn violation(&self, message: String) -> ErrorValue {
        ErrorValue::new(ErrorCode::ProviderSchemaViolation, message)
            .with_target(crate::ValueRef::name(&self.id.to_string()))
    }
}

impl PartialEq for Schema {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.fields == other.fields
            && self.identity == other.identity
            && self.identity_fallback == other.identity_fallback
            && self.default_view == other.default_view
    }
}

/// Assembles a [`Schema`], checking that identity and default view name declared fields.
#[derive(Debug, Clone)]
pub struct SchemaBuilder {
    id: SchemaId,
    name: Arc<str>,
    fields: Vec<FieldDef>,
    identity: Vec<Arc<str>>,
    identity_fallback: Vec<Arc<str>>,
    default_view: Vec<Arc<str>>,
    doc: Option<Arc<str>>,
}

impl SchemaBuilder {
    /// Appends a field. Declaration order is storage order (spec §25.1).
    #[must_use]
    pub fn field(mut self, field: FieldDef) -> Self {
        self.fields.push(field);
        self
    }

    /// Declares which fields identify an object (spec §27.3).
    #[must_use]
    pub fn identity<'a>(mut self, fields: impl IntoIterator<Item = &'a str>) -> Self {
        self.identity = fields.into_iter().map(Arc::from).collect();
        self
    }

    /// Declares the fields that join the identity when the declared one is incomplete
    /// (ADR-0553).
    #[must_use]
    pub fn identity_fallback<'a>(mut self, fields: impl IntoIterator<Item = &'a str>) -> Self {
        self.identity_fallback = fields.into_iter().map(Arc::from).collect();
        self
    }

    /// Declares the default table columns (spec §27.3).
    #[must_use]
    pub fn default_view<'a>(mut self, columns: impl IntoIterator<Item = &'a str>) -> Self {
        self.default_view = columns.into_iter().map(Arc::from).collect();
        self
    }

    /// Documents the schema.
    #[must_use]
    pub fn doc(mut self, doc: &str) -> Self {
        self.doc = Some(doc.into());
        self
    }

    /// Builds the schema.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeUnknownField`] if a field is declared twice, or if the identity
    /// or the default view names a field the schema does not declare.
    pub fn build(self) -> Result<Schema, ErrorValue> {
        let mut positions = HashMap::with_capacity(self.fields.len());
        for (index, field) in self.fields.iter().enumerate() {
            if positions.insert(Arc::from(field.name()), index).is_some() {
                return Err(ErrorValue::new(
                    ErrorCode::TypeUnknownField,
                    format!("{} declares `{}` twice", self.id, field.name()),
                ));
            }
        }
        for name in self
            .identity
            .iter()
            .chain(self.identity_fallback.iter())
            .chain(self.default_view.iter())
        {
            if !positions.contains_key(name) {
                return Err(ErrorValue::new(
                    ErrorCode::TypeUnknownField,
                    format!("{} refers to `{name}` but does not declare it", self.id),
                ));
            }
        }
        let identity_extended: Arc<[Arc<str>]> = self
            .identity
            .iter()
            .chain(self.identity_fallback.iter())
            .cloned()
            .collect();
        Ok(Schema {
            id: self.id,
            name: self.name,
            fields: self.fields.into(),
            positions,
            identity: self.identity.into(),
            identity_fallback: self.identity_fallback.into(),
            identity_extended,
            default_view: self.default_view.into(),
            doc: self.doc,
        })
    }
}

/// Every schema the shell knows about, by id.
///
/// ```
/// use ono_value::{SchemaId, builtin_schemas};
/// let process = builtin_schemas().get(&SchemaId::new("ono.process", 1));
/// assert!(process.is_some());
/// ```
#[derive(Debug, Clone, Default)]
pub struct SchemaRegistry {
    schemas: BTreeMap<SchemaId, Arc<Schema>>,
}

impl SchemaRegistry {
    /// An empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            schemas: BTreeMap::new(),
        }
    }

    /// Registers a schema, returning the shared handle callers should keep.
    ///
    /// Registering the identical schema twice is accepted, so two independent providers may both
    /// declare a schema they share.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::ResolveAmbiguous`] if a different schema already claims the id.
    pub fn register(&mut self, schema: Schema) -> Result<Arc<Schema>, ErrorValue> {
        if let Some(existing) = self.schemas.get(&schema.id) {
            if **existing == schema {
                return Ok(Arc::clone(existing));
            }
            return Err(ErrorValue::new(
                ErrorCode::ResolveAmbiguous,
                format!(
                    "{} is already registered as a different contract",
                    schema.id
                ),
            ));
        }
        let id = schema.id.clone();
        let handle = Arc::new(schema);
        self.schemas.insert(id, Arc::clone(&handle));
        Ok(handle)
    }

    /// The schema registered under `id`.
    #[must_use]
    pub fn get(&self, id: &SchemaId) -> Option<Arc<Schema>> {
        self.schemas.get(id).map(Arc::clone)
    }

    /// Every registered id, in order.
    pub fn ids(&self) -> impl Iterator<Item = &SchemaId> {
        self.schemas.keys()
    }

    /// Every registered schema, in id order.
    pub fn schemas(&self) -> impl Iterator<Item = &Arc<Schema>> {
        self.schemas.values()
    }

    /// The number of registered schemas.
    #[must_use]
    pub fn len(&self) -> usize {
        self.schemas.len()
    }

    /// Whether the registry holds no schemas.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
    }

    /// Checks a record against the registered version of its schema (spec §35.3).
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::ResolveTargetNotFound`] if the record's schema is not registered, or
    /// [`ErrorCode::ProviderSchemaViolation`] if the record does not satisfy it.
    pub fn validate(&self, record: &RecordValue) -> Result<(), ErrorValue> {
        let id = record.schema_id();
        let schema = self.get(id).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("no schema is registered as {id}"),
            )
        })?;
        schema.validate(record)
    }
}

/// Whether a schema change keeps existing readers working (spec §10.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Compatibility {
    /// Existing readers keep working.
    Compatible,
    /// Existing readers break; the schema needs a new version.
    Breaking,
}

/// What kind of change one schema made relative to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaChangeKind {
    /// The semantic type name changed.
    SchemaRenamed,
    /// The identity field list changed.
    IdentityChanged,
    /// The default view columns changed.
    DefaultViewChanged,
    /// A field was added.
    FieldAdded,
    /// A field was removed. A rename shows up as a removal plus an addition.
    FieldRemoved,
    /// A field's type widened without losing information.
    FieldTypeWidened,
    /// A field's type changed in a way that loses information.
    FieldTypeChanged,
    /// A field's unit changed, which changes its meaning.
    FieldUnitChanged,
    /// A field became required, or stopped being required.
    FieldRequirementChanged,
    /// A field that was never null may now be null.
    FieldNullabilityWidened,
    /// A field that could be null no longer can be.
    FieldNullabilityNarrowed,
}

/// One difference between two versions of a schema.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaChange {
    field: Option<Arc<str>>,
    kind: SchemaChangeKind,
    compatibility: Compatibility,
    detail: Arc<str>,
}

impl SchemaChange {
    /// The field the change is about, if it is about one field.
    #[must_use]
    pub fn field(&self) -> Option<&str> {
        self.field.as_deref()
    }

    /// What kind of change this is.
    #[must_use]
    pub const fn kind(&self) -> SchemaChangeKind {
        self.kind
    }

    /// Whether this single change breaks existing readers.
    #[must_use]
    pub const fn compatibility(&self) -> Compatibility {
        self.compatibility
    }

    /// A human explanation of the change.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for SchemaChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

/// Every difference between two versions of a schema.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SchemaDiff {
    changes: Vec<SchemaChange>,
}

impl SchemaDiff {
    /// Every difference, in the order they were found.
    #[must_use]
    pub fn changes(&self) -> &[SchemaChange] {
        &self.changes
    }

    /// The strictest verdict any single change carries.
    #[must_use]
    pub fn compatibility(&self) -> Compatibility {
        if self
            .changes
            .iter()
            .any(|change| change.compatibility == Compatibility::Breaking)
        {
            Compatibility::Breaking
        } else {
            Compatibility::Compatible
        }
    }

    /// Whether every change keeps existing readers working.
    #[must_use]
    pub fn is_compatible(&self) -> bool {
        self.compatibility() == Compatibility::Compatible
    }

    /// Only the changes that break existing readers.
    pub fn breaking(&self) -> impl Iterator<Item = &SchemaChange> {
        self.changes
            .iter()
            .filter(|change| change.compatibility == Compatibility::Breaking)
    }
}

/// Classifies the change from `old` to `new` against the rules of spec §10.4.
///
/// A rename cannot be told apart from a removal plus an addition by looking at two schemas, so it
/// is reported as both — and both are breaking, which is the answer the rule demands anyway.
///
/// ```
/// use ono_value::{Compatibility, FieldDef, FieldType, Schema, SchemaId, classify_change};
/// let old = Schema::builder(SchemaId::new("ono.demo", 1), "Demo")
///     .field(FieldDef::new("id", FieldType::Int).required())
///     .build()?;
/// let new = Schema::builder(SchemaId::new("ono.demo", 1), "Demo")
///     .field(FieldDef::new("id", FieldType::Int).required())
///     .field(FieldDef::new("note", FieldType::String).nullable())
///     .build()?;
/// assert_eq!(classify_change(&old, &new).compatibility(), Compatibility::Compatible);
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[must_use]
pub fn classify_change(old: &Schema, new: &Schema) -> SchemaDiff {
    let mut changes = Vec::new();

    if old.name() != new.name() {
        changes.push(SchemaChange {
            field: None,
            kind: SchemaChangeKind::SchemaRenamed,
            compatibility: Compatibility::Breaking,
            detail: format!("the type was renamed from {} to {}", old.name(), new.name()).into(),
        });
    }
    if old.identity() != new.identity() || old.identity_fallback() != new.identity_fallback() {
        changes.push(SchemaChange {
            field: None,
            kind: SchemaChangeKind::IdentityChanged,
            compatibility: Compatibility::Breaking,
            detail: "the identity fields changed".into(),
        });
    }
    if old.default_view() != new.default_view() {
        changes.push(SchemaChange {
            field: None,
            kind: SchemaChangeKind::DefaultViewChanged,
            compatibility: Compatibility::Compatible,
            detail: "the default view columns changed".into(),
        });
    }

    for field in old.fields() {
        let Some(next) = new.field(field.name()) else {
            changes.push(SchemaChange {
                field: Some(field.name().into()),
                kind: SchemaChangeKind::FieldRemoved,
                compatibility: Compatibility::Breaking,
                detail: format!("field `{}` was removed", field.name()).into(),
            });
            continue;
        };
        changes.extend(classify_field(field, next));
    }

    for field in new.fields() {
        if old.field(field.name()).is_some() {
            continue;
        }
        let compatibility = if field.is_required() {
            Compatibility::Breaking
        } else {
            Compatibility::Compatible
        };
        changes.push(SchemaChange {
            field: Some(field.name().into()),
            kind: SchemaChangeKind::FieldAdded,
            compatibility,
            detail: format!(
                "field `{}` was added as {}",
                field.name(),
                if field.is_required() {
                    "required"
                } else {
                    "optional"
                }
            )
            .into(),
        });
    }

    SchemaDiff { changes }
}

fn classify_field(old: &FieldDef, new: &FieldDef) -> Vec<SchemaChange> {
    let mut changes = Vec::new();
    let name: Arc<str> = old.name().into();

    if old.ty() != new.ty() {
        let widened = old.ty().widens_to(new.ty());
        changes.push(SchemaChange {
            field: Some(Arc::clone(&name)),
            kind: if widened {
                SchemaChangeKind::FieldTypeWidened
            } else {
                SchemaChangeKind::FieldTypeChanged
            },
            compatibility: if widened {
                Compatibility::Compatible
            } else {
                Compatibility::Breaking
            },
            detail: format!(
                "field `{name}` changed type from {} to {}",
                old.ty(),
                new.ty()
            )
            .into(),
        });
    }
    if old.unit() != new.unit() {
        changes.push(SchemaChange {
            field: Some(Arc::clone(&name)),
            kind: SchemaChangeKind::FieldUnitChanged,
            compatibility: Compatibility::Breaking,
            detail: format!("field `{name}` changed unit, and with it its meaning").into(),
        });
    }
    if old.is_required() != new.is_required() {
        changes.push(SchemaChange {
            field: Some(Arc::clone(&name)),
            kind: SchemaChangeKind::FieldRequirementChanged,
            compatibility: Compatibility::Breaking,
            detail: format!("field `{name}` changed whether it is required").into(),
        });
    }
    if old.is_nullable() != new.is_nullable() {
        let widened = new.is_nullable();
        changes.push(SchemaChange {
            field: Some(name.clone()),
            kind: if widened {
                SchemaChangeKind::FieldNullabilityWidened
            } else {
                SchemaChangeKind::FieldNullabilityNarrowed
            },
            compatibility: if widened {
                Compatibility::Breaking
            } else {
                Compatibility::Compatible
            },
            detail: if widened {
                format!("field `{name}` may now be null")
            } else {
                format!("field `{name}` can no longer be null")
            }
            .into(),
        });
    }
    changes
}
