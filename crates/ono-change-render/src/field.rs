//! Reading a v0.6 record without knowing what produced it.
//!
//! §50.1 splits the layers and the module architecture enforces the split: a renderer sits below
//! the crate that owns the change vocabulary and may not reach up to it. So this crate is handed
//! an `ono.change-plan/1`, an `ono.recovery-plan/1` or an `ono.recovery-asset/1` and reads fields
//! off it by the names §46's contracts fix. It cannot see a plan type, which means it cannot
//! change one, and the invariant is a property of the dependency graph rather than a rule.
//!
//! Two consequences shape every accessor here:
//!
//! - **A missing field is not a zero.** v0.2 §35.3 makes unknown a null and never a fabricated
//!   number, so the accessors answer `Option` and every caller decides what absence means in
//!   words rather than defaulting it into a count.
//! - **Every string is hostile.** A target label came off a process table, a reference off a
//!   package. v0.2 §49 assumes an escape sequence in either, and [`text`] neutralises it once for
//!   every path that reaches a sink (ADR-0015 T1).
//!
//! Records nest two ways in §46's contracts: a list of another schema's records
//! (`actions: list<ono.plan-action/1>`) and a list of anonymous maps (`coverage_exclusions`).
//! [`Item`] is the one shape both read as, so a caller writes one loop rather than two.

use jiff::Timestamp;
use ono_value::{ByteSize, MapValue, RecordValue, Value};

/// Anything that answers to a field name — a schema record or an anonymous map.
pub(crate) trait Fields {
    /// The value stored under `name`, where there is one.
    fn field(&self, name: &str) -> Option<&Value>;
}

impl Fields for RecordValue {
    fn field(&self, name: &str) -> Option<&Value> {
        self.get(name)
    }
}

impl Fields for MapValue {
    fn field(&self, name: &str) -> Option<&Value> {
        self.get(name)
    }
}

/// One element of a list field, whichever of §46's two nestings produced it.
#[derive(Debug, Clone)]
pub(crate) enum Item {
    /// A record of a named schema — `ono.plan-action/1`, `ono.protection-coverage/1`.
    Record(RecordValue),
    /// An anonymous map — a coverage exclusion, a risk finding, an impact node.
    Map(MapValue),
}

impl Fields for Item {
    fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Item::Record(record) => record.get(name),
            Item::Map(map) => map.get(name),
        }
    }
}

/// A text field, with its control characters neutralised (v0.2 §49, ADR-0015 T1).
///
/// An empty string answers `None`: §10.5 keeps unknown and empty apart, and a caller rendering a
/// label has no use for the difference between a field that was absent and one that was blank.
pub(crate) fn text(source: &dyn Fields, field: &str) -> Option<String> {
    match source.field(field) {
        Some(Value::String(text)) if !text.is_empty() => Some(ono_render::sanitise(text)),
        _ => None,
    }
}

/// A boolean field. Absence answers `false`, which is what §46's `required: true` bools mean.
pub(crate) fn flag(source: &dyn Fields, field: &str) -> bool {
    matches!(source.field(field), Some(Value::Bool(true)))
}

/// An integer field as a count. A negative or absent count answers `None` rather than zero.
pub(crate) fn count(source: &dyn Fields, field: &str) -> Option<usize> {
    match source.field(field) {
        Some(Value::Int(number)) if *number >= 0 => usize::try_from(*number).ok(),
        _ => None,
    }
}

/// A timestamp field.
pub(crate) fn timestamp(source: &dyn Fields, field: &str) -> Option<Timestamp> {
    match source.field(field) {
        Some(Value::Timestamp(instant)) => Some(*instant),
        _ => None,
    }
}

/// A byte-size field. `None` is v0.2 §35.3's unknown, which §38.2 forbids showing as free.
pub(crate) fn byte_size(source: &dyn Fields, field: &str) -> Option<ByteSize> {
    match source.field(field) {
        Some(Value::ByteSize(size)) => Some(*size),
        _ => None,
    }
}

/// A duration field, in whole seconds.
pub(crate) fn duration_seconds(source: &dyn Fields, field: &str) -> Option<u64> {
    match source.field(field) {
        Some(Value::Duration(span)) => {
            u64::try_from(span.nanoseconds().max(0) / 1_000_000_000).ok()
        }
        _ => None,
    }
}

/// A nested record or map field.
pub(crate) fn nested(source: &dyn Fields, field: &str) -> Option<Item> {
    match source.field(field) {
        Some(Value::Record(record)) => Some(Item::Record(RecordValue::clone(record))),
        Some(Value::Map(map)) => Some(Item::Map(MapValue::clone(map))),
        _ => None,
    }
}

/// The elements of a list field that are records or maps, in the order the field holds them.
///
/// Order is the producer's. §3.3 makes the action order part of what a plan is, and a view that
/// sorted it would be deciding something the planner already decided.
pub(crate) fn items(source: &dyn Fields, field: &str) -> Vec<Item> {
    match source.field(field) {
        Some(Value::List(values)) => values
            .iter()
            .filter_map(|value| match value {
                Value::Record(record) => Some(Item::Record(RecordValue::clone(record))),
                Value::Map(map) => Some(Item::Map(MapValue::clone(map))),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The strings of a `list<string>` field, sanitised.
pub(crate) fn strings(source: &dyn Fields, field: &str) -> Vec<String> {
    match source.field(field) {
        Some(Value::List(values)) => values
            .iter()
            .filter_map(|value| match value {
                Value::String(text) if !text.is_empty() => Some(ono_render::sanitise(text)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// How many elements a list field holds, whatever they are.
pub(crate) fn list_len(source: &dyn Fields, field: &str) -> usize {
    match source.field(field) {
        Some(Value::List(values)) => values.len(),
        _ => 0,
    }
}
