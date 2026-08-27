//! A container and the image it was created from — the "image relation" of spec §9.1's `trace
//! container`.

use std::sync::Arc;

use ono_provider_api::{Capability, ProviderRegistry, Query, Risk, Selector};
use ono_value::Value;

use crate::graph::Node;
use crate::provider::{Relationship, RelationshipProvider, Relationships};

const CONTAINER: &str = "ono.container/1";

/// The image a container runs: the `ono.image/1` whose digest the engine reported for it.
///
/// Exact: the engine records the image a container was created from, so the relationship is
/// the runtime's own bookkeeping and not a guess from the reference's name (ADR-0114). A
/// container whose image the engine no longer holds contributes nothing — absence, not a
/// failed read (spec §10.5).
#[derive(Debug)]
pub struct ContainerImage {
    registry: Arc<ProviderRegistry>,
}

impl ContainerImage {
    /// A provider resolving images through `registry`'s container provider.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for ContainerImage {
    fn id(&self) -> &str {
        "container.image"
    }

    fn subjects(&self) -> &[&str] {
        &[CONTAINER]
    }

    fn relations(&self) -> &[&str] {
        &["image"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("image.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        // The digest identifies the image; the reference is the fallback for an engine that
        // listed only the tag. Neither is a container the engine reported no image for.
        let query = match (subject.text("image_id"), subject.text("image")) {
            (Some(digest), _) => {
                Query::target("image").with(Selector::field("id", Value::string(&digest)))
            }
            (None, Some(reference)) => {
                Query::target("image").with(Selector::field("reference", Value::string(&reference)))
            }
            (None, None) => return Relationships::new(),
        };
        match super::lookup::one(&self.registry, &query).await {
            Ok(Some(record)) => {
                let Some(image) = Node::of(&record) else {
                    return Relationships::new();
                };
                let mut found = Relationships::new();
                found.push(Relationship::exact(subject, image, "image", self.id()));
                found
            }
            Ok(None) => Relationships::new(),
            Err(error) => Relationships::failed(error),
        }
    }
}
