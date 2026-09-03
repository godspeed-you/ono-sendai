//! The messages the supervisor and a plugin exchange (`docs/spec/kuang/protocol.v1.yaml`).
//!
//! Everything on the wire is one of three envelopes: the plugin's opening [`Hello`], a
//! [`Envelope::Request`] carrying a call id from the protocol contract, or a
//! [`Envelope::Response`] answering one. Values cross as the tagged JSON encoding of
//! `ono-value` — the lossless codec of `ono_value::to_json`/`from_json` — so a `ByteSize` does
//! not arrive as a bare number with its unit gone (protocol invariant `typed-units`; the binary
//! encoding of spec §31.61 is a later performance increment, ADR-0040).
//!
//! Flow control is pull-based in both directions (ADR-0022 §8): a plugin emits only against
//! credit the host granted, and the host grants credit only as its consumer takes values.

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as Json};

use crate::{KuangError, KuangErrorCode, PluginContract, WireError};

/// The call ids of `docs/spec/kuang/protocol.v1.yaml`, as they appear in `Request::method`.
pub mod method {
    /// Host → plugin: deliver the negotiated contract (spec §31.63).
    pub const LIFECYCLE_INIT: &str = "lifecycle.init";
    /// Host → plugin: drain and stop within the deadline.
    pub const LIFECYCLE_SHUTDOWN: &str = "lifecycle.shutdown";
    /// Host → plugin: run a contributed command (spec §31.22, §31.29).
    pub const COMMAND_INVOKE: &str = "command.invoke";
    /// Host → plugin: answer a query against a contributed target (spec §31.23; ADR-0040 —
    /// `protocol.v1.yaml` omitted the call, the provider conformance test forced it).
    pub const PROVIDER_QUERY: &str = "provider.query";
    /// Host → plugin: grant more credit on one of the plugin's output streams.
    pub const STREAM_DEMAND: &str = "stream.demand";
    /// Host → plugin: a stream was cancelled. Delivered, not inferred (spec §31.14).
    pub const STREAM_CANCEL: &str = "stream.cancel";
    /// Host → plugin: health check (spec §31.35).
    pub const HEALTH_PROBE: &str = "health.probe";

    /// Plugin → host: emit values against granted credit.
    pub const STREAMS_EMIT: &str = "streams.emit";
    /// Plugin → host: close an output stream, normally or with a terminal error.
    pub const STREAMS_CLOSE: &str = "streams.close";
    /// Plugin → host: check a grant without prompting (spec §31.61).
    pub const CAPABILITIES_CHECK: &str = "capabilities.check";
    /// Plugin → host: runtime capability request, against an explicit user action (spec §31.17).
    pub const CAPABILITIES_REQUEST: &str = "capabilities.request";
    /// Plugin → host: structured log record (spec §31.33).
    pub const AUDIT_LOG: &str = "audit.log";
    /// Plugin → host: add a security-relevant event to the audit trail (spec §31.37).
    pub const AUDIT_EVENT: &str = "audit.event";
    /// Plugin → host: read a value from the package's own store (spec §31.31).
    pub const STATE_GET: &str = "state.get";
    /// Plugin → host: write a value into the package's own store.
    pub const STATE_SET: &str = "state.set";
    /// Plugin → host: remove a key from the package's own store.
    pub const STATE_DELETE: &str = "state.delete";
    /// Plugin → host: wall-clock time. Virtual under the test host (spec §31.73).
    pub const CLOCK_NOW: &str = "clock.now";
    /// Plugin → host: read file bytes under the granted `paths` scope.
    pub const FILESYSTEM_READ: &str = "filesystem.read";
    /// Plugin → host: pull values from a stream the host produces (spec §31.15's credit).
    pub const STREAMS_NEXT: &str = "streams.next";
    /// Plugin → host: cancel a stream in either direction.
    pub const STREAMS_CANCEL: &str = "streams.cancel";
    /// Plugin → host: the context stack, and nothing beyond it (spec §31.12).
    pub const CONTEXT_GET: &str = "context.get";
    /// Plugin → host: one registered schema (spec §31.12, §31.64).
    pub const SCHEMAS_GET: &str = "schemas.get";
    /// Plugin → host: the registered schemas, as a stream (spec §31.12, §31.64).
    pub const SCHEMAS_LIST: &str = "schemas.list";
    /// Plugin → host: the model providers this package may use (spec §31.43).
    pub const MODELS_LIST: &str = "models.list";
    /// Plugin → host: operator-approved inference through the model broker (spec §31.43).
    pub const MODELS_INFER: &str = "models.infer";
}

