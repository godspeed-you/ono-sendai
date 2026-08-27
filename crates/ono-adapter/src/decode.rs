//! Turning a tool's output into canonical records (spec v0.3 §1.8, §1.10, §1.11, ADR-0057).
//!
//! A decoder is total: every input becomes records or one structured error. Coercion is
//! driven by the schema's declared field types, exactness by the contract's field map, and
//! nothing is ever invented — a field the tool did not report is `null`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_value::{
    AdapterTrace, ByteSize, Duration, ErrorValue, FieldType, MapValue, Percent, Provenance,
    RecordValue, Schema, SchemaId, SchemaRegistry, Value,
};
use serde_json::Value as Json;

use crate::contract::{Adapter, DecoderKind, Exactness, FieldMap, Stability, Unit};
use crate::version::Version;

/// How many raw bytes an error keeps, so a decode failure stays inspectable without
/// carrying a whole output around.
const RAW_KEPT: usize = 4096;

/// What an adapted run was, for provenance and for errors (spec v0.3 §1.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    /// The executable that ran.
    pub executable: PathBuf,
    /// Its version, when the probe found one.
    pub version: Option<Version>,
    /// The invocation as the user typed it.
    pub user_invocation: Vec<String>,
    /// The invocation that actually ran.
    pub actual_invocation: Vec<String>,
    /// The remote host, when the command ran across a link.
    pub host: Option<String>,
}

/// Decodes `bytes` — everything `adapter`'s plan wrote to stdout — into records.
///
/// # Errors
///
/// `adapter.decode_failed` when the bytes are not what the decoder reads, with the adapter,
/// the executable, the invocation, whether raw fallback is safe and the first 4 KiB of the
/// bytes in the metadata; `adapter.schema_violation` when a decoded field cannot become the
/// type the schema declares, naming the field.
pub fn decode(
    adapter: &Adapter,
    bytes: &[u8],
    trace: &Trace,
    schemas: &SchemaRegistry,
) -> Result<Vec<Value>, ErrorValue> {
    let mut decoding = Decoding::borrowed(adapter, trace.clone(), schemas)?;
    let mut values = Vec::new();
    for outcome in decoding.feed(bytes).into_iter().chain(decoding.finish()) {
        values.push(outcome?);
    }
    Ok(values)
}

/// Who owns the adapter a decoding reads for.
enum Held<'a> {
    Borrowed(&'a Adapter),
    Owned(crate::registry::AdapterPlan),
}

/// An incremental decoding: bytes go in as they arrive, records come out as soon as the
/// decoder can stand behind them (ADR-0057 point 4, ADR-0059).
///
/// A streaming kind (`jsonl`) yields a record per complete line from `feed`; every other kind
/// buffers and answers from `finish`. Either way every outcome is a value or a structured
/// error, never a panic.
pub struct Decoding<'a> {
    held: Held<'a>,
    trace: Trace,
    schema: Arc<Schema>,
    buffer: Vec<u8>,
    line: usize,
    observed: jiff::Timestamp,
}

impl<'a> Decoding<'a> {
    /// A decoding for `adapter`, borrowed.
    ///
    /// # Errors
    ///
    /// `adapter.schema_violation` when the contract's schema is not registered.
    pub fn borrowed(
        adapter: &'a Adapter,
        trace: Trace,
        schemas: &SchemaRegistry,
    ) -> Result<Self, ErrorValue> {
        let schema = registered_schema(adapter, &trace, schemas)?;
        Ok(Self {
            held: Held::Borrowed(adapter),
            trace,
            schema,
            buffer: Vec::new(),
            line: 0,
            observed: jiff::Timestamp::now(),
        })
    }

