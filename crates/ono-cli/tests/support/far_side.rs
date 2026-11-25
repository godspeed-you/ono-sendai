//! A far agent whose tables the test writes: a real listening agent — TLS, its own key, the
//! authorization a client must pass — serving fixture `connection`, `address` and `kernel`
//! targets instead of this kernel's (review TST-1, ADR-1098).
//!
//! Every real agent on one machine reads the one kernel's socket table, so it always holds the
//! mirror of every connection the shell has: a correlation proven against it cannot fail for the
//! reason it exists. This agent is the network's far side as the test needs it — the outside
//! world faked at the provider boundary, nothing of the shell's own layers (AGENTS.md §11).

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::panic,
    reason = "a shared test fixture states preconditions the same way a #[test] body does"
)]

use std::net::SocketAddr;
use std::str::FromStr as _;
use std::sync::Arc;

use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{Capability, Provider, ProviderRegistry, Query, Risk};
use ono_testkit::{Scratch, scratch};
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, Value};

/// Which kernel the far side says it runs (ADR-0985's boot identity).
#[derive(Debug, Clone, Copy)]
pub enum Boot {
    /// This machine's own boot: the far side is proven to be this machine.
    ThisMachine,
    /// A boot no machine here runs: the far side is another machine.
    Elsewhere,
}

/// What the far side's tables hold.
#[derive(Debug, Clone)]
pub struct Tables {
    /// `(local, remote)` of each established TCP socket the far side reports.
    pub sockets: Vec<(SocketAddr, SocketAddr)>,
    /// How many further, unrelated sockets it reports after `sockets`: enough of them cut the
    /// table at the reader's bound.
    pub padding: usize,
    /// Its boot.
    pub boot: Boot,
}

/// A running far agent.
pub struct FarAgent {
    /// `127.0.0.N:port`, as `link host … --transport tcp` takes it.
    pub address: String,
    /// The loopback host a client pins it under.
    pub host: String,
    /// The fingerprint its key proves.
    pub fingerprint: String,
    _home: Scratch,
    _runtime: tokio::runtime::Runtime,
}

impl FarAgent {
    /// Starts a far agent on `host` that serves `tables` to the client whose home is `client`.
    pub fn start(host: &str, client: &Scratch, tables: Tables) -> Self {
        let home = scratch();
        let identity = ono_remote::PeerIdentity::open_or_create(&home.path().join("key.pem"))
            .expect("a disposable identity");
        let fingerprint = identity.fingerprint().to_string();
        let client = ono_protocol::Fingerprint::from_str(&super::client_fingerprint(client))
            .expect("the client's fingerprint");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime for the far agent");
        let address = runtime.block_on(async {
            let listener = ono_remote::TlsListener::bind(&format!("{host}:0"), &identity)
                .await
                .expect("the far agent binds");
            let mut registry = ProviderRegistry::new();
            registry.register(Arc::new(Connections(tables.clone())));
            registry.register(Arc::new(Addresses));
            registry.register(Arc::new(Kernel(tables.boot)));
            let config = ono_remote::AgentConfig::new(Arc::new(registry));
            let agent = ono_remote::ListeningAgent::new(listener, config)
                .with_authorization_source(move || {
                    Ok(ono_protocol::AuthorizedClients::of([
                        ono_protocol::AuthorizedClient::observing(client),
                    ]))
                });
            let address = agent.local_addr().expect("bound").to_string();
            tokio::spawn(agent.run());
            address
        });
        Self {
            address,
            host: host.to_owned(),
            fingerprint,
            _home: home,
            _runtime: runtime,
        }
    }
}

/// Pins `far`'s key in `home`'s trust store.
pub fn pin(home: &Scratch, far: &FarAgent) {
    super::ono_at_home(
        home,
        &format!(
            "add host-key {} --fingerprint {}",
            far.host, far.fingerprint
        ),
    )
    .assert_success();
}

fn stream(values: Vec<Value>, failure: Option<ErrorValue>) -> ValueStream {
    ValueStream::spawn(
        PipelineConfig::new(),
        Boundedness::Bounded,
        move |sink| async move {
            for value in values {
                if sink.send(value).await.is_err() {
                    return;
                }
            }
            if let Some(failure) = failure {
                let _ = sink.fail(failure).await;
            }
        },
    )
}

#[derive(Debug)]
struct Connections(Tables);

