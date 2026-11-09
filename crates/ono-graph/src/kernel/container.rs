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

/// The processes running inside a container, found through the cgroup the engine puts them in
/// (spec §22.2; ADR-0968, #221).
///
/// The engine's record carries no pid, and the shell reads no engine socket here: every process
/// whose `/proc/<pid>/cgroup` names the container's id — `docker-<id>.scope`, `libpod-<id>.scope`,
/// `/docker/<id>` — is one the engine put in that container. The match is on the full id the
/// engine reported, which no other cgroup path can carry.
#[derive(Debug)]
pub struct ContainerProcesses {
    registry: Arc<ProviderRegistry>,
    root: std::path::PathBuf,
}

impl ContainerProcesses {
    /// A provider reading the running system's `/proc`.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            root: std::path::PathBuf::from("/"),
        }
    }

    /// Reads `<root>/proc` instead, which is how a fixture stands in for the kernel.
    #[must_use]
    pub fn rooted(mut self, root: impl AsRef<std::path::Path>) -> Self {
        self.root = root.as_ref().to_path_buf();
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for ContainerProcesses {
    fn id(&self) -> &str {
        "linux.container-processes"
    }

    fn subjects(&self) -> &[&str] {
        &[CONTAINER]
    }

    fn relations(&self) -> &[&str] {
        &["process"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("process.list", Risk::Read)]
    }

    fn availability(&self) -> ono_provider_api::Availability {
        super::process::proc_availability(&self.root)
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(id) = subject.text("id") else {
            return Relationships::failed(super::process::missing_field(subject, "id"));
        };
        let proc = super::procfs::proc_dir(&self.root);
        let pids = match super::procfs::pids(&proc) {
            Ok(pids) => pids,
            Err(error) => return Relationships::failed(error),
        };
        let mut found = Relationships::new();
        for pid in pids {
            // A process that exited during the scan, or one whose cgroup this user may not read,
            // has told us nothing about the container; the engine's own view would, and the
            // user can ask it.
            let Ok(cgroups) = super::procfs::cgroups(&proc, pid) else {
                continue;
            };
            let Some(cgroup) = cgroups.iter().find(|path| path.contains(id.as_str())) else {
                continue;
            };
            match super::lookup::process_node(&self.registry, pid).await {
                Ok(node) => found.push(
                    Relationship::exact(subject, node, "process", self.id())
                        .with_metadata("cgroup", Value::string(cgroup)),
                ),
                Err(error) => found.fail(error),
            }
        }
        found
    }
}
