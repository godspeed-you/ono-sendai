//! The `mount` and `filesystem` targets (spec §23.5, §28.6).
//!
//! Mounts come from `/proc/self/mountinfo`, which spec §23.5 names, and not from `/etc/mtab`:
//! `mtab` is a file userspace writes and can be stale or a symlink to the same procfs file
//! anyway, while `mountinfo` is what the kernel currently believes. Options stay a list, one
//! element per option, because "preserve structured mount options" is the whole point of asking
//! the kernel rather than reading `mount(8)`'s output.
//!
//! Capacity comes from `statvfs(3)`. A `ono.filesystem/1` is the thing a `ono.mount/1` mounts:
//! the mount says *where*, the filesystem says *what and how full*.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamSink, ValueStream};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
use ono_value::{ByteSize, ErrorValue, RecordValue, Schema, Uuid, Value};

use crate::common::{errno_error, io_error, provenance};
use crate::schemas;

/// The provider's stable id, as it appears in every record's provenance.
pub const PROVIDER_ID: &str = "linux.mountinfo";

/// One line of `/proc/self/mountinfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MountInfo {
    pub(crate) source: String,
    pub(crate) target: PathBuf,
    pub(crate) filesystem: String,
    pub(crate) options: Vec<String>,
    pub(crate) device: Option<String>,
}

impl MountInfo {
    fn read_only(&self) -> bool {
        self.options.iter().any(|option| option == "ro")
    }
}

/// Decodes `mountinfo(5)`.
///
/// The layout is `id parent major:minor root mountpoint options [optional…] - type source
/// superoptions`, where the number of optional fields is variable and the `-` is what separates
/// them from the rest. Splitting on a fixed column count is the bug this function exists to
/// avoid.
pub(crate) fn parse_mountinfo(text: &str) -> Vec<MountInfo> {
    text.lines().filter_map(parse_mountinfo_line).collect()
}

fn parse_mountinfo_line(line: &str) -> Option<MountInfo> {
    let fields: Vec<&str> = line.split(' ').filter(|field| !field.is_empty()).collect();
    let separator = fields.iter().position(|field| *field == "-")?;
    let device = fields.get(2)?;
    let target = unescape(fields.get(4)?);
    let mount_options = fields.get(5)?;
    let filesystem = fields.get(separator + 1)?;
    let source = unescape(fields.get(separator + 2)?);
    let super_options = fields.get(separator + 3).copied().unwrap_or_default();

    // The kernel reports per-mount and per-superblock options separately. A user asking "how is
    // this mounted" means both, so both are in the list, in that order, without duplicates.
    let mut options: Vec<String> = Vec::new();
    for option in mount_options
        .split(',')
        .chain(super_options.split(','))
        .filter(|option| !option.is_empty())
    {
        if !options.iter().any(|kept| kept == option) {
            options.push(option.to_owned());
        }
    }

    Some(MountInfo {
        source,
        target: PathBuf::from(target),
        filesystem: (*filesystem).to_owned(),
        options,
        // Major 0 is an anonymous device: tmpfs, procfs, an overlay. There is no block device
        // behind it, and saying `0:42` would suggest there is.
        device: match device.split_once(':') {
            Some(("0", _)) | None => None,
            Some(_) => Some((*device).to_owned()),
        },
    })
}

/// Decodes the octal escapes `mountinfo(5)` uses for space, tab, newline and backslash.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = String::with_capacity(field.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && let Some(escape) = field.get(index + 1..index + 4)
            && let Ok(byte) = u8::from_str_radix(escape, 8)
        {
            out.push(char::from(byte));
            index += 4;
            continue;
        }
        // Indices come from `as_bytes` on a `&str`, so this slice always starts on a boundary.
        if let Some(rest) = field.get(index..)
            && let Some(character) = rest.chars().next()
        {
            out.push(character);
            index += character.len_utf8();
        } else {
            break;
        }
    }
    out
}

/// The capacity numbers a filesystem reports, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Capacity {
    size: u128,
    used: u128,
    available: u128,
}

fn capacity(target: &Path) -> Result<Capacity, ErrorValue> {
    let stats = nix::sys::statvfs::statvfs(target).map_err(|errno| errno_error(errno, target))?;
    let block = u128::from(stats.fragment_size());
    let size = u128::from(stats.blocks()) * block;
    let free = u128::from(stats.blocks_free()) * block;
    Ok(Capacity {
        size,
        // Reserved blocks count as used: they are space nobody unprivileged can have, and
        // `size - available` would report them as free until a write failed.
        used: size.saturating_sub(free),
        available: u128::from(stats.blocks_available()) * block,
    })
}

