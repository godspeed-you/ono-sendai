//! The already-resolved parameters a transform is built from (ADR-0005).
//!
//! Each of these is a trait with a blanket implementation for the matching closure, so the
//! evaluator passes a closure and a plugin passes a type. Neither needs an AST: `ono-pipeline`
//! does not depend on `ono-parser`, and cannot.

use ono_value::{ErrorValue, Value};

/// Decides whether `where` keeps a value.
///
/// The result is a [`Value`] rather than a `bool` because ADR-0014 gives the question four
/// answers, not two: `Bool(true)` admits, `Bool(false)` excludes, `Null` excludes as unknown,
/// and an `Error` excludes and is reported. Collapsing those into a boolean is exactly the
/// ambiguity Ono exists to remove.
pub trait Predicate: Send + Sync + 'static {
    /// Evaluates the predicate for one value.
    fn test(&self, value: &Value) -> Value;
}

impl<F> Predicate for F
where
    F: Fn(&Value) -> Value + Send + Sync + 'static,
{
    fn test(&self, value: &Value) -> Value {
        self(value)
    }
}

/// Extracts the key `sort`, `group`, `measure`, `join` and `diff` work on.
pub trait KeyFn: Send + Sync + 'static {
    /// Reads the key of one value.
    ///
    /// # Errors
    ///
    /// Returns the failure of reading it — a field that does not exist on this value, or one
    /// whose access failed. The value is then excluded and the failure reported (spec §16.5).
    fn key(&self, value: &Value) -> Result<Value, ErrorValue>;
}

impl<F> KeyFn for F
where
    F: Fn(&Value) -> Result<Value, ErrorValue> + Send + Sync + 'static,
{
    fn key(&self, value: &Value) -> Result<Value, ErrorValue> {
        self(value)
    }
}

/// Maps one input value to zero, one or many outputs, for `each`.
///
/// Spec §53 warns that `each` "must be specified carefully to avoid accidental nested streams".
/// It cannot nest here: the outputs are a flat list and they are emitted flat.
pub trait Mapper: Send + Sync + 'static {
    /// Maps one value.
    ///
    /// # Errors
    ///
    /// Returns a failure that concerns this value alone. The value is dropped and the failure
    /// reported; the rest of the stream keeps running (spec §16.5).
    fn map(&self, value: &Value) -> Result<Vec<Value>, ErrorValue>;
}

impl<F> Mapper for F
where
    F: Fn(&Value) -> Result<Vec<Value>, ErrorValue> + Send + Sync + 'static,
{
    fn map(&self, value: &Value) -> Result<Vec<Value>, ErrorValue> {
        self(value)
    }
}

/// Folds an accumulator and a value into the next accumulator, for `reduce`.
pub trait Folder: Send + Sync + 'static {
    /// Folds one value into the accumulator.
    ///
    /// # Errors
    ///
    /// Returns a failure that concerns this value alone. The value is skipped and the failure
    /// reported; the fold continues with the accumulator it had.
    fn fold(&self, accumulator: &Value, value: &Value) -> Result<Value, ErrorValue>;
}

impl<F> Folder for F
where
    F: Fn(&Value, &Value) -> Result<Value, ErrorValue> + Send + Sync + 'static,
{
    fn fold(&self, accumulator: &Value, value: &Value) -> Result<Value, ErrorValue> {
        self(accumulator, value)
    }
}
