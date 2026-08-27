//! What a route depends on and what is bound to an interface (spec §22.3, ADR-0080).
//!
//! Every relationship here is read from the same rtnetlink and sock_diag dumps the object
//! providers answer `get` from, so a graph cannot disagree with `get route` about the machine.
//! One edge is inferred and says so: a socket bound to the unspecified address is bound to
//! every interface's addresses, which is a consequence rather than an observation.

use std::net::IpAddr;
use std::sync::Arc;

use ono_provider_api::{Capability, ProviderRegistry, Risk};
use ono_value::{RecordValue, Value};

use crate::graph::Node;
use crate::kernel::lookup::SharedSnapshots;
use crate::kernel::process::missing_field;
use crate::provider::{Relationship, RelationshipProvider, Relationships};

/// The schema of the objects the route relationship provider expands.
const ROUTE: &str = "ono.route/1";
/// The schema of the objects the interface relationship providers expand.
const INTERFACE: &str = "ono.interface/1";

/// The interface a route leaves through, and the neighbour that is its gateway.
///
/// Exact: the route names its interface, and the neighbour table is the kernel's own record of
/// the gateway's link-layer address. A gateway the neighbour table has not resolved contributes
/// no edge — there is no object to point at, and inventing one would be exactly what spec §22.4
/// forbids.
#[derive(Debug)]
pub struct RouteInterfaces {
    registry: Arc<ProviderRegistry>,
    snapshots: Arc<SharedSnapshots>,
}

