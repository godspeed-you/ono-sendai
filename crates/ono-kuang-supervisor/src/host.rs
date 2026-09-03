//! What the object, relation, history, process and secret domains of spec §31.12 reach: the
//! shell's own providers, graph, history and process control, behind a service the loader is
//! handed (ADR-0568).
//!
//! The supervisor has none of those. It has the capability broker, the audit trail and the
//! wire; the shell has the rest. So every domain call here is: check the grant against the
//! value the operation will use, audit it, then hand JSON to the host service and put what
//! comes back on the wire — a stream where the contract says stream, pulled with
//! `streams.next`. The service speaks JSON, so the supervisor depends on no provider crate and
//! the test host can hand a fake one.

use serde_json::Value as Json;
use tokio::sync::mpsc;

/// A stream the host produces live: values arrive as the source makes them, and an `Err`
/// ends it with a terminal failure.
pub type LiveStream = mpsc::Receiver<Result<Json, ono_kuang_protocol::WireError>>;

/// Why a host service could not answer, in the core error vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    /// The structured code the plugin sees.
    pub code: ono_core::ErrorCode,
    /// What was wrong.
    pub message: String,
}

impl HostError {
    /// The host serves nothing of the kind in this build.
    #[must_use]
    pub fn unavailable(what: &str) -> Self {
        Self {
            code: ono_core::ErrorCode::ProviderUnavailable,
            message: format!("this host serves no {what}"),
        }
    }

    /// The request named something that does not exist.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: ono_core::ErrorCode::ResolveTargetNotFound,
            message: message.into(),
        }
    }

    /// The request was not of the shape the contract declares.
    #[must_use]
    pub fn malformed(message: impl Into<String>) -> Self {
        Self {
            code: ono_core::ErrorCode::TypeMismatch,
            message: message.into(),
        }
    }
}

impl From<HostError> for ono_kuang_protocol::WireError {
    fn from(error: HostError) -> Self {
        Self::from_core(error.code, error.message)
    }
}

/// A brokered connection: what arrives, as `{"bytes": …}` values on a live stream, and where
/// the package's own bytes go. Dropping the sender closes the connection.
pub struct Connection {
    /// Received bytes, chunk by chunk, as `{"bytes": {"$bytes": …}}` values.
    pub incoming: LiveStream,
    /// Bytes to send.
    pub outgoing: mpsc::Sender<Vec<u8>>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Connection")
    }
}

/// What a package reaches through `objects.*`, `relations.*`, `history.*`, `process.*`,
/// `network.*` and `secrets.*`. Every method takes the wire's JSON and answers with it; the object ids,
/// queries and selectors are the shapes `protocol.v1.yaml` declares.
#[async_trait::async_trait]
pub trait HostServices: Send + Sync + std::fmt::Debug {
    /// `objects.get`: the record of one object, by `{schema, values}` or `{schema, <field>: …}`.
    async fn object_get(&self, id: Json) -> Result<Json, HostError>;
    /// `objects.query`: a finite stream of records for `{target, selectors, options, limit}`.
    async fn object_query(&self, query: Json) -> Result<LiveStream, HostError>;
    /// `objects.resolve`: the references a selector matches, for a target.
    async fn object_resolve(&self, target: String, selector: Json) -> Result<Vec<Json>, HostError>;
    /// `objects.snapshot`: a bounded stream of `snapshot` events.
    async fn object_snapshot(&self, query: Json) -> Result<LiveStream, HostError>;
    /// `objects.subscribe`: an unbounded stream of changes.
    async fn object_subscribe(
        &self,
        query: Json,
        overflow: Option<String>,
    ) -> Result<LiveStream, HostError>;
    /// `objects.watch`: a snapshot followed by changes, resampled by the host's policy.
    async fn object_watch(&self, query: Json, policy: Json) -> Result<LiveStream, HostError>;
    /// `relations.query`: `ono.graph-edge/1` records around an object.
    async fn relations_query(
        &self,
        from: Option<Json>,
        to: Option<Json>,
        relations: Option<Vec<String>>,
        depth: Option<u64>,
    ) -> Result<LiveStream, HostError>;
    /// `relations.contribute`: edges the package asserts; the host attributes them to it.
    async fn relations_contribute(&self, package: &str, edges: Vec<Json>)
    -> Result<u64, HostError>;
    /// `history.query`: bounded history entries, secret-bearing values redacted.
    async fn history_query(
        &self,
        window: Option<String>,
        filter: Option<Json>,
    ) -> Result<LiveStream, HostError>;
    /// `history.append`: an entry attributed to the package by the host.
    async fn history_append(&self, package: &str, entry: Json) -> Result<(), HostError>;
    /// `process.signal`: one `ono.action-result/1` for the object.
    async fn process_signal(&self, object: Json, signal: String) -> Result<Json, HostError>;
    /// `process.exec`: runs `program` with `arguments` under the host's own confinement, with
    /// `environment` and nothing inherited. The stream carries `{"stream": "stdout"|"stderr",
    /// "line": …}` values as the program writes them and ends with `{"exited": code}`.
    async fn process_exec(
        &self,
        package: &str,
        program: String,
        arguments: Vec<String>,
        environment: Vec<(String, String)>,
    ) -> Result<LiveStream, HostError>;
    /// `network.connect`: a brokered connection to `host:port` over `protocol`; the package
    /// never receives a descriptor.
    async fn network_connect(
        &self,
        host: String,
        port: u16,
        protocol: String,
    ) -> Result<Connection, HostError>;
    /// `network.listen`: a brokered listener on `port`; every accepted connection arrives on
    /// the channel with the peer's address, and the supervisor hands the package a handle for
    /// each. Dropping the receiver closes the listener.
    async fn network_listen(
        &self,
        port: u16,
        protocol: String,
    ) -> Result<mpsc::Receiver<(String, Connection)>, HostError>;
    /// `secrets.request`: whether the named secret exists for the package. The material stays
    /// with the host; the supervisor hands the package an opaque handle.
    async fn secret_request(
        &self,
        package: &str,
        name: &str,
        purpose: &str,
    ) -> Result<(), HostError>;
}