/// One frame's payload: the opening hello, a call, or an answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Envelope {
    /// The plugin's first frame, and only the plugin's (spec §31.63).
    Hello(Hello),
    /// A call in either direction.
    Request {
        /// The caller's sequence number, unique per direction.
        seq: u64,
        /// A call id from [`method`].
        method: String,
        /// The call's parameters, per the protocol contract.
        params: Json,
    },
    /// The answer to a request, carrying exactly one of `result` and `error`.
    Response {
        /// The sequence number of the request being answered.
        seq: u64,
        /// The successful result.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Json>,
        /// The structured failure.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<WireError>,
    },
}

/// The plugin's opening frame: who it is and what it brings (spec §31.63).
///
/// The host has already read and validated the package manifest before spawning anything —
/// manifest before code, spec §31.89 rule 1 — so the hello carries the identity to cross-check
/// and the contribution documents to validate and register, not authority.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    /// The package format the plugin was built against. Must be `kuang-package/1`.
    pub format: String,
    /// The package id. Must match the manifest the host validated.
    pub package: String,
    /// The package version. Must match the manifest.
    pub version: String,
    /// The host API range the plugin speaks, e.g. `>=11.1 <12`.
    pub kuang_api: String,
    /// The contribution documents, validated before registration (spec §31.22).
    #[serde(default)]
    pub contributions: ContributionSet,
}

/// The contribution documents a plugin surfaces at handshake.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ContributionSet {
    /// Contributed commands (spec §31.22).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandContribution>,
    /// Contributed targets (spec §31.23).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<TargetContribution>,
    /// Contributed schemas (spec §31.23).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schemas: Vec<SchemaContribution>,
}

/// A contributed command, in the same metadata shape core commands use
/// (`docs/spec/kuang/contributions.v1.yaml`). `provider` and origin are the host's to set at
/// registration; they are deliberately not wire fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandContribution {
    /// `<package.id>.command.<kebab-name>`.
    pub id: String,
    /// An existing verb wherever the semantics allow it (spec §31.22).
    pub verb: String,
    /// A core target or one this package contributes.
    pub target: String,
    /// One line, for `help` and completion.
    pub summary: String,
    /// The input type, e.g. `stream<ono.socket/1>`. `None` for a command taking no input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// The output type, e.g. `stream<dev.example.echo.item/1>`. Validated on every value.
    pub output: String,
    /// The KUANG/11 capabilities the command needs, checked at invocation.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The argument mode from ADR-0009's table.
    pub argument_mode: String,
    /// The risk level, required for a mutating command (spec §31.75).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    /// Documented examples. Each must parse and run under the test host (spec §31.22, §50).
    #[serde(default)]
    pub examples: Vec<String>,
}

/// The document a `contributions.commands` path names (spec §31.22, §31.68).
///
/// A package declares its commands twice over, in the same shape: once in a document beside its
/// manifest, so the host can register a registry placeholder without starting anything, and once
/// across the handshake, when the instance actually loads. One shape means the two cannot
/// disagree about what the package contributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandDocument {
    /// The commands the document declares.
    pub commands: Vec<CommandContribution>,
}

impl CommandDocument {
    /// Reads a declaration document.
    ///
    /// # Errors
    ///
    /// `package.invalid` when the document is not the shape
    /// `docs/spec/kuang/contributions.v1.yaml` describes.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        serde_yaml_ng::from_str(text).map_err(|error| {
            KuangError::new(
                KuangErrorCode::PackageInvalid,
                format!("a contributed command document does not read: {error}"),
            )
            .with_help(
                "the document is a `commands:` list of the contribution shape of \
                 `docs/spec/kuang/contributions.v1.yaml`",
            )
        })
    }
}

/// A contributed target (spec §31.23).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetContribution {
    /// The target word, e.g. `echo-item`.
    pub name: String,
    /// The schema id objects of this target carry.
    pub schema: String,
    /// One line, for `help` and completion.
    pub summary: String,
    /// What makes two observations the same object, in prose.
    pub identity_doc: String,
}

