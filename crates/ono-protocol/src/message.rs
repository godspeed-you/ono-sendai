//! The messages a link exchanges (spec §21.2, §21.4), and how they are written down.
//!
//! # Why the payload is the value model's own JSON
//!
//! Spec §21 exists so that "native operations execute over there" rather than text coming back:
//! a remote `Process` must arrive as the same record a local one is, schema, units, nulls and
//! provenance included. `ono-value` already has an encoding with exactly that property — the
//! tagged JSON of ADR-0016 item 6, where every semantic scalar is a single-key tagged object so
//! nothing is flattened into a bare number whose unit is gone. Inventing a second encoding for
//! the same values would mean two things to keep in step, two fuzz targets, and two chances to
//! disagree about what `null` means.
//!
//! So a payload is a JSON document, and every [`Value`] inside it goes through
//! [`ono_value::to_json`] and [`ono_value::from_json`]. The envelope around it — which stream,
//! which kind — is the binary frame of [`crate::frame`], because routing must be possible without
//! decoding, and because a length that is read before it is checked is how a protocol bomb works.
//!
//! The handshake is the one part that carries no [`Value`], so it is plain `serde` structures.
//!
//! # Decoding is bounded before it is done
//!
//! Every payload is parsed into a `serde_json::Value` first, checked against
//! [`Limits::max_value_depth`], and only then turned into an Ono value. The check is iterative
//! and happens before the recursive conversion, so a deeply nested document is refused rather
//! than descended into (ADR-0015 T7).

use std::sync::Arc;

use ono_provider_api::{ActionOutcome, EventKind, ObjectEvent, ObjectId, Query, Selector};
use ono_value::{
    ActionStatus, ErrorValue, RecordValue, SchemaId, SchemaRegistry, Value, from_json, to_json,
};
use serde_json::{Map, Value as Json};

use crate::handshake::{Accept, Hello, Reject};
use crate::{FrameKind, Limits, ProtocolError};

/// A request for objects, as it travels between two machines.
///
/// This mirrors [`ono_provider_api::Query`], which is what a provider is actually asked. It
/// exists as its own type for one reason: `Query` does not expose the provider options it holds
/// for enumeration, and a request that silently dropped `--recursive` on the way across a link
/// would be worse than one that could not carry it at all.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RemoteQuery {
    target: String,
    selectors: Vec<Selector>,
    options: Vec<(String, Value)>,
    limit: Option<usize>,
}

impl RemoteQuery {
    /// A request for every object of `target`.
    #[must_use]
    pub fn target(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            ..Self::default()
        }
    }

    /// Narrows the request.
    #[must_use]
    pub fn with(mut self, selector: Selector) -> Self {
        self.selectors.push(selector);
        self
    }

    /// Sets a provider option, such as `--recursive`.
    #[must_use]
    pub fn option(mut self, name: impl Into<String>, value: Value) -> Self {
        self.options.push((name.into(), value));
        self
    }

    /// Asks for at most `limit` objects.
    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// The request a local [`Query`] describes.
    ///
    /// The target, the selectors and the limit carry over. Options do not, because `Query` has no
    /// way to list them; add them with [`option`](Self::option).
    #[must_use]
    pub fn from_query(query: &Query) -> Self {
        Self {
            target: query.target_name().to_owned(),
            selectors: query.selectors().to_vec(),
            options: Vec::new(),
            limit: query.max(),
        }
    }

    /// The request as a [`Query`], to hand to a provider on the remote side.
    #[must_use]
    pub fn to_query(&self) -> Query {
        let mut query = Query::target(&self.target);
        for selector in &self.selectors {
            query = query.with(selector.clone());
        }
        for (name, value) in &self.options {
            query = query.option(name, value.clone());
        }
        if let Some(limit) = self.limit {
            query = query.limit(limit);
        }
        query
    }

    /// The target being asked for.
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target
    }

    /// The selectors narrowing the request.
    #[must_use]
    pub fn selectors(&self) -> &[Selector] {
        &self.selectors
    }

    /// Every provider option, in the order they were set.
    #[must_use]
    pub fn options(&self) -> &[(String, Value)] {
        &self.options
    }

    /// An option's value, if it was given.
    #[must_use]
    pub fn option_value(&self, name: &str) -> Option<&Value> {
        self.options
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value)
    }

    /// The maximum number of objects wanted, if one was given.
    #[must_use]
    pub const fn max(&self) -> Option<usize> {
        self.limit
    }
}

