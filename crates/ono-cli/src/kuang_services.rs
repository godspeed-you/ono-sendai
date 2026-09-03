//! The shell's side of the host API domains a package reaches through the supervisor
//! (spec §31.12; ADR-0568): objects from the session's providers, relations from the graph,
//! history from the history file, signals through the providers' `act`. The supervisor
//! checks the grant and audits; this module answers with the shell's own data, as JSON.

use std::path::PathBuf;
use std::sync::Arc;

use ono_kuang_supervisor::{HostError, HostServices, LiveStream, ready_stream};
use ono_provider_api::{Action, ObjectId, ProviderRegistry, Query, Selector};
use ono_value::{ErrorValue, SchemaId, Value, builtin_schemas, from_json, to_json};
use serde_json::{Value as Json, json};

/// What a history line is scrubbed of before a package sees it: the history file's own
/// patterns, plus a credential carried in a header or as a bare token (ADR-0015 T8).
const PACKAGE_REDACTIONS: &[&str] = &[
    r"(?i)--?(password|passwd|pass|token|secret|api[-_]?key|access[-_]?key|auth|credential)[=\s]+(\S+)",
    r"(?i)\b([A-Z0-9_]*(PASSWORD|TOKEN|SECRET|API_?KEY|CREDENTIAL)[A-Z0-9_]*)=(\S+)",
    r"(?i)(authorization:\s*(bearer|basic|token)\s+)(\S+)",
    r"(?i)\b(sk|pk|ghp|gho|xox[abp])[-_][A-Za-z0-9_-]{8,}",
];

/// What a loaded package's domain calls reach: the registry as it stood when the package was
/// loaded, and where the session's history file is.
#[derive(Clone)]
pub struct ShellHost {
    registry: ProviderRegistry,
    history: Option<PathBuf>,
}

impl std::fmt::Debug for ShellHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellHost")
            .field("history", &self.history)
            .finish_non_exhaustive()
    }
}

impl ShellHost {
    /// The services over `registry`, with the history at `history` when the session keeps one.
    #[must_use]
    pub fn new(registry: ProviderRegistry, history: Option<PathBuf>) -> Self {
        Self { registry, history }
    }

    /// The target a schema's objects are asked for: the provider that advertises the schema
    /// names it. `ono.package-source/1` is `package-source`, `ono.socket/1` is `socket`.
    fn target_of(&self, schema: &SchemaId) -> Option<String> {
        self.registry
            .providers()
            .iter()
            .find(|provider| provider.schemas().iter().any(|known| known.id() == schema))
            .and_then(|provider| {
                provider
                    .targets()
                    .first()
                    .map(|target| (*target).to_owned())
            })
            .or_else(|| {
                schema
                    .to_string()
                    .strip_prefix("ono.")
                    .and_then(|rest| rest.split('/').next())
                    .map(str::to_owned)
            })
    }

    /// The record of `id`, from a snapshot narrowed to its identity.
    async fn record_of(&self, target: &str, id: &ObjectId) -> Result<Value, HostError> {
        let query = Query::target(target).with(Selector::identity(id.clone()));
        let collected = self
            .registry
            .snapshot(&query)
            .map_err(|error| host_error(&error))?
            .collect()
            .await;
        collected
            .values()
            .iter()
            .find(|value| match value {
                Value::Record(record) => ObjectId::of(record).as_ref() == Some(id),
                _ => false,
            })
            .cloned()
            .ok_or_else(|| {
                collected.errors().first().map_or_else(
                    || HostError::not_found(format!("`{id}` is not there to be read")),
                    host_error,
                )
            })
    }
}

/// An `ErrorValue` as the wire carries it.
fn host_error(error: &ErrorValue) -> HostError {
    HostError {
        code: error.code(),
        message: error.message().to_owned(),
    }
}

