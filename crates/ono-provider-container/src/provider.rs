//! The `container` and `image` provider itself.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectId, ObjectRef, Provider, Query, Risk,
    Selector,
};
use ono_value::{ErrorValue, RecordValue, SchemaId, Value};

use crate::Endpoints;
use crate::http::{self, HttpError, Response};
use crate::record::{
    container_record, container_schema, image_matches, image_record, image_schema,
};

/// The id this provider signs its records with.
///
/// One id for Docker and Podman alike: they serve the same API on the same kind of socket, and
/// which one answered is a fact about the socket (`docker.sock`, `podman.sock`), not a different
/// provider. Spec §39's open question 15 — `docker:container` against `podman:container` — is
/// the case of two runtimes on one machine, which this build serves in socket order (ADR-0112).
pub const PROVIDER_ID: &str = "container-engine";

/// How long a read of the engine may take before the provider gives up on it.
const READ_BUDGET: Duration = Duration::from_secs(30);

/// The container provider: `ono.container/1` and `ono.image/1` records read from the engine
/// API over a Unix socket.
///
/// It never runs `docker` or `podman` and never parses their output (spec §23, §31.57, §50).
/// Where no runtime socket answers it reports [`Availability::Unavailable`] naming every socket
/// it tried, because an empty result would be indistinguishable from a machine with no
/// containers (spec §10.5, §35.3).
///
/// ```
/// use ono_provider_api::Provider;
/// use ono_provider_container::ContainerProvider;
///
/// let provider = ContainerProvider::from_environment([("DOCKER_HOST", "unix:///nowhere/none.sock")]);
/// let reason = provider.availability().reason().map(str::to_owned);
/// assert!(reason.is_some_and(|reason| reason.contains("/nowhere/none.sock")));
/// ```
#[derive(Debug)]
pub struct ContainerProvider {
    endpoints: Endpoints,
}

impl ContainerProvider {
    /// A provider over the sockets `environment` names, or the well-known ones.
    #[must_use]
    pub fn from_environment<'a>(environment: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self {
            endpoints: Endpoints::from_environment(environment),
        }
    }

    /// The socket that answers now, probed afresh: a runtime that was started after the shell
    /// is found the next time it is asked for, and one that stopped is reported as gone.
    fn socket(&self) -> Result<PathBuf, ErrorValue> {
        self.endpoints.probe().map_err(|reason| {
            ErrorValue::new(ErrorCode::ProviderUnavailable, reason).with_help(
                "`container` and `image` need a running Docker- or Podman-compatible engine. \
                 Point DOCKER_HOST or CONTAINER_HOST at its unix:// socket. Having no runtime is \
                 not the same as having no containers, so this is a refusal to answer rather \
                 than an empty answer.",
            )
        })
    }
}

/// How a query is answered: one object asked for by its handle, or an enumeration.
struct Plan {
    /// The handle to inspect directly — a container's id or name, an image's reference.
    named: Option<String>,
    /// The selectors still to apply to each record once it has been read.
    remaining: Vec<Selector>,
}

impl Plan {
    fn of(query: &Query, handles: &[&str]) -> Self {
        let mut named = None;
        let mut remaining = Vec::new();
        for selector in query.selectors() {
            match selector {
                Selector::Field { name, value }
                    if named.is_none() && handles.contains(&name.as_str()) =>
                {
                    match value.as_str() {
                        Ok(text) => named = Some(text.to_owned()),
                        Err(_) => remaining.push(selector.clone()),
                    }
                }
                Selector::Identity(id) if named.is_none() => {
                    match id.values().first().and_then(|value| value.as_str().ok()) {
                        Some(text) => named = Some(text.to_owned()),
                        None => remaining.push(selector.clone()),
                    }
                }
                other => remaining.push(other.clone()),
            }
        }
        Self { named, remaining }
    }

    fn keeps(&self, record: &RecordValue) -> bool {
        self.remaining
            .iter()
            .all(|selector| selector.matches(record))
    }
}

/// The error an engine answer is, when it is one.
fn engine_error(response: &Response, what: &str) -> ErrorValue {
    let message = response.message();
    match response.status {
        404 => ErrorValue::new(ErrorCode::IoNotFound, format!("{what}: {message}")),
        401 | 403 => ErrorValue::new(ErrorCode::IoPermissionDenied, format!("{what}: {message}"))
            .with_help("the engine refused; the socket's owner decides who may act on containers"),
        409 => ErrorValue::new(
            ErrorCode::SafetyConfirmationRequired,
            format!("{what}: {message}"),
        )
        .with_help(
            "the engine refuses this while the container runs; stop it first, or write `--force`",
        ),
        _ => ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!(
                "{what}: the engine answered HTTP {} — {message}",
                response.status
            ),
        ),
    }
}

fn transport_error(error: &HttpError) -> ErrorValue {
    match error {
        HttpError::Unreachable(_) => {
            ErrorValue::new(ErrorCode::ProviderUnavailable, error.to_string())
        }
        HttpError::Protocol(_) => {
            ErrorValue::new(ErrorCode::ProviderSchemaViolation, error.to_string())
                .with_help("the socket did not answer as a Docker-compatible engine")
        }
        HttpError::TimedOut(_) => {
            ErrorValue::new(ErrorCode::ProviderUnavailable, error.to_string()).with_retryable(true)
        }
    }
}

