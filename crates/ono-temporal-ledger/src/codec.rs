//! The versioned payload encoding of v0.5 §31.4: "typed payloads SHOULD use a versioned binary
//! representation such as CBOR ... The exact encoding MUST carry a schema/version identifier."
//!
//! Indexed scalar metadata stays relational — a timestamp a query filters on is a column, never a
//! field inside a blob — and everything an index never looks at travels as CBOR. Each blob starts
//! with its own schema id and version, so a row written by an older Ono is readable by a newer one
//! and a row a newer Ono cannot read fails *that row* rather than the process (§31.7).
//!
//! Every [`Value`] variant is representable, because a provider payload may hold any of them. A
//! value is a two-element CBOR array `[tag, body]`: the tag names the variant and the body carries
//! it exactly, with the wide integers (`i128`, `u128`, nanosecond instants) written as sixteen
//! big-endian bytes so no CBOR integer range ever truncates one.

use std::net::IpAddr;
use std::sync::Arc;

use ciborium::value::{Integer, Value as Cbor};
use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_spatial_core::{BootIdentity, ScopeKind, SpatialId, SpatialScope};
use ono_value::{
    ByteSize, Decimal, Duration, ErrorValue, IpNetwork, MapValue, Percent, Provenance, RecordValue,
    RegexValue, SchemaId, SchemaRegistry, Uuid, Value, ValueRef,
};

/// What a payload could not be read as. The caller turns it into `temporal.store_corrupt`.
pub(crate) type Decoded<T> = Result<T, String>;

/// The schema id every ledger payload carries in its first field (§31.4).
pub(crate) const PAYLOAD_SCHEMA: &str = "ono.temporal-ledger-payload";
/// The version of that encoding. A change to any tag below takes the next number.
pub(crate) const PAYLOAD_VERSION: u64 = 1;

const TAG_NULL: u64 = 0;
const TAG_BOOL: u64 = 1;
const TAG_INT: u64 = 2;
const TAG_FLOAT: u64 = 3;
const TAG_DECIMAL: u64 = 4;
const TAG_STRING: u64 = 5;
const TAG_BYTES: u64 = 6;
const TAG_PATH: u64 = 7;
const TAG_TIMESTAMP: u64 = 8;
const TAG_DURATION: u64 = 9;
const TAG_BYTESIZE: u64 = 10;
const TAG_PERCENT: u64 = 11;
const TAG_REGEX: u64 = 12;
const TAG_UUID: u64 = 13;
const TAG_IP: u64 = 14;
const TAG_IPNETWORK: u64 = 15;
const TAG_PORT: u64 = 16;
const TAG_LIST: u64 = 17;
const TAG_MAP: u64 = 18;
const TAG_RECORD: u64 = 19;
const TAG_ERROR: u64 = 20;

/// Writes a body as a self-describing payload blob (§31.4).
pub(crate) fn seal(body: Cbor) -> Decoded<Vec<u8>> {
    let framed = Cbor::Array(vec![
        Cbor::Text(PAYLOAD_SCHEMA.to_owned()),
        Cbor::Integer(Integer::from(PAYLOAD_VERSION)),
        body,
    ]);
    let mut bytes = Vec::new();
    ciborium::into_writer(&framed, &mut bytes)
        .map_err(|error| format!("a payload could not be encoded: {error}"))?;
    Ok(bytes)
}

/// Reads a payload blob back, refusing one that names a schema or a version this Ono cannot read.
pub(crate) fn unseal(bytes: &[u8]) -> Decoded<Cbor> {
    let framed: Cbor = ciborium::from_reader(bytes)
        .map_err(|error| format!("a payload did not decode as CBOR: {error}"))?;
    let parts = array(&framed)?;
    let [schema, version, body] = parts else {
        return Err(format!(
            "a payload frame carries {} fields rather than three",
            parts.len()
        ));
    };
    let schema = text(schema)?;
    if schema != PAYLOAD_SCHEMA {
        return Err(format!("a payload names the unknown schema `{schema}`"));
    }
    let version = unsigned(version)?;
    if version > PAYLOAD_VERSION {
        return Err(format!(
            "a payload was written at version {version}; this Ono reads up to {PAYLOAD_VERSION}"
        ));
    }
    Ok(body.clone())
}

