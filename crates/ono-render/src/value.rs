//! The bridge from a value to the text a terminal shows (spec §13.1, §13.4).
//!
//! Spec §5 splits the work in two and this module is the seam: a provider produces values and
//! never formats them, the renderer decides what they look like, and turning a value into text
//! never changes the value. Everything here is therefore a read: a [`Renderer`] borrows a
//! [`Value`] and returns new presentation objects beside it.
//!
//! Two text forms exist on purpose. The *human* form of spec §13.4 lives here — `1.20 GiB`,
//! `4d 03h`, a timestamp in the reader's own day. The *canonical* form lives in `ono-value` as
//! [`ono_value::canonical_text`] and is what [`View::Raw`] and every serializer use, because
//! spec §33.5 wants canonical values unless a human rendering was explicitly asked for.

use jiff::Timestamp;
use jiff::tz::TimeZone;
use ono_value::{FieldDef, FieldType, RecordValue, Schema, Unit, Value, canonical_text};

use crate::table::{Align, Cell, Column, Table};
use crate::theme::{Token, sanitise};
use crate::tree::TreeNode;

/// A built-in view of spec §13.6.
///
/// `json` and `yaml` are absent because they are serializations rather than layouts: they belong
/// to `to json` and `to yaml` in `ono-value`, and duplicating them here would give the shell two
/// JSON encoders that could drift. `graph` is absent because spec §13.6 requires it never to
/// "fabricate relationships from visual grouping", which needs the relationship providers of
/// phase G rather than a layout decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum View {
    /// A column per field, a row per value (spec §13.2).
    Table,
    /// A stacked block per value, one field per line.
    List,
    /// The ASCII tree of spec §22.4, for values with a shape.
    Tree,
    /// One canonical value per line, never shortened.
    Raw,
    /// A hexadecimal dump of the value's raw bytes.
    Hex,
}

/// What a value needs in order to become text: the reader's time zone, and optionally the
/// instant the reader calls "now".
///
/// Spec §13.4 asks a timestamp to render as "context-sensitive local time". The context is the
/// reader's clock, and it is passed in rather than read from the environment so that the same
/// values always produce the same output for the same context — which is what
/// `docs/ACCEPTANCE.md` §4.2 means by a deterministic rendering, and what makes these renderings
/// testable at all.
///
/// ```
/// use ono_render::Renderer;
/// use ono_value::{ByteSize, Value};
/// let renderer = Renderer::new();
/// assert_eq!(
///     renderer.cell(&Value::ByteSize(ByteSize::from_bytes(1_288_490_188))).text(),
///     "1.20 GiB"
/// );
/// ```
#[derive(Debug, Clone)]
pub struct Renderer {
    zone: TimeZone,
    now: Option<Timestamp>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    /// A renderer for the system's time zone, showing every timestamp in full.
    #[must_use]
    pub fn new() -> Self {
        Self {
            zone: TimeZone::system(),
            now: None,
        }
    }

    /// A renderer for a chosen time zone.
    #[must_use]
    pub fn in_zone(zone: TimeZone) -> Self {
        Self { zone, now: None }
    }

    /// Tells the renderer which instant is "now", enabling the shortened timestamps of §13.4.
    #[must_use]
    pub fn at(mut self, now: Timestamp) -> Self {
        self.now = Some(now);
        self
    }

    /// The cell one value occupies: its human text and the token that paints it.
    ///
    /// Control characters are neutralised here, once, for every path that leads to a terminal
    /// (spec §49, ADR-0015 T1). The value itself keeps them.
    ///
    /// ```
    /// use ono_render::{Renderer, Token};
    /// use ono_value::Value;
    /// let cell = Renderer::new().cell(&Value::Null);
    /// assert_eq!(cell.text(), "null");
    /// assert_eq!(cell.token(), Token::ValueNull);
    /// ```
    #[must_use]
    pub fn cell(&self, value: &Value) -> Cell {
        Cell::new(sanitise(&self.text(value))).with_token(token_of(value))
    }

