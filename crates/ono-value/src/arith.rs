//! Arithmetic and ordering over values, converting compatible units and rejecting incompatible
//! dimensions (spec §10.6, §6.3).
//!
//! Every unit of a dimension normalises onto one base quantity when it is parsed, so `512MiB` and
//! `1GiB` are already comparable by the time they reach an operator. What remains is the rule
//! spec §10.6 states: a dimension may only meet its own kind.

use std::cmp::Ordering;

use ono_core::ErrorCode;

use crate::{Decimal, Duration, ErrorValue, Value};

/// The physical dimension a value carries, if it carries one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dimension {
    /// A plain number, which has no dimension and combines with any scalar quantity.
    Number,
    /// A quantity of information.
    Information,
    /// A span of time.
    Time,
    /// An instant in time, which is a point rather than a quantity.
    Instant,
    /// A proportion.
    Proportion,
}

fn dimension(value: &Value) -> Option<Dimension> {
    match value {
        Value::Int(_) | Value::Float(_) | Value::Decimal(_) => Some(Dimension::Number),
        Value::ByteSize(_) => Some(Dimension::Information),
        Value::Duration(_) => Some(Dimension::Time),
        Value::Timestamp(_) => Some(Dimension::Instant),
        Value::Percent(_) => Some(Dimension::Proportion),
        _ => None,
    }
}

/// The error raised when two operands cannot meet, choosing the code by what went wrong: a unit
/// error when both sides carry a dimension and the dimensions disagree, a type error otherwise.
fn incompatible(left: &Value, right: &Value, operation: &str) -> ErrorValue {
    let dimensional = dimension(left).is_some() && dimension(right).is_some();
    let code = if dimensional {
        ErrorCode::TypeInvalidUnit
    } else {
        ErrorCode::TypeMismatch
    };
    let message = if dimensional {
        format!(
            "cannot {operation} {} and {}: incompatible units",
            left.type_name(),
            right.type_name()
        )
    } else {
        format!(
            "cannot {operation} {} and {}",
            left.type_name(),
            right.type_name()
        )
    };
    ErrorValue::new(code, message)
}

/// A number promoted far enough that both operands are the same kind.
enum Promoted {
    Ints(i128, i128),
    Decimals(Decimal, Decimal),
    Floats(f64, f64),
}

fn promote(left: &Value, right: &Value) -> Option<Promoted> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(Promoted::Ints(*a, *b)),
        (Value::Decimal(a), Value::Decimal(b)) => Some(Promoted::Decimals(*a, *b)),
        (Value::Decimal(a), Value::Int(b)) => Some(Promoted::Decimals(*a, Decimal::from_int(*b))),
        (Value::Int(a), Value::Decimal(b)) => Some(Promoted::Decimals(Decimal::from_int(*a), *b)),
        (Value::Float(a), Value::Float(b)) => Some(Promoted::Floats(*a, *b)),
        (Value::Float(a), Value::Int(b)) => Some(Promoted::Floats(*a, *b as f64)),
        (Value::Int(a), Value::Float(b)) => Some(Promoted::Floats(*a as f64, *b)),
        (Value::Float(a), Value::Decimal(b)) => Some(Promoted::Floats(*a, b.to_f64())),
        (Value::Decimal(a), Value::Float(b)) => Some(Promoted::Floats(a.to_f64(), *b)),
        _ => None,
    }
}

fn overflow(operation: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!("the result of this {operation} does not fit in 128 bits"),
    )
}

fn scalar_factor(value: &Value) -> Option<f64> {
    match value {
        Value::Int(number) => Some(*number as f64),
        Value::Float(number) => Some(*number),
        Value::Decimal(number) => Some(number.to_f64()),
        _ => None,
    }
}

impl Value {
    /// Adds two values.
    ///
    /// Numbers add as numbers, quantities add within their own dimension, and a duration added to
    /// a timestamp moves the instant.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] when both sides carry a dimension and the dimensions
    /// disagree — `10s + 512MiB` is an error, never a number — and [`ErrorCode::TypeMismatch`]
    /// when a side has no arithmetic at all.
    pub fn add(&self, other: &Value) -> Result<Value, ErrorValue> {
        if let Some(promoted) = promote(self, other) {
            return match promoted {
                Promoted::Ints(a, b) => a
                    .checked_add(b)
                    .map(Value::Int)
                    .ok_or_else(|| overflow("addition")),
                Promoted::Decimals(a, b) => a.checked_add(b).map(Value::Decimal),
                Promoted::Floats(a, b) => Ok(Value::Float(a + b)),
            };
        }
        match (self, other) {
            (Value::ByteSize(a), Value::ByteSize(b)) => a.checked_add(*b).map(Value::ByteSize),
            (Value::Duration(a), Value::Duration(b)) => a.checked_add(*b).map(Value::Duration),
            (Value::Percent(a), Value::Percent(b)) => Ok(Value::Percent(a.plus(*b))),
            (Value::Timestamp(instant), Value::Duration(span))
            | (Value::Duration(span), Value::Timestamp(instant)) => shift(*instant, *span),
            _ => Err(incompatible(self, other, "add")),
        }
    }