/// A contributed schema, in the field vocabulary of `docs/spec/schemas/*.v1.yaml`
/// (spec §31.23: contributed schemas are written in the same language core schemas are).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaContribution {
    /// `<package.id>.<kebab-name>/<major>`.
    pub id: String,
    /// The schema's display name.
    pub name: String,
    /// One line, what an object of this schema is.
    pub summary: String,
    /// The identity fields (spec §27.3).
    pub identity: Vec<String>,
    /// The fields, in declaration order.
    pub fields: Vec<SchemaFieldContribution>,
}

/// One field of a contributed schema. Exactly one of `required` and `nullable` is true
/// (ADR-0012 §8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaFieldContribution {
    /// The field name.
    pub name: String,
    /// The type name as the registries spell it, e.g. `int`, `string`, `list<string>`.
    #[serde(rename = "type")]
    pub field_type: String,
    /// The manifest is invalid without this field.
    #[serde(default)]
    pub required: bool,
    /// May be absent or null; absent means unknown, never a default (spec §10.5).
    #[serde(default)]
    pub nullable: bool,
}

// --- typed parameters and results, per protocol.v1.yaml ---------------------------------------

/// Parameters of [`method::LIFECYCLE_INIT`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitParams {
    /// The negotiated contract of spec §31.63.
    pub contract: PluginContract,
}

/// The plugin's answer to [`method::LIFECYCLE_INIT`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitResult {
    /// Whether the plugin is ready to serve.
    pub ready: bool,
    /// The features the plugin switched off because of denied optional capabilities —
    /// its own account of what it gave up (spec §31.63).
    #[serde(default)]
    pub disabled_features: Vec<String>,
    /// Why the plugin cannot serve, when `ready` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

/// Why the host is shutting the instance down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShutdownReason {
    /// `unload plugin`.
    Unload,
    /// A new version is taking over (spec §31.35).
    Upgrade,
    /// A capability the plugin holds was withdrawn.
    Revocation,
    /// Policy ended the instance.
    Policy,
    /// The instance was idle past its budget.
    Idle,
    /// The host itself is exiting.
    HostExit,
}

/// Parameters of [`method::LIFECYCLE_SHUTDOWN`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShutdownParams {
    /// Why.
    pub reason: ShutdownReason,
    /// How long the plugin has to drain, in milliseconds. After it, the instance is terminated.
    pub deadline_ms: u64,
}

/// Parameters of [`method::COMMAND_INVOKE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvokeParams {
    /// The contributed command id to run.
    pub command: String,
    /// Selectors and options by name, already bound and typed by the host's command layer.
    pub arguments: JsonMap<String, Json>,
    /// The output stream handle the plugin emits into.
    pub output: u64,
    /// The invocation every handle the plugin opens will belong to.
    pub invocation: u64,
    /// The initial emission credit on `output` (the pull protocol's opening window).
    pub credit: u32,
}

/// Parameters of [`method::PROVIDER_QUERY`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryParams {
    /// The contributed target being queried.
    pub target: String,
    /// Provider options by name.
    #[serde(default)]
    pub options: JsonMap<String, Json>,
    /// The output stream handle the plugin answers into.
    pub output: u64,
    /// The owning invocation.
    pub invocation: u64,
    /// The initial emission credit on `output`.
    pub credit: u32,
}

/// How an invocation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InvokeStatus {
    /// The invocation ran to completion.
    Completed,
    /// The invocation failed; `error` says how.
    Failed,
    /// The invocation observed cancellation and stopped.
    Cancelled,
}

/// The plugin's answer to [`method::COMMAND_INVOKE`] and [`method::PROVIDER_QUERY`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvokeResult {
    /// How it ended.
    pub status: InvokeStatus,
    /// The failure, for `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

/// Parameters of [`method::STREAM_DEMAND`]: credit is cumulative, never a rate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DemandParams {
    /// The plugin's output stream.
    pub handle: u64,
    /// How many more values the host will accept.
    pub credit: u32,
}

/// Why a stream was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CancelReason {
    /// The consumer went away.
    ConsumerGone,
    /// The invocation's deadline passed.
    Deadline,
    /// Policy ended the stream.
    Policy,
    /// A capability the stream depended on was revoked.
    Revocation,
    /// The operator cancelled.
    Operator,
}

/// Parameters of [`method::STREAM_CANCEL`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CancelParams {
    /// The stream being cancelled.
    pub handle: u64,
    /// Why.
    pub reason: CancelReason,
}

