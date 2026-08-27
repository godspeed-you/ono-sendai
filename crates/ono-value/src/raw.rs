//! The raw byte form of spec §12.2: `to bytes` out, `from bytes` back in.
//!
//! Ono is a good Unix citizen because it can always fall back to bytes (spec §12.1). This codec
//! is that fallback and nothing more: it carries the values that genuinely *are* bytes — a byte
//! string, text, a path — and refuses everything else rather than inventing an encoding for it.
//! Spec §12.3 is explicit that "an object pipeline cannot be silently sent to an arbitrary
//! process", so `to bytes` on a record is an error that names `to json`, not a guess.

use bytes::{BufMut, Bytes, BytesMut};
use ono_core::ErrorCode;

use crate::{ErrorValue, Value};

/// Serializes a value into its raw bytes.
///
/// A byte string is itself, text is its UTF-8 encoding, a path is the bytes the operating system
/// holds — never a lossy decode, so a path that is not valid text survives intact. A list is the
/// concatenation of its elements, which is what a stream of chunks means.
///
/// ```
/// use bytes::Bytes;
/// use ono_value::{Value, to_bytes};
/// assert_eq!(to_bytes(&Value::string("nginx"))?, Bytes::from_static(b"nginx"));
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
///
/// # Errors
///
/// Returns [`ErrorCode::TypeMismatch`] for a value with no raw byte form — a number, a
/// timestamp, a record, or the unknown of spec §10.5 — with help naming a codec that can carry
/// it.
pub fn to_bytes(value: &Value) -> Result<Bytes, ErrorValue> {
    let mut out = BytesMut::new();
    append(value, &mut out)?;
    Ok(out.freeze())
}

fn append(value: &Value, out: &mut BytesMut) -> Result<(), ErrorValue> {
    match value {
        Value::Bytes(raw) => out.put_slice(raw),
        Value::String(text) => out.put_slice(text.as_bytes()),
        Value::Path(path) => {
            out.put_slice(std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()));
        }
        Value::List(items) => {
            for item in items.iter() {
                append(item, out)?;
            }
        }
        other => {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("a {} has no raw byte form", other.type_name()),
            )
            .with_help("use `to json`, `to yaml`, `to csv` or `to text`"));
        }
    }
    Ok(())
}

/// Reads bytes as a value, which is always the byte string itself.
///
/// Nothing is decoded and nothing is guessed. Spec §12.2 asks for raw bytes to be retained and
/// text to be exposed only where decoding succeeds under a configured encoding; deciding that is
/// the caller's job, and doing it here would silently discard whatever failed to decode.
///
/// ```
/// use bytes::Bytes;
/// use ono_value::{Value, from_bytes};
/// assert_eq!(from_bytes(vec![0xff, 0xfe]), Value::Bytes(Bytes::from_static(&[0xff, 0xfe])));
/// ```
#[must_use]
pub fn from_bytes(bytes: impl Into<Bytes>) -> Value {
    Value::Bytes(bytes.into())
}