/// A request to change one object on the remote machine.
///
/// The counterpart of [`RemoteQuery`], and it exists for the same reason:
/// [`ono_provider_api::Action`] does not expose its arguments for enumeration.
#[derive(Debug, Clone, PartialEq)]
pub struct ActRequest {
    target: String,
    operation: String,
    object: ObjectId,
    arguments: Vec<(String, Value)>,
    dry_run: bool,
}

impl ActRequest {
    /// An action on one object.
    #[must_use]
    pub fn new(target: impl Into<String>, operation: impl Into<String>, object: ObjectId) -> Self {
        Self {
            target: target.into(),
            operation: operation.into(),
            object,
            arguments: Vec::new(),
            dry_run: false,
        }
    }

    /// Adds an argument, such as the signal a `stop process` should send.
    #[must_use]
    pub fn with_argument(mut self, name: impl Into<String>, value: Value) -> Self {
        self.arguments.push((name.into(), value));
        self
    }

    /// Asks the remote to report what it *would* do without doing it (spec §11.6).
    #[must_use]
    pub fn as_dry_run(mut self) -> Self {
        self.dry_run = true;
        self
    }

    /// The request as an [`Action`](ono_provider_api::Action), to hand to a remote provider.
    #[must_use]
    pub fn to_action(&self) -> ono_provider_api::Action {
        let mut action = ono_provider_api::Action::new(
            self.target.clone(),
            self.operation.clone(),
            self.object.clone(),
        );
        for (name, value) in &self.arguments {
            action = action.with(name, value.clone());
        }
        if self.dry_run {
            action = action.as_dry_run();
        }
        action
    }

    /// The target family the action belongs to.
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target
    }

    /// What is being asked for.
    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// Which object it is being asked of.
    #[must_use]
    pub const fn object(&self) -> &ObjectId {
        &self.object
    }

    /// Every argument, in the order they were set.
    #[must_use]
    pub fn arguments(&self) -> &[(String, Value)] {
        &self.arguments
    }

    /// An argument's value, if it was given.
    #[must_use]
    pub fn argument(&self, name: &str) -> Option<&Value> {
        self.arguments
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value)
    }

    /// Whether the remote was asked to report rather than to act.
    #[must_use]
    pub const fn is_dry_run(&self) -> bool {
        self.dry_run
    }
}

/// Everything a link can say.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Message {
    /// The opening offer of the handshake (spec §21.2).
    Hello(Hello),
    /// The answer that establishes a link.
    Accept(Accept),
    /// The answer that refuses one.
    Reject(Reject),
    /// Open a stream of the objects a request matches.
    StartQuery(RemoteQuery),
    /// Open a stream of the changes a request matches.
    StartSubscribe(RemoteQuery),
    /// Perform an action and answer with one outcome.
    Act(ActRequest),
    /// Stop a stream.
    Cancel,
    /// Grant the peer permission to send this many more messages on a stream.
    Credit(u32),
    /// One value a stream produced.
    Value(Value),
    /// One object event a subscription produced.
    Event(ObjectEvent),
    /// A failure concerning one item, leaving the stream running (spec §16.5).
    Failure(ErrorValue),
    /// The outcome of an action.
    Outcome(ActionOutcome),
    /// The stream produced everything it is going to.
    End,
}

impl Message {
    /// The frame kind that carries this message.
    #[must_use]
    pub const fn kind(&self) -> FrameKind {
        match self {
            Message::Hello(_) => FrameKind::Hello,
            Message::Accept(_) => FrameKind::Accept,
            Message::Reject(_) => FrameKind::Reject,
            Message::StartQuery(_) => FrameKind::StartQuery,
            Message::StartSubscribe(_) => FrameKind::StartSubscribe,
            Message::Act(_) => FrameKind::Act,
            Message::Cancel => FrameKind::Cancel,
            Message::Credit(_) => FrameKind::Credit,
            Message::Value(_) => FrameKind::Value,
            Message::Event(_) => FrameKind::Event,
            Message::Failure(_) => FrameKind::Failure,
            Message::Outcome(_) => FrameKind::Outcome,
            Message::End => FrameKind::End,
        }
    }
}