    /// A table over `values` (spec §13.2).
    ///
    /// Records of one schema take their columns from the schema's `default_view` when it has one
    /// and from its field order otherwise (spec §27.3). Maps that all share a key list take their
    /// columns from those keys. A stream of plain values gets a single `VALUE` column.
    ///
    /// Anything else is heterogeneous, and spec §11.4 wants that to be explicit: the table
    /// becomes `TYPE` and `VALUE`. It deliberately does not union the columns of the different
    /// shapes, because a row that lacks a column would then look exactly like a row whose value
    /// is unknown — the conflation spec §10.5 exists to prevent.
    #[must_use]
    pub fn table(&self, values: &[Value]) -> Table {
        match Shape::of(values) {
            Shape::Empty => Table::new(Vec::new()),
            Shape::Records(schema) => self.record_table(&schema, values),
            Shape::Maps(keys) => self.map_table(&keys, values),
            Shape::Scalars => {
                let mut table = Table::new(vec![Column::new("VALUE")]);
                for value in values {
                    table.push_row(vec![self.cell(value)]);
                }
                table
            }
            Shape::Mixed => {
                let mut table = Table::new(vec![Column::new("TYPE"), Column::new("VALUE")]);
                for value in values {
                    table.push_row(vec![
                        Cell::new(sanitise(&type_label(value))).with_token(Token::Dim),
                        self.cell(value),
                    ]);
                }
                table
            }
        }
    }

    /// The tree one value draws (spec §22.4).
    ///
    /// A record becomes its schema id with a child per field, a map becomes `map` with a child
    /// per key, and a list becomes `list (n)` with a child per element. A scalar is a leaf, so a
    /// tree of a scalar is one line.
    #[must_use]
    pub fn tree(&self, value: &Value) -> TreeNode {
        self.node(None, value)
    }

    fn node(&self, key: Option<&str>, value: &Value) -> TreeNode {
        let mut node = match value {
            Value::Record(record) => {
                let mut node = TreeNode::new(sanitise(&record.schema_id().to_string()))
                    .with_token(Token::Accent);
                for field in record.schema().fields() {
                    let child = record.get(field.name()).cloned().unwrap_or(Value::Null);
                    node.push_child(self.node(Some(field.name()), &child));
                }
                for (name, child) in record.extra() {
                    node.push_child(self.node(Some(name), child));
                }
                node
            }
            Value::Map(map) => {
                let mut node = TreeNode::new("map").with_token(Token::Dim);
                for (name, child) in map.iter() {
                    node.push_child(self.node(Some(name), child));
                }
                node
            }
            Value::List(items) => {
                let mut node =
                    TreeNode::new(format!("list ({})", items.len())).with_token(Token::Dim);
                for item in items.iter() {
                    node.push_child(self.node(None, item));
                }
                node
            }
            scalar => TreeNode::new(sanitise(&self.text(scalar))).with_token(token_of(scalar)),
        };
        if let Some(key) = key {
            node = node.with_key(sanitise(key));
        }
        node
    }

    fn record_table(&self, schema: &Schema, values: &[Value]) -> Table {
        let columns = columns_of(schema);

        let mut table = Table::new(
            columns
                .iter()
                .map(|name| {
                    let align = schema
                        .field(name)
                        .filter(|field| numeric(field.ty()))
                        .map_or(Align::Left, |_| Align::Right);
                    Column::new(name.to_uppercase()).align(align)
                })
                .collect(),
        );
        for value in values {
            let Ok(record) = value.as_record() else {
                continue;
            };
            table.push_row(
                columns
                    .iter()
                    .map(|name| self.field_cell(record, name))
                    .collect(),
            );
        }
        table
    }

    fn field_cell(&self, record: &RecordValue, name: &str) -> Cell {
        // A field the schema does not declare renders as unknown rather than as a blank: the
        // default view is a rendering hint and a stale one must not silently produce empty cells.
        let value = record.get(name).unwrap_or(&Value::Null);
        match record.schema().field(name) {
            Some(field) => self.declared_cell(field, value),
            None => self.cell(value),
        }
    }