    /// Subtracts two values.
    ///
    /// Two instants subtract to the span between them; an instant minus a span is an earlier
    /// instant.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] for incompatible dimensions or for a byte size that
    /// would go negative, and [`ErrorCode::TypeMismatch`] when a side has no arithmetic at all.
    pub fn sub(&self, other: &Value) -> Result<Value, ErrorValue> {
        if let Some(promoted) = promote(self, other) {
            return match promoted {
                Promoted::Ints(a, b) => a
                    .checked_sub(b)
                    .map(Value::Int)
                    .ok_or_else(|| overflow("subtraction")),
                Promoted::Decimals(a, b) => a.checked_sub(b).map(Value::Decimal),
                Promoted::Floats(a, b) => Ok(Value::Float(a - b)),
            };
        }
        match (self, other) {
            (Value::ByteSize(a), Value::ByteSize(b)) => a.checked_sub(*b).map(Value::ByteSize),
            (Value::Duration(a), Value::Duration(b)) => a.checked_sub(*b).map(Value::Duration),
            (Value::Percent(a), Value::Percent(b)) => Ok(Value::Percent(a.minus(*b))),
            (Value::Timestamp(a), Value::Timestamp(b)) => Ok(Value::Duration(
                Duration::from_nanoseconds(a.as_nanosecond() - b.as_nanosecond()),
            )),
            (Value::Timestamp(instant), Value::Duration(span)) => {
                shift(*instant, Duration::from_nanoseconds(-span.nanoseconds()))
            }
            _ => Err(incompatible(self, other, "subtract")),
        }
    }

    /// Multiplies two values.
    ///
    /// A quantity may be scaled by a plain number. Two quantities may not be multiplied: the
    /// shell models no dimension that would be the result.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] for two quantities and [`ErrorCode::TypeMismatch`]
    /// when a side has no arithmetic at all.
    pub fn mul(&self, other: &Value) -> Result<Value, ErrorValue> {
        if let Some(promoted) = promote(self, other) {
            return match promoted {
                Promoted::Ints(a, b) => a
                    .checked_mul(b)
                    .map(Value::Int)
                    .ok_or_else(|| overflow("multiplication")),
                Promoted::Decimals(a, b) => a.checked_mul(b).map(Value::Decimal),
                Promoted::Floats(a, b) => Ok(Value::Float(a * b)),
            };
        }
        let scaled =
            match (self, other) {
                (Value::ByteSize(size), scalar) => scalar_factor(scalar)
                    .map(|factor| size.checked_scale(factor).map(Value::ByteSize)),
                (scalar, Value::ByteSize(size)) => scalar_factor(scalar)
                    .map(|factor| size.checked_scale(factor).map(Value::ByteSize)),
                (Value::Duration(span), scalar) => scalar_factor(scalar)
                    .map(|factor| span.checked_scale(factor).map(Value::Duration)),
                (scalar, Value::Duration(span)) => scalar_factor(scalar)
                    .map(|factor| span.checked_scale(factor).map(Value::Duration)),
                (Value::Percent(percent), scalar) => {
                    scalar_factor(scalar).map(|factor| Ok(Value::Percent(percent.scale(factor))))
                }
                (scalar, Value::Percent(percent)) => {
                    scalar_factor(scalar).map(|factor| Ok(Value::Percent(percent.scale(factor))))
                }
                _ => None,
            };
        scaled.unwrap_or_else(|| Err(incompatible(self, other, "multiply")))
    }

    /// Divides two values.
    ///
    /// Dividing a quantity by its own dimension yields the plain ratio; dividing it by a number
    /// scales it. Integer division yields a float, because a shell that answers `7 / 2` with `3`
    /// is answering the wrong question.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] for division by zero or for operands with no
    /// arithmetic, and [`ErrorCode::TypeInvalidUnit`] for incompatible dimensions.
    pub fn div(&self, other: &Value) -> Result<Value, ErrorValue> {
        if promote(self, other).is_some() {
            let divisor = other.as_float()?;
            if divisor == 0.0 {
                return Err(ErrorValue::new(ErrorCode::TypeMismatch, "division by zero"));
            }
            if let (Value::Decimal(a), Value::Decimal(b)) = (self, other) {
                return a.checked_div(*b).map(Value::Decimal);
            }
            return Ok(Value::Float(self.as_float()? / divisor));
        }
        match (self, other) {
            (Value::ByteSize(a), Value::ByteSize(b)) => ratio(a.bytes() as f64, b.bytes() as f64),
            (Value::Duration(a), Value::Duration(b)) => {
                ratio(a.nanoseconds() as f64, b.nanoseconds() as f64)
            }
            (Value::Percent(a), Value::Percent(b)) => ratio(a.value(), b.value()),
            (Value::ByteSize(size), scalar) => match scalar_factor(scalar) {
                Some(factor) if factor != 0.0 => {
                    size.checked_scale(1.0 / factor).map(Value::ByteSize)
                }
                Some(_) => Err(ErrorValue::new(ErrorCode::TypeMismatch, "division by zero")),
                None => Err(incompatible(self, other, "divide")),
            },
            (Value::Duration(span), scalar) => match scalar_factor(scalar) {
                Some(factor) if factor != 0.0 => {
                    span.checked_scale(1.0 / factor).map(Value::Duration)
                }
                Some(_) => Err(ErrorValue::new(ErrorCode::TypeMismatch, "division by zero")),
                None => Err(incompatible(self, other, "divide")),
            },
            _ => Err(incompatible(self, other, "divide")),
        }
    }

