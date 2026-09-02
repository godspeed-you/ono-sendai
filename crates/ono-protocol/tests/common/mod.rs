//! Fixtures shared by the remote-protocol suites.
//!
//! Every helper builds input or asserts an observable outcome. None of them knows how the
//! transport is structured internally (AGENTS.md §11). The transport is always an in-memory
//! duplex, so no suite needs a network, a container or a clock.

#![allow(
    dead_code,
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared test fixture states preconditions the same way a #[test] body does, and \
              each test binary uses a different subset of the helpers"
)]

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use ono_core::ErrorCode;
use ono_protocol::{
    ActRequest, ClientConfig, HostKey, Identity, ProviderDescriptor, RemoteQuery, RemoteService,
    ServerConfig, StreamResponder, TrustPolicy, TrustStore, UnauthenticatedTransport, serve,
};
use ono_provider_api::{ActionOutcome, Capability, ObjectEvent, Risk};
use ono_value::{
    ErrorValue, FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, SchemaRegistry,
    Value,
};

/// A test that waits must fail rather than stall the suite, so every await is bounded.
pub const LIMIT: Duration = Duration::from_secs(20);

/// A throwaway directory for one test, beside the build rather than in the system temp directory.
///
/// `CARGO_TARGET_TMPDIR` is Cargo's own scratch space for integration tests. Using it rather than
/// `/tmp` keeps the trust-store suites from depending on a property of the machine that runs them
/// — a full or quota-bound system temp directory — which AGENTS.md §11 rules out.
#[derive(Debug)]
pub struct Scratch {
    path: std::path::PathBuf,
}

/// Creates a scratch directory unique to this process and call.
pub fn scratch() -> Scratch {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("ono-protocol-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&path)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", path.display()));
    Scratch { path }
}