/// An `ObjectId` from the wire: `{schema, values: [...]}` or `{schema, <identity field>: …}`.
fn object_id(json: &Json) -> Result<ObjectId, HostError> {
    let schema_text = json
        .get("schema")
        .and_then(Json::as_str)
        .ok_or_else(|| HostError::malformed("an object id names its `schema`"))?;
    let schema: SchemaId = schema_text
        .parse()
        .map_err(|_| HostError::malformed(format!("`{schema_text}` is not a schema id")))?;
    let registry = builtin_schemas();
    let decode = |value: &Json| from_json(value, registry).map_err(|error| host_error(&error));
    if let Some(values) = json.get("values").and_then(Json::as_array) {
        let values: Vec<Value> = values.iter().map(decode).collect::<Result<_, _>>()?;
        return Ok(ObjectId::new(schema, values));
    }
    let Some(definition) = registry.get(&schema) else {
        return Err(HostError::malformed(format!(
            "`{schema}` is not a schema this shell knows, so its identity fields cannot be named"
        )));
    };
    let mut values = Vec::new();
    for field in definition.identity() {
        let value = json.get(field.as_ref()).ok_or_else(|| {
            HostError::malformed(format!("an `{schema}` identity needs `{field}`"))
        })?;
        values.push(decode(value)?);
    }
    Ok(ObjectId::new(schema, values))
}

/// An `ObjectId` as the wire carries it.
fn object_id_json(id: &ObjectId) -> Json {
    json!({
        "schema": id.schema().to_string(),
        "values": id.values().iter().map(to_json).collect::<Vec<_>>(),
    })
}

/// A `Selector` from the wire: `{field: {name, value}}`, `{contains: {name, text}}` or
/// `{identity: <object id>}`.
fn selector(json: &Json) -> Result<Selector, HostError> {
    if let Some(field) = json.get("field") {
        let name = field
            .get("name")
            .and_then(Json::as_str)
            .ok_or_else(|| HostError::malformed("a field selector names its field"))?;
        let value = from_json(field.get("value").unwrap_or(&Json::Null), builtin_schemas())
            .map_err(|error| host_error(&error))?;
        return Ok(Selector::field(name, value));
    }
    if let Some(contains) = json.get("contains") {
        let name = contains
            .get("name")
            .and_then(Json::as_str)
            .ok_or_else(|| HostError::malformed("a contains selector names its field"))?;
        let text = contains
            .get("text")
            .and_then(Json::as_str)
            .unwrap_or_default();
        return Ok(Selector::contains(name, text));
    }
    if let Some(identity) = json.get("identity") {
        return Ok(Selector::identity(object_id(identity)?));
    }
    Err(HostError::malformed(
        "a selector is `{field}`, `{contains}` or `{identity}`",
    ))
}

/// A `Query` from the wire: `{target, selectors, options, limit}`.
fn query(json: &Json) -> Result<Query, HostError> {
    let target = json
        .get("target")
        .and_then(Json::as_str)
        .ok_or_else(|| HostError::malformed("a query names its `target`"))?;
    let mut query = Query::target(target);
    for item in json
        .get("selectors")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        query = query.with(selector(item)?);
    }
    if let Some(options) = json.get("options").and_then(Json::as_object) {
        for (name, value) in options {
            let value = from_json(value, builtin_schemas()).map_err(|error| host_error(&error))?;
            query = query.option(name.clone(), value);
        }
    }
    if let Some(limit) = json.get("limit").and_then(Json::as_u64) {
        query = query.limit(usize::try_from(limit).unwrap_or(usize::MAX));
    }
    Ok(query)
}

/// Pumps a value stream into a live stream the supervisor pulls from.
fn pump_values(mut stream: ono_pipeline::ValueStream) -> LiveStream {
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(event) = stream.recv().await {
            let item = match event {
                ono_pipeline::StreamEvent::Value(value) => Ok(to_json(&value)),
                ono_pipeline::StreamEvent::Failure(error) => Err(
                    ono_kuang_protocol::WireError::from_core(error.code(), error.message()),
                ),
            };
            if tx.send(item).await.is_err() {
                break;
            }
        }
    });
    rx
}

/// Pumps a value stream as `snapshot` events (spec §31.14).
fn pump_snapshots(mut stream: ono_pipeline::ValueStream) -> LiveStream {
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(event) = stream.recv().await {
            let item = match event {
                ono_pipeline::StreamEvent::Value(value) => Ok(json!({
                    "kind": "snapshot",
                    "at": jiff::Timestamp::now().to_string(),
                    "object": to_json(&value),
                })),
                ono_pipeline::StreamEvent::Failure(error) => Err(
                    ono_kuang_protocol::WireError::from_core(error.code(), error.message()),
                ),
            };
            if tx.send(item).await.is_err() {
                break;
            }
        }
    });
    rx
}

