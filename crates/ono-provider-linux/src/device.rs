//! The `device` target (spec §8.1, §9.1): the block and character device nodes under `/dev`.
//!
//! A device is identified by what the kernel says about its node — `stat(2)` on `/dev/sda2`
//! yields the node's type and its major/minor pair — and described further by what sysfs shows
//! under `/sys/dev/{block,char}/<major>:<minor>`: the size of a block device and the subsystem
//! it belongs to. Nothing is parsed from `lsblk`, `udevadm` or any other program's output
//! (spec §50, AGENTS.md §6); `/dev` and `/sys` are the kernel's own interfaces.
//!
//! Symlinks are skipped, so `/dev/disk/by-uuid/…` does not duplicate `/dev/sda2`; every record
//! is one real node, and the path is its identity. A device sysfs knows but `/dev` has no node
//! for is not listed — `get device` enumerates the nodes under `/dev`, as its contract says.

use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamSink, ValueStream};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
use ono_value::{ByteSize, ErrorValue, RecordValue, Schema, Value};

use crate::common::{io_error, provenance};
use crate::schemas;

/// The provider's stable id, as it appears in every record's provenance.
pub const PROVIDER_ID: &str = "linux.sysfs";

/// Whether a node is a block or a character device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceKind {
    Block,
    Char,
}

impl DeviceKind {
    fn name(self) -> &'static str {
        match self {
            DeviceKind::Block => "block",
            DeviceKind::Char => "char",
        }
    }

    /// The directory of `/sys/dev` that indexes this kind by `major:minor`.
    fn sysfs_index(self) -> &'static str {
        match self {
            DeviceKind::Block => "block",
            DeviceKind::Char => "char",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "block" => Some(DeviceKind::Block),
            "char" | "character" => Some(DeviceKind::Char),
            _ => None,
        }
    }
}

/// One device node, as `stat(2)` and sysfs describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceNode {
    pub(crate) path: PathBuf,
    pub(crate) kind: DeviceKind,
    pub(crate) major: u64,
    pub(crate) minor: u64,
    pub(crate) size: Option<u128>,
    pub(crate) subsystem: Option<String>,
}

/// Block and character devices.
#[derive(Debug)]
pub struct DeviceProvider {
    dev: PathBuf,
    sys_dev: PathBuf,
}

impl Default for DeviceProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceProvider {
    /// The devices of the machine this shell runs on.
    #[must_use]
    pub fn new() -> Self {
        Self::rooted("/")
    }