/// The plugin's answer to [`method::HEALTH_PROBE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
    /// The instance's own judgement of itself.
    pub state: HealthState,
    /// Optional detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A health answer (spec §31.35).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HealthState {
    /// Ready to serve.
    Ready,
    /// Serving, but at capacity.
    Busy,
    /// Running with disabled features.
    Degraded,
}

/// Parameters of [`method::STREAMS_EMIT`]: at most the credit the host last granted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmitParams {
    /// The output stream.
    pub handle: u64,
    /// The values, in the tagged JSON encoding of `ono-value`.
    pub values: Vec<Json>,
}

/// The host's answer to [`method::STREAMS_EMIT`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmitResult {
    /// How many more values the host will now accept on this stream.
    pub credit: u32,
}

/// Parameters of [`method::STREAMS_CLOSE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloseParams {
    /// The output stream.
    pub handle: u64,
    /// The terminal error, when the producer failed. `None` is a normal completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

/// Parameters of [`method::CAPABILITIES_CHECK`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckParams {
    /// The capability id.
    pub capability: String,
    /// The scope the plugin would use it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<JsonMap<String, Json>>,
}

/// The host's answer to [`method::CAPABILITIES_CHECK`]. Asking never prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckAnswer {
    /// The call would proceed.
    Granted,
    /// The call would be refused.
    Denied,
    /// A request would prompt the operator. Not a grant.
    Ask,
    /// The host cannot say.
    Unknown,
}

/// Parameters of [`method::CAPABILITIES_REQUEST`] (spec §31.17's runtime-requested class).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestOnceParams {
    /// The capability id.
    pub capability: String,
    /// The requested scope — no broader than the declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<JsonMap<String, Json>>,
    /// Why, in the package's words. Sanitised before display.
    pub purpose: String,
    /// The explicit user action the request answers. A request with none is denied without
    /// prompting.
    pub action_context: String,
}

/// Parameters of [`method::AUDIT_LOG`] (spec §31.33: structured records, never stderr).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditLogParams {
    /// The severity, e.g. `info`, `warn`, `error`.
    pub level: String,
    /// The message.
    pub message: String,
    /// Structured fields.
    #[serde(default)]
    pub fields: JsonMap<String, Json>,
}

/// Parameters of [`method::STATE_GET`] / [`method::STATE_DELETE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateKeyParams {
    /// A key in the package's own store.
    pub key: String,
    /// Which store (spec §31.31).
    pub class: String,
}

/// The host's answer to [`method::STATE_GET`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateGetResult {
    /// The stored value in tagged encoding, or `None` when the key is unset — which is not an
    /// empty value (spec §10.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Json>,
}

/// Parameters of [`method::STATE_SET`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateSetParams {
    /// A key in the package's own store.
    pub key: String,
    /// Which store.
    pub class: String,
    /// The value in tagged encoding.
    pub value: Json,
}

/// Parameters of [`method::FILESYSTEM_READ`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesystemReadParams {
    /// The path, which must resolve inside the granted `paths` scope.
    pub path: String,
    /// Byte offset. `None` starts at the beginning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// Bytes to read. `None` reads to the host's per-call ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
}

/// The host's answer to [`method::CLOCK_NOW`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClockNowResult {
    /// The instant, in tagged encoding (`{"$timestamp": …}`). Virtual under the test host.
    pub now: Json,
}

/// The host's answer to [`method::FILESYSTEM_READ`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesystemReadResult {
    /// The bytes read, in tagged encoding (`{"$bytes": …}`). Never interpreted.
    pub content: Json,
}

impl SchemaContribution {
    /// Converts the contribution into the `ono-value` schema both sides then build and
    /// validate records against (spec §31.23: contributed schemas are written in the same
    /// language core schemas are).
    ///
    /// # Errors
    ///
    /// Returns `package.invalid` for a field that is neither required nor nullable
    /// (ADR-0012 §8), an unknown type name, or a shape `ono-value` refuses.
    pub fn to_schema(&self) -> Result<ono_value::Schema, crate::KuangError> {
        use crate::{KuangError, KuangErrorCode};
        let invalid = |detail: String| KuangError::new(KuangErrorCode::PackageInvalid, detail);
        let id: ono_value::SchemaId = self
            .id
            .parse()
            .map_err(|_| invalid(format!("`{}` is not a schema id", self.id)))?;
        let mut builder = ono_value::Schema::builder(id, &self.name).doc(&self.summary);
        for field in &self.fields {
            if field.required == field.nullable {
                return Err(invalid(format!(
                    "field `{}` must be exactly one of required and nullable (ADR-0012 §8)",
                    field.name
                )));
            }
            let field_type = parse_type_name(&field.field_type).ok_or_else(|| {
                invalid(format!(
                    "field `{}` has unknown type `{}`",
                    field.name, field.field_type
                ))
            })?;
            let mut def = ono_value::FieldDef::new(&field.name, field_type);
            def = if field.required {
                def.required()
            } else {
                def.nullable()
            };
            builder = builder.field(def);
        }
        builder = builder.identity(self.identity.iter().map(String::as_str));
        builder
            .build()
            .map_err(|error| invalid(format!("schema `{}` is invalid: {error}", self.id)))
    }
}