// ---- primitives ------------------------------------------------------------------------------

pub(crate) fn array(value: &Cbor) -> Decoded<&[Cbor]> {
    match value {
        Cbor::Array(items) => Ok(items),
        other => Err(format!("expected an array, found {}", shape(other))),
    }
}

pub(crate) fn text(value: &Cbor) -> Decoded<&str> {
    match value {
        Cbor::Text(text) => Ok(text),
        other => Err(format!("expected text, found {}", shape(other))),
    }
}

pub(crate) fn optional_text(value: &Cbor) -> Decoded<Option<&str>> {
    match value {
        Cbor::Null => Ok(None),
        other => text(other).map(Some),
    }
}

pub(crate) fn boolean(value: &Cbor) -> Decoded<bool> {
    match value {
        Cbor::Bool(flag) => Ok(*flag),
        other => Err(format!("expected a boolean, found {}", shape(other))),
    }
}

pub(crate) fn unsigned(value: &Cbor) -> Decoded<u64> {
    match value {
        Cbor::Integer(number) => u64::try_from(*number)
            .map_err(|_| "an integer does not fit an unsigned 64-bit field".to_owned()),
        other => Err(format!("expected an integer, found {}", shape(other))),
    }
}

fn float(value: &Cbor) -> Decoded<f64> {
    match value {
        Cbor::Float(number) => Ok(*number),
        other => Err(format!("expected a float, found {}", shape(other))),
    }
}

fn bytes(value: &Cbor) -> Decoded<&[u8]> {
    match value {
        Cbor::Bytes(raw) => Ok(raw),
        other => Err(format!("expected bytes, found {}", shape(other))),
    }
}

fn shape(value: &Cbor) -> &'static str {
    match value {
        Cbor::Integer(_) => "an integer",
        Cbor::Bytes(_) => "bytes",
        Cbor::Float(_) => "a float",
        Cbor::Text(_) => "text",
        Cbor::Bool(_) => "a boolean",
        Cbor::Null => "null",
        Cbor::Tag(..) => "a tag",
        Cbor::Array(_) => "an array",
        Cbor::Map(_) => "a map",
        _ => "an unknown CBOR item",
    }
}

/// A signed 128-bit number as sixteen big-endian bytes, so no CBOR integer range truncates it.
pub(crate) fn wide(number: i128) -> Cbor {
    Cbor::Bytes(number.to_be_bytes().to_vec())
}

pub(crate) fn read_wide(value: &Cbor) -> Decoded<i128> {
    let raw = bytes(value)?;
    let sixteen: [u8; 16] = raw
        .try_into()
        .map_err(|_| format!("a wide integer carries {} bytes rather than 16", raw.len()))?;
    Ok(i128::from_be_bytes(sixteen))
}

fn wide_unsigned(number: u128) -> Cbor {
    Cbor::Bytes(number.to_be_bytes().to_vec())
}

fn read_wide_unsigned(value: &Cbor) -> Decoded<u128> {
    let raw = bytes(value)?;
    let sixteen: [u8; 16] = raw
        .try_into()
        .map_err(|_| format!("a wide integer carries {} bytes rather than 16", raw.len()))?;
    Ok(u128::from_be_bytes(sixteen))
}

pub(crate) fn instant(at: Timestamp) -> Cbor {
    wide(at.as_nanosecond())
}

pub(crate) fn read_instant(value: &Cbor) -> Decoded<Timestamp> {
    let nanos = read_wide(value)?;
    Timestamp::from_nanosecond(nanos)
        .map_err(|error| format!("a stored instant is not a timestamp: {error}"))
}

