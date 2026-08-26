//! The CSV codec of spec §7.1 and §12.3: `to csv` out, `from csv` back in.
//!
//! # What CSV can and cannot carry
//!
//! CSV has one type — text — and no nesting, so it is the one format in this crate that is
//! honestly lossy. The design says so out loud rather than papering over it:
//!
//! | Situation | What happens |
//! |---|---|
//! | a stream of records with the same columns | a header row and one row per record |
//! | a field whose value is unknown | the word `null`, never an empty cell (spec §10.5) |
//! | a field holding an empty string | an empty cell, which stays distinct from `null` |
//! | a field holding bytes that are not text | lower-case hex, so no byte is lost (spec §12.2) |
//! | a field holding a record, a map or a list | [`ErrorCode::TypeMismatch`] naming the field |
//! | a stream whose records do not share columns | [`ErrorCode::TypeMismatch`] |
//! | a value that is not a record or a map | [`ErrorCode::TypeMismatch`] |
//!
//! Nothing is stringified into a shape that cannot be read back, and nothing that CSV cannot
//! represent is quietly flattened: spec §12.3 exists precisely to keep hidden formatting from
//! becoming API behaviour.
//!
//! # Reading CSV back
//!
//! [`from_csv`] returns one map per row, whose values are strings — because a CSV cell carries no
//! type, and inferring one would fabricate data that spec §35.3 forbids. The single exception is
//! the exact text `null`, which becomes [`Value::Null`]: it is what [`to_csv`] writes for an
//! unknown, and reading it back as the string `"null"` would turn the round trip into a lie in
//! the other direction. A foreign document whose cell genuinely holds the word `null` is
//! therefore read as unknown. That is the same kind of accepted ambiguity as the `$`-tags of
//! ADR-0016 item 6, and it is the only one this codec has.
//!
//! So `from_csv(to_csv(v)) == v` does not hold, and this crate does not claim it. What does hold
//! is that the *document* is a fixed point from the second pass onwards, which is the strongest
//! property a typeless format allows.

use std::sync::Arc;

use ono_core::ErrorCode;

use crate::{ErrorValue, MapValue, Value, canonical_text};

/// The cell that means "this value is unknown" (spec §10.5).
const NULL_CELL: &str = "null";

/// Serializes a stream of records or maps as a CSV document.
///
/// ```
/// use ono_value::{MapValue, Value, to_csv};
/// use std::sync::Arc;
/// let mut row = MapValue::new();
/// row.insert("name".into(), Value::string("nginx"));
/// row.insert("pid".into(), Value::Int(4419));
/// assert_eq!(to_csv(&[Value::Map(Arc::new(row))])?, "name,pid\nnginx,4419\n");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
///
/// # Errors
///
/// Returns [`ErrorCode::TypeMismatch`] for a value that is not a record or a map, for a stream
/// whose rows do not share a column list, and for a field whose value CSV cannot represent.
pub fn to_csv(values: &[Value]) -> Result<String, ErrorValue> {
    let Some(first) = values.first() else {
        return Ok(String::new());
    };
    let columns = columns_of(first)?;

    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());
    writer
        .write_record(columns.iter().map(String::as_str))
        .map_err(write_failed)?;

    for value in values {
        let row = columns_of(value)?;
        if row != columns {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                "the stream is heterogeneous, and CSV has one header row for all of it",
            )
            .with_help(format!(
                "expected the columns [{}] but found [{}]; use `to json` or `select` the shared fields",
                columns.join(", "),
                row.join(", ")
            )));
        }
        let cells = columns
            .iter()
            .map(|column| cell(value, column))
            .collect::<Result<Vec<String>, ErrorValue>>()?;
        writer.write_record(&cells).map_err(write_failed)?;
    }

    let bytes = writer.into_inner().map_err(|error| {
        ErrorValue::new(ErrorCode::TypeMismatch, "the document could not be written")
            .with_help(error.to_string())
    })?;
    String::from_utf8(bytes).map_err(|error| {
        ErrorValue::new(ErrorCode::TypeMismatch, "the document is not valid text")
            .with_help(error.to_string())
    })
}

/// Parses a CSV document with a header row into one map per row.
///
/// ```
/// use ono_value::{Value, from_csv};
/// let rows = from_csv("name,pid\nnginx,4419\n")?;
/// assert_eq!(rows.as_list()?[0].as_map()?.get("name"), Some(&Value::string("nginx")));
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
///
/// # Errors
///
/// Returns [`ErrorCode::ParseSyntax`] if the text is not CSV or a row does not match the header.
pub fn from_csv(text: &str) -> Result<Value, ErrorValue> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(false)
        .from_reader(text.as_bytes());

    let headers: Vec<Arc<str>> = reader
        .headers()
        .map_err(read_failed)?
        .iter()
        .map(Arc::<str>::from)
        .collect();

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(read_failed)?;
        let mut row = MapValue::new();
        for (index, header) in headers.iter().enumerate() {
            let raw = record.get(index).unwrap_or_default();
            let value = if raw == NULL_CELL {
                Value::Null
            } else {
                Value::String(raw.into())
            };
            row.insert(Arc::clone(header), value);
        }
        rows.push(Value::Map(Arc::new(row)));
    }
    Ok(Value::list(rows))
}

/// The column list a value contributes: a record's declared fields, then its extension keys.
fn columns_of(value: &Value) -> Result<Vec<String>, ErrorValue> {
    match value {
        Value::Record(record) => Ok(record
            .schema()
            .fields()
            .iter()
            .map(|field| field.name().to_owned())
            .chain(record.extra().keys().map(str::to_owned))
            .collect()),
        Value::Map(map) => Ok(map.keys().map(str::to_owned).collect()),
        other => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("a {} has no columns", other.type_name()),
        )
        .with_help("CSV holds records; use `to json` for anything else")),
    }
}

fn cell(value: &Value, column: &str) -> Result<String, ErrorValue> {
    let field = match value {
        Value::Record(record) => record
            .get(column)
            .or_else(|| record.extra().get(column))
            .cloned()
            .unwrap_or(Value::Null),
        Value::Map(map) => map.get(column).cloned().unwrap_or(Value::Null),
        other => {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("a {} has no columns", other.type_name()),
            ));
        }
    };
    canonical_text(&field).map_err(|error| {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "`{column}` holds a {} and CSV cannot nest",
                field.type_name()
            ),
        )
        .with_help("use `to json`, or `select` the leaf fields you want as columns")
        .with_source(error)
    })
}

fn write_failed(error: csv::Error) -> ErrorValue {
    ErrorValue::new(ErrorCode::TypeMismatch, "the row could not be written")
        .with_help(error.to_string())
}

fn read_failed(error: csv::Error) -> ErrorValue {
    ErrorValue::new(ErrorCode::ParseSyntax, "the input is not valid CSV")
        .with_help(error.to_string())
}