/// `GET`s a listing and reads it as a JSON array.
async fn list(
    socket: &Path,
    path: &str,
    endpoint: &str,
) -> Result<Vec<serde_json::Value>, ErrorValue> {
    let response = http::request(socket, "GET", path, None, READ_BUDGET)
        .await
        .map_err(|error| transport_error(&error))?;
    if response.status != 200 {
        return Err(engine_error(&response, &format!("GET {path}")));
    }
    match response.json() {
        Some(serde_json::Value::Array(entries)) => Ok(entries),
        _ => Err(ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("GET {path} on {endpoint} did not answer with a JSON list"),
        )),
    }
}

/// `GET /containers/{handle}/json`: the one container, or none when the engine knows no such
/// id or name.
async fn inspect_container(
    socket: &Path,
    handle: &str,
) -> Result<Option<serde_json::Value>, ErrorValue> {
    let path = format!("/containers/{handle}/json");
    let response = http::request(socket, "GET", &path, None, READ_BUDGET)
        .await
        .map_err(|error| transport_error(&error))?;
    match response.status {
        200 => response.json().map(Some).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("GET {path} did not answer with JSON"),
            )
        }),
        404 => Ok(None),
        _ => Err(engine_error(&response, &format!("GET {path}"))),
    }
}

/// What a `container` action asks of the engine (ADR-0113).
struct Request {
    method: &'static str,
    path: String,
    body: Option<Vec<u8>>,
    /// What the engine is being asked to do, for a dry run's answer.
    described: String,
    /// How long the engine may take: a stop answers only once the container has stopped.
    budget: Duration,
}

impl Request {
    /// The request `action` asks for, or the reason it asks for nothing this provider does.
    fn of(action: &Action, id: &str) -> Result<Self, ErrorValue> {
        let unsupported = |message: String, help: &str| {
            Err(ErrorValue::new(ErrorCode::ProviderUnsupported, message).with_help(help))
        };
        let stop_timeout = match action.argument("timeout") {
            Some(Value::Duration(timeout)) => {
                let seconds = timeout.nanoseconds() / 1_000_000_000;
                Some(u64::try_from(seconds).unwrap_or(0))
            }
            _ => None,
        };
        let query = |timeout: Option<u64>| timeout.map_or(String::new(), |t| format!("?t={t}"));
        let simple = |method: &'static str, path: String, described: &str| Self {
            method,
            path,
            body: None,
            described: described.to_owned(),
            budget: READ_BUDGET + Duration::from_secs(stop_timeout.unwrap_or(10)),
        };
        match action.operation() {
            "start" => Ok(simple(
                "POST",
                format!("/containers/{id}/start"),
                "start it",
            )),
            "stop" => Ok(simple(
                "POST",
                format!("/containers/{id}/stop{}", query(stop_timeout)),
                "stop it",
            )),
            "restart" => Ok(simple(
                "POST",
                format!("/containers/{id}/restart{}", query(stop_timeout)),
                "restart it",
            )),
            "remove" => {
                let flag = |name: &str| matches!(action.argument(name), Some(Value::Bool(true)));
                Ok(simple(
                    "DELETE",
                    format!(
                        "/containers/{id}?force={}&v={}",
                        flag("force"),
                        flag("volumes")
                    ),
                    "remove it",
                ))
            }
            "set" => {
                let mut body = serde_json::Map::new();
                let mut described = Vec::new();
                match action.argument("memory") {
                    Some(Value::ByteSize(limit)) => {
                        let bytes = u64::try_from(limit.bytes()).unwrap_or(u64::MAX);
                        body.insert("Memory".to_owned(), serde_json::Value::from(bytes));
                        described.push(format!("memory limit to {bytes} bytes"));
                    }
                    Some(other) => {
                        return unsupported(
                            format!("`memory` is a byte size, not a {}", other.type_name()),
                            "write `--memory 512MiB`",
                        );
                    }
                    None => {}
                }
                match action.argument("cpus") {
                    Some(Value::Float(cpus)) if *cpus >= 0.0 => {
                        // The engine counts CPUs in billionths; a float is what the contract
                        // declares and what `docker update --cpus` takes.
                        #[allow(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "non-negative and bounded by the machine's CPUs times 1e9"
                        )]
                        let nano = (cpus * 1_000_000_000.0).round() as u64;
                        body.insert("NanoCpus".to_owned(), serde_json::Value::from(nano));
                        described.push(format!("cpu allowance to {cpus}"));
                    }
                    Some(Value::Int(cpus)) if *cpus >= 0 => {
                        let nano = u64::try_from(*cpus)
                            .unwrap_or(0)
                            .saturating_mul(1_000_000_000);
                        body.insert("NanoCpus".to_owned(), serde_json::Value::from(nano));
                        described.push(format!("cpu allowance to {cpus}"));
                    }
                    Some(other) => {
                        return unsupported(
                            format!("`cpus` is a non-negative number, not {other}"),
                            "write `--cpus 1.5`",
                        );
                    }
                    None => {}
                }
                if body.is_empty() {
                    return unsupported(
                        "the engine changes a container's `memory` and `cpus`, and `set` named \
                         neither"
                            .to_owned(),
                        "write `--memory 2GiB` or `--cpus 1.5`",
                    );
                }
                Ok(Self {
                    method: "POST",
                    path: format!("/containers/{id}/update"),
                    body: Some(serde_json::Value::Object(body).to_string().into_bytes()),
                    described: format!("set its {}", described.join(" and ")),
                    budget: READ_BUDGET,
                })
            }
            other => unsupported(
                format!("the container provider has no operation `{other}`"),
                "it can start, stop, restart and remove a container, and set `--memory` and \
                 `--cpus`",
            ),
        }
    }
}

