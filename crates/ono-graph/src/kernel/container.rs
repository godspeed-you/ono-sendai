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

/// The host paths the engine mounted into a container (spec §9.1; ADR-0974, #221).
///
/// Exact: the mount list is the engine's own bookkeeping, as the image is. Each entry names a
/// host path — a bind's source, a volume's data directory — and the node at the far end is that
/// path as the file provider describes it; where inside the container it appears travels on
/// the edge. A mount with no host path behind it, a `tmpfs`, contributes no edge.
#[derive(Debug)]
pub struct ContainerMounts {
    registry: Arc<ProviderRegistry>,
}

impl ContainerMounts {
    /// A provider resolving host paths through `registry`'s file provider.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for ContainerMounts {
    fn id(&self) -> &str {
        "container.mounts"
    }

    fn subjects(&self) -> &[&str] {
        &[CONTAINER]
    }

    fn relations(&self) -> &[&str] {
        &["mount"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("container.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(Value::List(mounts)) = subject.field("mounts") else {
            return Relationships::new();
        };
        let mut found = Relationships::new();
        for mount in mounts.iter() {
            let Value::Map(mount) = mount else {
                continue;
            };
            let Some(source) = mount.get("source").and_then(|value| value.as_str().ok()) else {
                continue;
            };
            match super::lookup::file_node(&self.registry, std::path::Path::new(source)).await {
                Ok(node) => {
                    let mut edge = Relationship::exact(subject, node, "mount", self.id());
                    for key in ["destination", "type", "read_only", "name"] {
                        if let Some(value) = mount.get(key).filter(|value| !value.is_null()) {
                            edge = edge.with_metadata(key, value.clone());
                        }
                    }
                    found.push(edge);
                }
                Err(error) => found.fail(error),
            }
        }
        found
    }
}

/// The kinds of namespace the kernel links under `/proc/<pid>/ns`, without the `*_for_children`
/// links, which name a namespace no process is in.
const NAMESPACE_KINDS: [&str; 8] = ["cgroup", "ipc", "mnt", "net", "pid", "time", "user", "uts"];

/// Where a container lives in the kernel: the control group and the namespaces its processes are
/// in (spec §9.1; ADR-0974, #221).
///
/// The engine's record names neither. The processes the engine put in the container do — the
/// same cgroup match [`ContainerProcesses`] makes finds them, each one's `/proc/<pid>/cgroup`
/// is the group and its `/proc/<pid>/ns/*` links are the namespaces. The objects at the far end
/// are whatever `get cgroup` and `get namespace` say they are.
#[derive(Debug)]
pub struct ContainerPlacement {
    registry: Arc<ProviderRegistry>,
    root: std::path::PathBuf,
}

impl ContainerPlacement {
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

/// The text a path or string field carries, for comparing a record with what procfs printed.
fn field_text(record: &ono_value::RecordValue, field: &str) -> Option<String> {
    match record.get(field)? {
        Value::Path(path) => Some(path.display().to_string()),
        Value::String(text) => Some(text.to_string()),
        _ => None,
    }
}

/// `net:[4026532281]`, the target of a namespace link, as its inode.
fn namespace_inode(link: &std::path::Path) -> Option<i128> {
    let text = link.to_str()?;
    let (_, inode) = text.split_once(":[")?;
    inode.strip_suffix(']')?.parse().ok()
}

#[async_trait::async_trait]
impl RelationshipProvider for ContainerPlacement {
    fn id(&self) -> &str {
        "linux.container-placement"
    }

    fn subjects(&self) -> &[&str] {
        &[CONTAINER]
    }

    fn relations(&self) -> &[&str] {
        &["cgroup", "namespace"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("cgroup.read", Risk::Read),
            Capability::new("namespace.read", Risk::Read),
        ]
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
        // In the order procfs gave them, each once: a container of forty processes is in one
        // cgroup and one set of namespaces, and the graph says so once.
        let mut groups: Vec<String> = Vec::new();
        let mut namespaces: Vec<(&'static str, i128)> = Vec::new();
        // The kinds a process refused to name, with the kernel's reason for the first refusal.
        let mut unread: Vec<(&'static str, ono_value::ErrorValue)> = Vec::new();
        for pid in pids {
            let Ok(cgroups) = super::procfs::cgroups(&proc, pid) else {
                continue;
            };
            let Some(cgroup) = cgroups.iter().find(|path| path.contains(id.as_str())) else {
                continue;
            };
            if !groups.contains(cgroup) {
                groups.push(cgroup.clone());
            }
            for kind in NAMESPACE_KINDS {
                // A process that exited during the scan has told us nothing. One that refuses
                // the read has: the namespace is there and this user may not see it, which is
                // reported unless another process of the container names that kind.
                let link = match super::procfs::link_target(
                    &proc,
                    &pid.to_string(),
                    &format!("ns/{kind}"),
                ) {
                    Ok(link) => link,
                    Err(error) => {
                        if error.code() != ono_core::ErrorCode::IoNotFound
                            && !unread.iter().any(|(seen, _)| *seen == kind)
                        {
                            unread.push((kind, error));
                        }
                        continue;
                    }
                };
                if let Some(inode) = namespace_inode(&link)
                    && !namespaces.contains(&(kind, inode))
                {
                    namespaces.push((kind, inode));
                }
            }
        }

        let mut found = Relationships::new();
        // §10.5: unread is not absent. One failure stands for every kind the container's
        // processes kept closed — they are closed for one reason.
        if let Some((_, error)) = unread
            .into_iter()
            .find(|(kind, _)| !namespaces.iter().any(|(seen, _)| seen == kind))
        {
            found.fail(error);
        }
        if !groups.is_empty() {
            match super::lookup::find(&self.registry, &Query::target("cgroup")).await {
                Ok((records, _)) => {
                    for group in &groups {
                        let node = records
                            .iter()
                            .find(|record| field_text(record, "path").as_ref() == Some(group))
                            .and_then(|record| Node::of(record));
                        match node {
                            Some(node) => {
                                found.push(Relationship::exact(subject, node, "cgroup", self.id()));
                            }
                            None => found.fail(undescribed(&format!("control group {group}"))),
                        }
                    }
                }
                Err(error) => found.fail(error),
            }
        }
        if !namespaces.is_empty() {
            match super::lookup::find(&self.registry, &Query::target("namespace")).await {
                Ok((records, _)) => {
                    for (kind, inode) in &namespaces {
                        let node = records
                            .iter()
                            .find(|record| {
                                record.get("id") == Some(&Value::Int(*inode))
                                    && field_text(record, "type").as_deref() == Some(kind)
                            })
                            .and_then(|record| Node::of(record));
                        match node {
                            Some(node) => found.push(Relationship::exact(
                                subject,
                                node,
                                "namespace",
                                self.id(),
                            )),
                            None => {
                                found.fail(undescribed(&format!("{kind} namespace {inode}")));
                            }
                        }
                    }
                }
                Err(error) => found.fail(error),
            }
        }
        found
    }
}

/// The failure for a far end procfs named and no provider served.
fn undescribed(what: &str) -> ono_value::ErrorValue {
    ono_value::ErrorValue::new(
        ono_core::ErrorCode::IoNotFound,
        format!("the container is in {what}, and no provider describes it"),
    )
}
