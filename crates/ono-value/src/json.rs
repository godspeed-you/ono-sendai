//! The JSON codec of spec §12.4 and §46: `to json` out, `from json` back in.
//!
//! # What JSON can and cannot carry
//!
//! JSON has four scalar types; Ono has twenty-one. Values whose JSON shape is unambiguous —
//! null, booleans, strings, lists, maps and numbers within `i64`/`f64` range — encode directly,
//! because that is the form an external tool expects to read. Every semantic scalar encodes as a
//! **single-key tagged object** instead, so nothing is silently flattened into a bare number
//! whose unit is gone:
//!
//! | Value | JSON |
//! |---|---|
//! | `ByteSize(1288490188)` | `{"$bytesize": 1288490188}` |
//! | `Duration` | `{"$duration": "250000000ns"}` |
//! | `Timestamp` | `{"$timestamp": "2026-08-26T06:13:04.182Z"}` |
//! | `Bytes` | `{"$bytes": "fffe0080"}` |
//! | `Path` | `{"$path": "/etc/passwd"}` or `{"$path_bytes": "…"}` when not valid text |
//! | `Record` | `{"$record": {"schema": …, "fields": …, "extra": …, "provenance": …}}` |
//!
//! Bytes and non-text paths encode as hex rather than as a decoded string, because spec §12.2
//! requires undecodable bytes never to be lost. An `Int` outside `i64` and a non-finite `Float`
//! take tagged forms for the same reason: JSON's number grammar cannot hold them.
//!
//! Decoding a record needs the schema back, so [`from_json`] takes the registry to resolve
//! `ono.process/1` into the field order the record was written with.
//!
//! The one ambiguity this design accepts: a foreign document whose object happens to have
//! exactly one key named `$bytesize` decodes as a byte size. Tagging is what makes the round trip
//! lossless, and no untagged encoding can be both lossless and natural.

use std::sync::Arc;

use ono_core::ErrorCode;
use serde_json::{Map, Number, Value as Json};

use crate::{
    ByteSize, Decimal, Duration, ErrorValue, IpNetwork, Link, MapValue, Percent, Provenance,
    RecordValue, RegexValue, Schema, SchemaId, SchemaRegistry, Uuid, Value, ValueRef,
};

/// Encodes a value as JSON.
///
/// ```
/// use ono_value::{ByteSize, Value, to_json};
/// let json = to_json(&Value::ByteSize(ByteSize::from_bytes(1024)));
/// assert_eq!(json.to_string(), r#"{"$bytesize":1024}"#);
/// ```
#[must_use]
pub fn to_json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(value) => Json::Bool(*value),
        Value::Int(value) => match i64::try_from(*value) {
            Ok(small) => Json::Number(Number::from(small)),
            Err(_) => tagged("$int", Json::String(value.to_string())),
        },
        Value::Float(value) => match Number::from_f64(*value) {
            Some(number) => Json::Number(number),
            None => tagged("$float", Json::String(non_finite_name(*value).to_owned())),
        },
        Value::Decimal(value) => tagged("$decimal", Json::String(value.to_string())),
        Value::String(value) => Json::String(value.to_string()),
        Value::Bytes(value) => tagged("$bytes", Json::String(crate::hex::encode(value))),
        Value::Path(value) => match value.to_str() {
            Some(text) => tagged("$path", Json::String(text.to_owned())),
            None => tagged(
                "$path_bytes",
                Json::String(crate::hex::encode(std::os::unix::ffi::OsStrExt::as_bytes(
                    value.as_os_str(),
                ))),
            ),
        },
        Value::Timestamp(value) => tagged("$timestamp", Json::String(value.to_string())),
        Value::Duration(value) => tagged("$duration", Json::String(value.exact())),
        Value::ByteSize(value) => tagged("$bytesize", byte_count(value.bytes())),
        Value::Percent(value) => tagged(
            "$percent",
            Number::from_f64(value.value()).map_or_else(
                || Json::String(non_finite_name(value.value()).to_owned()),
                Json::Number,
            ),
        ),
        Value::Regex(value) => tagged("$regex", Json::String(value.source().to_owned())),
        Value::Uuid(value) => tagged("$uuid", Json::String(value.to_string())),
        Value::Ip(value) => tagged("$ip", Json::String(value.to_string())),
        Value::IpNetwork(value) => tagged("$ipnetwork", Json::String(value.to_string())),
        Value::Port(value) => tagged("$port", Json::Number(Number::from(*value))),
        Value::List(items) => Json::Array(items.iter().map(to_json).collect()),
        Value::Map(map) => Json::Object(map_to_json(map)),
        Value::Record(record) => tagged("$record", record_to_json(record)),
        Value::Error(error) => tagged("$error", error_to_json(error)),
    }
}

