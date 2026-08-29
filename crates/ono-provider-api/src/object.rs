//! Object identity, references and events.
//!
//! Spec §31.13 requires that an extension operating on Ono values receives "schema identity,
//! typed units, null/absent distinction, object identity, provenance where policy permits,
//! stream cancellation, partial errors" — never rendered text. Spec §31.14 fixes the event
//! envelope. Both are defined here rather than inside KUANG/11, so that a core provider and a
//! plugin speak the same vocabulary and neither is a special case of the other.

use std::fmt;
use std::sync::Arc;

use jiff::Timestamp;
use ono_value::{Provenance, RecordValue, SchemaId, Value};

/// What makes two observations of the same thing the same thing.
///
/// The identity is the schema plus the values of the fields the schema declares as its identity
/// (spec §27.3). For a process that is `(pid, started)`, which is what keeps a signal from
/// reaching a recycled pid (ADR-0015 T13).
#[derive(Debug, Clone)]
pub struct ObjectId {
    schema: SchemaId,
    values: Vec<Value>,
    /// The identity values rendered once, so equality and hashing are total and cheap.
    ///
    /// `Value` is deliberately not `Eq` or `Hash`, because it can hold a float and a float is
    /// neither. An identity, though, must be usable as a map key: that is what lets a live view
    /// update the row for a process rather than print a second one (spec §18.3). Rendering the
    /// identity fields once gives a total, reflexive relation over exactly the values that make
    /// two observations the same object.
    key: String,
}

impl PartialEq for ObjectId {
    fn eq(&self, other: &Self) -> bool {
        self.schema == other.schema && self.key == other.key
    }
}

impl Eq for ObjectId {}

impl std::hash::Hash for ObjectId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.schema.hash(state);
        self.key.hash(state);
    }
}

impl ObjectId {
    /// An identity from a schema and its identity values.
    #[must_use]
    pub fn new(schema: SchemaId, values: impl IntoIterator<Item = Value>) -> Self {
        let values: Vec<Value> = values.into_iter().collect();
        let key = identity_key(&values);
        Self {
            schema,
            values,
            key,
        }
    }

    /// The identity of `record`, or `None` when the record states none.
    ///
    /// A record states no identity in two ways, and both mean the same thing. Its schema may
    /// declare none — those records are values rather than objects, a projection or a
    /// measurement, and giving them a synthetic identity would let a live view claim two
    /// unrelated rows were the same row. Or it may declare identity fields and the record supply
    /// **none** of them: a null is the absence of a value, not a value (spec §35.3), so a record
    /// whose every identity component is null says nothing about which object it is, and two such
    /// records are not thereby the same object (spec §2.17, ADR-0231).
    ///
    /// One present component is enough. `ono.route/1` identifies by five fields and the default
    /// route has no destination; that route is still an object.
    #[must_use]
    pub fn of(record: &RecordValue) -> Option<Self> {
        let schema = record.schema();
        if schema.identity().is_empty() {
            return None;
        }
        let values: Vec<Value> = schema
            .identity()
            .iter()
            .map(|field| record.get(field).cloned().unwrap_or(Value::Null))
            .collect();
        if values.iter().all(|value| matches!(value, Value::Null)) {
            return None;
        }
        let key = identity_key(&values);
        Some(Self {
            schema: schema.id().clone(),
            values,
            key,
        })
    }

    /// The schema the object belongs to.
    #[must_use]
    pub fn schema(&self) -> &SchemaId {
        &self.schema
    }

    /// The identity field values, in the order the schema declares them.
    #[must_use]
    pub fn values(&self) -> &[Value] {
        &self.values
    }
}

impl ObjectId {
    /// Whether `text` is already visible in the identity: the whole rendering, or one of the
    /// identity values as written — a mount point, a path, a unit name.
    #[must_use]
    pub fn shows(&self, text: &str) -> bool {
        self.to_string() == text
            || self
                .values
                .iter()
                .any(|value| ono_value::canonical_text(value).is_ok_and(|shown| shown == text))
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[", self.schema)?;
        for (index, value) in self.values.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            match ono_value::canonical_text(value) {
                Ok(text) => f.write_str(&text)?,
                Err(_) => f.write_str("?")?,
            }
        }
        f.write_str("]")
    }
}

/// The identity values as one comparable string.
fn identity_key(values: &[Value]) -> String {
    let mut key = String::new();
    for value in values {
        // The separator cannot appear in a rendered scalar, so two different identities cannot
        // render to the same key by splitting differently.
        key.push('\u{1f}');
        match ono_value::canonical_text(value) {
            Ok(text) => key.push_str(&text),
            // A value with no text form is still distinguishable by its type and its debug form,
            // which is enough for an identity nobody can render.
            Err(_) => key.push_str(&format!("{value:?}")),
        }
    }
    key
}

/// An object's identity together with enough of it to show a person which one it is.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectRef {
    id: ObjectId,
    label: String,
    provenance: Provenance,
}

impl ObjectRef {
    /// A reference to `record`, or `None` when its schema declares no identity.
    #[must_use]
    pub fn of(record: &RecordValue) -> Option<Self> {
        let id = ObjectId::of(record)?;
        // One label rule for an object (ADR-0226): the short form its schema declares, which is
        // what a person reads wherever the object is shown — a graph node, an `ActionResult`
        // target, a refusal. A schema that declares none falls back to the first default-view
        // column outside the identity, because the identity is printed beside this label and
        // what it must add is the thing the identity does not show.
        let label = crate::label::declared_label(record)
            .or_else(|| {
                record
                    .schema()
                    .default_view()
                    .iter()
                    .find(|column| !record.schema().identity().contains(column))
                    .and_then(|column| record.get(column))
                    .and_then(|value| ono_value::canonical_text(value).ok())
            })
            .unwrap_or_else(|| id.to_string());
        Some(Self {
            id,
            label,
            provenance: record.provenance().clone(),
        })
    }