    /// Orders two values, converting compatible units automatically (spec §10.6).
    ///
    /// Null orders before every known value so that a sort puts unknown data at one end instead
    /// of failing; two nulls are equal.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] when both sides carry a dimension and the dimensions
    /// disagree, and [`ErrorCode::TypeMismatch`] when the two values are simply not comparable.
    pub fn compare_to(&self, other: &Value) -> Result<Ordering, ErrorValue> {
        match (self, other) {
            (Value::Null, Value::Null) => return Ok(Ordering::Equal),
            (Value::Null, _) => return Ok(Ordering::Less),
            (_, Value::Null) => return Ok(Ordering::Greater),
            _ => {}
        }
        if let Some(promoted) = promote(self, other) {
            return match promoted {
                Promoted::Ints(a, b) => Ok(a.cmp(&b)),
                Promoted::Decimals(a, b) => {
                    a.partial_cmp(&b).ok_or_else(|| not_comparable(self, other))
                }
                Promoted::Floats(a, b) => {
                    a.partial_cmp(&b).ok_or_else(|| not_comparable(self, other))
                }
            };
        }
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => Ok(a.cmp(b)),
            (Value::String(a), Value::String(b)) => Ok(a.cmp(b)),
            (Value::Bytes(a), Value::Bytes(b)) => Ok(a.cmp(b)),
            (Value::Path(a), Value::Path(b)) => Ok(a.cmp(b)),
            // A path and the string that spells it compare as their text. Expression mode has no
            // path literal — `/proc` reads as a regex delimiter — so a quoted string is the only
            // way a user can write a path in a comparison, and `where target == "/proc"` must
            // mean what it says (ADR-0031). Text to text only: bytes stay bytes (spec §12.2).
            (Value::Path(a), Value::String(b)) => {
                Ok(a.as_os_str().cmp(std::ffi::OsStr::new(b.as_ref())))
            }
            (Value::String(a), Value::Path(b)) => {
                Ok(std::ffi::OsStr::new(a.as_ref()).cmp(b.as_os_str()))
            }
            (Value::Timestamp(a), Value::Timestamp(b)) => Ok(a.cmp(b)),
            (Value::Duration(a), Value::Duration(b)) => Ok(a.cmp(b)),
            (Value::ByteSize(a), Value::ByteSize(b)) => Ok(a.cmp(b)),
            (Value::Percent(a), Value::Percent(b)) => {
                a.compare(*b).ok_or_else(|| not_comparable(self, other))
            }
            (Value::Uuid(a), Value::Uuid(b)) => Ok(a.cmp(b)),
            (Value::Ip(a), Value::Ip(b)) => Ok(a.cmp(b)),
            (Value::IpNetwork(a), Value::IpNetwork(b)) => Ok(a.cmp(b)),
            (Value::Port(a), Value::Port(b)) => Ok(a.cmp(b)),
            // A port and the integer that spells it compare as numbers: spec §10.6 lets a port
            // "parse from integer context", and `where local.port == 443` is how every example
            // writes one (ADR-0089).
            (Value::Port(a), Value::Int(b)) => Ok(i128::from(*a).cmp(b)),
            (Value::Int(a), Value::Port(b)) => Ok(a.cmp(&i128::from(*b))),
            (Value::List(a), Value::List(b)) => compare_lists(a, b),
            _ => Err(incompatible(self, other, "compare")),
        }
    }
}

fn compare_lists(left: &[Value], right: &[Value]) -> Result<Ordering, ErrorValue> {
    for (a, b) in left.iter().zip(right.iter()) {
        let ordering = a.compare_to(b)?;
        if ordering != Ordering::Equal {
            return Ok(ordering);
        }
    }
    Ok(left.len().cmp(&right.len()))
}

fn not_comparable(left: &Value, right: &Value) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!(
            "cannot order {} against {}",
            left.type_name(),
            right.type_name()
        ),
    )
}

fn ratio(numerator: f64, denominator: f64) -> Result<Value, ErrorValue> {
    if denominator == 0.0 {
        return Err(ErrorValue::new(ErrorCode::TypeMismatch, "division by zero"));
    }
    Ok(Value::Float(numerator / denominator))
}

fn shift(instant: jiff::Timestamp, span: Duration) -> Result<Value, ErrorValue> {
    let nanos = instant
        .as_nanosecond()
        .checked_add(span.nanoseconds())
        .ok_or_else(|| overflow("shift"))?;
    jiff::Timestamp::from_nanosecond(nanos)
        .map(Value::Timestamp)
        .map_err(|error| {
            ErrorValue::new(
                ErrorCode::TypeInvalidUnit,
                "the shifted timestamp lies outside the representable range",
            )
            .with_help(error.to_string())
        })
}