/// Encodes a value as a JSON document.
///
/// # Errors
///
/// Returns [`ErrorCode::TypeMismatch`] if the encoded document cannot be serialized, which no
/// value produced by [`to_json`] can cause.
pub fn to_json_string(value: &Value) -> Result<String, ErrorValue> {
    serde_json::to_string(&to_json(value)).map_err(|error| {
        ErrorValue::new(ErrorCode::TypeMismatch, "the value could not be serialized")
            .with_help(error.to_string())
    })
}

/// Decodes a value from JSON, resolving record schemas through `schemas`.
///
/// # Errors
///
/// Returns [`ErrorCode::ResolveTargetNotFound`] when a record names a schema the registry does
/// not hold, and [`ErrorCode::TypeMismatch`] when a tagged value does not hold what its tag
/// promises.
pub fn from_json(json: &Json, schemas: &SchemaRegistry) -> Result<Value, ErrorValue> {
    match json {
        Json::Null => Ok(Value::Null),
        Json::Bool(value) => Ok(Value::Bool(*value)),
        Json::Number(number) => Ok(number_from_json(number)),
        Json::String(text) => Ok(Value::String(text.as_str().into())),
        Json::Array(items) => items
            .iter()
            .map(|item| from_json(item, schemas))
            .collect::<Result<Vec<Value>, ErrorValue>>()
            .map(Value::list),
        Json::Object(object) => object_from_json(object, schemas),
    }
}

/// Decodes a value from a JSON document.
///
/// # Errors
///
/// Returns [`ErrorCode::ParseSyntax`] if the text is not JSON, and whatever [`from_json`] returns
/// otherwise.
pub fn from_json_str(text: &str, schemas: &SchemaRegistry) -> Result<Value, ErrorValue> {
    let json: Json = serde_json::from_str(text).map_err(|error| {
        ErrorValue::new(ErrorCode::ParseSyntax, "the input is not valid JSON")
            .with_help(error.to_string())
    })?;
    from_json(&json, schemas)
}

fn tagged(tag: &str, payload: Json) -> Json {
    let mut object = Map::with_capacity(1);
    object.insert(tag.to_owned(), payload);
    Json::Object(object)
}

fn byte_count(bytes: u128) -> Json {
    u64::try_from(bytes).map_or_else(
        |_| Json::String(bytes.to_string()),
        |small| Json::Number(Number::from(small)),
    )
}

fn non_finite_name(value: f64) -> &'static str {
    if value.is_nan() {
        "nan"
    } else if value.is_sign_negative() {
        "-inf"
    } else {
        "inf"
    }
}