/// Pumps a provider's event stream as `ObjectEvent`s on the wire.
fn pump_events(mut events: ono_provider_api::EventStream) -> LiveStream {
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            let kind = match event.kind() {
                ono_provider_api::EventKind::Snapshot => "snapshot",
                ono_provider_api::EventKind::Added => "added",
                ono_provider_api::EventKind::Changed => "changed",
                ono_provider_api::EventKind::Removed => "removed",
            };
            let item = json!({
                "kind": kind,
                "object_id": object_id_json(event.object_id()),
                "schema": event.schema().to_string(),
                "at": event.at().to_string(),
                "sequence": event.sequence(),
                "value": event.value().map(|record| to_json(&Value::Record(Arc::clone(record)))),
                "changed_fields": event.changed_fields(),
            });
            if tx.send(Ok(item)).await.is_err() {
                break;
            }
        }
    });
    rx
}

#[async_trait::async_trait]
impl HostServices for ShellHost {
    async fn object_get(&self, id: Json) -> Result<Json, HostError> {
        let id = object_id(&id)?;
        let target = self.target_of(id.schema()).ok_or_else(|| {
            HostError::not_found(format!("no provider answers for `{}`", id.schema()))
        })?;
        let record = self.record_of(&target, &id).await?;
        Ok(to_json(&record))
    }

    async fn object_query(&self, query_json: Json) -> Result<LiveStream, HostError> {
        let query = query(&query_json)?;
        let stream = self
            .registry
            .snapshot(&query)
            .map_err(|error| host_error(&error))?;
        Ok(pump_values(stream))
    }

    async fn object_resolve(
        &self,
        target: String,
        selector_json: Json,
    ) -> Result<Vec<Json>, HostError> {
        let selector = selector(&selector_json)?;
        let found = self
            .registry
            .resolve(&target, &selector)
            .await
            .map_err(|error| host_error(&error))?;
        Ok(found
            .iter()
            .map(|reference| {
                json!({
                    "id": object_id_json(reference.id()),
                    "label": reference.label(),
                    "provider": reference.provenance().provider(),
                })
            })
            .collect())
    }

    async fn object_snapshot(&self, query_json: Json) -> Result<LiveStream, HostError> {
        let query = query(&query_json)?;
        let stream = self
            .registry
            .snapshot(&query)
            .map_err(|error| host_error(&error))?;
        Ok(pump_snapshots(stream))
    }

    async fn object_subscribe(
        &self,
        query_json: Json,
        _overflow: Option<String>,
    ) -> Result<LiveStream, HostError> {
        let query = query(&query_json)?;
        let events = self
            .registry
            .subscribe(&query)
            .map_err(|error| host_error(&error))?;
        Ok(pump_events(events))
    }

    async fn object_watch(&self, query_json: Json, policy: Json) -> Result<LiveStream, HostError> {
        let query = query(&query_json)?;
        let target = query.target_name().to_owned();
        let interval = policy.get("interval").and_then(Json::as_u64).map_or(
            std::time::Duration::from_secs(1),
            std::time::Duration::from_secs,
        );
        let stream = ono_command::watch_events(&self.registry, &target, query, interval)
            .map_err(|error| host_error(&error))?;
        Ok(pump_values(stream))
    }

    async fn relations_query(
        &self,
        from: Option<Json>,
        to: Option<Json>,
        relations: Option<Vec<String>>,
        _depth: Option<u64>,
    ) -> Result<LiveStream, HostError> {
        // One hop from an object: the traversal the contract offers. Inbound search from `to`
        // alone would be a walk of the whole graph, which is not offered either.
        let Some(from) = from else {
            return Err(HostError::malformed(
                "`relations.query` walks one hop from an object; name it in `from`",
            ));
        };
        let from = object_id(&from)?;
        let to = to.as_ref().map(object_id).transpose()?;
        let target = self.target_of(from.schema()).ok_or_else(|| {
            HostError::not_found(format!("no provider answers for `{}`", from.schema()))
        })?;
        let record = self.record_of(&target, &from).await?;
        let Value::Record(record) = record else {
            return Err(HostError::not_found(format!("`{from}` is not an object")));
        };
        let node = ono_graph::Node::of(&record).ok_or_else(|| {
            HostError::not_found(format!("`{from}` has no identity to walk from"))
        })?;
        let registry = Arc::new(self.registry.clone());
        let mut sources = ono_graph::kernel_relationships(Arc::clone(&registry));
        sources.push(Arc::new(ono_graph::ProcessUsers::new(registry)));
        let mut edges = Vec::new();
        for source in sources {
            if !source.subjects().iter().any(|subject| *subject == target) {
                continue;
            }
            let found = source.relationships(&node).await;
            for relationship in found.found() {
                let edge = relationship.edge();
                if relations
                    .as_ref()
                    .is_some_and(|wanted| !wanted.iter().any(|name| name == edge.relation()))
                {
                    continue;
                }
                if to.as_ref().is_some_and(|to| edge.to() != to) {
                    continue;
                }
                edges.push(json!({
                    "from": object_id_json(edge.from()),
                    "to": object_id_json(edge.to()),
                    "relation": edge.relation(),
                    "direction": match edge.direction() {
                        ono_graph::Direction::Directed => "directed",
                        ono_graph::Direction::Undirected => "undirected",
                    },
                    "confidence": match edge.confidence() {
                        ono_graph::Confidence::Exact => "exact",
                        _ => "inferred",
                    },
                    "provider": edge.provider(),
                    "metadata": to_json(&Value::Map(Arc::new(edge.metadata().clone()))),
                }));
            }
        }
        Ok(ready_stream(edges))
    }