    /// A decoding that owns its adapter through the plan, so it can move to a reader thread.
    ///
    /// # Errors
    ///
    /// `adapter.schema_violation` when the contract's schema is not registered.
    pub fn for_plan(
        plan: crate::registry::AdapterPlan,
        trace: Trace,
        schemas: &SchemaRegistry,
    ) -> Result<Decoding<'static>, ErrorValue> {
        let schema = registered_schema(plan.adapter(), &trace, schemas)?;
        Ok(Decoding {
            held: Held::Owned(plan),
            trace,
            schema,
            buffer: Vec::new(),
            line: 0,
            observed: jiff::Timestamp::now(),
        })
    }

    fn adapter(&self) -> &Adapter {
        match &self.held {
            Held::Borrowed(adapter) => adapter,
            Held::Owned(plan) => plan.adapter(),
        }
    }

    /// Whether records leave `feed` as the bytes arrive.
    #[must_use]
    pub fn streams(&self) -> bool {
        self.adapter().decoder().kind().streams()
    }

    /// Takes the next bytes the child wrote.
    ///
    /// A streaming decoder answers with every record completed by these bytes; the others
    /// answer nothing until `finish`.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Result<Value, ErrorValue>> {
        self.buffer.extend_from_slice(chunk);
        if !self.streams() {
            return Vec::new();
        }
        let mut outcomes = Vec::new();
        while let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            self.line += 1;
            let line = &line[..line.len() - 1];
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            outcomes.push(self.decode_line(line));
        }
        outcomes
    }

    /// Takes the end of the output: what a streaming decoder still holds is a fragment and
    /// fails; a buffering decoder decodes everything now.
    pub fn finish(&mut self) -> Vec<Result<Value, ErrorValue>> {
        let bytes = std::mem::take(&mut self.buffer);
        if self.streams() {
            if bytes.iter().all(u8::is_ascii_whitespace) {
                return Vec::new();
            }
            self.line += 1;
            return vec![Err(decode_failed(
                self.adapter(),
                &self.trace,
                &bytes,
                format!("line {} ended before its record did", self.line),
            ))];
        }
        let adapter = self.adapter();
        let items = match adapter.decoder().kind() {
            DecoderKind::Json => json_records(adapter, &bytes, &self.trace),
            DecoderKind::Lines => line_records(adapter, &bytes, &self.trace),
            DecoderKind::Properties => property_records(adapter, &bytes, &self.trace),
            DecoderKind::Jsonl => Ok(Vec::new()),
            DecoderKind::Builtin => Err(decode_failed(
                adapter,
                &self.trace,
                &bytes,
                format!(
                    "builtin decoder `{}` is not available in this binary",
                    adapter.decoder().id().unwrap_or("?")
                ),
            )),
        };
        let items = match items {
            Ok(items) => items,
            Err(error) => return vec![Err(error)],
        };
        let mut outcomes = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            outcomes.push(
                build_record(adapter, &self.schema, item, &self.trace, self.observed).map_err(
                    |reason| {
                        violation(
                            adapter,
                            &self.trace,
                            format!("record {}: {reason}", index + 1),
                        )
                    },
                ),
            );
            if outcomes.last().is_some_and(Result::is_err) {
                break;
            }
        }
        outcomes
    }

    fn decode_line(&self, line: &[u8]) -> Result<Value, ErrorValue> {
        let adapter = self.adapter();
        let document: Json = serde_json::from_slice(line).map_err(|error| {
            decode_failed(
                adapter,
                &self.trace,
                line,
                format!("line {} is not JSON: {error}", self.line),
            )
        })?;
        let Json::Object(fields) = document else {
            return Err(decode_failed(
                adapter,
                &self.trace,
                line,
                format!(
                    "line {} is {}, not an object",
                    self.line,
                    json_kind(&document)
                ),
            ));
        };
        let item = Item {
            fields,
            parent: None,
        };
        build_record(adapter, &self.schema, &item, &self.trace, self.observed).map_err(|reason| {
            violation(
                adapter,
                &self.trace,
                format!("line {}: {reason}", self.line),
            )
        })
    }
}

fn registered_schema(
    adapter: &Adapter,
    trace: &Trace,
    schemas: &SchemaRegistry,
) -> Result<Arc<Schema>, ErrorValue> {
    adapter
        .schema()
        .parse::<SchemaId>()
        .ok()
        .and_then(|id| schemas.get(&id))
        .ok_or_else(|| {
            violation(
                adapter,
                trace,
                format!("schema `{}` is not registered", adapter.schema()),
            )
        })
}

/// One decoded record: the tool's fields by name, plus the parent's fields for a tree.
#[derive(Debug, Clone)]
struct Item {
    fields: serde_json::Map<String, Json>,
    parent: Option<serde_json::Map<String, Json>>,
}