fn non_finite_value(name: &str) -> Option<f64> {
    match name {
        "nan" => Some(f64::NAN),
        "inf" => Some(f64::INFINITY),
        "-inf" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

fn map_to_json(map: &MapValue) -> Map<String, Json> {
    map.iter()
        .map(|(key, value)| (key.to_owned(), to_json(value)))
        .collect()
}

fn record_to_json(record: &RecordValue) -> Json {
    let mut fields = Map::new();
    for (index, field) in record.schema().fields().iter().enumerate() {
        let value = record.field_at(index).unwrap_or(&Value::Null);
        fields.insert(field.name().to_owned(), to_json(value));
    }
    let mut object = Map::new();
    object.insert(
        "schema".to_owned(),
        Json::String(record.schema_id().to_string()),
    );
    object.insert("fields".to_owned(), Json::Object(fields));
    object.insert(
        "extra".to_owned(),
        Json::Object(map_to_json(record.extra())),
    );
    object.insert(
        "provenance".to_owned(),
        provenance_to_json(record.provenance()),
    );
    Json::Object(object)
}

fn provenance_to_json(provenance: &Provenance) -> Json {
    let mut object = Map::new();
    object.insert(
        "provider".to_owned(),
        Json::String(provenance.provider().to_owned()),
    );
    object.insert(
        "observed".to_owned(),
        provenance
            .observed()
            .map_or(Json::Null, |observed| Json::String(observed.to_string())),
    );
    object.insert(
        "source".to_owned(),
        provenance
            .source()
            .map_or(Json::Null, |source| Json::String(source.to_owned())),
    );
    object.insert(
        "link".to_owned(),
        match provenance.link() {
            Link::Local => Json::String("local".to_owned()),
            Link::Remote(host) => tagged("remote", Json::String(host.to_string())),
        },
    );
    object.insert(
        "schema".to_owned(),
        Json::String(provenance.schema().to_string()),
    );
    object.insert(
        "confidence".to_owned(),
        provenance
            .confidence()
            .and_then(Number::from_f64)
            .map_or(Json::Null, Json::Number),
    );
    Json::Object(object)
}

fn error_to_json(error: &ErrorValue) -> Json {
    let mut object = Map::new();
    object.insert(
        "code".to_owned(),
        Json::String(error.code().name().to_owned()),
    );
    object.insert(
        "message".to_owned(),
        Json::String(error.message().to_owned()),
    );
    object.insert(
        "target".to_owned(),
        error.target().map_or(Json::Null, value_ref_to_json),
    );
    object.insert(
        "help".to_owned(),
        error
            .help()
            .map_or(Json::Null, |help| Json::String(help.to_owned())),
    );
    object.insert(
        "retryable".to_owned(),
        error.retryable().map_or(Json::Null, Json::Bool),
    );
    object.insert(
        "metadata".to_owned(),
        Json::Object(map_to_json(error.metadata())),
    );
    object.insert(
        "source".to_owned(),
        error.cause().map_or(Json::Null, error_to_json),
    );
    Json::Object(object)
}

fn value_ref_to_json(reference: &ValueRef) -> Json {
    let mut object = Map::new();
    match reference {
        ValueRef::Path(path) => {
            object.insert("kind".to_owned(), Json::String("path".to_owned()));
            object.insert("path".to_owned(), to_json(&Value::Path(Arc::clone(path))));
        }
        ValueRef::Name(name) => {
            object.insert("kind".to_owned(), Json::String("name".to_owned()));
            object.insert("name".to_owned(), Json::String(name.to_string()));
        }
        ValueRef::Object { schema, identity } => {
            object.insert("kind".to_owned(), Json::String("object".to_owned()));
            object.insert("schema".to_owned(), Json::String(schema.to_string()));
            object.insert("identity".to_owned(), Json::Object(map_to_json(identity)));
        }
    }
    Json::Object(object)
}

fn number_from_json(number: &Number) -> Value {
    if let Some(value) = number.as_i64() {
        return Value::Int(i128::from(value));
    }
    if let Some(value) = number.as_u64() {
        return Value::Int(i128::from(value));
    }
    Value::Float(number.as_f64().unwrap_or(f64::NAN))
}

fn object_from_json(
    object: &Map<String, Json>,
    schemas: &SchemaRegistry,
) -> Result<Value, ErrorValue> {
    if object.len() == 1
        && let Some((tag, payload)) = object.iter().next()
        && let Some(value) = tagged_from_json(tag, payload, schemas)?
    {
        return Ok(value);
    }
    let mut map = MapValue::new();
    for (key, value) in object {
        map.insert(key.as_str().into(), from_json(value, schemas)?);
    }
    Ok(Value::Map(Arc::new(map)))
}

fn tagged_from_json(
    tag: &str,
    payload: &Json,
    schemas: &SchemaRegistry,
) -> Result<Option<Value>, ErrorValue> {
    let text = || payload.as_str().ok_or_else(|| bad_tag(tag, payload));
    let value = match tag {
        "$int" => Value::Int(text()?.parse::<i128>().map_err(|_| bad_tag(tag, payload))?),
        "$float" => Value::Float(non_finite_value(text()?).ok_or_else(|| bad_tag(tag, payload))?),
        "$decimal" => Value::Decimal(Decimal::parse(text()?)?),
        "$bytes" => Value::Bytes(bytes::Bytes::from(
            crate::hex::decode(text()?).ok_or_else(|| bad_tag(tag, payload))?,
        )),
        "$path" => Value::Path(Arc::from(std::path::Path::new(text()?))),
        "$path_bytes" => {
            let raw = crate::hex::decode(text()?).ok_or_else(|| bad_tag(tag, payload))?;
            let os: &std::ffi::OsStr = std::os::unix::ffi::OsStrExt::from_bytes(raw.as_slice());
            Value::Path(Arc::from(std::path::Path::new(os)))
        }
        "$timestamp" => Value::Timestamp(
            text()?
                .parse::<jiff::Timestamp>()
                .map_err(|_| bad_tag(tag, payload))?,
        ),
        "$duration" => Value::Duration(Duration::parse(text()?)?),
        "$bytesize" => Value::ByteSize(ByteSize::from_bytes(match payload {
            Json::Number(number) => {
                u128::from(number.as_u64().ok_or_else(|| bad_tag(tag, payload))?)
            }
            Json::String(digits) => digits.parse::<u128>().map_err(|_| bad_tag(tag, payload))?,
            _ => return Err(bad_tag(tag, payload)),
        })),
        "$percent" => Value::Percent(Percent::new(match payload {
            Json::Number(number) => number.as_f64().ok_or_else(|| bad_tag(tag, payload))?,
            Json::String(name) => non_finite_value(name).ok_or_else(|| bad_tag(tag, payload))?,
            _ => return Err(bad_tag(tag, payload)),
        })),
        "$regex" => Value::Regex(Arc::new(RegexValue::new(text()?)?)),
        "$uuid" => Value::Uuid(Uuid::parse(text()?)?),
        "$ip" => Value::Ip(text()?.parse().map_err(|_| bad_tag(tag, payload))?),
        "$ipnetwork" => Value::IpNetwork(IpNetwork::parse(text()?)?),
        "$port" => Value::Port(
            payload
                .as_u64()
                .and_then(|port| u16::try_from(port).ok())
                .ok_or_else(|| bad_tag(tag, payload))?,
        ),
        "$record" => record_from_json(payload, schemas)?,
        "$error" => Value::Error(Arc::new(error_from_json(payload, schemas)?)),
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn record_from_json(payload: &Json, schemas: &SchemaRegistry) -> Result<Value, ErrorValue> {
    let object = payload
        .as_object()
        .ok_or_else(|| bad_tag("$record", payload))?;
    let id: SchemaId = object
        .get("schema")
        .and_then(Json::as_str)
        .ok_or_else(|| bad_tag("$record", payload))?
        .parse()?;
    let schema: Arc<Schema> = schemas.get(&id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            format!("no schema is registered as {id}"),
        )
    })?;
    let provenance =
        provenance_from_json(object.get("provenance").unwrap_or(&Json::Null), id.clone())?;
    let mut builder = RecordValue::builder(Arc::clone(&schema), provenance);
    if let Some(fields) = object.get("fields").and_then(Json::as_object) {
        for (name, value) in fields {
            builder = builder.set(name, from_json(value, schemas)?)?;
        }
    }
    if let Some(extra) = object.get("extra").and_then(Json::as_object) {
        for (key, value) in extra {
            builder = builder.set_extra(key, from_json(value, schemas)?);
        }
    }
    Ok(builder.build().into_value())
}

fn provenance_from_json(payload: &Json, schema: SchemaId) -> Result<Provenance, ErrorValue> {
    let Some(object) = payload.as_object() else {
        return Ok(Provenance::local("unknown", schema));
    };
    let provider = object
        .get("provider")
        .and_then(Json::as_str)
        .unwrap_or("unknown");
    let recorded_schema = object
        .get("schema")
        .and_then(Json::as_str)
        .map_or(Ok(schema), str::parse)?;
    let mut provenance = match object.get("link") {
        Some(Json::Object(link)) => match link.get("remote").and_then(Json::as_str) {
            Some(host) => Provenance::remote(provider, host, recorded_schema),
            None => Provenance::local(provider, recorded_schema),
        },
        _ => Provenance::local(provider, recorded_schema),
    };
    if let Some(observed) = object.get("observed").and_then(Json::as_str) {
        provenance = provenance.observed_at(
            observed
                .parse::<jiff::Timestamp>()
                .map_err(|_| bad_tag("observed", payload))?,
        );
    }
    if let Some(source) = object.get("source").and_then(Json::as_str) {
        provenance = provenance.from_source(source);
    }
    if let Some(confidence) = object.get("confidence").and_then(Json::as_f64) {
        provenance = provenance.with_confidence(confidence);
    }
    Ok(provenance)
}

fn error_from_json(payload: &Json, schemas: &SchemaRegistry) -> Result<ErrorValue, ErrorValue> {
    let object = payload
        .as_object()
        .ok_or_else(|| bad_tag("$error", payload))?;
    let name = object
        .get("code")
        .and_then(Json::as_str)
        .ok_or_else(|| bad_tag("$error", payload))?;
    let code = ono_core::ErrorCode::from_name(name).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            format!("`{name}` is not a known error code"),
        )
    })?;
    let message = object.get("message").and_then(Json::as_str).unwrap_or("");
    let mut error = ErrorValue::new(code, message);
    if let Some(target) = object.get("target").and_then(Json::as_object) {
        error = error.with_target(value_ref_from_json(target, schemas)?);
    }
    if let Some(help) = object.get("help").and_then(Json::as_str) {
        error = error.with_help(help);
    }
    if let Some(retryable) = object.get("retryable").and_then(Json::as_bool) {
        error = error.with_retryable(retryable);
    }
    if let Some(metadata) = object.get("metadata").and_then(Json::as_object) {
        for (key, value) in metadata {
            error = error.with_metadata(key, from_json(value, schemas)?);
        }
    }
    if let Some(source) = object.get("source").filter(|value| !value.is_null()) {
        error = error.with_source(error_from_json(source, schemas)?);
    }
    Ok(error)
}