/// The services of a host that has none: every call answers `provider.unavailable`, which is
/// the honest word for a domain this build does not serve (spec §35.3).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoHost;

#[async_trait::async_trait]
impl HostServices for NoHost {
    async fn object_get(&self, _id: Json) -> Result<Json, HostError> {
        Err(HostError::unavailable("objects"))
    }
    async fn object_query(&self, _query: Json) -> Result<LiveStream, HostError> {
        Err(HostError::unavailable("objects"))
    }
    async fn object_resolve(
        &self,
        _target: String,
        _selector: Json,
    ) -> Result<Vec<Json>, HostError> {
        Err(HostError::unavailable("objects"))
    }
    async fn object_snapshot(&self, _query: Json) -> Result<LiveStream, HostError> {
        Err(HostError::unavailable("objects"))
    }
    async fn object_subscribe(
        &self,
        _query: Json,
        _overflow: Option<String>,
    ) -> Result<LiveStream, HostError> {
        Err(HostError::unavailable("objects"))
    }
    async fn object_watch(&self, _query: Json, _policy: Json) -> Result<LiveStream, HostError> {
        Err(HostError::unavailable("objects"))
    }
    async fn relations_query(
        &self,
        _from: Option<Json>,
        _to: Option<Json>,
        _relations: Option<Vec<String>>,
        _depth: Option<u64>,
    ) -> Result<LiveStream, HostError> {
        Err(HostError::unavailable("relations"))
    }
    async fn relations_contribute(
        &self,
        _package: &str,
        _edges: Vec<Json>,
    ) -> Result<u64, HostError> {
        Err(HostError::unavailable("relations"))
    }
    async fn history_query(
        &self,
        _window: Option<String>,
        _filter: Option<Json>,
    ) -> Result<LiveStream, HostError> {
        Err(HostError::unavailable("history"))
    }
    async fn history_append(&self, _package: &str, _entry: Json) -> Result<(), HostError> {
        Err(HostError::unavailable("history"))
    }
    async fn process_signal(&self, _object: Json, _signal: String) -> Result<Json, HostError> {
        Err(HostError::unavailable("process control"))
    }
    async fn process_exec(
        &self,
        _package: &str,
        _program: String,
        _arguments: Vec<String>,
        _environment: Vec<(String, String)>,
    ) -> Result<LiveStream, HostError> {
        Err(HostError::unavailable("program execution"))
    }
    async fn network_connect(
        &self,
        _host: String,
        _port: u16,
        _protocol: String,
    ) -> Result<Connection, HostError> {
        Err(HostError::unavailable("network"))
    }
    async fn network_listen(
        &self,
        _port: u16,
        _protocol: String,
    ) -> Result<mpsc::Receiver<(String, Connection)>, HostError> {
        Err(HostError::unavailable("network"))
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

/// Turns a vector of values into a live stream that is already complete: the shape a host
/// service uses when it has everything at once.
#[must_use]
pub fn ready_stream(values: Vec<Json>) -> LiveStream {
    let (tx, rx) = mpsc::channel(values.len().max(1));
    for value in values {
        // The channel holds exactly this many; a send cannot fail before anyone reads.
        let _ = tx.try_send(Ok(value));
    }
    rx
}
