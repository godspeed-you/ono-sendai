//! The plugin-private, quota-bounded state store (spec §31.31).
//!
//! Keyed by package id at construction — no package can name another's store, so there is
//! nothing to scope. Exceeding the effective quota fails the write and evicts nothing: evicting
//! the package's other keys to satisfy a limit would corrupt state to enforce a bound.

use std::collections::BTreeMap;

use ono_kuang_protocol::{KuangError, KuangErrorCode};
use serde_json::Value as Json;

/// One class's key/value store, bounded by the negotiated quota.
#[derive(Debug, Default)]
pub struct StateStore {
    classes: BTreeMap<String, BTreeMap<String, Json>>,
    quota: u64,
}

impl StateStore {
    /// A store bounded by `quota` bytes per class.
    #[must_use]
    pub fn new(quota: u64) -> Self {
        Self {
            classes: BTreeMap::new(),
            quota,
        }
    }

    fn used(&self, class: &str) -> u64 {
        self.classes.get(class).map_or(0, |entries| {
            entries
                .iter()
                .map(|(key, value)| (key.len() + value.to_string().len()) as u64)
                .sum()
        })
    }

    /// Reads a key. `None` when the key is unset — which is not an empty value (spec §10.5).
    #[must_use]
    pub fn get(&self, class: &str, key: &str) -> Option<&Json> {
        self.classes.get(class)?.get(key)
    }

    /// Writes a key, refusing rather than evicting when the quota would be exceeded.
    ///
    /// # Errors
    ///
    /// Returns `state.quota_exceeded` and leaves every existing key untouched.
    pub fn set(&mut self, class: &str, key: &str, value: Json) -> Result<(), KuangError> {
        let addition = (key.len() + value.to_string().len()) as u64;
        let existing = self
            .classes
            .get(class)
            .and_then(|entries| entries.get(key))
            .map_or(0, |old| (key.len() + old.to_string().len()) as u64);
        let projected = self.used(class) - existing + addition;
        if projected > self.quota {
            return Err(KuangError::new(
                KuangErrorCode::StateQuotaExceeded,
                format!(
                    "writing `{key}` would use {projected} bytes of a {}-byte quota",
                    self.quota
                ),
            )
            .with_help(
                "the write failed; nothing was evicted. Raise the quota deliberately or prune",
            ));
        }
        self.classes
            .entry(class.to_owned())
            .or_default()
            .insert(key.to_owned(), value);
        Ok(())
    }

    /// Removes a key, reporting whether a value existed.
    pub fn delete(&mut self, class: &str, key: &str) -> bool {
        self.classes
            .get_mut(class)
            .is_some_and(|entries| entries.remove(key).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_refuse_a_write_beyond_quota_and_keep_existing_keys_intact() {
        let mut store = StateStore::new(32);
        store
            .set("session", "small", Json::String("x".into()))
            .expect("fits");
        let error = store
            .set("session", "big", Json::String("y".repeat(64)))
            .unwrap_err();
        assert_eq!(error.code(), KuangErrorCode::StateQuotaExceeded);
        assert_eq!(
            store.get("session", "small"),
            Some(&Json::String("x".into())),
            "a refused write evicts nothing"
        );
    }
}
