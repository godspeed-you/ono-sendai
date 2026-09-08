//! The content digest every temporal identity is built from (ADR-0620).
//!
//! One shape for all of them: named parts, separated the way `ono_spatial_core::SpatialIdentity`
//! separates them (`0x1f` between parts, `0x1e` between a name and its value), reduced with
//! SHA-256 and rendered as the hex prefix the id's length asks for. Equal inputs therefore give
//! equal ids, which is the whole of v0.5 §6.8's deduplication.

use std::fmt::Write as _;

use sha2::{Digest as _, Sha256};

use ono_value::{MapValue, Value};

/// Separates one named part from the next.
const PART: u8 = 0x1f;
/// Separates a part's name from its value.
const NAME: u8 = 0x1e;

/// Accumulates the named parts of an identity.
pub(crate) struct Digest(Sha256);

impl Digest {
    /// A digest of `kind`, so two identities of different kinds never collide.
    pub(crate) fn new(kind: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(kind.as_bytes());
        Self(hasher)
    }

    /// Adds one named part.
    pub(crate) fn part(&mut self, name: &str, value: &str) {
        self.0.update([PART]);
        self.0.update(name.as_bytes());
        self.0.update([NAME]);
        self.0.update(value.as_bytes());
    }

    /// Adds a named part that may be absent, distinguishing absence from an empty value.
    pub(crate) fn optional(&mut self, name: &str, value: Option<&str>) {
        match value {
            Some(value) => self.part(name, value),
            None => self.part(name, "\u{0}"),
        }
    }

    /// The first `bytes` bytes of the digest, as lowercase hex.
    pub(crate) fn finish(self, bytes: usize) -> String {
        let digest = self.0.finalize();
        let mut hex = String::with_capacity(bytes * 2);
        for byte in digest.iter().take(bytes) {
            let _ = write!(hex, "{byte:02x}");
        }
        hex
    }
}

/// A canonical, order-independent text for a value, so equal values digest equally.
///
/// Map keys are sorted because two reports of one observation may build the same map in a
/// different order, and §6.8 asks for them to collide rather than to be told apart by an
/// accident of iteration.
pub(crate) fn token(value: &Value) -> String {
    let mut out = String::new();
    write_token(value, &mut out);
    out
}

fn write_token(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::List(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_token(item, out);
            }
            out.push(']');
        }
        Value::Map(map) => write_map(map, out),
        Value::Record(record) => {
            let _ = write!(out, "{}", record.schema_id());
            out.push('{');
            for (index, field) in record.schema().fields().iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(field.name());
                out.push('=');
                write_token(record.field_at(index).unwrap_or(&Value::Null), out);
            }
            out.push('}');
        }
        Value::Error(error) => {
            let _ = write!(out, "error({}:{})", error.code().code(), error.message());
        }
        other => match ono_value::canonical_text(other) {
            Ok(text) => {
                let _ = write!(out, "{}:{text}", other.type_name());
            }
            // Only a path or a byte string that is not text reaches here, and its bytes still
            // have to contribute, or two different observations would digest alike.
            Err(_) => {
                let _ = write!(out, "{}:{other:?}", other.type_name());
            }
        },
    }
}

fn write_map(map: &MapValue, out: &mut String) {
    let mut keys: Vec<&str> = map.keys().collect();
    keys.sort_unstable();
    out.push('{');
    for (index, key) in keys.into_iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(key);
        out.push('=');
        write_token(map.get(key).unwrap_or(&Value::Null), out);
    }
    out.push('}');
}
