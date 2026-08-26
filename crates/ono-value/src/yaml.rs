//! The YAML codec of spec §7.1 and §13.6: `to yaml` out, `from yaml` back in.
//!
//! YAML carries exactly what JSON carries, so this is the tagged codec of ADR-0016 item 6 in a
//! different syntax rather than a second design. A byte size is `$bytesize: 1288490188`, bytes
//! that are not text are hex, and a record keeps its schema id so it can be rebuilt — the same
//! rules, the same guarantees, the same accepted ambiguity around a foreign document whose map
//! happens to have exactly one `$`-prefixed key.
//!
//! [`to_yaml_data`] is the other half of the pair, mirroring [`to_json_data`]: the interop
//! encoding of spec §33.5, with no Ono envelope around the data, which is what the user-facing
//! `to yaml` writes.
//!
//! ```text
//! $record:
//!   schema: ono.process/1
//!   fields:
//!     pid: 812
//!     memory:
//!       $bytesize: 1288490188
//! ```

use ono_core::ErrorCode;

use crate::{ErrorValue, SchemaRegistry, Value, from_json, to_json, to_json_data};

/// Serializes a value as a YAML document.
///
/// ```
/// use ono_value::{ByteSize, Value, to_yaml};
/// assert_eq!(to_yaml(&Value::ByteSize(ByteSize::from_bytes(1024)))?, "$bytesize: 1024\n");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
///
/// # Errors
///
/// Returns [`ErrorCode::TypeMismatch`] if the document cannot be emitted as YAML.
pub fn to_yaml(value: &Value) -> Result<String, ErrorValue> {
    serde_yaml_ng::to_string(&to_json(value)).map_err(|error| {
        ErrorValue::new(ErrorCode::TypeMismatch, "the value could not be serialized")
            .with_help(error.to_string())
    })
}

/// Parses a YAML document into a value, resolving record schemas through `schemas`.
///
/// ```
/// use ono_value::{SchemaRegistry, Value, from_yaml};
/// let value = from_yaml("name: nginx\n", &SchemaRegistry::new())?;
/// assert_eq!(value.as_map()?.get("name"), Some(&Value::string("nginx")));
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
///
/// # Errors
///
/// Returns [`ErrorCode::ParseSyntax`] if the text is not YAML, and whatever [`from_json`]
/// returns for the document it describes.
pub fn from_yaml(text: &str, schemas: &SchemaRegistry) -> Result<Value, ErrorValue> {
    let json: serde_json::Value = serde_yaml_ng::from_str(text).map_err(|error| {
        ErrorValue::new(ErrorCode::ParseSyntax, "the input is not valid YAML")
            .with_help(error.to_string())
    })?;
    from_json(&json, schemas)
}

/// Serializes a value as YAML for a reader that knows nothing about Ono (spec §33.5, §12.3).
///
/// This is what `to yaml` writes: the same interop job as [`to_json_data`], in YAML's syntax.
///
/// ```
/// use ono_value::{ByteSize, Value, to_yaml_data};
/// assert_eq!(to_yaml_data(&Value::ByteSize(ByteSize::from_bytes(1024)))?, "1024\n");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
///
/// # Errors
///
/// Returns [`ErrorCode::TypeMismatch`] if the document cannot be emitted as YAML.
pub fn to_yaml_data(value: &Value) -> Result<String, ErrorValue> {
    serde_yaml_ng::to_string(&to_json_data(value)).map_err(|error| {
        ErrorValue::new(ErrorCode::TypeMismatch, "the value could not be serialized")
            .with_help(error.to_string())
    })
}
