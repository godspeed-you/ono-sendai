//! The `mount` and `filesystem` targets (spec §23.5, §28.6).
//!
//! Mounts come from `/proc/self/mountinfo`, which spec §23.5 names, and not from `/etc/mtab`:
//! `mtab` is a file userspace writes and can be stale or a symlink to the same procfs file
//! anyway, while `mountinfo` is what the kernel currently believes. Options stay a list, one
//! element per option, because "preserve structured mount options" is the whole point of asking
//! the kernel rather than reading `mount(8)`'s output.
//!
//! Capacity comes from `statvfs(3)`. A `ono.filesystem/1` is the thing a `ono.mount/1` mounts:
//! the mount says *where*, the filesystem says *what and how full*. A filesystem that is not
//! mounted is still a filesystem: a block device udev found a signature on — linked from
//! `/dev/disk/by-uuid` or `by-label`, typed in udev's database under `/run/udev/data` — whose
//! device number (`/sys/class/block/<name>/dev`) is the source of no mount (ADR-0097).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nix::mount::{MntFlags, MsFlags};
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamSink, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectRef, Provider, Query, Risk, Selector,
};
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

/// A block device carrying a filesystem signature that is not the source of any mount.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UnmountedFilesystem {
    device: PathBuf,
    fs_type: String,
}

/// Mounts and filesystems.
#[derive(Debug)]
pub struct StorageProvider {
    root: PathBuf,
    mountinfo: PathBuf,
    disk_by_uuid: PathBuf,
    disk_by_label: PathBuf,
    sys_class_block: PathBuf,
    udev_data: PathBuf,
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
            sys_class_block: root.join("sys/class/block"),
            udev_data: root.join("run/udev/data"),
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

    /// The block devices udev recorded a filesystem on that no mount uses.
    ///
    /// A device counts as mounted when its number (`sys/class/block/<name>/dev`) is a mount's
    /// device, or its path is a mount's source — the number is what makes `/dev/mapper/x` and
    /// `/dev/dm-0` one device. The type comes from udev's database; a device without a record
    /// there has no known type and, `type` being required, is not reported (spec §35.3).
    fn unmounted(
        &self,
        mounts: &[MountInfo],
        uuids: &HashMap<PathBuf, String>,
        labels: &HashMap<PathBuf, String>,
    ) -> Vec<UnmountedFilesystem> {
        let mut devices: Vec<&PathBuf> = uuids.keys().chain(labels.keys()).collect();
        devices.sort();
        devices.dedup();
        let mut found = Vec::new();
        for device in devices {
            let Some(name) = device.file_name() else {
                continue;
            };
            let number = fs::read_to_string(self.sys_class_block.join(name).join("dev"))
                .ok()
                .map(|text| text.trim().to_owned());
            let source = device.display().to_string();
            let mounted = mounts.iter().any(|mount| {
                mount.source == source || (number.is_some() && mount.device == number)
            });
            if mounted {
                continue;
            }
            let Some(fs_type) = number
                .as_deref()
                .and_then(|number| self.udev_property(number, "ID_FS_TYPE"))
            else {
                continue;
            };
            found.push(UnmountedFilesystem {
                device: device.clone(),
                fs_type,
            });
        }
        found
    }

    /// One `E:<key>=<value>` property of a block device's udev database record.
    fn udev_property(&self, number: &str, key: &str) -> Option<String> {
        let text = fs::read_to_string(self.udev_data.join(format!("b{number}"))).ok()?;
        let prefix = format!("E:{key}=");
        text.lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }

    fn unmounted_record(
        filesystem: &UnmountedFilesystem,
        schema: &Arc<Schema>,
        uuids: &HashMap<PathBuf, String>,
        labels: &HashMap<PathBuf, String>,
    ) -> Result<RecordValue, ErrorValue> {
        let uuid = uuids
            .get(&filesystem.device)
            .and_then(|text| Uuid::parse(text).ok())
            .map_or(Value::Null, Value::Uuid);
        let label = labels
            .get(&filesystem.device)
            .map_or(Value::Null, |label| Value::string(label));
        let device = filesystem.device.display().to_string();
        Ok(RecordValue::builder(
            Arc::clone(schema),
            provenance(
                PROVIDER_ID,
                schema.id(),
                "/dev/disk/by-uuid + /sys/class/block + /run/udev/data",
            ),
        )
        .set("source", Value::string(&device))?
        .set("type", Value::string(&filesystem.fs_type))?
        .set("uuid", uuid)?
        .set("label", label)?
        .set("target", Value::Null)?
        .set("size", Value::Null)?
        .set("used", Value::Null)?
        .set("available", Value::Null)?
        .set("read_only", Value::Null)?
        .set("device", Value::string(&device))?
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
    unmounted: Vec<UnmountedFilesystem>,
    schema: Arc<Schema>,
    uuids: HashMap<PathBuf, String>,
    labels: HashMap<PathBuf, String>,
    sink: StreamSink,
) {
    // One filesystem can be mounted at several points — a bind mount is the everyday case — and
    // `get filesystem` should answer once per filesystem, not once per mount.
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let records = mounts
        .iter()
        .filter(|mount| seen.insert((mount.source.clone(), mount.filesystem.clone())))
        .map(|mount| StorageProvider::filesystem_record(mount, &schema, &uuids, &labels))
        .chain(unmounted.iter().map(|filesystem| {
            StorageProvider::unmounted_record(filesystem, &schema, &uuids, &labels)
        }));
    for record in records {
        match record {
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
            // `docs/spec/capabilities.yaml` gives `mount.manage` elevation `required`: mount(2)
            // and umount2(2) need CAP_SYS_ADMIN.
            Capability::new("mount.manage", Risk::Mutate).needing_elevation(),
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
                // `--mounted` restricts to one side; without it a filesystem is listed whether
                // or not it is mounted, which is what the contract's summary says.
                let wanted_mounted = match query.option_value("mounted") {
                    Some(Value::Bool(mounted)) => Some(*mounted),
                    _ => None,
                };
                let unmounted = if wanted_mounted == Some(true) {
                    Vec::new()
                } else {
                    self.unmounted(&mounts, &uuids, &labels)
                };
                if wanted_mounted == Some(false) {
                    mounts.clear();
                }
                Ok(ValueStream::spawn(
                    PipelineConfig::new(),
                    Boundedness::Bounded,
                    move |sink| async move {
                        stream_filesystems(mounts, unmounted, schema, uuids, labels, sink).await;
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

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        match action.operation() {
            "mount" => Ok(self.mount(action)),
            "unmount" => Ok(self.unmount(action)),
            other => Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{PROVIDER_ID} does not implement `{other}`"),
            )),
        }
    }
}

// --- the mutations: mount(2) and umount2(2), asked for real (ADR-0098) --------------------------

/// The mount options that are flags to the kernel rather than data for the filesystem, as
/// `mount(8)` spells them. Everything else travels in the data string, one option per element
/// of the list the user gave — never re-split from a joined string (spec §23.5).
fn mount_flag(option: &str) -> Option<MsFlags> {
    Some(match option {
        "ro" => MsFlags::MS_RDONLY,
        "nosuid" => MsFlags::MS_NOSUID,
        "nodev" => MsFlags::MS_NODEV,
        "noexec" => MsFlags::MS_NOEXEC,
        "sync" => MsFlags::MS_SYNCHRONOUS,
        "dirsync" => MsFlags::MS_DIRSYNC,
        "noatime" => MsFlags::MS_NOATIME,
        "nodiratime" => MsFlags::MS_NODIRATIME,
        "relatime" => MsFlags::MS_RELATIME,
        "strictatime" => MsFlags::MS_STRICTATIME,
        "lazytime" => MsFlags::MS_LAZYTIME,
        "bind" => MsFlags::MS_BIND,
        "rbind" => MsFlags::MS_BIND | MsFlags::MS_REC,
        "remount" => MsFlags::MS_REMOUNT,
        "silent" => MsFlags::MS_SILENT,
        "mand" => MsFlags::MS_MANDLOCK,
        // `rw`, `defaults`, `async`, `atime`, `dev`, `exec`, `suid` name the absence of a flag.
        "rw" | "defaults" | "async" | "atime" | "dev" | "exec" | "suid" | "diratime" | "nomand"
        | "nolazytime" | "loud" => MsFlags::empty(),
        _ => return None,
    })
}

/// Splits a list of options into the kernel's flags and the filesystem's data string.
fn split_options(options: &[String]) -> (MsFlags, String) {
    let mut flags = MsFlags::empty();
    let mut data = Vec::new();
    for option in options {
        match mount_flag(option) {
            Some(flag) => flags |= flag,
            None => data.push(option.as_str()),
        }
    }
    (flags, data.join(","))
}

/// The path a value names, whether it arrived typed or as text.
fn path_of(value: &Value) -> Option<PathBuf> {
    match value {
        Value::Path(path) => Some(path.to_path_buf()),
        Value::String(text) => Some(PathBuf::from(text.as_ref())),
        _ => None,
    }
}

/// The text a value carries, for a source that may be a path or a name.
fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Path(path) => Some(path.display().to_string()),
        Value::String(text) => Some(text.to_string()),
        _ => None,
    }
}

/// The elements of a repeatable option: a list when it was written more than once, one value
/// when once, nothing when never.
fn list_of(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::List(items)) => items.iter().filter_map(text_of).collect(),
        Some(other) => text_of(other).into_iter().collect(),
        None => Vec::new(),
    }
}