fn json_records(adapter: &Adapter, bytes: &[u8], trace: &Trace) -> Result<Vec<Item>, ErrorValue> {
    let document: Json = serde_json::from_slice(bytes)
        .map_err(|error| decode_failed(adapter, trace, bytes, format!("not JSON: {error}")))?;
    let list: Vec<Json> = match adapter.decoder().records() {
        Some(key) => match document.get(key) {
            Some(Json::Array(items)) => items.clone(),
            Some(other) => {
                return Err(decode_failed(
                    adapter,
                    trace,
                    bytes,
                    format!("`{key}` holds {} rather than a list", json_kind(other)),
                ));
            }
            None => {
                return Err(decode_failed(
                    adapter,
                    trace,
                    bytes,
                    format!("the document has no `{key}` list"),
                ));
            }
        },
        None => match document {
            Json::Array(items) => items,
            object @ Json::Object(_) => vec![object],
            other => {
                return Err(decode_failed(
                    adapter,
                    trace,
                    bytes,
                    format!(
                        "the document is {}, not a record or a list",
                        json_kind(&other)
                    ),
                ));
            }
        },
    };
    let mut items = Vec::new();
    if let Some(key) = adapter.decoder().children() {
        for (index, entry) in list.iter().enumerate() {
            let Json::Object(parent) = entry else {
                return Err(decode_failed(
                    adapter,
                    trace,
                    bytes,
                    format!(
                        "record {} is {}, not an object",
                        index + 1,
                        json_kind(entry)
                    ),
                ));
            };
            match parent.get(key) {
                None | Some(Json::Null) => {}
                Some(Json::Array(children)) => {
                    for (position, child) in children.iter().enumerate() {
                        let Json::Object(fields) = child else {
                            return Err(decode_failed(
                                adapter,
                                trace,
                                bytes,
                                format!(
                                    "`{key}` entry {} of record {} is {}, not an object",
                                    position + 1,
                                    index + 1,
                                    json_kind(child)
                                ),
                            ));
                        };
                        items.push(Item {
                            fields: fields.clone(),
                            parent: Some(parent.clone()),
                        });
                    }
                }
                Some(other) => {
                    return Err(decode_failed(
                        adapter,
                        trace,
                        bytes,
                        format!(
                            "`{key}` of record {} is {}, not a list",
                            index + 1,
                            json_kind(other)
                        ),
                    ));
                }
            }
        }
        return Ok(items);
    }
    flatten(adapter, &list, None, &mut items, 0)
        .map_err(|reason| decode_failed(adapter, trace, bytes, reason))?;
    Ok(items)
}

fn flatten(
    adapter: &Adapter,
    list: &[Json],
    parent: Option<&serde_json::Map<String, Json>>,
    into: &mut Vec<Item>,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("the tree nests deeper than any device tree does".to_owned());
    }
    for (index, entry) in list.iter().enumerate() {
        let Json::Object(fields) = entry else {
            return Err(format!(
                "record {} is {}, not an object",
                index + 1,
                json_kind(entry)
            ));
        };
        let mut fields = fields.clone();
        let children = adapter
            .decoder()
            .nested()
            .and_then(|key| fields.remove(key));
        into.push(Item {
            fields: fields.clone(),
            parent: parent.cloned(),
        });
        if let Some(Json::Array(children)) = children {
            flatten(adapter, &children, Some(&fields), into, depth + 1)?;
        }
    }
    Ok(())
}

fn line_records(adapter: &Adapter, bytes: &[u8], trace: &Trace) -> Result<Vec<Item>, ErrorValue> {
    let decoder = adapter.decoder();
    let (Some(field_separator), Some(record_separator), Some(columns)) = (
        decoder.field_separator(),
        decoder.record_separator(),
        decoder.columns(),
    ) else {
        return Err(decode_failed(
            adapter,
            trace,
            bytes,
            "the lines decoder is incomplete".to_owned(),
        ));
    };
    let field_separator = unescape(field_separator);
    let record_separator = unescape(record_separator);
    let mut items = Vec::new();
    for (index, record) in bytes
        .split(|byte| record_separator.contains(byte))
        .enumerate()
    {
        if record.is_empty() {
            continue;
        }
        let parts: Vec<&[u8]> = record
            .split(|byte| field_separator.contains(byte))
            .collect();
        if parts.len() != columns.len() {
            return Err(decode_failed(
                adapter,
                trace,
                bytes,
                format!(
                    "record {} has {} fields where the contract expects {}",
                    index + 1,
                    parts.len(),
                    columns.len()
                ),
            ));
        }
        let mut fields = serde_json::Map::new();
        for (column, part) in columns.iter().zip(parts) {
            fields.insert(
                column.clone(),
                Json::String(String::from_utf8_lossy(part).into_owned()),
            );
        }
        items.push(Item {
            fields,
            parent: None,
        });
    }
    Ok(items)
}

fn unescape(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push(b'\t'),
                Some('n') => out.push(b'\n'),
                Some('0') => out.push(0),
                Some(other) => {
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                }
                None => out.push(b'\\'),
            }
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    out
}