impl RouteInterfaces {
    /// A provider reading through `registry`'s interface and neighbour providers.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            snapshots: Arc::default(),
        }
    }

    pub(crate) fn sharing(mut self, snapshots: Arc<SharedSnapshots>) -> Self {
        self.snapshots = snapshots;
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for RouteInterfaces {
    fn id(&self) -> &str {
        "linux.route-interfaces"
    }

    fn subjects(&self) -> &[&str] {
        &[ROUTE]
    }

    fn relations(&self) -> &[&str] {
        &["via", "gateway"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("interface.list", Risk::Read),
            Capability::new("neighbor.list", Risk::Read),
        ]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(interface) = subject.text("interface") else {
            return Relationships::failed(missing_field(subject, "interface"));
        };
        let mut found = Relationships::new();
        match self
            .snapshots
            .one(
                &self.registry,
                "interface",
                "name",
                &Value::String(interface.as_str().into()),
            )
            .await
        {
            Ok(Some(record)) => {
                if let Some(node) = Node::of(&record) {
                    found.push(Relationship::exact(subject, node, "via", self.id()));
                }
            }
            Ok(None) => {}
            Err(error) => found.fail(error),
        }

        let Some(Value::Ip(gateway)) = subject.field("gateway") else {
            return found;
        };
        match self.snapshots.all(&self.registry, "neighbor").await {
            Ok((neighbors, failures)) => {
                for failure in failures {
                    found.fail(failure);
                }
                let neighbor = neighbors.iter().find(|record| {
                    record.get("address") == Some(&Value::Ip(*gateway))
                        && text(record, "interface").as_deref() == Some(interface.as_str())
                });
                if let Some(node) = neighbor.and_then(|record| Node::of(record)) {
                    found.push(Relationship::exact(subject, node, "gateway", self.id()));
                }
            }
            Err(error) => found.fail(error),
        }
        found
    }
}

/// The routes over an interface and the neighbours reached through it, in every table.
#[derive(Debug)]
pub struct InterfaceRoutes {
    registry: Arc<ProviderRegistry>,
    snapshots: Arc<SharedSnapshots>,
}

impl InterfaceRoutes {
    /// A provider reading through `registry`'s route and neighbour providers.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            snapshots: Arc::default(),
        }
    }

    pub(crate) fn sharing(mut self, snapshots: Arc<SharedSnapshots>) -> Self {
        self.snapshots = snapshots;
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for InterfaceRoutes {
    fn id(&self) -> &str {
        "linux.interface-routes"
    }

    fn subjects(&self) -> &[&str] {
        &[INTERFACE]
    }

    fn relations(&self) -> &[&str] {
        &["route", "neighbor"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("route.list", Risk::Read),
            Capability::new("neighbor.list", Risk::Read),
        ]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(name) = subject.text("name") else {
            return Relationships::failed(missing_field(subject, "name"));
        };
        let mut found = Relationships::new();
        for (target, relation) in [("route", "route"), ("neighbor", "neighbor")] {
            let (records, failures) = match self.snapshots.all(&self.registry, target).await {
                Ok(found) => found,
                Err(error) => {
                    found.fail(error);
                    continue;
                }
            };
            for failure in failures {
                found.fail(failure);
            }
            let mut nodes: Vec<Node> = records
                .iter()
                .filter(|record| text(record, "interface").as_deref() == Some(name.as_str()))
                .filter_map(|record| Node::of(record))
                .collect();
            // Identity order, so two traces of one machine draw the same graph.
            nodes.sort_by_key(|node| node.id().to_string());
            for node in nodes {
                found.push(Relationship::exact(subject, node, relation, self.id()));
            }
        }
        found
    }
}

/// The sockets bound to an interface's addresses.
///
/// A socket bound to one of the interface's addresses is bound to the interface: exact. A
/// socket bound to the unspecified address (`0.0.0.0`, `::`) is reachable through every
/// interface, which is a consequence of how binding works rather than something the kernel
/// records per interface — so that edge is inferred, and its evidence says why.
#[derive(Debug)]
pub struct InterfaceSockets {
    registry: Arc<ProviderRegistry>,
    snapshots: Arc<SharedSnapshots>,
}

impl InterfaceSockets {
    /// A provider reading through `registry`'s socket provider.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            snapshots: Arc::default(),
        }
    }

    pub(crate) fn sharing(mut self, snapshots: Arc<SharedSnapshots>) -> Self {
        self.snapshots = snapshots;
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for InterfaceSockets {
    fn id(&self) -> &str {
        "linux.interface-sockets"
    }

    fn subjects(&self) -> &[&str] {
        &[INTERFACE]
    }

    fn relations(&self) -> &[&str] {
        &["bound"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("socket.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(Value::List(addresses)) = subject.field("addresses") else {
            return Relationships::failed(missing_field(subject, "addresses"));
        };
        let addresses: Vec<IpAddr> = addresses
            .iter()
            .filter_map(|value| match value {
                Value::IpNetwork(network) => Some(network.address()),
                Value::Ip(address) => Some(*address),
                _ => None,
            })
            .collect();
        let (sockets, failures) = match self.snapshots.all(&self.registry, "socket").await {
            Ok(found) => found,
            Err(error) => return Relationships::failed(error),
        };

        let mut found = Relationships::new();
        for failure in failures {
            found.fail(failure);
        }
        let mut bound: Vec<(i64, IpAddr, Node)> = sockets
            .iter()
            .filter_map(|record| {
                let address = local_address(record)?;
                let node = Node::of(record)?;
                Some((node.integer("inode")?, address, node))
            })
            .collect();
        bound.sort_by_key(|(inode, _, _)| *inode);
        for (_, address, node) in bound {
            if addresses.contains(&address) {
                found.push(
                    Relationship::exact(subject, node, "bound", self.id())
                        .with_metadata("address", Value::Ip(address)),
                );
            } else if address.is_unspecified() {
                found.push(
                    Relationship::inferred(
                        subject,
                        node,
                        "bound",
                        self.id(),
                        "bound to the unspecified address, which every interface's addresses \
                         fall under",
                    )
                    .with_metadata("address", Value::Ip(address)),
                );
            }
        }
        found
    }
}

/// The address a socket's local endpoint is bound to, when it has one.
fn local_address(socket: &RecordValue) -> Option<IpAddr> {
    match socket.get("local")? {
        Value::Record(endpoint) => match endpoint.get("address")? {
            Value::Ip(address) => Some(*address),
            _ => None,
        },
        _ => None,
    }
}

/// A record's text field, or `None` when it is unknown, failed or absent.
fn text(record: &RecordValue, field: &str) -> Option<String> {
    match record.get(field)? {
        Value::Null | Value::Error(_) => None,
        value => ono_value::canonical_text(value).ok(),
    }
}