fn flag_of(action: &Action, name: &str) -> bool {
    matches!(action.argument(name), Some(Value::Bool(true)))
}

fn missing(action: &Action, what: &str) -> ActionOutcome {
    ActionOutcome::failed(
        action,
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "`{}` needs a {what}, and none was given",
                action.operation()
            ),
        ),
    )
}

impl StorageProvider {
    /// The mount point an action is about: its `target` argument, or the identity of the
    /// `ono.mount/1` it was resolved from or piped in as.
    fn action_target(action: &Action) -> Option<PathBuf> {
        action
            .argument("target")
            .and_then(path_of)
            .or_else(|| action.target().values().first().and_then(path_of))
    }

    /// The filesystem type of a block device, as udev recorded it — for a `mount` that named
    /// no `--type`.
    fn detect_type(&self, source: &str) -> Option<String> {
        let name = Path::new(source).file_name()?;
        let number = fs::read_to_string(self.sys_class_block.join(name).join("dev")).ok()?;
        self.udev_property(number.trim(), "ID_FS_TYPE")
    }

    fn mount(&self, action: &Action) -> ActionOutcome {
        let Some(source) = action.argument("source").and_then(text_of) else {
            return missing(action, "source");
        };
        let Some(target) = Self::action_target(action) else {
            return missing(action, "target");
        };
        let fs_type = match action.argument("type").and_then(text_of) {
            Some(fs_type) => fs_type,
            None => {
                match self.detect_type(&source) {
                    Some(fs_type) => fs_type,
                    None => {
                        return ActionOutcome::failed(
                        action,
                        ErrorValue::new(
                            ErrorCode::TypeMismatch,
                            format!("no filesystem type was given and none is recorded for `{source}`"),
                        )
                        .with_help("name it with `--type`, as in `--type ext4`"),
                    );
                    }
                }
            }
        };
        let (mut flags, data) = split_options(&list_of(action.argument("option")));
        if flag_of(action, "read-only") {
            flags |= MsFlags::MS_RDONLY;
        }
        if action.is_dry_run() {
            return ActionOutcome::skipped(
                action,
                format!("would mount {source} ({fs_type}) at {}", target.display()),
            );
        }
        match nix::mount::mount(
            Some(source.as_str()),
            &target,
            Some(fs_type.as_str()),
            flags,
            Some(data.as_str()),
        ) {
            Ok(()) => ActionOutcome::succeeded(action, true),
            Err(errno) => ActionOutcome::failed(action, mount_error(errno, &target)),
        }
    }