/// Encodes a message into the payload of a frame of its [`Message::kind`].
///
/// # Errors
///
/// Returns [`ProtocolError::FrameTooLarge`] when the encoded message would not fit in a frame,
/// and [`ProtocolError::MalformedPayload`] when the message cannot be serialized at all — which
/// no message this crate builds can cause.
///
/// ```
/// use ono_protocol::{Limits, Message, decode_message, encode_message};
/// use ono_value::{SchemaRegistry, Value};
///
/// let message = Message::Value(Value::Int(7));
/// let payload = encode_message(&message, &Limits::default())?;
/// let decoded = decode_message(message.kind(), &payload, &SchemaRegistry::new(), &Limits::default())?;
/// assert_eq!(decoded, message);
/// # Ok::<(), ono_protocol::ProtocolError>(())
/// ```
pub fn encode_message(message: &Message, limits: &Limits) -> Result<Vec<u8>, ProtocolError> {
    let kind = message.kind();
    let json = match message {
        Message::Hello(hello) => to_serde(kind, hello)?,
        Message::Accept(accept) => to_serde(kind, accept)?,
        Message::Reject(reject) => to_serde(kind, reject)?,
        Message::StartQuery(query) | Message::StartSubscribe(query) => query_to_json(query),
        Message::Act(request) => act_to_json(request),
        Message::Cancel | Message::End => Json::Null,
        Message::Credit(credit) => Json::from(*credit),
        Message::Value(value) => to_json(value),
        Message::Event(event) => event_to_json(event),
        Message::Failure(error) => to_json(&Value::Error(Arc::new(error.clone()))),
        Message::Outcome(outcome) => outcome_to_json(outcome),
    };
    let bytes = serde_json::to_vec(&json).map_err(|error| ProtocolError::MalformedPayload {
        kind,
        detail: error.to_string(),
    })?;
    if bytes.len() > limits.max_frame_payload() {
        return Err(ProtocolError::FrameTooLarge {
            claimed: bytes.len(),
            limit: limits.max_frame_payload(),
        });
    }
    Ok(bytes)
}

/// Decodes the payload of a frame of `kind`, resolving record schemas through `schemas`.
///
/// # Errors
///
/// Returns [`ProtocolError::MalformedPayload`] when the payload is not the document its kind
/// promises, and [`ProtocolError::ValueTooDeep`] when it nests deeper than `limits` allows. It
/// never panics and never allocates in proportion to anything but the bytes it was given.
pub fn decode_message(
    kind: FrameKind,
    payload: &[u8],
    schemas: &SchemaRegistry,
    limits: &Limits,
) -> Result<Message, ProtocolError> {
    // Parsing first and converting second is what makes the depth bound effective: serde_json
    // refuses absurd nesting itself, and the check below refuses everything past our own limit
    // before a single recursive conversion runs.
    let json: Json = serde_json::from_slice(payload).map_err(|error| bad(kind, error))?;
    check_depth(&json, limits.max_value_depth())?;
    match kind {
        FrameKind::Hello => from_serde(kind, json).map(Message::Hello),
        FrameKind::Accept => from_serde(kind, json).map(Message::Accept),
        FrameKind::Reject => from_serde(kind, json).map(Message::Reject),
        FrameKind::StartQuery => query_from_json(kind, &json, schemas).map(Message::StartQuery),
        FrameKind::StartSubscribe => {
            query_from_json(kind, &json, schemas).map(Message::StartSubscribe)
        }
        FrameKind::Act => act_from_json(kind, &json, schemas).map(Message::Act),
        FrameKind::Cancel => Ok(Message::Cancel),
        FrameKind::End => Ok(Message::End),
        FrameKind::Credit => json
            .as_u64()
            .and_then(|credit| u32::try_from(credit).ok())
            .map(Message::Credit)
            .ok_or_else(|| bad(kind, "a credit grant is a number of messages")),
        FrameKind::Value => from_json(&json, schemas)
            .map(Message::Value)
            .map_err(|error| bad(kind, error.render_terse())),
        FrameKind::Event => event_from_json(kind, &json, schemas).map(Message::Event),
        FrameKind::Failure => match from_json(&json, schemas) {
            Ok(Value::Error(error)) => Ok(Message::Failure(error.as_ref().clone())),
            Ok(_) => Err(bad(kind, "a failure carries a structured error")),
            Err(error) => Err(bad(kind, error.render_terse())),
        },
        FrameKind::Outcome => outcome_from_json(kind, &json, schemas).map(Message::Outcome),
    }
}