pub(crate) fn optional_instant(at: Option<Timestamp>) -> Cbor {
    at.map_or(Cbor::Null, instant)
}

pub(crate) fn read_optional_instant(value: &Cbor) -> Decoded<Option<Timestamp>> {
    match value {
        Cbor::Null => Ok(None),
        other => read_instant(other).map(Some),
    }
}

pub(crate) fn optional_string(text: Option<&str>) -> Cbor {
    text.map_or(Cbor::Null, |text| Cbor::Text(text.to_owned()))
}

// ---- scopes and identities -------------------------------------------------------------------

/// The separator between the parts of one scope level.
const PART: char = '\u{1e}';
/// The separator between scope levels, and the terminator of the last one.
///
/// The trailing separator is what makes a prefix comparison exact: `host:web0\u{1f}` is not a
/// prefix of `host:web01\u{1f}`, so a scope filter never widens itself by one character.
pub(crate) const LEVEL: char = '\u{1f}';

/// A scope as an ordered, prefix-comparable path.
///
/// `SpatialScope::contains(other)` is true exactly when this scope's path is a prefix of the
/// other's, which turns §32.3's "timeline for one place" into an index range scan.
pub(crate) fn scope_path(scope: &SpatialScope) -> String {
    let mut path = String::new();
    for level in scope.chain() {
        path.push_str(level.kind().as_str());
        path.push(PART);
        path.push_str(level.id());
        path.push(PART);
        path.push_str(level.boot().map_or("", BootIdentity::as_str));
        path.push(LEVEL);
    }
    path
}

/// The upper bound of the range of paths a scope filter accepts.
///
/// Every path under `prefix` sorts below `prefix` with its final separator raised by one, so the
/// filter is `path >= prefix AND path < upper_bound(prefix)` and an index answers it.
pub(crate) fn scope_upper_bound(prefix: &str) -> String {
    let mut bound = prefix.to_owned();
    bound.pop();
    bound.push('\u{20}');
    bound
}

pub(crate) fn read_scope_path(path: &str) -> Decoded<SpatialScope> {
    let mut levels = path.split(LEVEL).filter(|level| !level.is_empty());
    let root = levels
        .next()
        .ok_or_else(|| "a scope path is empty".to_owned())?;
    let (kind, id, boot) = read_level(root)?;
    let boot = boot.ok_or_else(|| "a root scope carries no boot identity".to_owned())?;
    let mut scope = match kind {
        ScopeKind::Host => SpatialScope::host(id, boot),
        ScopeKind::RemoteHost => SpatialScope::remote_host(id, boot),
        other => {
            return Err(format!(
                "a scope path is rooted in `{}` rather than a host",
                other.as_str()
            ));
        }
    };
    for level in levels {
        let (kind, id, _) = read_level(level)?;
        scope = scope.nest(kind, id);
    }
    Ok(scope)
}

fn read_level(level: &str) -> Decoded<(ScopeKind, &str, Option<BootIdentity>)> {
    let mut parts = level.split(PART);
    let kind = parts
        .next()
        .and_then(ScopeKind::from_name)
        .ok_or_else(|| format!("`{level}` does not name a scope kind"))?;
    let id = parts
        .next()
        .ok_or_else(|| format!("`{level}` names no scope id"))?;
    let boot = parts.next().filter(|boot| !boot.is_empty()).map(read_boot);
    Ok((kind, id, boot))
}

fn read_boot(text: &str) -> BootIdentity {
    match text.rsplit_once('/') {
        Some((host, boot)) => BootIdentity::new(host, boot),
        None => BootIdentity::unknown_boot(text),
    }
}

pub(crate) fn read_spatial_id(text: &str) -> Decoded<SpatialId> {
    SpatialId::parse(text).ok_or_else(|| format!("`{text}` is not a spatial identity"))
}

// ---- provenance ------------------------------------------------------------------------------

