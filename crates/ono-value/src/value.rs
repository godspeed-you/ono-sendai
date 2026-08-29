//! The runtime value of spec §10.2 and §25.

use std::fmt;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use jiff::Timestamp;
use ono_core::ErrorCode;

use crate::{
    ByteSize, Decimal, Duration, ErrorValue, FieldAccess, FieldStep, FieldType, IpNetwork,
    MapValue, Percent, RecordValue, RegexValue, Uuid,
};

/// A value flowing through a pipeline (spec §10.2, §25).
///
/// Every compound case sits behind an `Arc`, so cloning a value is a refcount bump whatever it
/// holds: spec §34 budgets a pipeline stage in microseconds, and a value model that copies its
/// payload cannot meet that.
///
/// Streams are deliberately not a variant. Spec §25 asks for them to be execution-layer objects
/// unless their consumption semantics are extremely clear, and a clonable value whose second
/// clone yields nothing is not that.
///
/// Equality is structural: `Value::Int(1)` and `Value::Float(1.0)` are different values even
/// though they compare equal numerically. Use [`compare_to`](Self::compare_to) for the ordering
/// the language's `==` and `<` operators need.
///
/// ```
/// use ono_value::{ByteSize, Value};
/// let size = Value::ByteSize(ByteSize::parse("512MiB")?);
/// assert_eq!(size.type_name(), "bytesize");
/// assert!(size.compare_to(&Value::ByteSize(ByteSize::parse("1GiB")?))?.is_lt());
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    /// A value that is known to be unknown (spec §10.5).
    #[default]
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Int(i128),
    /// A binary floating-point number.
    Float(f64),
    /// An exact decimal number.
    Decimal(Decimal),
    /// Text.
    String(Arc<str>),
    /// Raw bytes, which may not be valid text (spec §12.2).
    Bytes(Bytes),
    /// A filesystem path.
    Path(Arc<Path>),
    /// An instant in time.
    Timestamp(Timestamp),
    /// A signed span of time.
    Duration(Duration),
    /// A quantity of information.
    ByteSize(ByteSize),
    /// A percentage.
    Percent(Percent),
    /// A regular expression.
    Regex(Arc<RegexValue>),
    /// A universally unique identifier.
    Uuid(Uuid),
    /// An IP address.
    Ip(IpAddr),
    /// An IP address with a prefix length.
    IpNetwork(IpNetwork),
    /// A TCP or UDP port number.
    Port(u16),
    /// An ordered list of values.
    List(Arc<[Value]>),
    /// A map with string keys.
    Map(Arc<MapValue>),
    /// A schema-bound record.
    Record(Arc<RecordValue>),
    /// A structured error carried as data (spec §16.1).
    Error(Arc<ErrorValue>),
}