/// `key=value` lines, one record per block (spec v0.3 §1.9 tier B: `systemctl show`).
fn property_records(
    adapter: &Adapter,
    bytes: &[u8],
    trace: &Trace,
) -> Result<Vec<Item>, ErrorValue> {
    let decoder = adapter.decoder();
    let (Some(field_separator), Some(record_separator)) =
        (decoder.field_separator(), decoder.record_separator())
    else {
        return Err(decode_failed(
            adapter,
            trace,
            bytes,
            "the properties decoder is incomplete".to_owned(),
        ));
    };
    let field_separator = unescape(field_separator);
    let record_separator = unescape(record_separator);
    let text = String::from_utf8_lossy(bytes);
    let field_separator = String::from_utf8_lossy(&field_separator).into_owned();
    let record_separator = String::from_utf8_lossy(&record_separator).into_owned();
    let mut items = Vec::new();
    for (index, block) in text.split(record_separator.as_str()).enumerate() {
        if block.trim().is_empty() {
            continue;
        }
        let mut fields = serde_json::Map::new();
        for line in block.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(field_separator.as_str()) else {
                return Err(decode_failed(
                    adapter,
                    trace,
                    bytes,
                    format!(
                        "record {}: `{}` is not `key{field_separator}value`",
                        index + 1,
                        line.chars().take(64).collect::<String>()
                    ),
                ));
            };
            let value = if value.is_empty() {
                Json::Null
            } else {
                Json::String(value.to_owned())
            };
            fields.insert(key.to_owned(), value);
        }
        items.push(Item {
            fields,
            parent: None,
        });
    }
    Ok(items)
}

fn build_record(
    adapter: &Adapter,
    schema: &Arc<Schema>,
    item: &Item,
    trace: &Trace,
    observed: jiff::Timestamp,
) -> Result<Value, String> {
    let actual = trace.actual_invocation.join(" ");
    let mut adapter_trace = AdapterTrace::new(
        &adapter.full_id(),
        adapter.pack_version(),
        &trace.executable,
    )
    .executable_version_of(trace.version.as_ref().map(ToString::to_string).as_deref())
    .invocations(&trace.user_invocation.join(" "), &actual)
    .decoded_by(
        match adapter.decoder().kind() {
            DecoderKind::Json => "json",
            DecoderKind::Jsonl => "jsonl",
            DecoderKind::Lines => "lines",
            DecoderKind::Properties => "properties",
            DecoderKind::Builtin => adapter.decoder().id().unwrap_or("builtin"),
        },
        match adapter.decoder().stability() {
            Some(Stability::VersionConstrained) => "version-constrained",
            _ => "stable",
        },
    );
    let limits: Vec<String> = schema
        .fields()
        .iter()
        .filter(|field| !adapter.fields().contains_key(field.name()))
        .map(|field| format!("`{}` is not reported by {}", field.name(), adapter.id()))
        .collect();
    adapter_trace = adapter_trace.with_limits(limits);

    let provider = format!("adapter:{}", adapter.full_id());
    let provenance = match &trace.host {
        Some(host) => Provenance::remote(&provider, host, schema.id().clone()),
        None => Provenance::local(&provider, schema.id().clone()),
    }
    .from_source(&actual)
    .observed_at(observed);

    let mut builder = RecordValue::builder(Arc::clone(schema), provenance.clone());
    let mut referenced: Vec<&str> = Vec::new();
    for (target, map) in adapter.fields() {
        let Some(field) = schema.field(target) else {
            return Err(format!("`{target}` is not a field of {}", schema.id()));
        };
        let whole = Json::Object(item.fields.clone());
        let raw = if map.from().is_empty() {
            Some(&whole)
        } else {
            lookup(item, map.from())
        };
        if !map.from().starts_with("$parent.") && !map.from().is_empty() {
            referenced.push(map.from());
        }
        if let Some(template) = map.template() {
            referenced.extend(placeholders(template));
        }
        let value = match raw {
            None | Some(Json::Null) => Value::Null,
            Some(raw) => coerce_mapped(raw, map, field.ty())
                .map_err(|why| format!("field `{target}` of {} {why}", schema.id()))?,
        };
        builder = builder
            .set(target, value)
            .map_err(|error| error.message().to_owned())?;
        match map.exactness() {
            Exactness::Exact => {}
            Exactness::Normalized => {
                adapter_trace = adapter_trace.field_exactness(target, "normalized");
            }
            Exactness::Inferred => {
                adapter_trace = adapter_trace.field_exactness(target, "inferred");
            }
        }
    }

    // Fields the invocation itself implies — `family` for `ip -6 route` — are constants the
    // contract states; the tool never printed them, so they are not decoded, only set.
    for (target, literal) in adapter.literals() {
        let Some(field) = schema.field(target) else {
            return Err(format!("`{target}` is not a field of {}", schema.id()));
        };
        let value = coerce(&yaml_to_json(literal), field.ty(), None)
            .map_err(|why| format!("literal `{target}` of {} {why}", schema.id()))?;
        builder = builder
            .set(target, value)
            .map_err(|error| error.message().to_owned())?;
    }

    // What the tool reported and the contract did not map stays visible under the adapter's
    // own namespace (spec v0.3 §1.11), never in a canonical field and never dropped.
    let mut extension = MapValue::new();
    for (name, raw) in &item.fields {
        if !referenced.contains(&name.as_str()) {
            extension.insert(name.as_str().into(), plain(raw));
        }
    }
    if !extension.is_empty() {
        builder = builder.set_extra(&adapter.full_id(), Value::Map(Arc::new(extension)));
    }

    let record = builder
        .provenance(provenance.adapted_by(adapter_trace))
        .build();
    schema
        .validate(&record)
        .map_err(|error| error.message().to_owned())?;
    Ok(record.into_value())
}