/// Provenance without its adapter trace.
///
/// §10.6 keeps adapter invocation detail out of the persisted ledger: it names an external
/// executable and the arguments it was run with, which is exactly the material §30.4 refuses to
/// bulk-record. The provider, the instant, the source, the link, the schema and the confidence are
/// what `inspect` reads back.
pub(crate) fn provenance(provenance: &Provenance) -> Cbor {
    Cbor::Array(vec![
        Cbor::Text(provenance.provider().to_owned()),
        optional_instant(provenance.observed()),
        optional_string(provenance.source()),
        Cbor::Text(provenance.link().to_string()),
        Cbor::Bool(matches!(provenance.link(), ono_value::Link::Local)),
        Cbor::Text(provenance.schema().to_string()),
        provenance.confidence().map_or(Cbor::Null, Cbor::Float),
    ])
}

pub(crate) fn read_provenance(value: &Cbor) -> Decoded<Provenance> {
    let parts = array(value)?;
    let [provider, observed, source, link, local, schema, confidence] = parts else {
        return Err(format!(
            "a provenance record carries {} fields rather than seven",
            parts.len()
        ));
    };
    let schema = read_schema_id(text(schema)?)?;
    let provider = text(provider)?;
    let mut built = if boolean(local)? {
        Provenance::local(provider, schema)
    } else {
        Provenance::remote(provider, text(link)?, schema)
    };
    if let Some(observed) = read_optional_instant(observed)? {
        built = built.observed_at(observed);
    }
    if let Some(source) = optional_text(source)? {
        built = built.from_source(source);
    }
    if let Cbor::Float(confidence) = confidence {
        built = built.with_confidence(*confidence);
    }
    Ok(built)
}

fn read_schema_id(text: &str) -> Decoded<SchemaId> {
    text.parse::<SchemaId>()
        .map_err(|error| format!("`{text}` is not a schema id: {}", error.message()))
}

// ---- values ----------------------------------------------------------------------------------

fn tagged(tag: u64, body: Cbor) -> Cbor {
    Cbor::Array(vec![Cbor::Integer(Integer::from(tag)), body])
}

/// Encodes any [`Value`], exactly, so a payload reads back as what was written.
pub(crate) fn value(value: &Value) -> Cbor {
    match value {
        Value::Null => tagged(TAG_NULL, Cbor::Null),
        Value::Bool(flag) => tagged(TAG_BOOL, Cbor::Bool(*flag)),
        Value::Int(number) => tagged(TAG_INT, wide(*number)),
        Value::Float(number) => tagged(TAG_FLOAT, Cbor::Float(*number)),
        Value::Decimal(number) => tagged(
            TAG_DECIMAL,
            Cbor::Array(vec![
                wide(number.mantissa()),
                Cbor::Integer(Integer::from(number.scale())),
            ]),
        ),
        Value::String(text) => tagged(TAG_STRING, Cbor::Text(text.to_string())),
        Value::Bytes(raw) => tagged(TAG_BYTES, Cbor::Bytes(raw.to_vec())),
        Value::Path(path) => tagged(TAG_PATH, Cbor::Bytes(path_bytes(path))),
        Value::Timestamp(at) => tagged(TAG_TIMESTAMP, instant(*at)),
        Value::Duration(span) => tagged(TAG_DURATION, wide(span.nanoseconds())),
        Value::ByteSize(size) => tagged(TAG_BYTESIZE, wide_unsigned(size.bytes())),
        Value::Percent(percent) => tagged(TAG_PERCENT, Cbor::Float(percent.value())),
        Value::Regex(regex) => tagged(TAG_REGEX, Cbor::Text(regex.source().to_owned())),
        Value::Uuid(uuid) => tagged(TAG_UUID, Cbor::Bytes(uuid.as_bytes().to_vec())),
        Value::Ip(address) => tagged(TAG_IP, Cbor::Text(address.to_string())),
        Value::IpNetwork(network) => tagged(
            TAG_IPNETWORK,
            Cbor::Array(vec![
                Cbor::Text(network.address().to_string()),
                Cbor::Integer(Integer::from(network.prefix_len())),
            ]),
        ),
        Value::Port(port) => tagged(TAG_PORT, Cbor::Integer(Integer::from(*port))),
        Value::List(items) => tagged(
            TAG_LIST,
            Cbor::Array(items.iter().map(self::value).collect()),
        ),
        Value::Map(map) => tagged(TAG_MAP, encode_map(map)),
        Value::Record(record) => tagged(TAG_RECORD, encode_record(record)),
        Value::Error(error) => tagged(TAG_ERROR, encode_error(error)),
    }
}

