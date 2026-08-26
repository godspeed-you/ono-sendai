//! A total order over sort keys, with unknown values where ADR-0014 puts them.

use std::cmp::Ordering;

use ono_value::Value;

/// Which end of the order a sort runs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    /// Smallest first. Nulls last (ADR-0014).
    #[default]
    Ascending,
    /// Largest first. Nulls first, which is the same rule read backwards.
    Descending,
}

/// Orders two sort keys ascending, with unknown values last.
///
/// `Value::compare_to` orders null *before* everything, which is right for a comparison operator
/// and wrong for a sort: ADR-0014 requires that unknown data never be mistaken for the smallest
/// value. So null is ranked last here, and a descending sort — the exact reverse — puts it
/// first, again as ADR-0014 requires.
///
/// The order is total, which the sort algorithm depends on. Ranks separate the values that
/// cannot be compared at all (null, and a floating-point NaN) from the ones that can; numbers
/// compare numerically across `int`, `float` and `decimal`; anything else falls back to the type
/// name, so a heterogeneous stream sorts into type groups instead of failing.
pub(crate) fn compare_keys(left: &Value, right: &Value) -> Ordering {
    let (left_rank, right_rank) = (rank(left), rank(right));
    if left_rank != right_rank {
        return left_rank.cmp(&right_rank);
    }
    if left_rank != Rank::Comparable {
        return Ordering::Equal;
    }
    match (is_number(left), is_number(right)) {
        (true, true) => left.compare_to(right).unwrap_or(Ordering::Equal),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => {
            let (left_type, right_type) = (left.type_name(), right.type_name());
            if left_type == right_type {
                left.compare_to(right).unwrap_or(Ordering::Equal)
            } else {
                left_type.cmp(right_type)
            }
        }
    }
}

/// Orders two sort keys in `direction`.
pub(crate) fn compare_in(direction: Direction, left: &Value, right: &Value) -> Ordering {
    match direction {
        Direction::Ascending => compare_keys(left, right),
        Direction::Descending => compare_keys(left, right).reverse(),
    }
}

/// Where a value sits relative to the values that can actually be ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Comparable,
    /// A number that is not one: it compares equal to nothing, so it cannot sit among the others
    /// without making the order intransitive.
    NotANumber,
    /// Unknown. Last ascending, first descending (ADR-0014).
    Unknown,
}

fn rank(value: &Value) -> Rank {
    match value {
        Value::Null => Rank::Unknown,
        Value::Float(number) if number.is_nan() => Rank::NotANumber,
        Value::Percent(percent) if percent.value().is_nan() => Rank::NotANumber,
        _ => Rank::Comparable,
    }
}

fn is_number(value: &Value) -> bool {
    matches!(value, Value::Int(_) | Value::Float(_) | Value::Decimal(_))
}

/// A hashable stand-in for a value used as a grouping or join key.
///
/// Values are not `Hash` — a float and a record cannot share one sensible hash — but `group` and
/// `join` need to bucket by key without an O(n²) scan. The canonical form of a scalar plus its
/// type name is injective for every key a shell actually groups by, and two values that are not
/// scalars land in the same bucket only when they render identically *and* share a type, which
/// is then re-checked by equality.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct KeyRepr(String);

impl KeyRepr {
    pub(crate) fn of(value: &Value) -> Self {
        Self(format!("{}\u{1}{value}", value.type_name()))
    }
}