fn value_ref_from_json(
    object: &Map<String, Json>,
    schemas: &SchemaRegistry,
) -> Result<ValueRef, ErrorValue> {
    let payload = Json::Object(object.clone());
    match object.get("kind").and_then(Json::as_str) {
        Some("path") => {
            let path = from_json(object.get("path").unwrap_or(&Json::Null), schemas)?;
            Ok(ValueRef::path(path.as_path()?))
        }
        Some("name") => Ok(ValueRef::name(
            object
                .get("name")
                .and_then(Json::as_str)
                .ok_or_else(|| bad_tag("target", &payload))?,
        )),
        Some("object") => {
            let id: SchemaId = object
                .get("schema")
                .and_then(Json::as_str)
                .ok_or_else(|| bad_tag("target", &payload))?
                .parse()?;
            let mut identity = MapValue::new();
            if let Some(fields) = object.get("identity").and_then(Json::as_object) {
                for (key, value) in fields {
                    identity.insert(key.as_str().into(), from_json(value, schemas)?);
                }
            }
            Ok(ValueRef::object(id, identity))
        }
        _ => Err(bad_tag("target", &payload)),
    }
}

fn bad_tag(tag: &str, payload: &Json) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!("`{tag}` does not hold what its tag promises"),
    )
    .with_help(payload.to_string())
}