    /// The cell of a value read through the field that declares it (spec §13.1 point 1).
    ///
    /// Two things a value cannot say about itself live on its declaration, and spec §13.2 prints
    /// both: `cpu` is a bare `float` whose *meaning* is a percentage, and `user` is a reference to
    /// an account rather than a copy of it. A field with neither renders exactly as it would
    /// anywhere else, so a cell never depends on the declaration for anything the value knows.
    fn declared_cell(&self, field: &FieldDef, value: &Value) -> Cell {
        // Only `percent`. `bytes` and `seconds` have a value type of their own — `rx_bytes` is a
        // `bytesize` and renders as `1.20 GiB` without help — and where a field carries the unit
        // over a plain integer it is because the number is the point: an MTU reads as `1500`,
        // never as `1.46 KiB`. `percent` is the one unit with no other spelling.
        if field.unit() == Some(Unit::Percent)
            && let Some(number) = percent_number(value)
        {
            return Cell::new(sanitise(&percentage(number))).with_token(Token::ValueUnit);
        }
        if matches!(field.ty(), FieldType::Ref(_))
            && let Ok(target) = value.as_record()
        {
            return Cell::new(sanitise(&reference_text(target))).with_token(Token::ValueString);
        }
        self.cell(value)
    }

    /// A record as one line: the fields of its default view, spelled the way a record literal is.
    ///
    /// A nested record is data a reader asked for. Showing its schema id instead would be the
    /// conflation spec §10.5 exists to prevent — `ono.endpoint/1 {}` and an endpoint that really
    /// is empty would read identically. Which fields is the same question the table already
    /// answers for columns, so it is the same answer: the default view, or every field when the
    /// schema declares none.
    fn record_text(&self, record: &RecordValue) -> String {
        let fields = columns_of(record.schema())
            .into_iter()
            .map(|name| {
                let value = record.get(&name).unwrap_or(&Value::Null);
                format!("{name}: {}", self.text(value))
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{{fields}}}")
    }

    fn map_table(&self, keys: &[String], values: &[Value]) -> Table {
        let mut table = Table::new(
            keys.iter()
                .map(|key| Column::new(key.to_uppercase()))
                .collect(),
        );
        for value in values {
            let Ok(map) = value.as_map() else {
                continue;
            };
            table.push_row(
                keys.iter()
                    .map(|key| self.cell(map.get(key).unwrap_or(&Value::Null)))
                    .collect(),
            );
        }
        table
    }

    /// The human text of spec §13.4, before sanitising.
    ///
    /// A semantic scalar renders in the unit a reader thinks in — `1.20 GiB`, `4d 03h`, `843ms`,
    /// `24.8%` — which is what `Display` on those types already means. Everything else falls back
    /// to its canonical text, because for a port or an IP address the canonical form *is* the
    /// human form and having two spellings of it would only invite them to drift.
    fn text(&self, value: &Value) -> String {
        match value {
            Value::ByteSize(size) => size.to_string(),
            Value::Duration(span) => span.to_string(),
            Value::Percent(percent) => percentage(percent.value()),
            Value::Timestamp(instant) => self.timestamp(*instant),
            Value::Error(error) => format!("{}: {}", error.code().name(), error.message()),
            Value::Path(path) => path.display().to_string(),
            Value::Record(record) => self.record_text(record),
            other => canonical_text(other).unwrap_or_else(|_| other.to_string()),
        }
    }

    /// A timestamp as spec §13.4 asks for it: the closer it is, the less of it a reader needs.
    fn timestamp(&self, instant: Timestamp) -> String {
        let zoned = instant.to_zoned(self.zone.clone());
        let Some(now) = self.now else {
            return zoned.strftime("%Y-%m-%d %H:%M:%S %:z").to_string();
        };
        let today = now.to_zoned(self.zone.clone()).date();
        let date = zoned.date();
        if date == today {
            zoned.strftime("%H:%M:%S").to_string()
        } else if date.year() == today.year() {
            zoned.strftime("%b %d %H:%M").to_string()
        } else {
            zoned.strftime("%Y-%m-%d").to_string()
        }
    }
}

/// What a stream of values has in common, which is what decides the table's columns.
enum Shape {
    Empty,
    Records(std::sync::Arc<Schema>),
    Maps(Vec<String>),
    Scalars,
    Mixed,
}

impl Shape {
    fn of(values: &[Value]) -> Self {
        let Some(first) = values.first() else {
            return Shape::Empty;
        };
        match first {
            Value::Record(record) => {
                let schema = std::sync::Arc::clone(record.schema());
                let same = values.iter().all(|value| {
                    value
                        .as_record()
                        .is_ok_and(|other| other.schema_id() == schema.id())
                });
                if same {
                    Shape::Records(schema)
                } else {
                    Shape::Mixed
                }
            }
            Value::Map(map) => {
                let keys: Vec<String> = map.keys().map(str::to_owned).collect();
                let same = values.iter().all(|value| {
                    value.as_map().is_ok_and(|other| {
                        other.len() == keys.len()
                            && other.keys().zip(keys.iter()).all(|(a, b)| a == b)
                    })
                });
                if same {
                    Shape::Maps(keys)
                } else {
                    Shape::Mixed
                }
            }
            _ => {
                if values
                    .iter()
                    .all(|value| !matches!(value, Value::Record(_) | Value::Map(_)))
                {
                    Shape::Scalars
                } else {
                    Shape::Mixed
                }
            }
        }
    }
}

/// The fields a schema puts on show: its default view, or every field when it declares none.
///
/// A table's columns and a nested record's inline fields are the same question asked twice, so
/// they get the same answer — a schema that names its readable fields names them once.
fn columns_of(schema: &Schema) -> Vec<String> {
    if schema.default_view().is_empty() {
        schema
            .fields()
            .iter()
            .map(|field| field.name().to_owned())
            .collect()
    } else {
        schema
            .default_view()
            .iter()
            .map(|column| column.to_string())
            .collect()
    }
}

/// A percentage as spec §13.2 prints it: `24.8%`, one decimal.
///
/// The digits a `f64` happens to carry are an artifact of the arithmetic that produced them, and
/// printing all seventeen of them says "this is exact" about a sampled quantity that is not.
/// Spec §33.5 keeps the exact number reachable — `to json` and `inspect` are unaffected, because
/// they use the canonical form and this is the human one.
fn percentage(value: f64) -> String {
    format!("{value:.1}%")
}

/// The number in a value a field declares as a percentage, if the value is one at all.
///
/// A null, a string or an error in a percent-typed field renders as itself: spec §10.5 makes
/// unknown and failed distinct from a value, and a unit must not turn either into `0.0%`.
fn percent_number(value: &Value) -> Option<f64> {
    match value {
        Value::Float(number) => Some(*number),
        Value::Percent(percent) => Some(percent.value()),
        #[expect(
            clippy::cast_precision_loss,
            reason = "a percentage past 2^53 is a broken measurement, not a rendering problem"
        )]
        Value::Int(number) => Some(*number as f64),
        _ => None,
    }
}

