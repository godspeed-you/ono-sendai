//! A mount, the device and filesystem behind it, and the processes on it (spec §22.2, §22.3,
//! ADR-0079).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_provider_api::{Availability, Capability, ProviderRegistry, Risk};
use ono_value::{ErrorValue, Value};

use crate::graph::Node;
use crate::kernel::lookup::{self, SharedSnapshots};
use crate::kernel::process::missing_field;
use crate::kernel::procfs;
use crate::provider::{Relationship, RelationshipProvider, Relationships};

/// The schema of the objects the mount relationship providers expand.
const MOUNT: &str = "ono.mount/1";

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
        &[MOUNT]
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

/// The filesystem mounted at a mount point: the `ono.filesystem/1` record with the same target.
///
/// Exact: both records come from the same line of `/proc/self/mountinfo`, so the relationship
/// is the kernel's own bookkeeping rather than a guess. A mount whose filesystem the provider
/// cannot describe contributes nothing — absence, not a failed read (spec §10.5).
#[derive(Debug)]
pub struct MountFilesystems {
    registry: Arc<ProviderRegistry>,
    snapshots: Arc<SharedSnapshots>,
}

impl MountFilesystems {
    /// A provider reading through `registry`'s filesystem provider.
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
impl RelationshipProvider for MountFilesystems {
    fn id(&self) -> &str {
        "linux.mount-filesystems"
    }

    fn subjects(&self) -> &[&str] {
        &[MOUNT]
    }

    fn relations(&self) -> &[&str] {
        &["filesystem"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("filesystem.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(target) = subject.field("target").cloned() else {
            return Relationships::failed(missing_field(subject, "target"));
        };
        let filesystem = match self
            .snapshots
            .one(&self.registry, "filesystem", "target", &target)
            .await
        {
            Ok(found) => found,
            Err(error) => return Relationships::failed(error),
        };
        let mut found = Relationships::new();
        if let Some(node) = filesystem.as_deref().and_then(Node::of) {
            found.push(Relationship::exact(subject, node, "filesystem", self.id()));
        }
        found
    }
}

/// The processes using a mount: those whose root or working directory lies on it.
///
/// A path lies on the mount whose target is its longest prefix among all mounts — the same
/// rule the kernel resolves paths by — so a process working in `/home/x` on a separate `/home`
/// mount is not a user of `/`. Only processes in this shell's mount namespace are considered:
/// a path read from another namespace names something else, and an edge across that line
/// would be an invention (spec §22.4). Processes whose links this user may not read are
/// counted and reported once, as [`SocketOwners`](super::SocketOwners) does.
#[derive(Debug)]
pub struct MountUsers {
    registry: Arc<ProviderRegistry>,
    root: PathBuf,
    snapshots: Arc<SharedSnapshots>,
}

impl MountUsers {
    /// A provider reading the running system's `/proc`.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            root: PathBuf::from("/"),
            snapshots: Arc::default(),
        }
    }

    /// Reads `<root>/proc` instead.
    #[must_use]
    pub fn rooted(mut self, root: impl AsRef<Path>) -> Self {
        self.root = root.as_ref().to_path_buf();
        self
    }

    pub(crate) fn sharing(mut self, snapshots: Arc<SharedSnapshots>) -> Self {
        self.snapshots = snapshots;
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for MountUsers {
    fn id(&self) -> &str {
        "linux.mount-users"
    }

    fn subjects(&self) -> &[&str] {
        &[MOUNT]
    }

    fn relations(&self) -> &[&str] {
        &["root", "cwd"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("process.list", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        super::process::proc_availability(&self.root)
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(target) = subject.text("target") else {
            return Relationships::failed(missing_field(subject, "target"));
        };
        let target = PathBuf::from(target);
        let (mounts, _) = match self.snapshots.all(&self.registry, "mount").await {
            Ok(found) => found,
            Err(error) => return Relationships::failed(error),
        };
        let targets: Vec<PathBuf> = mounts
            .iter()
            .filter_map(|mount| match mount.get("target") {
                Some(Value::Path(path)) => Some(path.to_path_buf()),
                _ => None,
            })
            .collect();

        let proc = procfs::proc_dir(&self.root);
        let pids = match procfs::pids(&proc) {
            Ok(pids) => pids,
            Err(error) => return Relationships::failed(error),
        };
        let own_namespace = procfs::link_target(&proc, "self", "ns/mnt").ok();

        let mut found = Relationships::new();
        let mut hidden = 0usize;
        for pid in pids {
            // A process in another mount namespace sees other paths; comparing its links with
            // this namespace's mount table would relate the wrong objects.
            if own_namespace.is_some()
                && procfs::link_target(&proc, &pid.to_string(), "ns/mnt").ok() != own_namespace
            {
                continue;
            }
            for relation in ["root", "cwd"] {
                let path = match procfs::link_target(&proc, &pid.to_string(), relation) {
                    Ok(path) => path,
                    Err(error) if error.code() == ErrorCode::IoPermissionDenied => {
                        hidden += 1;
                        break;
                    }
                    // A process that exited during the scan has nothing left to relate.
                    Err(_) => continue,
                };
                if owning_mount(&targets, &path) != Some(target.as_path()) {
                    continue;
                }
                match self
                    .snapshots
                    .one(
                        &self.registry,
                        "process",
                        "pid",
                        &Value::Int(i128::from(pid)),
                    )
                    .await
                {
                    Ok(Some(record)) => {
                        if let Some(node) = Node::of(&record) {
                            found.push(
                                Relationship::exact(subject, node, relation, self.id())
                                    .with_metadata("path", Value::Path(Arc::from(path))),
                            );
                        }
                    }
                    Ok(None) => {}
                    Err(error) => found.fail(error),
                }
            }
        }
        if hidden > 0 {
            found.fail(
                ErrorValue::new(
                    ErrorCode::IoPermissionDenied,
                    format!("{hidden} process(es) did not let their root or cwd be read"),
                )
                .with_help(
                    "users of this mount may be among them; running with more privilege \
                     would show them",
                ),
            );
        }
        found
    }
}

/// The mount a path lies on: the mount target that is its longest prefix.
fn owning_mount<'a>(targets: &'a [PathBuf], path: &Path) -> Option<&'a Path> {
    targets
        .iter()
        .filter(|target| path.starts_with(target))
        .max_by_key(|target| target.as_os_str().len())
        .map(PathBuf::as_path)
}