/// Parses a registry type name (`int`, `list<string>`, `enum<a|b>`, `record<x/1>`, …) into the
/// `ono-value` type it names, or `None` for a name the vocabulary does not carry.
#[must_use]
pub fn parse_type_name(name: &str) -> Option<ono_value::FieldType> {
    use ono_value::FieldType;
    let name = name.trim();
    if let Some(inner) = name
        .strip_prefix("list<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return Some(FieldType::list(parse_type_name(inner)?));
    }
    if let Some(inner) = name
        .strip_prefix("enum<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        let variants: Vec<&str> = inner.split('|').collect();
        return Some(FieldType::enumeration(&variants));
    }
    if let Some(inner) = name
        .strip_prefix("record<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return Some(FieldType::Record(inner.parse().ok()?));
    }
    if let Some(inner) = name
        .strip_prefix("ref<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return Some(FieldType::Ref(inner.parse().ok()?));
    }
    Some(match name {
        "any" => FieldType::Any,
        "bool" => FieldType::Bool,
        "int" => FieldType::Int,
        "float" => FieldType::Float,
        "decimal" => FieldType::Decimal,
        "string" => FieldType::String,
        "bytes" => FieldType::Bytes,
        "path" => FieldType::Path,
        "timestamp" => FieldType::Timestamp,
        "duration" => FieldType::Duration,
        "bytesize" => FieldType::ByteSize,
        "percent" => FieldType::Percent,
        "regex" => FieldType::Regex,
        "uuid" => FieldType::Uuid,
        "ip" => FieldType::Ip,
        "ipnetwork" => FieldType::IpNetwork,
        "port" => FieldType::Port,
        "map" => FieldType::Map,
        "error" => FieldType::Error,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_round_trip_an_envelope_when_serialised() {
        let envelope = Envelope::Request {
            seq: 7,
            method: method::STREAMS_EMIT.to_owned(),
            params: serde_json::json!({"handle": 1, "values": [3]}),
        };
        let text = serde_json::to_string(&envelope).expect("serialises");
        let back: Envelope = serde_json::from_str(&text).expect("parses");
        assert_eq!(back, envelope);
    }

    #[test]
    fn should_keep_result_and_error_distinct_when_a_response_crosses() {
        let text = r#"{"kind":"response","seq":3,"error":{"code":"Ono-Sendai-K11301","name":"capability.denied","message":"no grant"}}"#;
        let envelope: Envelope = serde_json::from_str(text).expect("parses");
        let Envelope::Response { result, error, .. } = envelope else {
            panic!("expected a response");
        };
        assert!(result.is_none());
        assert_eq!(error.expect("error").name, "capability.denied");
    }
}

/// Parameters of [`method::STREAMS_NEXT`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NextParams {
    /// The stream to read.
    pub handle: u64,
    /// How many values the plugin is ready for: the credit. The host sends no more than this.
    pub max: u64,
    /// How long to wait for the first value. Absent uses the invocation's deadline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<serde_json::Value>,
}

/// Parameters of [`method::STREAMS_CANCEL`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamHandleParams {
    /// Any stream in either direction.
    pub handle: u64,
}

/// The answer to [`method::STREAMS_NEXT`]: `complete` with no error is a normal end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NextResult {
    /// The values, at most `max` of them, in order.
    pub values: Vec<serde_json::Value>,
    /// Whether the stream has nothing more to give.
    pub complete: bool,
    /// The terminal failure, when the stream ended in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

/// Parameters of [`method::SCHEMAS_GET`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaGetParams {
    /// A schema id, e.g. `ono.process/1`.
    pub id: String,
}

/// Parameters of [`method::SCHEMAS_LIST`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SchemaListParams {
    /// Restrict to ids under a namespace. Absent lists every registered schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}
