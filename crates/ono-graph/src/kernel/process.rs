//! The process relationships spec §22.2 calls exact: the process tree, the file descriptor
//! table, and the sockets a process holds.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_provider_api::{Availability, Capability, ProviderRegistry, Query, Risk};
use ono_value::{ErrorValue, Value};

use crate::graph::Node;
use crate::kernel::{lookup, procfs};
use crate::provider::{Relationship, RelationshipProvider, Relationships};

/// The schema of the objects the process relationship providers expand.
const PROCESS: &str = "ono.process/1";
/// The schema of the objects the socket relationship providers expand.
const SOCKET: &str = "ono.socket/1";

/// A process's parent and children, read from the process provider (spec §22.2).
///
/// Exact at observation time: both ends come from the same kernel snapshot, and a process that
/// exits a moment later does not make the observation retrospectively wrong.
#[derive(Debug)]
pub struct ProcessTree {
    registry: Arc<ProviderRegistry>,
}

impl ProcessTree {
    /// A provider reading through `registry`'s process provider.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for ProcessTree {
    fn id(&self) -> &str {
        "linux.process-tree"
    }

    fn subjects(&self) -> &[&str] {
        &[PROCESS]
    }

    fn relations(&self) -> &[&str] {
        &["parent", "child"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("process.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(pid) = subject.integer("pid") else {
            return Relationships::failed(missing_field(subject, "pid"));
        };
        let (records, failures) =
            match lookup::find(&self.registry, &Query::target("process")).await {
                Ok(found) => found,
                Err(error) => return Relationships::failed(error),
            };

        let mut nodes: Vec<(i64, Option<i64>, Node)> = records
            .iter()
            .filter_map(|record| {
                let node = Node::of(record)?;
                Some((node.integer("pid")?, node.integer("ppid"), node))
            })
            .collect();
        nodes.sort_by_key(|(pid, _, _)| *pid);

        let mut found = Relationships::new();
        for failure in failures {
            found.fail(failure);
        }
        let parent_of_subject = nodes
            .iter()
            .find(|(candidate, _, _)| *candidate == pid)
            .and_then(|(_, ppid, _)| *ppid);
        if let Some(ppid) = parent_of_subject
            && let Some((_, _, parent)) = nodes.iter().find(|(candidate, _, _)| *candidate == ppid)
        {
            found.push(Relationship::exact(
                subject,
                parent.clone(),
                "parent",
                self.id(),
            ));
        }
        for (child_pid, ppid, node) in &nodes {
            if *ppid == Some(pid) && *child_pid != pid {
                found.push(Relationship::exact(
                    subject,
                    node.clone(),
                    "child",
                    self.id(),
                ));
            }
        }
        found
    }
}

/// The files a process has open, read from `/proc/<pid>/fd` (spec §22.2).
#[derive(Debug)]
pub struct OpenFiles {
    registry: Arc<ProviderRegistry>,
    root: PathBuf,
}

impl OpenFiles {
    /// A provider reading the running system's `/proc`.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            root: PathBuf::from("/"),
        }
    }

    /// Reads `<root>/proc` instead, which is how a fixture stands in for the kernel.
    #[must_use]
    pub fn rooted(mut self, root: impl AsRef<Path>) -> Self {
        self.root = root.as_ref().to_path_buf();
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for OpenFiles {
    fn id(&self) -> &str {
        "linux.open-files"
    }

    fn subjects(&self) -> &[&str] {
        &[PROCESS]
    }

    fn relations(&self) -> &[&str] {
        &["reads", "writes", "opens"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("process.trace", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        proc_availability(&self.root)
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(pid) = subject.integer("pid") else {
            return Relationships::failed(missing_field(subject, "pid"));
        };
        let proc = procfs::proc_dir(&self.root);
        let descriptors = match procfs::descriptors(&proc, pid) {
            Ok(descriptors) => descriptors,
            // A process that will not show its descriptors has told us nothing about whether it
            // holds files, which is not the same as holding none (spec §10.5).
            Err(error) => return Relationships::failed(error),
        };

        let mut found = Relationships::new();
        for descriptor in descriptors {
            let Some(path) = descriptor.file_path() else {
                continue;
            };
            let node = match lookup::file_node(&self.registry, &path).await {
                Ok(node) => node,
                Err(error) => {
                    found.fail(error);
                    continue;
                }
            };
            let relation = descriptor
                .access
                .map_or("opens", crate::kernel::procfs::Access::relation);
            found.push(
                Relationship::exact(subject, node, relation, self.id())
                    .with_metadata("fd", Value::Int(i128::from(descriptor.fd)))
                    .with_metadata(
                        "access",
                        descriptor
                            .access
                            .map_or(Value::Null, |access| Value::String(access.as_str().into())),
                    ),
            );
        }
        found
    }
}

/// The sockets a process holds, matched to socket objects by inode (spec §22.2).
#[derive(Debug)]
pub struct ProcessSockets {
    registry: Arc<ProviderRegistry>,
    root: PathBuf,
}

impl ProcessSockets {
    /// A provider reading the running system's `/proc`.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            root: PathBuf::from("/"),
        }
    }

    /// Reads `<root>/proc` instead.
    #[must_use]
    pub fn rooted(mut self, root: impl AsRef<Path>) -> Self {
        self.root = root.as_ref().to_path_buf();
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for ProcessSockets {
    fn id(&self) -> &str {
        "linux.process-sockets"
    }

    fn subjects(&self) -> &[&str] {
        &[PROCESS]
    }

    fn relations(&self) -> &[&str] {
        &["listens", "connects"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("socket.trace", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        proc_availability(&self.root)
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(pid) = subject.integer("pid") else {
            return Relationships::failed(missing_field(subject, "pid"));
        };
        let proc = procfs::proc_dir(&self.root);
        let descriptors = match procfs::descriptors(&proc, pid) {
            Ok(descriptors) => descriptors,
            Err(error) => return Relationships::failed(error),
        };

        let mut found = Relationships::new();
        for descriptor in descriptors {
            let Some(inode) = descriptor.socket_inode() else {
                continue;
            };
            let node = match lookup::socket_node(&self.registry, inode).await {
                Ok(node) => node,
                Err(error) => {
                    found.fail(error);
                    continue;
                }
            };
            let relation = if node.text("state").as_deref() == Some("listen") {
                "listens"
            } else {
                "connects"
            };
            found.push(
                Relationship::exact(subject, node, relation, self.id())
                    .with_metadata("fd", Value::Int(i128::from(descriptor.fd)))
                    .with_metadata("inode", Value::Int(i128::from(inode))),
            );
        }
        found
    }
}

/// The process owning a socket: the inverse of [`ProcessSockets`], found by the same inode.
#[derive(Debug)]
pub struct SocketOwners {
    registry: Arc<ProviderRegistry>,
    root: PathBuf,
}

impl SocketOwners {
    /// A provider reading the running system's `/proc`.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            root: PathBuf::from("/"),
        }
    }

    /// Reads `<root>/proc` instead.
    #[must_use]
    pub fn rooted(mut self, root: impl AsRef<Path>) -> Self {
        self.root = root.as_ref().to_path_buf();
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for SocketOwners {
    fn id(&self) -> &str {
        "linux.socket-owners"
    }

    fn subjects(&self) -> &[&str] {
        &[SOCKET]
    }

    fn relations(&self) -> &[&str] {
        &["owner"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("socket.trace", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        proc_availability(&self.root)
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(inode) = subject.integer("inode") else {
            return Relationships::failed(missing_field(subject, "inode"));
        };
        let proc = procfs::proc_dir(&self.root);
        let pids = match procfs::pids(&proc) {
            Ok(pids) => pids,
            Err(error) => return Relationships::failed(error),
        };

        let wanted = format!("socket:[{inode}]");
        let mut found = Relationships::new();
        let mut hidden = 0usize;
        for pid in pids {
            let descriptors = match procfs::descriptors(&proc, pid) {
                Ok(descriptors) => descriptors,
                Err(error) if error.code() == ErrorCode::IoPermissionDenied => {
                    hidden += 1;
                    continue;
                }
                // A process that exited during the scan simply has no descriptors to report.
                Err(_) => continue,
            };
            for descriptor in descriptors {
                if descriptor.target != wanted {
                    continue;
                }
                match lookup::process_node(&self.registry, pid).await {
                    Ok(node) => found.push(
                        Relationship::exact(subject, node, "owner", self.id())
                            .with_metadata("fd", Value::Int(i128::from(descriptor.fd)))
                            .with_metadata("inode", Value::Int(i128::from(inode))),
                    ),
                    Err(error) => found.fail(error),
                }
            }
        }
        // One line about the whole scan rather than one per process: on a shared machine most
        // processes belong to somebody else, and a hundred identical errors would bury the
        // owners that *were* found. Saying nothing is still not an option — the socket may have
        // an owner this user cannot see.
        if hidden > 0 {
            found.fail(
                ErrorValue::new(
                    ErrorCode::IoPermissionDenied,
                    format!("{hidden} process(es) did not let their open files be read"),
                )
                .with_help(
                    "the owner of this socket may be among them; running with more privilege \
                     would show it",
                ),
            );
        }
        found
    }
}

/// Whether a `/proc` is there to be read at all.
pub(crate) fn proc_availability(root: &Path) -> Availability {
    let proc = procfs::proc_dir(root);
    if proc.is_dir() {
        Availability::Available
    } else {
        Availability::unavailable(format!(
            "{} is not mounted, so kernel relationships cannot be read",
            proc.display()
        ))
    }
}

/// The error for a node that does not carry the field a relationship is read from.
pub(crate) fn missing_field(subject: &Node, field: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderSchemaViolation,
        format!(
            "{} has no readable `{field}`, so its relationships cannot be followed",
            subject.label()
        ),
    )
}
