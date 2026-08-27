//! The canonical text of a scalar, and the line-oriented `to text` of spec §29.1.
//!
//! ```text
//! get file . --recursive | select path | to text --field path | xargs wc -l
//! ```
//!
//! This is the bridge that lets an object pipeline feed an ordinary Unix tool, so it is
//! deliberately narrow. One value becomes exactly one line. A value that cannot become one line —
//! a record, a list, a string containing a newline, a byte sequence that is not text — is a
//! structured error naming what to use instead (spec §12.3), never a best-effort string.

use std::fmt::Write as _;

use ono_core::ErrorCode;

use crate::{ErrorValue, FieldStep, Value};

/// The canonical text of a scalar: the form that parses back, not the form a human prefers.
///
/// Spec §33.5 asks for canonical values unless a human rendering is explicitly requested, so a
/// byte size is `1288490188B` here and `1.20 GiB` in the renderer. `null` is the word `null`,
/// because spec §10.5 forbids an unknown from becoming an empty string.
///
/// ```
/// use ono_value::{ByteSize, Value, canonical_text};
/// assert_eq!(canonical_text(&Value::ByteSize(ByteSize::from_bytes(1024)))?, "1024B");
/// assert_eq!(canonical_text(&Value::Null)?, "null");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
///
/// # Errors
///
/// Returns [`ErrorCode::TypeMismatch`] for a list, a map or a record, none of which a single
/// text has any faithful shape for.
pub fn canonical_text(value: &Value) -> Result<String, ErrorValue> {
    match value {
        Value::Null => Ok("null".to_owned()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Int(value) => Ok(value.to_string()),
        Value::Float(value) => Ok(value.to_string()),
        Value::Decimal(value) => Ok(value.to_string()),
        Value::String(value) => Ok(value.to_string()),
        // Hex rather than a lossy decode: spec §12.2 requires undecodable bytes never to be lost.
        Value::Bytes(value) => Ok(crate::hex::encode(value)),
        Value::Path(value) => value.to_str().map(str::to_owned).ok_or_else(|| {
            not_text("this path is not valid text").with_help(
                "use `to json`, which keeps the raw bytes of a path that is not valid text",
            )
        }),
        Value::Timestamp(value) => Ok(value.to_string()),
        Value::Duration(value) => Ok(value.exact()),
        Value::ByteSize(value) => Ok(value.exact()),
        Value::Percent(value) => Ok(value.to_string()),
        Value::Regex(value) => Ok(value.source().to_owned()),
        Value::Uuid(value) => Ok(value.to_string()),
        Value::Ip(value) => Ok(value.to_string()),
        Value::IpNetwork(value) => Ok(value.to_string()),
        Value::Port(value) => Ok(value.to_string()),
        // One line, never the two that `render_terse` may produce: the code keeps the failure
        // identifiable and the message keeps it readable.
        Value::Error(error) => Ok(format!("{}: {}", error.code().name(), error.message())),
        Value::List(_) | Value::Map(_) | Value::Record(_) => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("a {} has no single text form", value.type_name()),
        )
        .with_help("use `to json`, `to yaml` or `format table`")),
    }
}

/// Serializes values as one line each, optionally projecting one field first (spec §29.1).
///
/// `field` is the `--field` of `to text --field path`, and accepts a dotted path. Reading it
/// follows the rules of spec §10.5 exactly: an unknown value becomes the word `null`, and a
/// field whose read failed propagates that failure instead of becoming a blank line.
///
/// ```
/// use ono_value::{Value, to_text};
/// assert_eq!(to_text(&[Value::Int(1), Value::string("two")], None)?, "1\ntwo\n");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
///
/// # Errors
///
/// Returns [`ErrorCode::TypeMismatch`] for a value with no single-line form,
/// [`ErrorCode::TypeUnknownField`] when `field` names no field, and whatever error a failed
/// field access recorded.
pub fn to_text(values: &[Value], field: Option<&str>) -> Result<String, ErrorValue> {
    let mut out = String::new();
    for value in values {
        let projected = match field {
            Some(path) => {
                let steps: Vec<FieldStep<'_>> = path.split('.').map(FieldStep::required).collect();
                value.follow(&steps)?
            }
            None => value.clone(),
        };
        let line = line_text(&projected)?;
        let _ = writeln!(out, "{line}");
    }
    Ok(out)
}

/// The text of one line: canonical text, except that bytes are decoded rather than hexed.
fn line_text(value: &Value) -> Result<String, ErrorValue> {
    let text = match value {
        Value::Bytes(raw) => std::str::from_utf8(raw)
            .map_err(|_| {
                not_text("these bytes are not valid text").with_help(
                    "use `to bytes` for the raw form, or `to json`, which keeps them as hex",
                )
            })?
            .to_owned(),
        Value::Record(_) | Value::Map(_) | Value::List(_) => {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("a {} does not fit on one line", value.type_name()),
            )
            .with_help("name one field with `--field`, or use `to json` or `format table`"));
        }
        other => canonical_text(other)?,
    };
    if text.contains('\n') {
        return Err(not_text("this value contains a line break").with_help(
            "a line-oriented format cannot carry it without turning one value into two; use `to json`",
        ));
    }
    Ok(text)
}

fn not_text(message: &str) -> ErrorValue {
    ErrorValue::new(ErrorCode::TypeMismatch, message.to_owned())
}