/// Mounts and filesystems.
#[derive(Debug)]
pub struct StorageProvider {
    root: PathBuf,
    mountinfo: PathBuf,
    disk_by_uuid: PathBuf,
    disk_by_label: PathBuf,
}

impl Default for StorageProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageProvider {
    /// Mounts of the machine this shell runs on.
    #[must_use]
    pub fn new() -> Self {
        Self::rooted("/")
    }

    /// Mounts declared by the kernel interfaces under `root`.
    ///
    /// `root` locates `proc/self/mountinfo` and `dev/disk/`; the mount points inside a record
    /// are the ones the kernel itself reported.
    #[must_use]
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            root: root.to_path_buf(),
            mountinfo: root.join("proc/self/mountinfo"),
            disk_by_uuid: root.join("dev/disk/by-uuid"),
            disk_by_label: root.join("dev/disk/by-label"),
        }
    }

    fn mounts(&self) -> Result<Vec<MountInfo>, ErrorValue> {
        let text = fs::read_to_string(&self.mountinfo).map_err(|error| {
            io_error(&error, &self.mountinfo).with_help(
                "`mount` reads the kernel's own mount table; without a mounted procfs there is \
                 nothing to read, which is not the same as nothing being mounted",
            )
        })?;
        Ok(parse_mountinfo(&text))
    }

    /// Maps a device path to the UUID or label udev recorded for it.
    ///
    /// The `by-uuid` and `by-label` directories are symlink farms the kernel's device names sit
    /// behind. Reading them is structured lookup, not text scraping — and when they are absent,
    /// as they are inside most containers, the field is null rather than invented.
    fn by_link(&self, directory: &Path) -> HashMap<PathBuf, String> {
        let mut found = HashMap::new();
        let Ok(entries) = fs::read_dir(directory) else {
            return found;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(target) = fs::read_link(entry.path()) else {
                continue;
            };
            let resolved = if target.is_absolute() {
                target
            } else {
                normalise(&directory.join(target))
            };
            found.insert(
                self.device_name(&resolved),
                entry.file_name().to_string_lossy().into_owned(),
            );
        }
        found
    }

    /// The device path as the kernel writes it into `mountinfo`.
    ///
    /// A fixture's `dev/disk` lives under its own root, while the mount source it has to match is
    /// the absolute path the kernel reported. Re-anchoring at `/` makes a fixture behave exactly
    /// as the real thing does, where the two are already the same path.
    fn device_name(&self, resolved: &Path) -> PathBuf {
        resolved
            .strip_prefix(&self.root)
            .map_or_else(|_| resolved.to_path_buf(), |rest| Path::new("/").join(rest))
    }

    fn mount_record(mount: &MountInfo, schema: &Arc<Schema>) -> Result<RecordValue, ErrorValue> {
        Ok(RecordValue::builder(
            Arc::clone(schema),
            provenance(PROVIDER_ID, schema.id(), "/proc/self/mountinfo"),
        )
        .set("source", Value::string(&mount.source))?
        .set("target", Value::Path(Arc::from(mount.target.clone())))?
        .set("filesystem", Value::string(&mount.filesystem))?
        .set(
            "options",
            Value::list(mount.options.iter().map(|option| Value::string(option))),
        )?
        .set("read_only", Value::Bool(mount.read_only()))?
        .set(
            "device",
            mount
                .device
                .as_ref()
                .map_or(Value::Null, |device| Value::string(device)),
        )?
        .build())
    }

    fn filesystem_record(
        mount: &MountInfo,
        schema: &Arc<Schema>,
        uuids: &HashMap<PathBuf, String>,
        labels: &HashMap<PathBuf, String>,
    ) -> Result<RecordValue, ErrorValue> {
        let device = PathBuf::from(&mount.source);
        let uuid = uuids
            .get(&device)
            .and_then(|text| Uuid::parse(text).ok())
            .map_or(Value::Null, Value::Uuid);
        let label = labels
            .get(&device)
            .map_or(Value::Null, |label| Value::string(label));
        let (size, used, available) = match capacity(&mount.target) {
            Ok(capacity) => (
                Value::ByteSize(ByteSize::from_bytes(capacity.size)),
                Value::ByteSize(ByteSize::from_bytes(capacity.used)),
                Value::ByteSize(ByteSize::from_bytes(capacity.available)),
            ),
            // The mount point vanished or is not readable. That is a failed read, and spec §10.5
            // keeps it apart from "this filesystem reports no capacity".
            Err(error) => (
                error.clone().into_value(),
                error.clone().into_value(),
                error.into_value(),
            ),
        };
        Ok(RecordValue::builder(
            Arc::clone(schema),
            provenance(
                PROVIDER_ID,
                schema.id(),
                &format!("/proc/self/mountinfo + statvfs({})", mount.target.display()),
            ),
        )
        .set("source", Value::string(&mount.source))?
        .set("type", Value::string(&mount.filesystem))?
        .set("uuid", uuid)?
        .set("label", label)?
        .set("target", Value::Path(Arc::from(mount.target.clone())))?
        .set("size", size)?
        .set("used", used)?
        .set("available", available)?
        .set("read_only", Value::Bool(mount.read_only()))?
        .set(
            "device",
            mount
                .device
                .as_ref()
                .map_or(Value::Null, |device| Value::string(device)),
        )?
        .build())
    }

    /// The mount point a selector pins the query to, when one does.
    fn wanted_target(query: &Query) -> Option<PathBuf> {
        query
            .selectors()
            .iter()
            .find_map(|selector| match selector {
                Selector::Field { name, value } if name == "target" => match value {
                    Value::Path(path) => Some(path.to_path_buf()),
                    Value::String(text) => Some(PathBuf::from(text.as_ref())),
                    _ => None,
                },
                _ => None,
            })
    }
}