/// The decoded field a map reads: the item's own, or the parent's for `$parent.<field>`.
fn lookup<'a>(item: &'a Item, from: &str) -> Option<&'a Json> {
    match from.strip_prefix("$parent.") {
        Some(field) => item.parent.as_ref().and_then(|parent| parent.get(field)),
        None => item.fields.get(from),
    }
}

/// Applies the map's translations, then coerces into the schema's type.
/// The `{field}` names a template reads.
fn placeholders(template: &str) -> Vec<&str> {
    template
        .split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
        .collect()
}

/// Fills a template from one decoded object; a placeholder the object lacks is an error,
/// because a half-filled address would be a fabricated one.
fn fill(template: &str, object: &serde_json::Map<String, Json>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some((literal, after)) = rest.split_once('{') {
        out.push_str(literal);
        let (name, tail) = after
            .split_once('}')
            .ok_or_else(|| format!("template `{template}` has an unclosed placeholder"))?;
        match object.get(name).and_then(scalar_text) {
            Some(text) => out.push_str(&text),
            None => {
                return Err(format!(
                    "the tool reported no `{name}` for template `{template}`"
                ));
            }
        }
        rest = tail;
    }
    out.push_str(rest);
    Ok(out)
}

fn coerce_mapped(raw: &Json, map: &FieldMap, ty: &FieldType) -> Result<Value, String> {
    let mut current = raw.clone();
    if let Some(template) = map.template() {
        current = match &current {
            Json::Object(object) => Json::String(fill(template, object)?),
            Json::Array(items) => Json::Array(
                items
                    .iter()
                    .map(|item| match item {
                        Json::Object(object) => fill(template, object).map(Json::String),
                        other => Err(format!("cannot fill a template from {}", json_kind(other))),
                    })
                    .collect::<Result<Vec<Json>, String>>()?,
            ),
            other => return Err(format!("cannot fill a template from {}", json_kind(other))),
        };
    }
    if map.takes_first() {
        current = match &current {
            Json::Array(items) => items.first().cloned().unwrap_or(Json::Null),
            other => return Err(format!("cannot take the first of {}", json_kind(other))),
        };
        if current.is_null() {
            return Ok(Value::Null);
        }
    }
    if let Some(inference) = map.infer() {
        current = match inference {
            crate::contract::Inference::IpFamily => match &current {
                Json::String(text) => {
                    let address = text.split('/').next().unwrap_or(text);
                    if address.parse::<std::net::Ipv6Addr>().is_ok() {
                        Json::String("inet6".to_owned())
                    } else if address.parse::<std::net::Ipv4Addr>().is_ok() {
                        Json::String("inet".to_owned())
                    } else {
                        return Err(format!("cannot infer an address family from `{text}`"));
                    }
                }
                other => {
                    return Err(format!(
                        "cannot infer an address family from {}",
                        json_kind(other)
                    ));
                }
            },
        };
    }
    if let Some(translations) = map.map()
        && let Some(key) = scalar_text(&current)
        && let Some(translated) = translations.get(&key)
    {
        current = yaml_to_json(translated);
        // A translation to null says the tool's value means "not applicable" — `MainPID=0`,
        // a `static` unit — and null is exactly what that is (spec §10.5).
        if current.is_null() {
            return Ok(Value::Null);
        }
    }
    if let Some(separator) = map.split() {
        let Json::String(text) = &current else {
            return Err(format!("cannot split {}", json_kind(&current)));
        };
        let parts: Vec<Json> = if text.is_empty() {
            Vec::new()
        } else {
            text.split(separator)
                .map(|part| Json::String(part.to_owned()))
                .collect()
        };
        current = Json::Array(parts);
    }
    if let Some(literal) = map.contains() {
        let Json::Array(parts) = &current else {
            return Err(format!(
                "cannot search {} for `{literal}`",
                json_kind(&current)
            ));
        };
        current = Json::Bool(parts.iter().any(|part| part.as_str() == Some(literal)));
    }
    coerce(&current, ty, map.unit())
}