impl Scratch {
    /// The directory's path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Writes `contents` to `relative` and returns its path.
    pub fn write(&self, relative: &str, contents: impl AsRef<[u8]>) -> std::path::PathBuf {
        let target = self.path.join(relative);
        std::fs::write(&target, contents)
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", target.display()));
        target
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A failure here must not mask the test's own outcome.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Runs `future` under a hard timeout.
pub async fn within<F: Future>(future: F) -> F::Output {
    match tokio::time::timeout(LIMIT, future).await {
        Ok(output) => output,
        Err(_) => panic!("the link did not finish within {LIMIT:?}: it hung"),
    }
}

/// Lets every task that can make progress make it, without touching the clock.
pub async fn settle() {
    for _ in 0..5_000 {
        tokio::task::yield_now().await;
    }
}

/// The schema the streaming suites carry across the link.
pub fn demo_schema() -> Schema {
    Schema::builder(SchemaId::new("ono.test.remote", 1), "RemoteDemo")
        .field(FieldDef::new("pid", FieldType::Int).required())
        .field(FieldDef::new("name", FieldType::String).nullable())
        .identity(["pid"])
        .default_view(["pid", "name"])
        .build()
        .expect("the fixture schema is well formed")
}

/// A registry holding the fixture schema, shared by both ends of a link.
pub fn schemas() -> Arc<SchemaRegistry> {
    let mut registry = SchemaRegistry::new();
    registry
        .register(demo_schema())
        .expect("the fixture schema registers");
    Arc::new(registry)
}

/// A record as a remote provider would produce it.
pub fn remote_record(pid: i128, name: &str) -> RecordValue {
    let schema = schemas()
        .get(&SchemaId::new("ono.test.remote", 1))
        .expect("the fixture schema is registered");
    let provenance = Provenance::remote("demo.provider", "testhost", schema.id().clone())
        .observed_at("2026-08-26T10:00:00Z".parse().expect("a fixed timestamp"));
    RecordValue::builder(schema, provenance)
        .set("pid", Value::Int(pid))
        .expect("pid is a field of the fixture schema")
        .set("name", Value::String(name.into()))
        .expect("name is a field of the fixture schema")
        .build()
}

/// The key the fixture server presents when it authenticates itself.
pub fn server_key() -> HostKey {
    HostKey::new("ed25519", *b"the-fixture-server-public-key---")
}

/// A different key, standing in for the impersonator of ADR-0015 T5/T6.
pub fn impostor_key() -> HostKey {
    HostKey::new("ed25519", *b"a-completely-different-public-ky")
}

/// A client configuration that accepts an unauthenticated transport, for suites about
/// something other than trust.
pub fn client_config(host: &str) -> ClientConfig {
    ClientConfig::new(host)
        .with_schemas(schemas())
        .with_trust_policy(TrustPolicy::Unauthenticated)
        .with_identity(Identity::new("tester"))
}

/// A client configuration that pins host keys into `store`.
pub fn pinning_client_config(host: &str, store: TrustStore) -> ClientConfig {
    ClientConfig::new(host)
        .with_schemas(schemas())
        .with_trust_store(store)
        .with_trust_policy(TrustPolicy::Required)
        .with_identity(Identity::new("tester"))
}

/// The server configuration the fixture service is served with.
pub fn server_config() -> ServerConfig {
    ServerConfig::new()
        .with_schemas(schemas())
        .with_identity(Identity::new("remote-user"))
        .with_provider(
            ProviderDescriptor::new("linux.procfs")
                .with_targets(["process"])
                .with_capabilities(["process.list"])
                .with_capability(
                    &Capability::new("process.signal", Risk::Mutate).needing_elevation(),
                ),
        )
        .with_provider(
            ProviderDescriptor::new("linux.systemd")
                .with_targets(["service"])
                .unavailable("systemd is not running in this container"),
        )
        .with_capabilities(["process.list", "process.signal"])
        .with_compression(["zstd", "none"])
}

/// What the fixture service observed while it ran.
#[derive(Debug, Default)]
pub struct Observed {
    /// How many values the service handed to the responder successfully.
    pub sent: AtomicUsize,
    /// Whether a producer noticed that its stream had gone away.
    pub cancelled: AtomicBool,
}

impl Observed {
    /// How many values were sent so far.
    pub fn sent(&self) -> usize {
        self.sent.load(Ordering::SeqCst)
    }

    /// Whether a producer observed cancellation.
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// The remote side of the fixture link: a handful of targets with predictable behaviour.
#[derive(Debug, Default)]
pub struct DemoService {
    observed: Arc<Observed>,
}

impl DemoService {
    /// A service reporting what it did into `observed`.
    pub fn new(observed: Arc<Observed>) -> Self {
        Self { observed }
    }
}

#[async_trait::async_trait]
impl RemoteService for DemoService {
    async fn query(
        &self,
        query: RemoteQuery,
        responder: &StreamResponder,
    ) -> Result<(), ErrorValue> {
        let base = match query.option_value("base") {
            Some(Value::Int(base)) => *base,
            _ => 0,
        };
        match query.target_name() {
            "demo" => {
                let count = query.max().unwrap_or(3);
                for index in 0..count {
                    if responder
                        .send(Value::Int(base + index as i128))
                        .await
                        .is_err()
                    {
                        return Ok(());
                    }
                    self.observed.sent.fetch_add(1, Ordering::SeqCst);
                }
                Ok(())
            }
            "endless" => {
                let mut index = 0i128;
                loop {
                    if responder.send(Value::Int(index)).await.is_err() {
                        self.observed.cancelled.store(true, Ordering::SeqCst);
                        return Ok(());
                    }
                    self.observed.sent.fetch_add(1, Ordering::SeqCst);
                    index += 1;
                }
            }
            "record" => {
                let _ = responder
                    .send(remote_record(4419, "nginx").into_value())
                    .await;
                Ok(())
            }
            "flaky" => {
                let _ = responder.send(Value::Int(1)).await;
                let _ = responder
                    .fail(ErrorValue::new(
                        ErrorCode::IoPermissionDenied,
                        "one object could not be read",
                    ))
                    .await;
                let _ = responder.send(Value::Int(2)).await;
                Ok(())
            }
            other => Err(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("the remote has no target `{other}`"),
            )),
        }
    }

    async fn subscribe(
        &self,
        _query: RemoteQuery,
        responder: &StreamResponder,
    ) -> Result<(), ErrorValue> {
        let _ = responder
            .send_event(ObjectEvent::snapshot(&remote_record(4419, "nginx")).with_sequence(1))
            .await;
        let _ = responder
            .send_event(ObjectEvent::removed(&remote_record(4419, "nginx")).with_sequence(2))
            .await;
        Ok(())
    }

    async fn act(&self, action: ActRequest) -> Result<ActionOutcome, ErrorValue> {
        let request = action.to_action();
        Ok(ActionOutcome::succeeded(&request, true))
    }
}

/// A connected client and the join handle of the server that answers it.
pub struct Fixture {
    /// The client end of the link.
    pub link: ono_protocol::Link,
    /// What the remote service observed.
    pub observed: Arc<Observed>,
    /// The task running the remote end.
    pub server: tokio::task::JoinHandle<Result<(), ErrorValue>>,
}

/// Connects a client to the fixture service over an in-memory duplex.
pub async fn connect() -> Fixture {
    try_connect(client_config("testhost"), server_config(), None)
        .await
        .expect("the fixture handshake succeeds")
}

/// Attempts a connection, reporting whatever the handshake decided.
///
/// `presents` is the key the *client's* transport says it authenticated about the peer — what a
/// real TLS or Noise transport would report, and what the trust store is asked about.
pub async fn try_connect(
    client: ClientConfig,
    server: ServerConfig,
    presents: Option<HostKey>,
) -> Result<Fixture, ErrorValue> {
    let (near, far) = tokio::io::duplex(16 * 1024);
    let observed = Arc::new(Observed::default());
    let service = DemoService::new(Arc::clone(&observed));
    let handle =
        tokio::spawn(
            async move { serve(UnauthenticatedTransport::new(far), server, service).await },
        );
    let mut transport = UnauthenticatedTransport::new(near);
    if let Some(key) = presents {
        transport = transport.with_peer_key(key);
    }
    let link = within(ono_protocol::Link::connect(transport, client)).await?;
    Ok(Fixture {
        link,
        observed,
        server: handle,
    })
}