/// Refuses a document that nests deeper than `limit`, without recursing into it.
fn check_depth(json: &Json, limit: usize) -> Result<(), ProtocolError> {
    let mut pending = vec![(json, 1usize)];
    while let Some((node, depth)) = pending.pop() {
        if depth > limit {
            return Err(ProtocolError::ValueTooDeep { depth, limit });
        }
        match node {
            Json::Array(items) => pending.extend(items.iter().map(|item| (item, depth + 1))),
            Json::Object(fields) => {
                pending.extend(fields.iter().map(|(_, value)| (value, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}

fn to_serde<T: serde::Serialize>(kind: FrameKind, value: &T) -> Result<Json, ProtocolError> {
    serde_json::to_value(value).map_err(|error| bad(kind, error))
}

fn from_serde<T: serde::de::DeserializeOwned>(
    kind: FrameKind,
    json: Json,
) -> Result<T, ProtocolError> {
    serde_json::from_value(json).map_err(|error| bad(kind, error))
}

fn bad(kind: FrameKind, detail: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::MalformedPayload {
        kind,
        detail: detail.to_string(),
    }
}

fn object(pairs: impl IntoIterator<Item = (&'static str, Json)>) -> Json {
    Json::Object(
        pairs
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<Map<String, Json>>(),
    )
}

fn pairs_to_json(pairs: &[(String, Value)]) -> Json {
    Json::Array(
        pairs
            .iter()
            .map(|(name, value)| Json::Array(vec![Json::String(name.clone()), to_json(value)]))
            .collect(),
    )
}

fn pairs_from_json(
    kind: FrameKind,
    json: Option<&Json>,
    schemas: &SchemaRegistry,
) -> Result<Vec<(String, Value)>, ProtocolError> {
    let Some(Json::Array(items)) = json else {
        return Ok(Vec::new());
    };
    let mut pairs = Vec::with_capacity(items.len());
    for item in items {
        let Some([Json::String(name), value]) = item.as_array().map(Vec::as_slice) else {
            return Err(bad(kind, "a named value is a two-element array"));
        };
        pairs.push((
            name.clone(),
            from_json(value, schemas).map_err(|error| bad(kind, error.render_terse()))?,
        ));
    }
    Ok(pairs)
}

fn object_id_to_json(id: &ObjectId) -> Json {
    object([
        ("schema", Json::String(id.schema().to_string())),
        (
            "values",
            Json::Array(id.values().iter().map(to_json).collect()),
        ),
    ])
}

fn object_id_from_json(
    kind: FrameKind,
    json: &Json,
    schemas: &SchemaRegistry,
) -> Result<ObjectId, ProtocolError> {
    let schema: SchemaId = json
        .get("schema")
        .and_then(Json::as_str)
        .ok_or_else(|| bad(kind, "an object identity names its schema"))?
        .parse()
        .map_err(|error: ErrorValue| bad(kind, error.render_terse()))?;
    let Some(Json::Array(values)) = json.get("values") else {
        return Err(bad(kind, "an object identity carries its identity values"));
    };
    let values = values
        .iter()
        .map(|value| from_json(value, schemas).map_err(|error| bad(kind, error.render_terse())))
        .collect::<Result<Vec<Value>, ProtocolError>>()?;
    Ok(ObjectId::new(schema, values))
}

fn query_to_json(query: &RemoteQuery) -> Json {
    let selectors = query
        .selectors
        .iter()
        .map(|selector| match selector {
            Selector::Field { name, value } => object([
                ("kind", Json::String("field".to_owned())),
                ("name", Json::String(name.clone())),
                ("value", to_json(value)),
            ]),
            Selector::Contains { name, text } => object([
                ("kind", Json::String("contains".to_owned())),
                ("name", Json::String(name.clone())),
                ("text", Json::String(text.clone())),
            ]),
            Selector::Identity(id) => object([
                ("kind", Json::String("identity".to_owned())),
                ("object", object_id_to_json(id)),
            ]),
        })
        .collect();
    object([
        ("target", Json::String(query.target.clone())),
        ("selectors", Json::Array(selectors)),
        ("options", pairs_to_json(&query.options)),
        (
            "limit",
            query
                .limit
                .and_then(|limit| u64::try_from(limit).ok())
                .map_or(Json::Null, Json::from),
        ),
    ])
}

fn query_from_json(
    kind: FrameKind,
    json: &Json,
    schemas: &SchemaRegistry,
) -> Result<RemoteQuery, ProtocolError> {
    let target = json
        .get("target")
        .and_then(Json::as_str)
        .ok_or_else(|| bad(kind, "a request names its target"))?;
    let mut query = RemoteQuery::target(target);
    if let Some(Json::Array(selectors)) = json.get("selectors") {
        for selector in selectors {
            query = query.with(selector_from_json(kind, selector, schemas)?);
        }
    }
    query.options = pairs_from_json(kind, json.get("options"), schemas)?;
    if let Some(limit) = json.get("limit").and_then(Json::as_u64) {
        query = query.limit(usize::try_from(limit).unwrap_or(usize::MAX));
    }
    Ok(query)
}

fn selector_from_json(
    kind: FrameKind,
    json: &Json,
    schemas: &SchemaRegistry,
) -> Result<Selector, ProtocolError> {
    let name = || {
        json.get("name")
            .and_then(Json::as_str)
            .ok_or_else(|| bad(kind, "a selector names the field it narrows on"))
    };
    match json.get("kind").and_then(Json::as_str) {
        Some("field") => Ok(Selector::field(
            name()?,
            from_json(json.get("value").unwrap_or(&Json::Null), schemas)
                .map_err(|error| bad(kind, error.render_terse()))?,
        )),
        Some("contains") => Ok(Selector::contains(
            name()?,
            json.get("text")
                .and_then(Json::as_str)
                .ok_or_else(|| bad(kind, "a contains selector carries the text to look for"))?,
        )),
        Some("identity") => Ok(Selector::identity(object_id_from_json(
            kind,
            json.get("object").unwrap_or(&Json::Null),
            schemas,
        )?)),
        _ => Err(bad(kind, "a selector says which kind of selector it is")),
    }
}

fn act_to_json(request: &ActRequest) -> Json {
    object([
        ("target", Json::String(request.target.clone())),
        ("operation", Json::String(request.operation.clone())),
        ("object", object_id_to_json(&request.object)),
        ("arguments", pairs_to_json(&request.arguments)),
        ("dry_run", Json::Bool(request.dry_run)),
    ])
}

fn act_from_json(
    kind: FrameKind,
    json: &Json,
    schemas: &SchemaRegistry,
) -> Result<ActRequest, ProtocolError> {
    let target = json
        .get("target")
        .and_then(Json::as_str)
        .ok_or_else(|| bad(kind, "an action names its target"))?;
    let operation = json
        .get("operation")
        .and_then(Json::as_str)
        .ok_or_else(|| bad(kind, "an action names its operation"))?;
    let object = object_id_from_json(kind, json.get("object").unwrap_or(&Json::Null), schemas)?;
    let mut request = ActRequest::new(target, operation, object);
    request.arguments = pairs_from_json(kind, json.get("arguments"), schemas)?;
    if json.get("dry_run").and_then(Json::as_bool) == Some(true) {
        request = request.as_dry_run();
    }
    Ok(request)
}

fn event_to_json(event: &ObjectEvent) -> Json {
    let record = event.value().map_or(Json::Null, |record| {
        to_json(&Value::Record(Arc::clone(record)))
    });
    object([
        ("kind", Json::String(event.kind().as_str().to_owned())),
        ("sequence", event.sequence().map_or(Json::Null, Json::from)),
        (
            "changed_fields",
            event.changed_fields().map_or(Json::Null, |fields| {
                Json::Array(fields.iter().map(|f| Json::String(f.clone())).collect())
            }),
        ),
        ("record", record),
    ])
}

fn event_from_json(
    kind: FrameKind,
    json: &Json,
    schemas: &SchemaRegistry,
) -> Result<ObjectEvent, ProtocolError> {
    let record = match from_json(json.get("record").unwrap_or(&Json::Null), schemas) {
        Ok(Value::Record(record)) => record,
        Ok(_) => return Err(bad(kind, "an object event carries the object's record")),
        Err(error) => return Err(bad(kind, error.render_terse())),
    };
    let fields: Vec<String> = json
        .get("changed_fields")
        .and_then(Json::as_array)
        .map(|fields| {
            fields
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let record: &RecordValue = record.as_ref();
    let event = match json.get("kind").and_then(Json::as_str) {
        Some(name) if name == EventKind::Snapshot.as_str() => ObjectEvent::snapshot(record),
        Some(name) if name == EventKind::Added.as_str() => ObjectEvent::added(record),
        Some(name) if name == EventKind::Changed.as_str() => ObjectEvent::changed(record, fields),
        Some(name) if name == EventKind::Removed.as_str() => ObjectEvent::removed(record),
        _ => return Err(bad(kind, "an object event says what happened")),
    };
    Ok(match json.get("sequence").and_then(Json::as_u64) {
        Some(sequence) => event.with_sequence(sequence),
        None => event,
    })
}

fn outcome_to_json(outcome: &ActionOutcome) -> Json {
    object([
        ("object", object_id_to_json(outcome.target())),
        ("operation", Json::String(outcome.operation().to_owned())),
        ("status", Json::String(outcome.status().as_str().to_owned())),
        ("changed", Json::Bool(outcome.changed())),
        (
            "error",
            outcome.error().map_or(Json::Null, |error| {
                to_json(&Value::Error(Arc::new(error.clone())))
            }),
        ),
        (
            "message",
            outcome_message(outcome).map_or(Json::Null, Json::String),
        ),
    ])
}

/// The note an outcome carries, such as why an action was skipped.
///
/// `ActionOutcome` records the note but does not expose it directly; the `ActionResult` it turns
/// into does (spec §11.5). Going through that conversion is what keeps `skipped: the process had
/// already exited` from being dropped on the way across a link.
fn outcome_message(outcome: &ActionOutcome) -> Option<String> {
    outcome
        .clone()
        .into_record(ono_value::Duration::from_nanoseconds(0))
        .message()
        .map(str::to_owned)
}

fn outcome_from_json(
    kind: FrameKind,
    json: &Json,
    schemas: &SchemaRegistry,
) -> Result<ActionOutcome, ProtocolError> {
    let object = object_id_from_json(kind, json.get("object").unwrap_or(&Json::Null), schemas)?;
    let operation = json
        .get("operation")
        .and_then(Json::as_str)
        .ok_or_else(|| bad(kind, "an outcome names the operation it reports on"))?;
    let target = object.schema().name().to_owned();
    let action = ActRequest::new(target, operation, object).to_action();
    let message = json.get("message").and_then(Json::as_str).unwrap_or("");
    let status = json
        .get("status")
        .and_then(Json::as_str)
        .and_then(ActionStatus::from_name)
        .ok_or_else(|| bad(kind, "an outcome carries one of the statuses of spec §11.5"))?;
    Ok(match status {
        ActionStatus::Success => ActionOutcome::succeeded(
            &action,
            json.get("changed").and_then(Json::as_bool).unwrap_or(false),
        ),
        ActionStatus::Skipped => ActionOutcome::skipped(&action, message),
        ActionStatus::Failed => {
            let error = match from_json(json.get("error").unwrap_or(&Json::Null), schemas) {
                Ok(Value::Error(error)) => error.as_ref().clone(),
                _ => ErrorValue::new(ono_core::ErrorCode::ProviderUnavailable, message),
            };
            ActionOutcome::failed(&action, error)
        }
    })
}