/// Coerces a decoded value into the declared type, or says why it cannot.
fn coerce(raw: &Json, ty: &FieldType, unit: Option<Unit>) -> Result<Value, String> {
    let wrong =
        |expected: &str| format!("must be {expected} but the tool reported {}", describe(raw));
    match ty {
        FieldType::Any => Ok(plain(raw)),
        FieldType::Bool => match raw {
            Json::Bool(value) => Ok(Value::Bool(*value)),
            Json::Number(number) if number.as_i64() == Some(0) => Ok(Value::Bool(false)),
            Json::Number(number) if number.as_i64() == Some(1) => Ok(Value::Bool(true)),
            Json::String(text) => match text.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" | "on" => Ok(Value::Bool(true)),
                "false" | "no" | "0" | "off" => Ok(Value::Bool(false)),
                _ => Err(wrong("bool")),
            },
            _ => Err(wrong("bool")),
        },
        FieldType::Int => match raw {
            Json::Number(number) => number
                .as_i64()
                .map(i128::from)
                .or_else(|| number.as_u64().map(i128::from))
                .map(Value::Int)
                .ok_or_else(|| wrong("int")),
            Json::String(text) => text
                .trim()
                .parse::<i128>()
                .map(Value::Int)
                .map_err(|_| wrong("int")),
            _ => Err(wrong("int")),
        },
        FieldType::Float => match raw {
            Json::Number(number) => number
                .as_f64()
                .map(Value::Float)
                .ok_or_else(|| wrong("float")),
            Json::String(text) => text
                .trim()
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| wrong("float")),
            _ => Err(wrong("float")),
        },
        FieldType::Decimal => match raw {
            Json::Number(number) => ono_value::Decimal::parse(&number.to_string())
                .map(Value::Decimal)
                .map_err(|_| wrong("decimal")),
            Json::String(text) => ono_value::Decimal::parse(text.trim())
                .map(Value::Decimal)
                .map_err(|_| wrong("decimal")),
            _ => Err(wrong("decimal")),
        },
        FieldType::String => match raw {
            Json::String(text) => Ok(Value::string(text)),
            Json::Number(number) => Ok(Value::string(&number.to_string())),
            Json::Bool(value) => Ok(Value::string(if *value { "true" } else { "false" })),
            _ => Err(wrong("string")),
        },
        FieldType::Bytes => match raw {
            Json::String(text) => Ok(Value::Bytes(text.as_bytes().to_vec().into())),
            _ => Err(wrong("bytes")),
        },
        FieldType::Path => match raw {
            Json::String(text) => Ok(Value::Path(Arc::from(Path::new(text)))),
            _ => Err(wrong("path")),
        },
        FieldType::Timestamp => {
            let epoch = |amount: i64| match unit {
                Some(Unit::Microseconds) => jiff::Timestamp::from_microsecond(amount).ok(),
                Some(Unit::Milliseconds) => jiff::Timestamp::from_millisecond(amount).ok(),
                _ => jiff::Timestamp::from_second(amount).ok(),
            };
            match raw {
                Json::String(text) => {
                    let text = text.trim();
                    if unit.is_some() {
                        return text
                            .parse::<i64>()
                            .ok()
                            .and_then(epoch)
                            .map(Value::Timestamp)
                            .ok_or_else(|| wrong("timestamp"));
                    }
                    text.parse::<jiff::Timestamp>()
                        .ok()
                        .or_else(|| systemd_timestamp(text))
                        .map(Value::Timestamp)
                        .ok_or_else(|| wrong("timestamp"))
                }
                Json::Number(number) => number
                    .as_i64()
                    .and_then(epoch)
                    .map(Value::Timestamp)
                    .ok_or_else(|| wrong("timestamp")),
                _ => Err(wrong("timestamp")),
            }
        }
        FieldType::Duration => match raw {
            Json::Number(number) => {
                let amount = number.as_f64().ok_or_else(|| wrong("duration"))?;
                let nanoseconds = match unit {
                    Some(Unit::Milliseconds) => amount * 1_000_000.0,
                    _ => amount * 1_000_000_000.0,
                };
                Ok(Value::Duration(Duration::from_nanoseconds(
                    nanoseconds as i128,
                )))
            }
            Json::String(text) => Duration::parse(text.trim())
                .map(Value::Duration)
                .map_err(|_| wrong("duration")),
            _ => Err(wrong("duration")),
        },
        FieldType::ByteSize => match raw {
            Json::Number(number) => {
                let amount = number
                    .as_u64()
                    .map(u128::from)
                    .ok_or_else(|| wrong("a byte size"))?;
                let factor: u128 = match unit {
                    Some(Unit::Kib) => 1024,
                    Some(Unit::Mib) => 1024 * 1024,
                    _ => 1,
                };
                Ok(Value::ByteSize(ByteSize::from_bytes(amount * factor)))
            }
            Json::String(text) => ByteSize::parse(text.trim())
                .map(Value::ByteSize)
                .map_err(|_| wrong("a byte size")),
            _ => Err(wrong("a byte size")),
        },
        FieldType::Percent => match raw {
            Json::Number(number) => number
                .as_f64()
                .map(|value| Value::Percent(Percent::new(value)))
                .ok_or_else(|| wrong("a percentage")),
            Json::String(text) => Percent::parse(text.trim())
                .map(Value::Percent)
                .map_err(|_| wrong("a percentage")),
            _ => Err(wrong("a percentage")),
        },
        FieldType::Uuid => match raw {
            Json::String(text) => ono_value::Uuid::parse(text.trim())
                .map(Value::Uuid)
                .map_err(|_| wrong("a uuid")),
            _ => Err(wrong("a uuid")),
        },
        FieldType::Ip => match raw {
            Json::String(text) => text
                .trim()
                .parse::<std::net::IpAddr>()
                .map(Value::Ip)
                .map_err(|_| wrong("an ip address")),
            _ => Err(wrong("an ip address")),
        },
        FieldType::IpNetwork => match raw {
            // A bare address is the host network — `ip route` prints a host route as
            // `10.1.0.1`, meaning `10.1.0.1/32` — so the prefix the tool left implicit is
            // written out rather than refused.
            Json::String(text) => {
                let text = text.trim();
                let spelled = match text.parse::<std::net::IpAddr>() {
                    Ok(std::net::IpAddr::V4(_)) => format!("{text}/32"),
                    Ok(std::net::IpAddr::V6(_)) => format!("{text}/128"),
                    Err(_) => text.to_owned(),
                };
                ono_value::IpNetwork::parse(&spelled)
                    .map(Value::IpNetwork)
                    .map_err(|_| wrong("an ip network"))
            }
            _ => Err(wrong("an ip network")),
        },
        FieldType::Port => match raw {
            Json::Number(number) => number
                .as_u64()
                .and_then(|port| u16::try_from(port).ok())
                .map(Value::Port)
                .ok_or_else(|| wrong("a port")),
            Json::String(text) => text
                .trim()
                .parse::<u16>()
                .map(Value::Port)
                .map_err(|_| wrong("a port")),
            _ => Err(wrong("a port")),
        },
        FieldType::Enum(names) => match raw {
            Json::String(text) if names.iter().any(|name| name.as_ref() == text) => {
                Ok(Value::string(text))
            }
            _ => Err(wrong(&format!("one of {}", names.join(", ")))),
        },
        FieldType::List(inner) => match raw {
            Json::Array(items) => items
                .iter()
                .map(|item| coerce(item, inner, unit))
                .collect::<Result<Vec<Value>, String>>()
                .map(Value::list),
            _ => Err(wrong("a list")),
        },
        FieldType::Map => match raw {
            Json::Object(_) => Ok(plain(raw)),
            _ => Err(wrong("a map")),
        },
        // A reference is carried the way the native providers carry one: the name (or the
        // number) that identifies the object, which `trace` and `get` resolve on demand.
        FieldType::Ref(_) => match raw {
            Json::String(text) => Ok(Value::string(text)),
            Json::Number(number) => number
                .as_i64()
                .map(|n| Value::Int(i128::from(n)))
                .ok_or_else(|| wrong("a reference")),
            _ => Err(wrong("a reference")),
        },
        FieldType::Regex | FieldType::Record(_) | FieldType::Error => {
            Err(format!("is declared as {ty}, which no adapter can decode"))
        }
    }
}

