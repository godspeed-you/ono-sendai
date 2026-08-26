//! The one relationship in this crate that is *not* an observation: the host behind a peer
//! address (spec §22.2).

use std::net::IpAddr;
use std::sync::Arc;

use ono_provider_api::{Capability, Risk};
use ono_value::{ErrorValue, MapValue, SchemaId, Value};

use crate::graph::Node;
use crate::provider::{Relationship, RelationshipProvider, Relationships};

/// Something that can turn an address back into a name.
///
/// It is a parameter rather than a call to the system resolver, because reverse resolution is
/// network I/O with a policy attached: ADR-0015 keeps name lookups out of the paths that must
/// not block or reach the network, and a test must be able to answer from a table.
#[async_trait::async_trait]
pub trait Resolver: Send + Sync + std::fmt::Debug {
    /// The resolver's id. It becomes part of the evidence on every edge it produces, so a
    /// surprising name can be traced to whatever answered for it.
    fn id(&self) -> &str;

    /// The name `address` resolves back to, or `None` when nothing does.
    ///
    /// # Errors
    ///
    /// Returns a structured error when the lookup itself failed — which is not the same answer
    /// as "this address has no name".
    async fn reverse(&self, address: IpAddr) -> Result<Option<String>, ErrorValue>;
}

/// The host a socket's peer address resolves back to.
///
/// **Inferred, always.** Spec §22.2 requires exact and derived relationships to stay apart, and a
/// reverse lookup is derived: the answer comes from whoever controls the reverse zone, it can be
/// stale, and it can be made to say anything. The edge therefore carries
/// [`Confidence::Inferred`](ono_render::Confidence::Inferred) and the evidence it was derived
/// from — the address and the resolver that answered — so a reader can judge it (spec §31.25).
#[derive(Debug)]
pub struct RemoteHosts {
    resolver: Arc<dyn Resolver>,
}

impl RemoteHosts {
    /// A provider resolving through `resolver`.
    #[must_use]
    pub fn new(resolver: Arc<dyn Resolver>) -> Self {
        Self { resolver }
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for RemoteHosts {
    fn id(&self) -> &str {
        "dns.reverse"
    }

    fn subjects(&self) -> &[&str] {
        &["ono.socket/1", "ono.connection/1"]
    }

    fn relations(&self) -> &[&str] {
        &["resolves-to"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("host.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(remote) = subject.field("remote").and_then(|value| value.as_record().ok()) else {
            return Relationships::new();
        };
        let (Some(Value::Ip(address)), port) = (remote.get("address"), remote.get("port").cloned())
        else {
            return Relationships::new();
        };
        let address = *address;

        let host = match self.resolver.reverse(address).await {
            Ok(Some(host)) => host,
            // No name is an answer, not a failure: an address with no PTR record simply has none.
            Ok(None) => return Relationships::new(),
            Err(error) => return Relationships::failed(error),
        };

        let mut identity = MapValue::new();
        identity.insert("address".into(), Value::Ip(address));
        identity.insert("port".into(), port.clone().unwrap_or(Value::Null));
        let mut summary = identity.clone();
        summary.insert("host".into(), Value::String(host.as_str().into()));

        let node = Node::new(SchemaId::new("ono.endpoint", 1), identity, host.clone())
            .with_summary(summary);
        let evidence = format!(
            "reverse DNS: {address} was answered as {host} by {}",
            self.resolver.id()
        );
        let mut found = Relationships::new();
        found.push(
            Relationship::inferred(subject, node, "resolves-to", self.id(), &evidence)
                .with_metadata("address", Value::Ip(address))
                .with_metadata("resolver", Value::String(self.resolver.id().into())),
        );
        found
    }
}
