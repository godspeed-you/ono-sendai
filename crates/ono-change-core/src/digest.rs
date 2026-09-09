//! The canonical plan digest (spec v0.6 §4.4).
//!
//! §4.4 lists what the seal covers: plan revision, target identities, the action graph, provider
//! identities and relevant versions, preconditions, protection policy, verification contracts and
//! accepted risk overrides. §2.6, §7.5 and §28.5 then depend on the digest changing whenever any
//! of those does, so the composition is written once, here, and every contributor supplies its own
//! `digest_text`.
//!
//! The encoding rule is boring on purpose: each contribution is separated by `US` (0x1f), each
//! section by `RS` (0x1e), and nothing is length-prefixed because nothing may contain a separator.
//! [`value_text`] is the only interesting part — it renders a [`Value`] canonically, so a plan
//! whose expected value is the integer 4 does not re-seal because a renderer chose a different
//! spelling for it.

use ono_value::Value;
use sha2::{Digest as _, Sha256};

/// The separator between two contributions inside a section.
pub const UNIT: char = '\u{1f}';

/// The separator between two sections of the digest.
pub const SECTION: char = '\u{1e}';

/// Builds the canonical digest of a plan out of its sections.
#[derive(Debug, Default)]
pub struct DigestBuilder {
    text: String,
}

impl DigestBuilder {
    /// A fresh builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one section, named so that two adjacent empty sections cannot be confused.
    #[must_use]
    pub fn section(mut self, name: &str, body: &str) -> Self {
        self.text.push_str(name);
        self.text.push(UNIT);
        self.text.push_str(body);
        self.text.push(SECTION);
        self
    }

    /// The digest, as 64 lowercase hexadecimal characters.
    #[must_use]
    pub fn finish(self) -> String {
        let bytes = Sha256::digest(self.text.as_bytes());
        let mut text = String::with_capacity(64);
        for byte in bytes {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
        }
        text
    }

    /// The canonical text the digest was taken over, for `explain` and for tests.
    #[must_use]
    pub fn canonical_text(&self) -> &str {
        &self.text
    }
}

/// A value, rendered so that equal values render equally and unequal ones do not.
///
/// `Value` has no canonical form of its own — `to_json_data` loses the type of a byte size, and
/// `Debug` is not a contract. This is deliberately a small, total function over the variants a
/// precondition or an expected value can hold; anything else falls back to a tagged debug form,
/// which is stable for the same value and different for different ones.
#[must_use]
pub fn value_text(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => format!("bool:{flag}"),
        Value::Int(number) => format!("int:{number}"),
        Value::Float(number) => format!("float:{number:?}"),
        Value::String(text) => format!("string:{text}"),
        Value::Path(path) => format!("path:{}", path.display()),
        Value::Port(port) => format!("port:{port}"),
        Value::Ip(address) => format!("ip:{address}"),
        Value::Timestamp(instant) => format!("timestamp:{instant}"),
        Value::List(items) => {
            let mut text = String::from("list:[");
            for item in items.iter() {
                text.push_str(&value_text(item));
                text.push(',');
            }
            text.push(']');
            text
        }
        other => format!("{}:{other:?}", other.type_name()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_give_the_same_sections_the_same_digest() {
        let first = DigestBuilder::new()
            .section("targets", "a")
            .section("actions", "b")
            .finish();
        let second = DigestBuilder::new()
            .section("targets", "a")
            .section("actions", "b")
            .finish();
        assert_eq!(
            first, second,
            "§63.2: a sealed plan's digest must be reproducible or it verifies nothing"
        );
    }

    #[test]
    fn should_tell_two_plans_apart_when_a_section_moves_between_them() {
        let split = DigestBuilder::new()
            .section("targets", "a")
            .section("actions", "b")
            .finish();
        let merged = DigestBuilder::new()
            .section("targets", "ab")
            .section("actions", "")
            .finish();
        assert_ne!(
            split, merged,
            "a separator that can be forged is a digest that proves nothing"
        );
    }

    #[test]
    fn should_render_equal_values_equally_and_unequal_ones_differently() {
        assert_eq!(value_text(&Value::Int(4)), value_text(&Value::Int(4)));
        assert_ne!(value_text(&Value::Int(4)), value_text(&Value::string("4")));
        assert_ne!(value_text(&Value::Null), value_text(&Value::string("null")));
    }

    #[test]
    fn should_render_a_digest_as_sixty_four_hexadecimal_characters() {
        let digest = DigestBuilder::new().section("x", "y").finish();
        assert_eq!(digest.len(), 64, "got {digest}");
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