    /// A reference to an object a provider *named* but does not serve as a record of its own.
    ///
    /// The far end of a connection, the control group a process reports, the namespace a pid was
    /// read in: each is a real thing a provider told us about inside another object's record, and
    /// spec v0.4 §42.3 requires an edge to reach "a known spatial object, an explicit unresolved
    /// endpoint object, or a remote/opaque reference with correct type" rather than a dangling
    /// id. This is how the second and third of those are built.
    ///
    /// `provenance` is the provenance of the record that named it, never a new attribution: the
    /// observation belongs to whoever made it (spec §26).
    #[must_use]
    pub fn derived(id: ObjectId, label: impl Into<String>, provenance: Provenance) -> Self {
        Self {
            id,
            label: label.into(),
            provenance,
        }
    }

    /// The object's identity.
    #[must_use]
    pub fn id(&self) -> &ObjectId {
        &self.id
    }

    /// A short human label for the object.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Where the observation came from.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// What happened to an object, from spec §31.14.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// Part of the initial state, before any change.
    Snapshot,
    /// The object came into existence, or into the query's scope.
    Added,
    /// The object's fields changed.
    Changed,
    /// The object ceased to exist, or left the query's scope.
    Removed,
}

impl EventKind {
    /// The name spec §31.14 gives the kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            EventKind::Snapshot => "snapshot",
            EventKind::Added => "added",
            EventKind::Changed => "changed",
            EventKind::Removed => "removed",
        }
    }
}

/// One change to one object, in the envelope spec §31.14 defines.
///
/// Ordering is per object and per provider. The runtime makes no claim of a total order across
/// unrelated providers, because none exists (spec §31.14).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectEvent {
    kind: EventKind,
    object_id: ObjectId,
    schema: SchemaId,
    at: Timestamp,
    sequence: Option<u64>,
    value: Option<Arc<RecordValue>>,
    changed_fields: Option<Vec<String>>,
    provenance: Provenance,
}

impl ObjectEvent {
    /// An event carrying part of the initial state.
    ///
    /// # Panics
    ///
    /// Panics if `record`'s schema declares no identity; a stream of unidentifiable objects
    /// cannot be a change stream, and a provider that tries has a schema bug.
    #[must_use]
    pub fn snapshot(record: &RecordValue) -> Self {
        Self::new(EventKind::Snapshot, record, None)
    }

    /// An event saying an object appeared.
    #[must_use]
    pub fn added(record: &RecordValue) -> Self {
        Self::new(EventKind::Added, record, None)
    }

    /// An event saying an object changed, naming the fields that moved.
    #[must_use]
    pub fn changed<I, S>(record: &RecordValue, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(
            EventKind::Changed,
            record,
            Some(fields.into_iter().map(Into::into).collect()),
        )
    }

    /// An event saying an object went away. It carries the last value seen, so a consumer can
    /// report what disappeared rather than only that something did.
    #[must_use]
    pub fn removed(record: &RecordValue) -> Self {
        Self::new(EventKind::Removed, record, None)
    }

    fn new(kind: EventKind, record: &RecordValue, changed_fields: Option<Vec<String>>) -> Self {
        let id = ObjectId::of(record).unwrap_or_else(|| {
            // A schema with no identity cannot take part in a change stream; the alternative is
            // to invent an identity, which would let two unrelated rows claim to be the same one.
            ObjectId::new(record.schema_id().clone(), [Value::Null])
        });
        Self {
            kind,
            schema: record.schema_id().clone(),
            object_id: id,
            at: record
                .provenance()
                .observed()
                .unwrap_or_else(Timestamp::now),
            sequence: None,
            provenance: record.provenance().clone(),
            value: Some(Arc::new(record.clone())),
            changed_fields,
        }
    }

    /// Numbers the event within its provider's stream.
    #[must_use]
    pub fn with_sequence(mut self, sequence: u64) -> Self {
        self.sequence = Some(sequence);
        self
    }

    /// What happened.
    #[must_use]
    pub fn kind(&self) -> EventKind {
        self.kind
    }

    /// Which object it happened to.
    #[must_use]
    pub fn object_id(&self) -> &ObjectId {
        &self.object_id
    }

    /// The object's schema.
    #[must_use]
    pub fn schema(&self) -> &SchemaId {
        &self.schema
    }

    /// When it happened, as the provider observed it.
    #[must_use]
    pub fn at(&self) -> Timestamp {
        self.at
    }

    /// The event's position in its provider's stream, where the provider numbers them.
    #[must_use]
    pub fn sequence(&self) -> Option<u64> {
        self.sequence
    }

    /// The object's value, where the event carries one.
    #[must_use]
    pub fn value(&self) -> Option<&Arc<RecordValue>> {
        self.value.as_ref()
    }

    /// Which fields moved, for a change.
    #[must_use]
    pub fn changed_fields(&self) -> Option<&[String]> {
        self.changed_fields.as_deref()
    }

    /// Where the observation came from.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}