/// Decodes a value against `schemas`, which resolves the contract of every nested record.
pub(crate) fn read_value(item: &Cbor, schemas: &SchemaRegistry) -> Decoded<Value> {
    let parts = array(item)?;
    let [tag, body] = parts else {
        return Err(format!(
            "a value carries {} fields rather than a tag and a body",
            parts.len()
        ));
    };
    match unsigned(tag)? {
        TAG_NULL => Ok(Value::Null),
        TAG_BOOL => Ok(Value::Bool(boolean(body)?)),
        TAG_INT => Ok(Value::Int(read_wide(body)?)),
        TAG_FLOAT => Ok(Value::Float(float(body)?)),
        TAG_DECIMAL => {
            let parts = array(body)?;
            let [mantissa, scale] = parts else {
                return Err("a decimal carries a mantissa and a scale".to_owned());
            };
            let scale = u32::try_from(unsigned(scale)?)
                .map_err(|_| "a decimal scale does not fit 32 bits".to_owned())?;
            Decimal::new(read_wide(mantissa)?, scale)
                .map(Value::Decimal)
                .map_err(|error| format!("a stored decimal is invalid: {}", error.message()))
        }
        TAG_STRING => Ok(Value::string(text(body)?)),
        TAG_BYTES => Ok(Value::Bytes(bytes(body)?.to_vec().into())),
        TAG_PATH => Ok(Value::Path(read_path(bytes(body)?)?)),
        TAG_TIMESTAMP => Ok(Value::Timestamp(read_instant(body)?)),
        TAG_DURATION => Ok(Value::Duration(Duration::from_nanoseconds(read_wide(
            body,
        )?))),
        TAG_BYTESIZE => Ok(Value::ByteSize(ByteSize::from_bytes(read_wide_unsigned(
            body,
        )?))),
        TAG_PERCENT => Ok(Value::Percent(Percent::new(float(body)?))),
        TAG_REGEX => RegexValue::new(text(body)?)
            .map(|regex| Value::Regex(Arc::new(regex)))
            .map_err(|error| format!("a stored regex is invalid: {}", error.message())),
        TAG_UUID => {
            let raw = bytes(body)?;
            let sixteen: [u8; 16] = raw
                .try_into()
                .map_err(|_| format!("a uuid carries {} bytes rather than 16", raw.len()))?;
            Ok(Value::Uuid(Uuid::from_bytes(sixteen)))
        }
        TAG_IP => Ok(Value::Ip(read_address(text(body)?)?)),
        TAG_IPNETWORK => {
            let parts = array(body)?;
            let [address, prefix] = parts else {
                return Err("an ip network carries an address and a prefix".to_owned());
            };
            let prefix = u8::try_from(unsigned(prefix)?)
                .map_err(|_| "a prefix length does not fit 8 bits".to_owned())?;
            IpNetwork::new(read_address(text(address)?)?, prefix)
                .map(Value::IpNetwork)
                .map_err(|error| format!("a stored network is invalid: {}", error.message()))
        }
        TAG_PORT => u16::try_from(unsigned(body)?)
            .map(Value::Port)
            .map_err(|_| "a port does not fit 16 bits".to_owned()),
        TAG_LIST => {
            let mut items = Vec::new();
            for item in array(body)? {
                items.push(read_value(item, schemas)?);
            }
            Ok(Value::list(items))
        }
        TAG_MAP => Ok(Value::Map(Arc::new(read_map(body, schemas)?))),
        TAG_RECORD => Ok(Value::Record(Arc::new(read_record(body, schemas)?))),
        TAG_ERROR => Ok(Value::Error(Arc::new(read_error(body, schemas)?))),
        unknown => Err(format!("a value carries the unknown type tag {unknown}")),
    }
}