    fn unmount(&self, action: &Action) -> ActionOutcome {
        let Some(target) = Self::action_target(action) else {
            return missing(action, "mount point");
        };
        // Decidable before any privileged call: the kernel's own table says whether there is
        // a mount at that path. A directory that is no mount point is `io.not_found` — the
        // mount is the resource, and it does not exist.
        match self.mounts() {
            Ok(mounts) if mounts.iter().any(|mount| mount.target == target) => {}
            Ok(_) => {
                return ActionOutcome::failed(
                    action,
                    ErrorValue::new(
                        ErrorCode::IoNotFound,
                        format!("nothing is mounted at {}", target.display()),
                    )
                    .with_target(ono_value::ValueRef::path(&target))
                    .with_help("`get mount` lists the mount points there are"),
                );
            }
            Err(error) => return ActionOutcome::failed(action, error),
        }
        let mut flags = MntFlags::empty();
        if flag_of(action, "lazy") {
            flags |= MntFlags::MNT_DETACH;
        }
        if flag_of(action, "force") {
            flags |= MntFlags::MNT_FORCE;
        }
        if action.is_dry_run() {
            return ActionOutcome::skipped(action, format!("would unmount {}", target.display()));
        }
        match nix::mount::umount2(&target, flags) {
            Ok(()) => ActionOutcome::succeeded(action, true),
            Err(errno) => ActionOutcome::failed(action, mount_error(errno, &target)),
        }
    }
}

/// A refused mount call, with the one help line every unprivileged user needs.
fn mount_error(errno: nix::errno::Errno, target: &Path) -> ErrorValue {
    let error = errno_error(errno, target);
    match errno {
        nix::errno::Errno::EPERM | nix::errno::Errno::EACCES => error.with_help(
            "mounting and unmounting need CAP_SYS_ADMIN; run this as root, or through a \
             privilege broker the policy admits",
        ),
        _ => error,
    }
}

#[cfg(test)]
mod fuzz {
    //! `mountinfo` is written by the kernel and read here, and its fields are separated by a
    //! marker (` - `) that a mount point's own name is allowed to contain in escaped form. Spec
    //! §35.6 requires the decoder be fuzzed; ADR-0015 T7 makes an unbounded allocation a
    //! release-blocking threat.

    use super::parse_mountinfo;
    use ono_testkit::Rng;

    const PIECES: &[&str] = &[
        "36",
        "35",
        "/",
        " ",
        "-",
        " - ",
        "\\040",
        "\\011",
        "\\012",
        "\\134",
        "rw",
        "ro",
        "relatime",
        ",",
        ":",
        "ext4",
        "tmpfs",
        "/dev/sda1",
        "none",
        "\n",
        "\t",
        "0:1",
        "18446744073709551615",
        "é",
        "\u{0}",
    ];

    #[test]
    fn should_never_panic_on_anything_that_arrives_as_a_mount_table() {
        let mut rng = Rng::seeded(0x4d_4e_54);
        for _ in 0..4000 {
            let _ = parse_mountinfo(&rng.assemble(PIECES, 40));
        }
    }

    #[test]
    fn should_return_rather_than_recurse_on_a_pathologically_long_table() {
        for length in [1_000usize, 50_000] {
            let _ = parse_mountinfo(&" - ".repeat(length));
            let _ = parse_mountinfo(&"\\040".repeat(length));
        }
    }
}