/// Resolves `..` textually, so a `by-uuid` symlink lands on the device name it points at.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

async fn stream_mounts(mounts: Vec<MountInfo>, schema: Arc<Schema>, sink: StreamSink) {
    for mount in mounts {
        match StorageProvider::mount_record(&mount, &schema) {
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

async fn stream_filesystems(
    mounts: Vec<MountInfo>,
    schema: Arc<Schema>,
    uuids: HashMap<PathBuf, String>,
    labels: HashMap<PathBuf, String>,
    sink: StreamSink,
) {
    // One filesystem can be mounted at several points — a bind mount is the everyday case — and
    // `get filesystem` should answer once per filesystem, not once per mount.
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for mount in mounts {
        if !seen.insert((mount.source.clone(), mount.filesystem.clone())) {
            continue;
        }
        match StorageProvider::filesystem_record(&mount, &schema, &uuids, &labels) {
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
impl Provider for StorageProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["mount", "filesystem"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        [schemas::mount_id(), schemas::filesystem_id()]
            .iter()
            .filter_map(|id| schemas::require(id).ok())
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("mount.list", Risk::Read),
            Capability::new("filesystem.list", Risk::Read),
        ]
    }

    fn availability(&self) -> Availability {
        if self.mountinfo.is_file() {
            Availability::Available
        } else {
            Availability::unavailable(format!("{} is not readable", self.mountinfo.display()))
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let mut mounts = self.mounts()?;
        if let Some(target) = Self::wanted_target(query) {
            mounts.retain(|mount| mount.target == target);
        }
        mounts.truncate(query.max().unwrap_or(usize::MAX));
        match query.target_name() {
            "mount" => {
                let schema = schemas::require(&schemas::mount_id())?;
                Ok(ValueStream::spawn(
                    PipelineConfig::new(),
                    Boundedness::Bounded,
                    move |sink| async move { stream_mounts(mounts, schema, sink).await },
                ))
            }
            "filesystem" => {
                let schema = schemas::require(&schemas::filesystem_id())?;
                let uuids = self.by_link(&self.disk_by_uuid);
                let labels = self.by_link(&self.disk_by_label);
                Ok(ValueStream::spawn(
                    PipelineConfig::new(),
                    Boundedness::Bounded,
                    move |sink| async move {
                        stream_filesystems(mounts, schema, uuids, labels, sink).await;
                    },
                ))
            }
            other => Err(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("{PROVIDER_ID} does not answer `{other}`"),
            )),
        }
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let schema = schemas::require(&schemas::mount_id())?;
        let mut found = Vec::new();
        for mount in self.mounts()? {
            let record = Self::mount_record(&mount, &schema)?;
            if selector.matches(&record)
                && let Some(reference) = ObjectRef::of(&record)
            {
                found.push(reference);
            }
        }
        Ok(found)
    }
}