impl Value {
    /// The stable name of the value's type, as `type` reports it and as error messages spell it.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Decimal(_) => "decimal",
            Value::String(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::Path(_) => "path",
            Value::Timestamp(_) => "timestamp",
            Value::Duration(_) => "duration",
            Value::ByteSize(_) => "bytesize",
            Value::Percent(_) => "percent",
            Value::Regex(_) => "regex",
            Value::Uuid(_) => "uuid",
            Value::Ip(_) => "ip",
            Value::IpNetwork(_) => "ipnetwork",
            Value::Port(_) => "port",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Record(_) => "record",
            Value::Error(_) => "error",
        }
    }

    /// Whether the value is the unknown of spec §10.5.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Whether the value is a structured error.
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Value::Error(_))
    }

    /// Builds a string value.
    #[must_use]
    pub fn string(text: &str) -> Self {
        Value::String(text.into())
    }

    /// Builds a list value.
    #[must_use]
    pub fn list(items: impl IntoIterator<Item = Value>) -> Self {
        Value::List(items.into_iter().collect())
    }

    /// Reads a field path, honouring the optional access of spec §11.4.
    ///
    /// An optional step turns an absent field or a null receiver into null. It never swallows a
    /// failed access: spec §10.5 keeps "unknown" and "could not be read" apart, so a field
    /// holding an error propagates that error however the step was written.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeUnknownField`] when a required step names no field,
    /// [`ErrorCode::TypeMismatch`] when the receiver has no fields at all, and the recorded
    /// failure when a field's access failed.
    pub fn follow(&self, steps: &[FieldStep<'_>]) -> Result<Value, ErrorValue> {
        let mut current = self.clone();
        let mut unknown = false;
        for step in steps {
            // A record-typed field the schema declares and the provider could not fill is an
            // unknown record, and so is everything beneath it: `local.port` on a socket whose
            // `local` is null is not a type error but the same unknown, and a predicate over it
            // does not match (spec §10.5, ADR-0014, ADR-0089). Only a null that came from a
            // field *with fields* propagates; a null string has no `.name`, and a null that was
            // never a field still refuses a required step.
            if unknown && matches!(current, Value::Null) {
                continue;
            }
            unknown = matches!(
                &current,
                Value::Record(record)
                    if matches!(record.access(step.name()), FieldAccess::Unknown)
                        && record.schema().field(step.name()).is_some_and(|field| {
                            matches!(
                                field.ty(),
                                FieldType::Record(_) | FieldType::Ref(_) | FieldType::Map | FieldType::Any
                            )
                        })
            );
            current = follow_step(&current, *step)?;
        }
        Ok(current)
    }

    /// The value as a boolean.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a boolean.
    pub fn as_bool(&self) -> Result<bool, ErrorValue> {
        match self {
            Value::Bool(value) => Ok(*value),
            other => Err(other.mismatch("bool")),
        }
    }

    /// The value as an integer.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not an integer.
    pub fn as_int(&self) -> Result<i128, ErrorValue> {
        match self {
            Value::Int(value) => Ok(*value),
            other => Err(other.mismatch("int")),
        }
    }

    /// The value as a floating-point number, accepting integers and decimals because every one of
    /// them is a number.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not numeric.
    pub fn as_float(&self) -> Result<f64, ErrorValue> {
        match self {
            Value::Float(value) => Ok(*value),
            Value::Int(value) => Ok(*value as f64),
            Value::Decimal(value) => Ok(value.to_f64()),
            other => Err(other.mismatch("float")),
        }
    }

    /// The value as an exact decimal.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a decimal or an integer.
    pub fn as_decimal(&self) -> Result<Decimal, ErrorValue> {
        match self {
            Value::Decimal(value) => Ok(*value),
            Value::Int(value) => Ok(Decimal::from_int(*value)),
            other => Err(other.mismatch("decimal")),
        }
    }

    /// The value as text.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a string.
    pub fn as_str(&self) -> Result<&str, ErrorValue> {
        match self {
            Value::String(value) => Ok(value),
            other => Err(other.mismatch("string")),
        }
    }

    /// The value as raw bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a byte string.
    pub fn as_bytes(&self) -> Result<&Bytes, ErrorValue> {
        match self {
            Value::Bytes(value) => Ok(value),
            other => Err(other.mismatch("bytes")),
        }
    }

    /// The value as a filesystem path.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a path.
    pub fn as_path(&self) -> Result<&Path, ErrorValue> {
        match self {
            Value::Path(value) => Ok(value),
            other => Err(other.mismatch("path")),
        }
    }

    /// The current wall-clock instant, as `now()` answers it (spec §6.3, ADR-0071).
    #[must_use]
    pub fn now() -> Self {
        Value::Timestamp(Timestamp::now())
    }

    /// Reads a timestamp literal in the RFC 3339 spelling (ADR-0071).
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] when the text is not such a timestamp — a day that
    /// does not exist, an hour past 23.
    pub fn parse_timestamp(text: &str) -> Result<Self, ErrorValue> {
        text.parse::<Timestamp>()
            .map(Value::Timestamp)
            .map_err(|error| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{text}` is not a timestamp: {error}"),
                )
            })
    }

    /// The value as an instant in time.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a timestamp.
    pub fn as_timestamp(&self) -> Result<Timestamp, ErrorValue> {
        match self {
            Value::Timestamp(value) => Ok(*value),
            other => Err(other.mismatch("timestamp")),
        }
    }

    /// The value as a span of time.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a duration.
    pub fn as_duration(&self) -> Result<Duration, ErrorValue> {
        match self {
            Value::Duration(value) => Ok(*value),
            other => Err(other.mismatch("duration")),
        }
    }

    /// The value as a quantity of information.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a byte size.
    pub fn as_byte_size(&self) -> Result<ByteSize, ErrorValue> {
        match self {
            Value::ByteSize(value) => Ok(*value),
            other => Err(other.mismatch("bytesize")),
        }
    }

    /// The value as a percentage.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a percentage.
    pub fn as_percent(&self) -> Result<Percent, ErrorValue> {
        match self {
            Value::Percent(value) => Ok(*value),
            other => Err(other.mismatch("percent")),
        }
    }

    /// The value as a regular expression.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a regular expression.
    pub fn as_regex(&self) -> Result<&RegexValue, ErrorValue> {
        match self {
            Value::Regex(value) => Ok(value),
            other => Err(other.mismatch("regex")),
        }
    }

    /// The value as a universally unique identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a UUID.
    pub fn as_uuid(&self) -> Result<Uuid, ErrorValue> {
        match self {
            Value::Uuid(value) => Ok(*value),
            other => Err(other.mismatch("uuid")),
        }
    }

    /// The value as an IP address.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not an IP address.
    pub fn as_ip(&self) -> Result<IpAddr, ErrorValue> {
        match self {
            Value::Ip(value) => Ok(*value),
            other => Err(other.mismatch("ip")),
        }
    }

    /// The value as an IP network.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not an IP network.
    pub fn as_ip_network(&self) -> Result<IpNetwork, ErrorValue> {
        match self {
            Value::IpNetwork(value) => Ok(*value),
            other => Err(other.mismatch("ipnetwork")),
        }
    }

    /// The value as a port number.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a port.
    pub fn as_port(&self) -> Result<u16, ErrorValue> {
        match self {
            Value::Port(value) => Ok(*value),
            other => Err(other.mismatch("port")),
        }
    }

    /// The value as a list.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a list.
    pub fn as_list(&self) -> Result<&[Value], ErrorValue> {
        match self {
            Value::List(value) => Ok(value),
            other => Err(other.mismatch("list")),
        }
    }

    /// The value as a map.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a map.
    pub fn as_map(&self) -> Result<&MapValue, ErrorValue> {
        match self {
            Value::Map(value) => Ok(value),
            other => Err(other.mismatch("map")),
        }
    }

    /// The value as a record.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not a record.
    pub fn as_record(&self) -> Result<&RecordValue, ErrorValue> {
        match self {
            Value::Record(value) => Ok(value),
            other => Err(other.mismatch("record")),
        }
    }

    /// Whether two values hold the same data, whoever observed them and when.
    ///
    /// Identical to `==` for every value but a record, where it ignores provenance
    /// ([`RecordValue::same_data`], ADR-0229). Lists and maps compare their elements the same
    /// way, so a record nested inside one is compared by its data too.
    #[must_use]
    pub fn same_data(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Record(left), Value::Record(right)) => left.same_data(right),
            (Value::List(left), Value::List(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right.iter())
                        .all(|(left, right)| left.same_data(right))
            }
            (Value::Map(left), Value::Map(right)) => {
                left.len() == right.len()
                    && left.iter().zip(right.iter()).all(
                        |((left_key, left), (right_key, right))| {
                            left_key == right_key && left.same_data(right)
                        },
                    )
            }
            (left, right) => left == right,
        }
    }

    /// The value as a structured error.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the value is not an error.
    pub fn as_error(&self) -> Result<&ErrorValue, ErrorValue> {
        match self {
            Value::Error(value) => Ok(value),
            other => Err(other.mismatch("error")),
        }
    }

    fn mismatch(&self, wanted: &str) -> ErrorValue {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("expected {wanted} but found {}", self.type_name()),
        )
    }
}

fn follow_step(receiver: &Value, step: FieldStep<'_>) -> Result<Value, ErrorValue> {
    match receiver {
        Value::Record(record) => match record.access(step.name()) {
            FieldAccess::Known(value) => Ok(value),
            FieldAccess::Unknown => Ok(Value::Null),
            FieldAccess::Failed(error) => Err((*error).clone()),
            FieldAccess::Absent => absent(receiver, step),
        },
        Value::Map(map) => match map.get(step.name()) {
            Some(Value::Error(error)) => Err((**error).clone()),
            Some(value) => Ok(value.clone()),
            None => absent(receiver, step),
        },
        Value::Null if step.is_optional() => Ok(Value::Null),
        // An error in hand is a record of the shape `ono.error/1` declares, so a path descends
        // into it: `error.name` is the selector, `error.source.message` walks the chain of spec
        // §16.1 (ADR-0215). A raised failure never reaches here — `access` reports that one
        // level up, and it still refuses.
        Value::Error(error) => match error.field(step.name()) {
            Some(value) => Ok(value),
            None => absent(receiver, step),
        },
        other => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "cannot read field `{}` on a value of type {}",
                step.name(),
                other.type_name()
            ),
        )),
    }
}

fn absent(receiver: &Value, step: FieldStep<'_>) -> Result<Value, ErrorValue> {
    if step.is_optional() {
        return Ok(Value::Null);
    }
    let mut error = ErrorValue::new(
        ErrorCode::TypeUnknownField,
        format!(
            "no field `{}` on this {}",
            step.name(),
            receiver.type_name()
        ),
    );
    if let Value::Record(record) = receiver {
        error = error
            .with_target(record.to_ref())
            .with_help(format!("`{}` declares no such field", record.schema_id()));
    }
    Err(error)
}

impl fmt::Display for Value {
    /// A compact diagnostic rendering, used inside error metadata and `Debug`-adjacent output.
    ///
    /// This is not the presentation engine of spec §13: choosing tables, columns and truncation
    /// is a renderer's job, and the value model must not decide it (spec §13.1).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(value) => write!(f, "{value}"),
            Value::Int(value) => write!(f, "{value}"),
            Value::Float(value) => write!(f, "{value}"),
            Value::Decimal(value) => write!(f, "{value}"),
            Value::String(value) => f.write_str(value),
            Value::Bytes(value) => {
                for byte in value {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            }
            Value::Path(value) => write!(f, "{}", value.display()),
            Value::Timestamp(value) => write!(f, "{value}"),
            Value::Duration(value) => write!(f, "{value}"),
            Value::ByteSize(value) => write!(f, "{value}"),
            Value::Percent(value) => write!(f, "{value}"),
            Value::Regex(value) => write!(f, "{value}"),
            Value::Uuid(value) => write!(f, "{value}"),
            Value::Ip(value) => write!(f, "{value}"),
            Value::IpNetwork(value) => write!(f, "{value}"),
            Value::Port(value) => write!(f, "{value}"),
            Value::List(items) => {
                f.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Value::Map(map) => write!(f, "{map}"),
            Value::Record(record) => write!(f, "{} {}", record.schema_id(), record.identity()),
            Value::Error(error) => write!(f, "{error}"),
        }
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl From<i128> for Value {
    fn from(value: i128) -> Self {
        Value::Int(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Int(i128::from(value))
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Value::Int(i128::from(value))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Float(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::String(value.into())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value.into())
    }
}

impl From<RecordValue> for Value {
    fn from(record: RecordValue) -> Self {
        record.into_value()
    }
}