#[cfg(unix)]
fn path_bytes(path: &std::path::Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &std::path::Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn read_path(raw: &[u8]) -> Decoded<Arc<std::path::Path>> {
    use std::os::unix::ffi::OsStrExt as _;
    Ok(Arc::from(std::path::Path::new(
        std::ffi::OsStr::from_bytes(raw),
    )))
}

#[cfg(not(unix))]
fn read_path(raw: &[u8]) -> Decoded<Arc<std::path::Path>> {
    let text = std::str::from_utf8(raw).map_err(|_| "a stored path is not text".to_owned())?;
    Ok(Arc::from(std::path::Path::new(text)))
}

fn read_address(text: &str) -> Decoded<IpAddr> {
    text.parse()
        .map_err(|_| format!("`{text}` is not an ip address"))
}

fn encode_map(map: &MapValue) -> Cbor {
    Cbor::Array(
        map.iter()
            .map(|(key, held)| Cbor::Array(vec![Cbor::Text(key.to_owned()), value(held)]))
            .collect(),
    )
}

fn read_map(body: &Cbor, schemas: &SchemaRegistry) -> Decoded<MapValue> {
    let mut map = MapValue::new();
    for entry in array(body)? {
        let parts = array(entry)?;
        let [key, held] = parts else {
            return Err("a map entry carries a key and a value".to_owned());
        };
        map.insert(Arc::from(text(key)?), read_value(held, schemas)?);
    }
    Ok(map)
}

fn encode_record(record: &RecordValue) -> Cbor {
    let fields: Vec<Cbor> = record
        .schema()
        .fields()
        .iter()
        .enumerate()
        .map(|(index, field)| {
            Cbor::Array(vec![
                Cbor::Text(field.name().to_owned()),
                value(record.field_at(index).unwrap_or(&Value::Null)),
            ])
        })
        .collect();
    Cbor::Array(vec![
        Cbor::Text(record.schema_id().to_string()),
        provenance(record.provenance()),
        Cbor::Array(fields),
        encode_map(record.extra()),
    ])
}

/// Reads a record back against the registry the store was opened with.
///
/// A field the current schema no longer declares survives as a provider extension rather than
/// being dropped: §31.4 defers to the schema evolution rules, and those keep unknown data rather
/// than discarding it. A schema the registry does not know at all fails the row (§31.7).
fn read_record(body: &Cbor, schemas: &SchemaRegistry) -> Decoded<RecordValue> {
    let parts = array(body)?;
    let [schema, provenance, fields, extra] = parts else {
        return Err(format!(
            "a record carries {} fields rather than four",
            parts.len()
        ));
    };
    let id = read_schema_id(text(schema)?)?;
    let contract = schemas
        .get(&id)
        .ok_or_else(|| format!("no schema `{id}` is registered, so the record cannot be read"))?;
    let mut builder = RecordValue::builder(contract, read_provenance(provenance)?);
    for entry in array(fields)? {
        let parts = array(entry)?;
        let [name, held] = parts else {
            return Err("a record field carries a name and a value".to_owned());
        };
        let name = text(name)?;
        let held = read_value(held, schemas)?;
        let attempt = builder.clone();
        builder = match attempt.set(name, held.clone()) {
            Ok(filled) => filled,
            Err(_) => builder.set_extra(name, held),
        };
    }
    for (name, held) in read_map(extra, schemas)?.iter() {
        builder = builder.set_extra(name, held.clone());
    }
    Ok(builder.build())
}

fn encode_error(error: &ErrorValue) -> Cbor {
    Cbor::Array(vec![
        Cbor::Text(error.code().name().to_owned()),
        Cbor::Text(error.message().to_owned()),
        error
            .help()
            .map_or(Cbor::Null, |help| Cbor::Text(help.to_owned())),
        error.retryable().map_or(Cbor::Null, Cbor::Bool),
        error.target().map_or(Cbor::Null, encode_reference),
        error.cause().map_or(Cbor::Null, encode_error),
        encode_map(error.metadata()),
    ])
}

fn read_error(body: &Cbor, schemas: &SchemaRegistry) -> Decoded<ErrorValue> {
    let parts = array(body)?;
    let [code, message, help, retryable, target, cause, metadata] = parts else {
        return Err(format!(
            "an error carries {} fields rather than seven",
            parts.len()
        ));
    };
    let code = text(code)?;
    let code = ErrorCode::from_name(code)
        .ok_or_else(|| format!("`{code}` is not a structured error code"))?;
    let mut error = ErrorValue::new(code, text(message)?.to_owned());
    if let Some(help) = optional_text(help)? {
        error = error.with_help(help.to_owned());
    }
    if let Cbor::Bool(retryable) = retryable {
        error = error.with_retryable(*retryable);
    }
    if !matches!(target, Cbor::Null) {
        error = error.with_target(read_reference(target, schemas)?);
    }
    if !matches!(cause, Cbor::Null) {
        error = error.with_source(read_error(cause, schemas)?);
    }
    for (key, held) in read_map(metadata, schemas)?.iter() {
        error = error.with_metadata(key, held.clone());
    }
    Ok(error)
}

fn encode_reference(reference: &ValueRef) -> Cbor {
    match reference {
        ValueRef::Path(path) => Cbor::Array(vec![
            Cbor::Text("path".to_owned()),
            Cbor::Bytes(path_bytes(path)),
        ]),
        ValueRef::Name(name) => Cbor::Array(vec![
            Cbor::Text("name".to_owned()),
            Cbor::Text(name.to_string()),
        ]),
        ValueRef::Object { schema, identity } => Cbor::Array(vec![
            Cbor::Text("object".to_owned()),
            Cbor::Text(schema.to_string()),
            encode_map(identity),
        ]),
    }
}

fn read_reference(body: &Cbor, schemas: &SchemaRegistry) -> Decoded<ValueRef> {
    let parts = array(body)?;
    match parts.split_first() {
        Some((kind, [payload])) if text(kind)? == "path" => {
            Ok(ValueRef::path(&read_path(bytes(payload)?)?))
        }
        Some((kind, [payload])) if text(kind)? == "name" => Ok(ValueRef::name(text(payload)?)),
        Some((kind, [schema, identity])) if text(kind)? == "object" => Ok(ValueRef::object(
            read_schema_id(text(schema)?)?,
            read_map(identity, schemas)?,
        )),
        _ => Err("a value reference names no known kind".to_owned()),
    }
}

/// Turns a decoding failure into the §34 refusal a caller sees.
pub(crate) fn corrupt(segment: &str, detail: &str) -> ErrorValue {
    ono_temporal_core::error::store_corrupt(segment, detail)
}

/// Decode a stored payload blob, for a fuzz target and for nothing else.
///
/// §31.4's payload is a versioned CBOR document, and §31.7 requires a damaged one to fail its row
/// rather than the process. The bytes that reach it are the bytes a plugin or a remote host
/// contributed, written and read back, so it is an attacker-influenced decoder in the sense v0.2
/// §35.6 means. Exposed so `fuzz` can reach it without the store around it — the store is fuzzed
/// separately, and a decoder read through a database spends its budget on SQLite.
///
/// # Errors
///
/// Returns the reason the bytes are not a payload this build wrote.
pub fn decode_payload(bytes: &[u8]) -> Result<ono_value::Value, String> {
    let sealed = unseal(bytes)?;
    read_value(&sealed, ono_value::builtin_schemas())
}