/// What a reference names: the object on the other end, as a person says it.
///
/// Spec §13.2 prints `postgres` in the `USER` column of the process table — a reference stands
/// for an object the reader already knows how to name, so spelling out its whole record would
/// bury the row it sits in. Where nothing resolved, spec §23.6 keeps the numeric identity, and
/// that identity is what shows: `{uid: 0}` says which account this is, and an empty cell would
/// not.
fn reference_text(record: &RecordValue) -> String {
    if let Some(Ok(name)) = record
        .get("name")
        .filter(|value| !value.is_null())
        .map(canonical_text)
        && !name.is_empty()
    {
        return name;
    }
    record.identity().to_string()
}

/// The label the `TYPE` column of a heterogeneous table shows.
fn type_label(value: &Value) -> String {
    match value {
        Value::Record(record) => record.schema_id().to_string(),
        other => other.type_name().to_owned(),
    }
}

/// The semantic token a value is painted with (spec §44).
#[must_use]
fn token_of(value: &Value) -> Token {
    match value {
        Value::Null => Token::ValueNull,
        Value::Int(_) | Value::Float(_) | Value::Decimal(_) | Value::Port(_) => Token::ValueNumber,
        Value::ByteSize(_) | Value::Duration(_) | Value::Percent(_) => Token::ValueUnit,
        Value::Path(_) => Token::Path,
        Value::Timestamp(_) => Token::Timestamp,
        Value::Error(_) => Token::ErrorCode,
        Value::Bool(_) | Value::List(_) | Value::Map(_) | Value::Record(_) => Token::Foreground,
        _ => Token::ValueString,
    }
}

/// Whether a field's values should end at the same column, so magnitudes compare by eye.
fn numeric(ty: &ono_value::FieldType) -> bool {
    matches!(
        ty,
        ono_value::FieldType::Int
            | ono_value::FieldType::Float
            | ono_value::FieldType::Decimal
            | ono_value::FieldType::ByteSize
            | ono_value::FieldType::Duration
            | ono_value::FieldType::Percent
            | ono_value::FieldType::Port
    )
}
