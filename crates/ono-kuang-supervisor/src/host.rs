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
/// `network.*`, `secrets.*`, `temporal.*`, `change.*` and `recovery.*`. Every method takes the wire's JSON and answers with it; the object ids,
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

    // --- the temporal domain of v0.5 §37 ------------------------------------------------------
    //
    // Defaulted, all six, because a host that keeps no history is a host that answers "I serve
    // none" rather than one that fails to compile. Every existing implementation keeps working
    // unchanged, which is what makes `11.3` additive (ADR-0722).

    /// `temporal.context`: `ono.temporal-context/1` — whether the session is historical, and at
    /// which instant (v0.5 §4, §30.7).
    async fn temporal_context(&self) -> Result<Json, HostError> {
        Err(HostError::unavailable("temporal context"))
    }

    /// `temporal.query`: recorded events within the window the grant allows (v0.5 §11, §30.7).
    ///
    /// The window has already been narrowed to the granted scope by the supervisor; a host
    /// implementation never widens it.
    async fn temporal_query(&self, _query: Json) -> Result<LiveStream, HostError> {
        Err(HostError::unavailable("recorded history"))
    }

    /// `temporal.evidence`: the evidence and coverage behind a temporal claim (v0.5 §7, §30.7).
    async fn temporal_evidence(
        &self,
        _evidence: Vec<String>,
        _events: Vec<String>,
    ) -> Result<LiveStream, HostError> {
        Err(HostError::unavailable("temporal evidence"))
    }

    /// `temporal.contribute.events`: events the host attributes to the package (v0.5 §37.3).
    ///
    /// Everything §37.3 asks the host to validate has already been validated when this is
    /// called, and `source` names the evidence source the host stamped on them. What is left is
    /// storing them.
    async fn temporal_contribute_events(
        &self,
        _package: &str,
        _source: &str,
        _events: Vec<Json>,
    ) -> Result<u64, HostError> {
        Err(HostError::unavailable("a temporal ledger to contribute to"))
    }

    /// `temporal.contribute.causality`: causal links from a rule the package declared (§37.4).
    ///
    /// The strength each link carries has already been lowered to the ceiling of §37.4; a host
    /// implementation stores what it is given and never re-derives a stronger claim (§7.2).
    async fn temporal_contribute_causality(
        &self,
        _package: &str,
        _links: Vec<Json>,
    ) -> Result<u64, HostError> {
        Err(HostError::unavailable("a causal registry to contribute to"))
    }

    /// `temporal.recorder`: start, stop or report the persistent recorder (v0.5 §10.3, §30.7).
    async fn temporal_recorder(&self, _action: String) -> Result<Json, HostError> {
        Err(HostError::unavailable("a history recorder"))
    }

    // --- the change and recovery domain of v0.6 §48 -------------------------------------------
    //
    // Defaulted, all four, for the reason the temporal six are: a host that plans no changes
    // answers "I serve none" rather than failing to compile, and every existing implementation
    // keeps working unchanged.
    //
    // Four methods rather than eleven, because the eleven calls of `protocol.v1.yaml` are eleven
    // *capability* boundaries and not eleven services. What separates them is what the broker
    // checks and what the audit records, which is the supervisor's work; what reaches the host is
    // "read a plan", "add to a plan", "report a recovery fact" and "report a verification".

    /// `change.plan.read`: the `ono.change-plan/1` record for one plan (v0.6 §48.3, §5.1).
    ///
    /// §48.4: reading is where a package that describes impact stops. Nothing a host returns
    /// here is an instruction, and there is no companion call that executes what it describes.
    async fn change_plan_read(&self, _plan: &str) -> Result<Json, HostError> {
        Err(HostError::unavailable("change plans"))
    }

    /// `change.plan.contribute`: actions, effects, impact edges and risk findings the package
    /// adds to a plan (v0.6 §48.2, §48.3).
    ///
    /// The contribution has already been validated when this is called: every effect names a
    /// domain, a kind and a confidence of v0.6's own vocabulary, and every risk finding names a
    /// rule the package declared and a class that rule may emit. What is left is composing it
    /// into the plan, and §19.2 fixes how: the class is the strongest any rule found, so a host
    /// implementation folds with a maximum and has no operation that lowers one.
    async fn change_plan_contribute(
        &self,
        _package: &str,
        _contribution: Json,
    ) -> Result<Json, HostError> {
        Err(HostError::unavailable("a change planner to contribute to"))
    }

    /// The `recovery.*` calls: one fact a recovery provider reports, named by `call` (§12.1).
    ///
    /// `call` is the protocol call id — `recovery.discover`, `recovery.restore` — so a host that
    /// records provider activity records which operation it was without the supervisor having to
    /// project seven near-identical methods onto it. The capability behind each is checked and
    /// audited before this is reached.
    async fn recovery_report(
        &self,
        _package: &str,
        _call: &str,
        _report: Json,
    ) -> Result<Json, HostError> {
        Err(HostError::unavailable("recovery providers"))
    }

    /// `verification.observe`: the result of observing one verification contract (§23, §25.1).
    ///
    /// §23.5 forbids treating an unanswerable check as success, and §25.3 forbids a scopeless
    /// claim that recovery worked. Both are properties of what the package reported, which the
    /// supervisor has already settled against the vocabulary before calling this.
    async fn verification_observe(&self, _package: &str, _result: Json) -> Result<Json, HostError> {
        Err(HostError::unavailable("change verification"))
    }
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