    /// The devices whose nodes live under `root/dev` and whose sysfs index is `root/sys/dev`.
    ///
    /// The paths in the records are the ones under `root`; a fixture root is a different
    /// machine, not this one seen through a prefix.
    #[must_use]
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            dev: root.join("dev"),
            sys_dev: root.join("sys/dev"),
        }
    }

    /// Every device node under `/dev`, in path order.
    fn nodes(&self) -> Result<Vec<DeviceNode>, ErrorValue> {
        let mut found = Vec::new();
        let mut pending = vec![self.dev.clone()];
        while let Some(directory) = pending.pop() {
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                // `/dev` itself missing is a failed read; a subdirectory this user may not list
                // is the everyday case and costs nothing but the devices behind it.
                Err(error) if directory == self.dev => return Err(io_error(&error, &directory)),
                Err(_) => continue,
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                let Ok(metadata) = fs::symlink_metadata(&path) else {
                    continue;
                };
                let file_type = metadata.file_type();
                if file_type.is_symlink() {
                    continue;
                }
                if file_type.is_dir() {
                    pending.push(path);
                    continue;
                }
                let kind = if file_type.is_block_device() {
                    DeviceKind::Block
                } else if file_type.is_char_device() {
                    DeviceKind::Char
                } else {
                    continue;
                };
                let rdev = metadata.rdev();
                let major = nix::sys::stat::major(rdev);
                let minor = nix::sys::stat::minor(rdev);
                let sysfs = self
                    .sys_dev
                    .join(kind.sysfs_index())
                    .join(format!("{major}:{minor}"));
                found.push(DeviceNode {
                    path,
                    kind,
                    major,
                    minor,
                    size: match kind {
                        DeviceKind::Block => block_size(&sysfs),
                        DeviceKind::Char => None,
                    },
                    subsystem: subsystem(&sysfs),
                });
            }
        }
        found.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(found)
    }

    fn record(node: &DeviceNode, schema: &Arc<Schema>) -> Result<RecordValue, ErrorValue> {
        let name = node
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(RecordValue::builder(
            Arc::clone(schema),
            provenance(
                PROVIDER_ID,
                schema.id(),
                &format!(
                    "stat({}) + /sys/dev/{}/{}:{}",
                    node.path.display(),
                    node.kind.sysfs_index(),
                    node.major,
                    node.minor
                ),
            ),
        )
        .set("path", Value::Path(Arc::from(node.path.clone())))?
        .set("name", Value::string(&name))?
        .set("kind", Value::string(node.kind.name()))?
        .set("major", Value::Int(i128::from(node.major)))?
        .set("minor", Value::Int(i128::from(node.minor)))?
        .set(
            "size",
            node.size.map_or(Value::Null, |size| {
                Value::ByteSize(ByteSize::from_bytes(size))
            }),
        )?
        .set(
            "subsystem",
            node.subsystem.as_deref().map_or(Value::Null, Value::string),
        )?
        .build())
    }
}

/// The size of a block device, from its sysfs `size` attribute (in 512-byte sectors, whatever
/// the device's own sector size — that is how the kernel defines the attribute).
fn block_size(sysfs: &Path) -> Option<u128> {
    let text = fs::read_to_string(sysfs.join("size")).ok()?;
    let sectors: u128 = text.trim().parse().ok()?;
    sectors.checked_mul(512)
}

/// The subsystem a device belongs to: the name the sysfs `subsystem` symlink points at.
fn subsystem(sysfs: &Path) -> Option<String> {
    let target = fs::read_link(sysfs.join("subsystem")).ok()?;
    target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

async fn stream_devices(nodes: Vec<DeviceNode>, schema: Arc<Schema>, sink: StreamSink) {
    for node in nodes {
        match DeviceProvider::record(&node, &schema) {
            Ok(record) => {
                if sink.send(record.into_value()).await.is_err() {
                    return;
                }
            }
            Err(error) => {
                if sink.fail(error).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl Provider for DeviceProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["device"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        schemas::require(&schemas::device_id())
            .ok()
            .into_iter()
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("device.list", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        if self.dev.is_dir() {
            Availability::Available
        } else {
            Availability::unavailable(format!("{} is not a directory", self.dev.display()))
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        if query.target_name() != "device" {
            return Err(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("{PROVIDER_ID} does not answer `{}`", query.target_name()),
            ));
        }
        let wanted_kind = match query.option_value("kind") {
            None => None,
            Some(value) => {
                let text = value.as_str().map_err(|_| {
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        "`--kind` names a device kind, `block` or `char`",
                    )
                })?;
                Some(DeviceKind::parse(text).ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("`--kind {text}` is neither `block` nor `char`"),
                    )
                })?)
            }
        };
        let mut nodes = self.nodes()?;
        if let Some(kind) = wanted_kind {
            nodes.retain(|node| node.kind == kind);
        }
        nodes.truncate(query.max().unwrap_or(usize::MAX));
        let schema = schemas::require(&schemas::device_id())?;
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move { stream_devices(nodes, schema, sink).await },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let schema = schemas::require(&schemas::device_id())?;
        let mut found = Vec::new();
        for node in self.nodes()? {
            let record = Self::record(&node, &schema)?;
            if selector.matches(&record)
                && let Some(reference) = ObjectRef::of(&record)
            {
                found.push(reference);
            }
        }
        Ok(found)
    }
}