/// systemd's `Fri 2026-08-21 12:01:25 UTC`, as `--timestamp=utc` prints it.
///
/// Only the UTC spelling is read: a zone abbreviation is ambiguous, and an instant guessed
/// from one would be a fabricated instant (spec §10.5).
fn systemd_timestamp(text: &str) -> Option<jiff::Timestamp> {
    let civil = text
        .strip_suffix(" UTC")?
        .split_once(' ')
        .map(|(_, rest)| rest)?;
    let datetime = jiff::civil::DateTime::strptime("%Y-%m-%d %H:%M:%S", civil).ok()?;
    datetime
        .to_zoned(jiff::tz::TimeZone::UTC)
        .ok()
        .map(|zoned| zoned.timestamp())
}

/// A JSON value as the plain Ono value it is, with no schema in play.
fn plain(raw: &Json) -> Value {
    match raw {
        Json::Null => Value::Null,
        Json::Bool(value) => Value::Bool(*value),
        Json::Number(number) => number
            .as_i64()
            .map(|n| Value::Int(i128::from(n)))
            .or_else(|| number.as_u64().map(|n| Value::Int(i128::from(n))))
            .or_else(|| number.as_f64().map(Value::Float))
            .unwrap_or(Value::Null),
        Json::String(text) => Value::string(text),
        Json::Array(items) => Value::list(items.iter().map(plain)),
        Json::Object(object) => {
            let mut map = MapValue::new();
            for (key, value) in object {
                map.insert(key.as_str().into(), plain(value));
            }
            Value::Map(Arc::new(map))
        }
    }
}