/// The container id an identity names.
fn container_id(id: &ObjectId) -> Result<&str, ErrorValue> {
    let expected = SchemaId::new("ono.container", 1);
    id.values()
        .first()
        .and_then(|value| value.as_str().ok())
        .filter(|_| id.schema() == &expected)
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`{id}` does not name a container"),
            )
            .with_help("a container action needs an `ono.container/1` identity")
        })
}

#[async_trait::async_trait]
impl Provider for ContainerProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["container", "image"]
    }

    fn schemas(&self) -> Vec<Arc<ono_value::Schema>> {
        vec![container_schema(), image_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("container.list", Risk::Read),
            Capability::new("image.list", Risk::Read),
            // `docs/spec/capabilities.yaml` gives `container.manage` elevation `conditional`:
            // the socket's permissions decide, and the engine's 403 is the structured form of
            // that decision (ADR-0113 §3).
            Capability::new("container.manage", Risk::Mutate),
        ]
    }

    fn availability(&self) -> Availability {
        match self.endpoints.probe() {
            Ok(_) => Availability::Available,
            Err(reason) => Availability::unavailable(reason),
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let socket = self.socket()?;
        let endpoint = format!("unix://{}", socket.display());
        let target = query.target_name().to_owned();
        let plan = Plan::of(
            query,
            if target == "image" {
                &["reference", "id"]
            } else {
                &["id", "name"]
            },
        );
        let limit = query.max();

        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let entries = if target == "image" {
                    list(&socket, "/images/json", &endpoint).await
                } else if let Some(handle) = &plan.named {
                    inspect_container(&socket, handle)
                        .await
                        .map(|found| found.into_iter().collect())
                } else {
                    list(&socket, "/containers/json?all=1", &endpoint).await
                };
                let entries = match entries {
                    Ok(entries) => entries,
                    Err(error) => {
                        let _ = sink.fail(error).await;
                        return;
                    }
                };

                let mut emitted = 0usize;
                for entry in &entries {
                    if limit.is_some_and(|limit| emitted >= limit) {
                        return;
                    }
                    let record = if target == "image" {
                        image_record(entry, &endpoint)
                    } else {
                        container_record(entry, &endpoint)
                    };
                    match record {
                        Ok(record) => {
                            if target == "image"
                                && plan
                                    .named
                                    .as_deref()
                                    .is_some_and(|reference| !image_matches(&record, reference))
                            {
                                continue;
                            }
                            if !plan.keeps(&record) {
                                continue;
                            }
                            emitted += 1;
                            if sink.send(record.into_value()).await.is_err() {
                                return;
                            }
                        }
                        // One malformed entry must not cost the others (spec §16.5).
                        Err(error) => {
                            if sink.fail(error).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        // A selector on `reference` names an image; everything else names a container.
        let target = match selector.field_name() {
            Some("reference") => "image",
            _ => "container",
        };
        let query = Query::target(target).with(selector.clone());
        let collected = self.snapshot(&query)?.collect().await;
        if let Some(error) = collected.errors().first()
            && collected.values().is_empty()
        {
            return Err(error.clone());
        }
        Ok(collected
            .values()
            .iter()
            .filter_map(|value| match value {
                Value::Record(record) => ObjectRef::of(record),
                _ => None,
            })
            .collect())
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let socket = self.socket()?;
        let id = container_id(action.target())?;
        let request = Request::of(action, id)?;
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!(
                    "would {} with `{} {}`",
                    request.described, request.method, request.path
                ),
            ));
        }
        let response = match http::request(
            &socket,
            request.method,
            &request.path,
            request.body.as_deref(),
            request.budget,
        )
        .await
        {
            Ok(response) => response,
            Err(error) => return Ok(ActionOutcome::failed(action, transport_error(&error))),
        };
        // The engine's status is the outcome (ADR-0113 §2): a 2xx did it, a 304 found it
        // already so, anything else is the engine's refusal with its own reason.
        Ok(match response.status {
            200..=299 => ActionOutcome::succeeded(action, true),
            304 => ActionOutcome::skipped(
                action,
                format!("the engine reports {id} already in that state"),
            ),
            _ => ActionOutcome::failed(
                action,
                engine_error(&response, &format!("{} {}", request.method, request.path)),
            ),
        })
    }
}
