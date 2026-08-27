//! A host, the links this session holds to it, and what each link negotiated (spec §21.2,
//! §22.2, §33.4; ADR-0105).
//!
//! These read the session's own bookkeeping — the `link` and `host` tables the shell publishes
//! for its `ono.shell` provider — rather than the kernel, and they are exact for the same
//! reason a process's parent is: the shell observed the handshake itself.

use std::sync::Arc;

use ono_provider_api::{Capability, ProviderRegistry, Query, Risk, Selector};
use ono_value::{MapValue, SchemaId, Value};

use crate::graph::Node;
use crate::kernel::lookup;
use crate::kernel::process::missing_field;
use crate::provider::{Relationship, RelationshipProvider, Relationships};

/// The schema of a host.
const HOST: &str = "ono.host/1";
/// The schema of a link.
const LINK: &str = "ono.link/1";

/// The links this session holds to a host.
///
/// Exact: a link record names the host it points at, and the shell made the link.
#[derive(Debug)]
pub struct HostLinks {
    registry: Arc<ProviderRegistry>,
}

impl HostLinks {
    /// A provider reading the link table through `registry`'s `ono.shell`.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for HostLinks {
    fn id(&self) -> &str {
        "ono.host-links"
    }

    fn subjects(&self) -> &[&str] {
        &[HOST]
    }

    fn relations(&self) -> &[&str] {
        &["link"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("link.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(name) = subject.field("name").cloned() else {
            return Relationships::failed(missing_field(subject, "name"));
        };
        let query = Query::target("link").with(Selector::field("host", name));
        let (links, failures) = match lookup::find(&self.registry, &query).await {
            Ok(found) => found,
            Err(error) => return Relationships::failed(error),
        };
        let mut found = Relationships::new();
        for failure in failures {
            found.fail(failure);
        }
        for link in links {
            if let Some(node) = Node::of(&link) {
                let transport = link.get("transport").cloned().unwrap_or(Value::Null);
                found.push(
                    Relationship::exact(subject, node, "link", self.id())
                        .with_metadata("transport", transport),
                );
            }
        }
        found
    }
}

/// What a link negotiated: the providers the far side offers (spec §21.2), which keep their
/// ids across the link (ADR-0036).
///
/// Exact: the ids are the handshake's own answer, recorded on the link. A provider is a node
/// of `ono.provider/1`, identified by its id; a link that was never established offers
/// nothing, which is absence rather than a failed read (spec §10.5).
#[derive(Debug)]
pub struct LinkProviders;

impl LinkProviders {
    /// A provider reading the negotiated provider ids off the link record itself.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinkProviders {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for LinkProviders {
    fn id(&self) -> &str {
        "ono.link-providers"
    }

    fn subjects(&self) -> &[&str] {
        &[LINK]
    }

    fn relations(&self) -> &[&str] {
        &["offers"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("link.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(Value::List(providers)) = subject.field("providers") else {
            return Relationships::new();
        };
        let mut found = Relationships::new();
        for id in providers.iter().filter_map(|id| id.as_str().ok()) {
            let mut identity = MapValue::new();
            identity.insert("id".into(), Value::string(id));
            let node = Node::new(SchemaId::new("ono.provider", 1), identity.clone(), id)
                .with_summary(identity);
            found.push(Relationship::exact(subject, node, "offers", self.id()));
        }
        found
    }
}