    async fn relations_contribute(
        &self,
        _package: &str,
        _edges: Vec<Json>,
    ) -> Result<u64, HostError> {
        Err(HostError::unavailable("store for contributed relations"))
    }

    async fn history_query(
        &self,
        window: Option<String>,
        filter: Option<Json>,
    ) -> Result<LiveStream, HostError> {
        let Some(path) = self.history.as_ref() else {
            return Err(HostError::unavailable("history: the session keeps none"));
        };
        // The history file's own policy hides secret-bearing arguments; at the package boundary
        // the net is wider (ADR-0015 T8, ADR-0568): a credential in a header is as much a secret
        // as one in a flag.
        let policy = ono_history::Policy::default().redacting(PACKAGE_REDACTIONS);
        let entries = match ono_history::History::open(path, ono_history::Policy::default()) {
            Ok(history) => history.entries().to_vec(),
            Err(_) if !path.exists() => Vec::new(),
            Err(error) => {
                return Err(HostError {
                    code: ono_core::ErrorCode::ProviderUnavailable,
                    message: format!(
                        "the history at `{}` could not be read: {error}",
                        path.display()
                    ),
                });
            }
        };
        let since = window
            .as_deref()
            .and_then(|window| window.parse::<jiff::SignedDuration>().ok())
            .and_then(|span| jiff::Timestamp::now().checked_sub(span).ok());
        let contains = filter
            .as_ref()
            .and_then(|filter| filter.get("command_contains"))
            .and_then(Json::as_str)
            .map(str::to_owned);
        let shown: Vec<Json> = entries
            .iter()
            .filter(|entry| since.is_none_or(|since| entry.at() >= since))
            .filter(|entry| {
                contains
                    .as_deref()
                    .is_none_or(|needle| entry.command_text().contains(needle))
            })
            .map(|entry| {
                json!({
                    "id": entry.id(),
                    "at": entry.at().to_string(),
                    "command": policy.redact(entry.command_text()),
                    "cwd": entry.cwd().display().to_string(),
                    "status": entry.exit_status().map(|status| status.code()),
                    "duration_ms": entry.duration().map(|duration| duration.as_millis()),
                    "session": entry.session(),
                })
            })
            .collect();
        Ok(ready_stream(shown))
    }

    async fn history_append(&self, _package: &str, _entry: Json) -> Result<(), HostError> {
        Err(HostError::unavailable(
            "attributed history entries: the history record has no field for a package's authorship yet",
        ))
    }

    async fn process_signal(&self, object: Json, signal: String) -> Result<Json, HostError> {
        let id = object_id(&object)?;
        let action = Action::new("process", "signal", id).with("signal", Value::string(&signal));
        let started = std::time::Instant::now();
        let outcome = self
            .registry
            .act(&action)
            .await
            .map_err(|error| host_error(&error))?;
        let status = match outcome.status() {
            ono_value::ActionStatus::Success => "success",
            ono_value::ActionStatus::Skipped => "skipped",
            ono_value::ActionStatus::Failed => "failed",
        };
        Ok(json!({
            "target": object,
            "operation": "ono.signal.send",
            "status": status,
            "changed": outcome.changed(),
            "message": outcome.message(),
            "error": outcome.error().map(|error| json!({"code": error.code().code(), "message": error.message()})),
            "duration_ms": started.elapsed().as_millis(),
        }))
    }

    async fn secret_request(
        &self,
        _package: &str,
        _name: &str,
        _purpose: &str,
    ) -> Result<(), HostError> {
        Err(HostError::unavailable("secret store"))
    }
}
