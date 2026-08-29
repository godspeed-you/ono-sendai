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
    let depth = yaml_depth(text);
    if depth > MAX_YAML_DEPTH {
        return Err(ErrorValue::new(
            ErrorCode::ParseSyntax,
            format!(
                "the document nests {depth} collections deep, and {MAX_YAML_DEPTH} is the limit"
            ),
        )
        .with_help(
            "a document this deep is refused by the YAML parser too; it is refused here so that \
             saying no costs no more than reading it (spec §49)",
        ));
    }
    let json: serde_json::Value = serde_yaml_ng::from_str(text).map_err(|error| {
        ErrorValue::new(ErrorCode::ParseSyntax, "the input is not valid YAML")
            .with_help(error.to_string())
    })?;
    from_json(&json, schemas)
}

/// The deepest flow-collection nesting any decoder in this workspace hands to the YAML parser.
///
/// The parser refuses deeper documents on its own, and the work it does before refusing grows
/// with the square of the depth: 100 kB of `{e: {e: {…` took seven seconds to be turned down.
/// Counting the nesting first — one linear scan — turns that into the same refusal at the speed
/// of reading. Found by the §35.6 serializer fuzz target (ADR-0313).
///
/// Every decoder that reads YAML written by somebody else — a plugin manifest, a package
/// signature, an adapter pack, an operator's policy or trust store — checks [`yaml_depth`]
/// against this before parsing. The compiled-in contracts of this build do not: they are not
/// input.
pub const MAX_YAML_DEPTH: usize = 128;

/// The deepest `{`/`[` nesting in `text`, skipping quoted scalars and comments.
///
/// It counts only unquoted brackets, so it can under-count — a `{` this reads as being inside a
/// string is not counted — and never over-count. Under-counting costs nothing: the parser still
/// refuses what this lets through. Over-counting would refuse a document that is fine, which is
/// why the quoting is tracked at all.
///
/// ```
/// use ono_value::{MAX_YAML_DEPTH, yaml_depth};
/// assert_eq!(yaml_depth("a: [1, [2, [3]]]\n"), 3);
/// assert_eq!(yaml_depth("a: \"{{{{\"\n"), 0);
/// assert!(yaml_depth(&"{e: ".repeat(1_000)) > MAX_YAML_DEPTH);
/// ```
#[must_use]
pub fn yaml_depth(text: &str) -> usize {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Plain,
        Single,
        Double,
        Comment,
    }

    let mut state = State::Plain;
    let mut depth = 0_usize;
    let mut deepest = 0_usize;
    let mut previous = b' ';
    let mut bytes = text.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        match state {
            State::Comment => {
                if byte == b'\n' {
                    state = State::Plain;
                }
            }
            State::Single => {
                if byte == b'\'' {
                    state = State::Plain;
                }
            }
            State::Double => match byte {
                b'\\' => {
                    let _ = bytes.next();
                }
                b'"' => state = State::Plain,
                _ => {}
            },
            State::Plain => match byte {
                b'\'' => state = State::Single,
                b'"' => state = State::Double,
                // A `#` opens a comment only at a word boundary; `a#b` is a plain scalar.
                b'#' if previous.is_ascii_whitespace() => state = State::Comment,
                b'{' | b'[' => {
                    depth += 1;
                    deepest = deepest.max(depth);
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            },
        }
        previous = byte;
    }
    deepest
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
