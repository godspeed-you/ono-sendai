//! `select`: project fields and expressions into records (spec §53).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ono_value::{ErrorValue, FieldDef, FieldStep, RecordValue, Schema, SchemaId, Value};

use crate::function::KeyFn;
use crate::schemas::{projection_schema, provenance, selection_schema};
use crate::{Transform, ValueStream};

/// One step of a projected field path, as written with `.` or `?.` (spec §11.4).
#[derive(Debug, Clone)]
pub struct PathSegment {
    name: Arc<str>,
    optional: bool,
}

impl PathSegment {
    /// A `.name` step: a field the value must have.
    #[must_use]
    pub fn required(name: &str) -> Self {
        Self {
            name: name.into(),
            optional: false,
        }
    }

    /// A `?.name` step: an absent field, or a null receiver, projects as null.
    #[must_use]
    pub fn optional(name: &str) -> Self {
        Self {
            name: name.into(),
            optional: true,
        }
    }

    /// The field this step reads.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    fn step(&self) -> FieldStep<'_> {
        if self.optional {
            FieldStep::optional(&self.name)
        } else {
            FieldStep::required(&self.name)
        }
    }
}

/// One field of a `select` projection.
pub struct SelectField {
    name: Arc<str>,
    source: Source,
}

enum Source {
    Path(Vec<PathSegment>),
    Computed(Box<dyn KeyFn>),
}

impl SelectField {
    /// `select pid` — keeps a top-level field under its own name.
    #[must_use]
    pub fn field(name: &str) -> Self {
        Self::path([PathSegment::required(name)])
    }

    /// `select user.name` — reads a nested path.
    ///
    /// The output is named after the last segment, which is the column a user expects to see;
    /// [`named`](Self::named) overrides it when two paths would collide. An empty path projects
    /// the value itself, under the name `value`.
    #[must_use]
    pub fn path(segments: impl IntoIterator<Item = PathSegment>) -> Self {
        let segments: Vec<PathSegment> = segments.into_iter().collect();
        let name = segments
            .last()
            .map_or_else(|| Arc::from("value"), |segment| segment.name.clone());
        Self {
            name,
            source: Source::Path(segments),
        }
    }

    /// `select {mem_mb: memory / 1MiB}` — a field computed from the whole value.
    ///
    /// The expression arrives already resolved, as a function: turning source text into one is
    /// the evaluator's job (ADR-0005).
    #[must_use]
    pub fn computed(name: &str, compute: impl KeyFn) -> Self {
        Self {
            name: name.into(),
            source: Source::Computed(Box::new(compute)),
        }
    }

    /// Renames the projected field.
    #[must_use]
    pub fn named(mut self, name: &str) -> Self {
        self.name = name.into();
        self
    }

    /// The name the field is projected under.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Reads this field out of one value.
    fn read(&self, value: &Value) -> Result<Value, ErrorValue> {
        match &self.source {
            Source::Path(segments) => {
                let steps: Vec<FieldStep<'_>> = segments.iter().map(PathSegment::step).collect();
                value.follow(&steps)
            }
            Source::Computed(compute) => compute.key(value),
        }
    }
}

/// Projects fields and expressions into records (spec §53).
///
/// A failed field read is projected as the [`Value::Error`] it is, not as null: spec §10.5
/// insists that "unknown" and "could not be read" stay apart, and flattening them here would
/// undo that one stage after a provider took care to distinguish them. The error stays in the
/// field, where the renderer and any later `where` can see it, rather than being duplicated onto
/// the error channel as if the row had failed — the row did not fail, one of its fields did.
pub struct Select {
    fields: Vec<SelectField>,
    /// The shape a projection has when nothing better is known: every field `any`, no unit.
    schema: Arc<Schema>,
    /// The shape a projection has over records of one source schema, which carries the source
    /// field's declaration wherever a projected field is one field of it (ADR-0555).
    ///
    /// Derived on first sight of a source schema and kept, because a stream is almost always
    /// homogeneous and deriving it per record would be a schema build per row.
    derived: Mutex<HashMap<SchemaId, Arc<Schema>>>,
}

impl Select {
    /// Projects `fields`, in the order given.
    ///
    /// # Errors
    ///
    /// Returns [`ono_core::ErrorCode::TypeUnknownField`] when two fields would be projected
    /// under the same name.
    pub fn new(fields: impl IntoIterator<Item = SelectField>) -> Result<Self, ErrorValue> {
        let fields: Vec<SelectField> = fields.into_iter().collect();
        let names: Vec<Arc<str>> = fields.iter().map(|field| field.name.clone()).collect();
        let schema = selection_schema(&names)?;
        Ok(Self {
            fields,
            schema,
            derived: Mutex::new(HashMap::new()),
        })
    }
}

impl Transform for Select {
    fn name(&self) -> &'static str {
        "select"
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        // A projection is one value in, one value out: boundedness is unchanged (spec §11.1).
        let boundedness = input.boundedness();
        input.stage(boundedness, move |mut input, sink| async move {
            while let Some(value) = input.next_value(&sink).await {
                let projected = self.project(&value);
                if sink.send(projected).await.is_err() {
                    return;
                }
            }
        })
    }
}

impl Select {
    /// The declaration of the one source field `field` reads, where it reads one.
    ///
    /// A single-segment path names a field of the source schema, and that is the whole of what
    /// can be carried through: a nested path ends in a field of another schema this record only
    /// refers to, and a computed expression produces a value no field declared.
    fn source_field<'a>(field: &SelectField, source: &'a Schema) -> Option<&'a FieldDef> {
        match &field.source {
            Source::Path(segments) => match segments.as_slice() {
                [segment] => source.field(segment.name()),
                _ => None,
            },
            Source::Computed(_) => None,
        }
    }

    /// The projection schema for records of `source`, derived once and then remembered.
    fn schema_for(&self, source: &Schema) -> Arc<Schema> {
        if let Ok(cache) = self.derived.lock()
            && let Some(schema) = cache.get(source.id())
        {
            return Arc::clone(schema);
        }
        let derived = projection_schema(self.fields.iter().map(|field| {
            (
                field.name.clone(),
                Self::source_field(field, source).cloned(),
            )
        }))
        .unwrap_or_else(|_| Arc::clone(&self.schema));
        if let Ok(mut cache) = self.derived.lock() {
            cache.insert(source.id().clone(), Arc::clone(&derived));
        }
        derived
    }

    fn project(&self, value: &Value) -> Value {
        let schema = value.as_record().map_or_else(
            |_| Arc::clone(&self.schema),
            |record| self.schema_for(record.schema()),
        );
        let mut builder = RecordValue::builder(Arc::clone(&schema), provenance(&schema));
        for field in &self.fields {
            let projected = match field.read(value) {
                Ok(projected) => projected,
                Err(error) => error.into_value(),
            };
            builder = match builder.set(&field.name, projected) {
                Ok(builder) => builder,
                // Unreachable: the schema was derived from exactly these names.
                Err(_) => return Value::Null,
            };
        }
        builder.build().into_value()
    }
}

crate::transform::debug_as_name!(Select);