fn endpoint(address: SocketAddr) -> Value {
    let schema = ono_provider_netlink::endpoint_schema();
    let record = RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("test.far-side", schema.id().clone()),
    )
    .set("address", Value::Ip(address.ip()))
    .and_then(|builder| builder.set("port", Value::Port(address.port())))
    .expect("an endpoint record")
    .build();
    Value::Record(Arc::new(record))
}

#[async_trait::async_trait]
impl Provider for Connections {
    fn id(&self) -> &str {
        "test.far-side.connections"
    }

    fn targets(&self) -> &[&str] {
        &["socket", "connection"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![
            ono_provider_netlink::socket_schema(),
            ono_provider_netlink::endpoint_schema(),
        ]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("socket.list", Risk::Read),
            Capability::new("connection.list", Risk::Read),
        ]
    }

    fn snapshot(&self, _query: &Query) -> Result<ValueStream, ErrorValue> {
        let schema = ono_provider_netlink::socket_schema();
        // Unrelated sockets between ports nothing here listens on.
        let padding = (0..self.0.padding).map(|n| {
            let port = u16::try_from(10_000 + n % 50_000).unwrap_or(10_000);
            (
                SocketAddr::from(([127, 0, 0, 99], port)),
                SocketAddr::from(([127, 0, 0, 98], port)),
            )
        });
        let records: Vec<Value> = self
            .0
            .sockets
            .iter()
            .copied()
            .chain(padding)
            .map(|(local, remote)| {
                let record = RecordValue::builder(
                    Arc::clone(&schema),
                    Provenance::local("test.far-side", schema.id().clone()),
                )
                .set("protocol", Value::string("tcp"))
                .and_then(|b| b.set("family", Value::string("inet")))
                .and_then(|b| b.set("local", endpoint(local)))
                .and_then(|b| b.set("remote", endpoint(remote)))
                .and_then(|b| b.set("state", Value::string("established")))
                .expect("a socket record")
                .build();
                Value::Record(Arc::new(record))
            })
            .collect();
        Ok(stream(records, None))
    }
    async fn resolve(
        &self,
        _selector: &ono_provider_api::Selector,
    ) -> Result<Vec<ono_provider_api::ObjectRef>, ErrorValue> {
        Ok(Vec::new())
    }
}

#[derive(Debug)]
struct Addresses;

#[async_trait::async_trait]
impl Provider for Addresses {
    fn id(&self) -> &str {
        "test.far-side.addresses"
    }

    fn targets(&self) -> &[&str] {
        &["address"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![ono_provider_netlink::address_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("address.list", Risk::Read)]
    }

    fn snapshot(&self, _query: &Query) -> Result<ValueStream, ErrorValue> {
        // The far side's interfaces carry only addresses every host has; none is evidence.
        Ok(stream(Vec::new(), None))
    }
    async fn resolve(
        &self,
        _selector: &ono_provider_api::Selector,
    ) -> Result<Vec<ono_provider_api::ObjectRef>, ErrorValue> {
        Ok(Vec::new())
    }
}

#[derive(Debug)]
struct Kernel(Boot);

#[async_trait::async_trait]
impl Provider for Kernel {
    fn id(&self) -> &str {
        "test.far-side.kernel"
    }

    fn targets(&self) -> &[&str] {
        &["kernel"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        ono_provider_linux::KernelProvider::default().schemas()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("kernel.read", Risk::Read)]
    }

    fn snapshot(&self, _query: &Query) -> Result<ValueStream, ErrorValue> {
        let boot = match self.0 {
            Boot::ThisMachine => std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .expect("this machine's boot id")
                .trim()
                .to_owned(),
            Boot::Elsewhere => "00000000-0000-4000-8000-000000000000".to_owned(),
        };
        let schema = self
            .schemas()
            .into_iter()
            .next()
            .expect("the kernel schema");
        let record = RecordValue::builder(
            Arc::clone(&schema),
            Provenance::local("test.far-side", schema.id().clone()),
        )
        .set("boot_id", Value::string(&boot))
        .expect("a kernel record")
        .build();
        Ok(stream(vec![Value::Record(Arc::new(record))], None))
    }
    async fn resolve(
        &self,
        _selector: &ono_provider_api::Selector,
    ) -> Result<Vec<ono_provider_api::ObjectRef>, ErrorValue> {
        Ok(Vec::new())
    }
}
