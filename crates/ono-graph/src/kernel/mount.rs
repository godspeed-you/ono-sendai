//! A mount and the device behind it (spec §22.2).

use std::path::Path;
use std::sync::Arc;

use ono_provider_api::{Capability, ProviderRegistry, Risk};
use ono_value::Value;

use crate::graph::Node;
use crate::kernel::lookup;
use crate::kernel::process::missing_field;
use crate::provider::{Relationship, RelationshipProvider, Relationships};

/// The device a mount is backed by.
///
/// Exact: the source comes from the kernel's own mount table, and the device object is whatever
/// the file provider says lives at that path. A filesystem with no device — `tmpfs`, `proc`, an
/// overlay, a network share — contributes no edge at all, because it genuinely has none. That is
/// absence, not a failed read, and the two must not look alike (spec §10.5).
#[derive(Debug)]
pub struct MountDevices {
    registry: Arc<ProviderRegistry>,
}

impl MountDevices {
    /// A provider resolving devices through `registry`'s file provider.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for MountDevices {
    fn id(&self) -> &str {
        "linux.mount-devices"
    }

    fn subjects(&self) -> &[&str] {
        &["ono.mount/1"]
    }

    fn relations(&self) -> &[&str] {
        &["backed-by"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("mount.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(source) = subject.text("source") else {
            return Relationships::failed(missing_field(subject, "source"));
        };
        let path = Path::new(&source);
        if !path.is_absolute() || !source.starts_with("/dev/") {
            return Relationships::new();
        }

        match lookup::file_node(&self.registry, path).await {
            Ok(node) => {
                let mut found = Relationships::new();
                found.push(
                    Relationship::exact(subject, node, "backed-by", self.id())
                        .with_metadata("source", Value::String(source.as_str().into())),
                );
                found
            }
            Err(error) => Relationships::failed(error),
        }
    }
}