fn scalar_text(raw: &Json) -> Option<String> {
    match raw {
        Json::String(text) => Some(text.clone()),
        Json::Number(number) => Some(number.to_string()),
        Json::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn yaml_to_json(value: &serde_yaml_ng::Value) -> Json {
    serde_json::to_value(value).unwrap_or(Json::Null)
}

fn json_kind(raw: &Json) -> &'static str {
    match raw {
        Json::Null => "null",
        Json::Bool(_) => "a boolean",
        Json::Number(_) => "a number",
        Json::String(_) => "a string",
        Json::Array(_) => "a list",
        Json::Object(_) => "an object",
    }
}

fn describe(raw: &Json) -> String {
    match raw {
        Json::String(text) => format!("the string `{}`", text.chars().take(64).collect::<String>()),
        other => json_kind(other).to_owned(),
    }
}

fn decode_failed(adapter: &Adapter, trace: &Trace, bytes: &[u8], reason: String) -> ErrorValue {
    with_payload(
        ErrorValue::new(
            ErrorCode::AdapterDecodeFailed,
            format!(
                "adapter {} could not decode the output of {}: {reason}",
                adapter.full_id(),
                trace.executable.display()
            ),
        )
        .with_help(format!(
            "nothing was fabricated. `raw {}` shows the output as it is; check the tool's \
             version against the adapter's range ({})",
            trace.user_invocation.join(" "),
            adapter.executable().versions()
        )),
        adapter,
        trace,
    )
    .with_metadata(
        "raw",
        Value::Bytes(bytes[..bytes.len().min(RAW_KEPT)].to_vec().into()),
    )
}

fn violation(adapter: &Adapter, trace: &Trace, reason: String) -> ErrorValue {
    with_payload(
        ErrorValue::new(
            ErrorCode::AdapterSchemaViolation,
            format!(
                "adapter {} decoded a value outside {}: {reason}",
                adapter.full_id(),
                adapter.schema()
            ),
        )
        .with_help(format!(
            "this is an adapter defect; report it with the tool's version. `raw {}` runs the \
             program as typed meanwhile",
            trace.user_invocation.join(" ")
        )),
        adapter,
        trace,
    )
}

/// The payload every `adapter.*` error carries (spec v0.3 §1.65, ADR-0053).
pub(crate) fn with_payload(error: ErrorValue, adapter: &Adapter, trace: &Trace) -> ErrorValue {
    error
        .with_metadata("adapter", Value::string(&adapter.full_id()))
        .with_metadata("adapter_version", Value::string(adapter.pack_version()))
        .with_metadata(
            "executable",
            Value::string(&trace.executable.display().to_string()),
        )
        .with_metadata(
            "executable_version",
            trace
                .version
                .as_ref()
                .map_or(Value::Null, |version| Value::string(&version.to_string())),
        )
        .with_metadata(
            "invocation",
            Value::string(&trace.user_invocation.join(" ")),
        )
        .with_metadata("raw_fallback_safe", Value::Bool(true))
        .with_metadata(
            "recovery",
            Value::string(&format!("raw {}", trace.user_invocation.join(" "))),
        )
}

/// The exactness the contract records for a field, as text.
#[must_use]
pub fn exactness_name(exactness: Exactness) -> &'static str {
    match exactness {
        Exactness::Exact => "exact",
        Exactness::Normalized => "normalized",
        Exactness::Inferred => "inferred",
    }
}

/// Adapter → schema field → declared type, for callers that render support claims.
#[must_use]
pub fn field_types(adapter: &Adapter, schemas: &SchemaRegistry) -> BTreeMap<String, String> {
    adapter
        .schema()
        .parse::<SchemaId>()
        .ok()
        .and_then(|id| schemas.get(&id))
        .map(|schema| {
            schema
                .fields()
                .iter()
                .map(|field| (field.name().to_owned(), field.ty().to_string()))
                .collect()
        })
        .unwrap_or_default()
}
