//! The `Map` value of spec §10.2: string keys, insertion-ordered, order-insensitive equality.

use std::fmt;
use std::sync::Arc;

use crate::Value;

/// A map from string keys to values, keeping insertion order for rendering.
///
/// Two maps are equal when they hold the same keys with the same values, whatever order they
/// were built in; iteration order is insertion order, because a renderer that reorders a user's
/// keys is surprising.
///
/// ```
/// use ono_value::{MapValue, Value};
/// let mut map = MapValue::new();
/// map.insert("pid".into(), Value::Int(1));
/// assert_eq!(map.get("pid"), Some(&Value::Int(1)));
/// ```
#[derive(Debug, Clone, Default)]
pub struct MapValue {
    entries: Vec<(Arc<str>, Value)>,
}

impl MapValue {
    /// An empty map.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// The number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value stored under `key`, if any.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(entry, _)| &**entry == key)
            .map(|(_, value)| value)
    }

    /// Whether the map holds `key` at all, which is not the same as holding a non-null value.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Stores `value` under `key`, returning what was there before.
    ///
    /// Re-inserting an existing key keeps that key's original position.
    pub fn insert(&mut self, key: Arc<str>, value: Value) -> Option<Value> {
        match self.entries.iter_mut().find(|(entry, _)| *entry == key) {
            Some((_, slot)) => Some(std::mem::replace(slot, value)),
            None => {
                self.entries.push((key, value));
                None
            }
        }
    }

    /// Removes `key`, returning what was stored under it.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let position = self.entries.iter().position(|(entry, _)| &**entry == key)?;
        Some(self.entries.remove(position).1)
    }

    /// Every entry, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.entries.iter().map(|(key, value)| (&**key, value))
    }

    /// Every key, in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(key, _)| &**key)
    }

    /// Every value, in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, value)| value)
    }
}

impl PartialEq for MapValue {
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self
                .entries
                .iter()
                .all(|(key, value)| other.get(key) == Some(value))
    }
}

impl FromIterator<(Arc<str>, Value)> for MapValue {
    fn from_iter<I: IntoIterator<Item = (Arc<str>, Value)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (key, value) in iter {
            map.insert(key, value);
        }
        map
    }
}

impl<'a> IntoIterator for &'a MapValue {
    type Item = (&'a str, &'a Value);
    type IntoIter = Box<dyn Iterator<Item = (&'a str, &'a Value)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl fmt::Display for MapValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{")?;
        for (index, (key, value)) in self.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{key}: {value}")?;
        }
        f.write_str("}")
    }
}
