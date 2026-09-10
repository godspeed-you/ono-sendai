//! The other half of a Btrfs identity: which subvolume a mount actually shows (Appendix B.9).
//!
//! `btrfs subvolume list` says which subvolumes exist and how they nest. It does not say which of
//! them a given path leads into, because that is a property of the *mount*: the same subvolume can
//! appear at several mountpoints, and a mountpoint's `subvol=` and `subvolid=` options are what
//! decide which tree a path descends. Appendix B.9 needs both halves — "resolve subvolume/root IDs
//! and nested subvolume boundaries" — so this module reads `mountinfo(5)` for the second one.
//!
//! The mount root field is what makes it work. In the recorded table
//!
//! ```text
//! 7493 7619 0:87 /@     /mnt/root      … btrfs /dev/loop20 …,subvolid=256,subvol=/@
//! 7596 7493 0:87 /@var  /mnt/root/var  … btrfs /dev/loop20 …,subvolid=258,subvol=/@var
//! ```
//!
//! the tree path `@var/lib-app` is visible at `/mnt/root/var/lib-app`, and knowing that is the
//! difference between offering to protect the nested subvolume and silently missing it (§14.3).
//!
//! Nothing here trusts a name. A mount whose filesystem type is not `btrfs` is dropped, and a
//! path component that merely looks like `@home` never becomes a subvolume: the subvolume id
//! comes from the mount option, the tree path from the mount root.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

use crate::parse::{FS_TREE_ID, FilesystemInfo};

/// The kernel mount table Appendix B.1 resolves against.
pub const MOUNTINFO: &str = "/proc/self/mountinfo";

/// One Btrfs mount, as `mountinfo(5)` describes it (Appendix B.1, B.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtrfsMount {
    mount_id: Arc<str>,
    device: Arc<str>,
    root: Arc<str>,
    mount_point: Arc<str>,
    source: Arc<str>,
    subvolume_id: Option<u64>,
    subvolume: Option<Arc<str>>,
    read_only: bool,
    filesystem_uuid: Option<Arc<str>>,
}

impl BtrfsMount {
    /// The kernel's mount id.
    #[must_use]
    pub fn mount_id(&self) -> &str {
        &self.mount_id
    }

    /// The `major:minor` of the superblock, which is the identity two mounts of one filesystem
    /// share and two filesystems never do (Appendix B.3).
    #[must_use]
    pub fn device(&self) -> &str {
        &self.device
    }

    /// The mount's root inside the filesystem, such as `/@var`.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Where it is mounted.
    #[must_use]
    pub fn mount_point(&self) -> &str {
        &self.mount_point
    }

    /// The backing device.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The `subvolid=` option — half of §14.1's stable identity.
    #[must_use]
    pub const fn subvolume_id(&self) -> Option<u64> {
        self.subvolume_id
    }

    /// The `subvol=` option.
    #[must_use]
    pub fn subvolume(&self) -> Option<&str> {
        self.subvolume.as_deref()
    }

    /// Whether the mount is read-only, which blocks a restore into it (Appendix G.2).
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// The UUID of the filesystem this mount shows, once [`BtrfsMounts::identified_by`] tied it to
    /// one (§56.2).
    ///
    /// `mountinfo(5)` carries a device and a superblock number, never the UUID an asset's scope
    /// names, and subvolume ids start at 256 on every Btrfs filesystem. So a mount that has not
    /// been identified is not matched against any scope at all: `None` is "unknown", and §56.3
    /// makes unknown a refusal.
    #[must_use]
    pub fn filesystem_uuid(&self) -> Option<&str> {
        self.filesystem_uuid.as_deref()
    }

    /// Whether this mount shows the top level of the filesystem rather than a named subvolume.
    #[must_use]
    pub fn is_filesystem_tree(&self) -> bool {
        self.subvolume_id == Some(FS_TREE_ID) || self.root.trim_matches('/').is_empty()
    }

    /// The subvolume tree path this mount shows, with no leading slash — `@var` for `/@var`.
    #[must_use]
    pub fn tree_path(&self) -> &str {
        self.root.trim_matches('/')
    }

    /// Where `tree_path` is visible through this mount, when it is visible here at all.
    ///
    /// `@var/lib-app` is visible through the `/@var` mount at `<mountpoint>/lib-app`, and through
    /// the top-level mount at `<mountpoint>/@var/lib-app`. It is not visible through the `/@home`
    /// mount at all, and answering `None` for that is what stops a plan from claiming a path it
    /// cannot reach.
    #[must_use]
    pub fn visible_path(&self, tree_path: &str) -> Option<PathBuf> {
        let wanted = tree_path.trim_matches('/');
        let root = self.tree_path();
        let relative = if root.is_empty() {
            wanted
        } else if wanted == root {
            ""
        } else {
            wanted
                .strip_prefix(root)
                .and_then(|rest| rest.strip_prefix('/'))?
        };
        let mut path = PathBuf::from(self.mount_point.as_ref());
        if !relative.is_empty() {
            path.push(relative);
        }
        Some(path)
    }
}

/// Every Btrfs mount a resolution is performed against (Appendix B.1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BtrfsMounts {
    mounts: Vec<BtrfsMount>,
}

impl BtrfsMounts {
    /// Reads the Btrfs mounts out of recorded `mountinfo(5)` text.
    ///
    /// Lines that are not Btrfs are dropped, which is why the recorded table's leading ext4 bind
    /// mount does not become a Btrfs subvolume with an odd name.
    #[must_use]
    pub fn from_mountinfo(text: &str) -> Self {
        Self {
            mounts: text.lines().filter_map(parse_mount_line).collect(),
        }
    }

    /// Reads the mounts this process can see.
    ///
    /// # Errors
    ///
    /// A structured error when `/proc/self/mountinfo` cannot be read. §56.3 makes an unreadable
    /// mount table a refusal: without it no path has a subvolume, and inventing one is how a plan
    /// comes to claim protection it does not have.
    pub fn from_proc() -> Result<Self, ErrorValue> {
        let text = std::fs::read_to_string(MOUNTINFO).map_err(|error| {
            ErrorValue::new(
                match error.kind() {
                    std::io::ErrorKind::NotFound => ErrorCode::IoNotFound,
                    std::io::ErrorKind::PermissionDenied => ErrorCode::IoPermissionDenied,
                    _ => ErrorCode::ProviderInconclusive,
                },
                format!("the kernel mount table at {MOUNTINFO} could not be read: {error}"),
            )
            .with_help(
                "v0.6 Appendix B.1: a Btrfs subvolume identity is resolved from a mount, so \
                 without the mount table nothing about protection can be established",
            )
            .with_metadata("path", Value::string(MOUNTINFO))
        })?;
        Ok(Self::from_mountinfo(&text))
    }

    /// Builds a table from mounts already decoded.
    #[must_use]
    pub const fn new(mounts: Vec<BtrfsMount>) -> Self {
        Self { mounts }
    }

    /// The mounts, in the order the kernel listed them.
    #[must_use]
    pub fn mounts(&self) -> &[BtrfsMount] {
        &self.mounts
    }

    /// Ties each mount to the filesystem `btrfs filesystem show` lists its device under (§56.2).
    ///
    /// A mount whose source is one of a filesystem's devices gets that filesystem's UUID, and so
    /// does every other mount of the same superblock (`major:minor`), which is how a mount whose
    /// source the listing spells differently is still tied to the right filesystem. A mount that
    /// matches nothing stays unidentified, and nothing is matched against it.
    #[must_use]
    pub fn identified_by(mut self, filesystems: &[FilesystemInfo]) -> Self {
        for mount in &mut self.mounts {
            if let Some(filesystem) = filesystems.iter().find(|filesystem| {
                filesystem
                    .devices()
                    .iter()
                    .any(|device| device.as_ref() == mount.source())
            }) {
                mount.filesystem_uuid = Some(Arc::from(filesystem.uuid()));
            }
        }
        let known: Vec<(Arc<str>, Arc<str>)> = self
            .mounts
            .iter()
            .filter_map(|mount| {
                mount
                    .filesystem_uuid
                    .as_ref()
                    .map(|uuid| (Arc::clone(&mount.device), Arc::clone(uuid)))
            })
            .collect();
        for mount in &mut self.mounts {
            if mount.filesystem_uuid.is_none()
                && let Some((_, uuid)) = known.iter().find(|(device, _)| *device == mount.device)
            {
                mount.filesystem_uuid = Some(Arc::clone(uuid));
            }
        }
        self
    }

    /// Whether every mount carries the UUID of its filesystem.
    #[must_use]
    pub fn is_identified(&self) -> bool {
        self.mounts
            .iter()
            .all(|mount| mount.filesystem_uuid.is_some())
    }

    /// The mounts of the filesystem with this UUID, in the order the kernel listed them (§56.2).
    #[must_use]
    pub fn of_filesystem(&self, uuid: &str) -> Vec<&BtrfsMount> {
        self.mounts
            .iter()
            .filter(|mount| mount.filesystem_uuid() == Some(uuid))
            .collect()
    }

    /// The mounts of the filesystem `mount` shows, as a table of their own (§14.3).
    ///
    /// Two mounts show one filesystem when they share a superblock, or when both were identified
    /// as the same UUID. A subvolume layout is one filesystem's, and a mount of another
    /// filesystem that happens to show a subvolume with the same id or the same tree path is a
    /// different subvolume.
    #[must_use]
    pub fn same_filesystem_as(&self, mount: &BtrfsMount) -> Self {
        Self {
            mounts: self
                .mounts
                .iter()
                .filter(|other| {
                    other.device() == mount.device()
                        || other
                            .filesystem_uuid()
                            .zip(mount.filesystem_uuid())
                            .is_some_and(|(left, right)| left == right)
                })
                .cloned()
                .collect(),
        }
    }

    /// Whether any Btrfs mount is visible at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }

    /// The deepest Btrfs mount that contains `path`, which is the one serving it.
    ///
    /// Deepest wins, and among mounts at the same point the last one wins: that is what the
    /// kernel shows, and a `/var` subvolume mounted over a directory inside `@` is exactly the
    /// case where getting it wrong claims the wrong subvolume's snapshot as protection.
    #[must_use]
    pub fn covering(&self, path: &Path) -> Option<&BtrfsMount> {
        let target = path.to_string_lossy().into_owned();
        let mut best: Option<&BtrfsMount> = None;
        for mount in &self.mounts {
            if !contains(mount.mount_point(), &target) {
                continue;
            }
            let deeper =
                best.is_none_or(|current| mount.mount_point().len() >= current.mount_point().len());
            if deeper {
                best = Some(mount);
            }
        }
        best
    }

    /// The mount showing the subvolume with this id, where one is mounted.
    #[must_use]
    pub fn for_subvolume_id(&self, id: u64) -> Option<&BtrfsMount> {
        self.mounts
            .iter()
            .find(|mount| mount.subvolume_id() == Some(id))
    }

    /// The mount showing this tree path as its own root, where one is mounted.
    #[must_use]
    pub fn for_tree_path(&self, tree_path: &str) -> Option<&BtrfsMount> {
        let wanted = tree_path.trim_matches('/');
        self.mounts
            .iter()
            .find(|mount| mount.tree_path() == wanted && !wanted.is_empty())
    }

    /// A mount showing the top level of the filesystem, where there is one.
    ///
    /// Appendix D.8's recovery namespace lives at the top level, so this is the mount through
    /// which a snapshot destination is addressed.
    #[must_use]
    pub fn filesystem_tree(&self) -> Option<&BtrfsMount> {
        self.mounts.iter().find(|mount| mount.is_filesystem_tree())
    }

    /// Whether `mount` is the operating system's root subvolume (§14.6).
    ///
    /// Two shapes count, and the second is the reason this is a method rather than a string
    /// comparison against `/`. A subvolume mounted at `/` is the root. So is a subvolume that
    /// other subvolume mounts of the same filesystem hang beneath — which is what a recorded or
    /// containerised layout looks like when the whole tree has been mounted somewhere else for
    /// inspection. The filesystem's own top level is never the root subvolume: it is the
    /// container the named subvolumes live in.
    #[must_use]
    pub fn is_root_subvolume(&self, mount: &BtrfsMount) -> bool {
        if mount.is_filesystem_tree() {
            return false;
        }
        if mount.mount_point() == "/" {
            return true;
        }
        self.mounts.iter().any(|other| {
            !std::ptr::eq(other, mount)
                && !other.is_filesystem_tree()
                && other.device() == mount.device()
                && contains(mount.mount_point(), other.mount_point())
                && other.mount_point() != mount.mount_point()
        })
    }
}

/// Whether `path` is at or below `directory`, compared by path component.
///
/// `/mnt/root` contains `/mnt/root/var` and does not contain `/mnt/rooted`.
#[must_use]
pub fn contains(directory: &str, path: &str) -> bool {
    if directory == "/" {
        return path.starts_with('/');
    }
    path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Decodes one `mountinfo(5)` line, keeping only Btrfs.
fn parse_mount_line(line: &str) -> Option<BtrfsMount> {
    let (before, after) = line.split_once(" - ")?;
    let head: Vec<&str> = before.split_whitespace().collect();
    let tail: Vec<&str> = after.split_whitespace().collect();
    if head.len() < 6 || tail.len() < 2 {
        return None;
    }
    if *tail.first()? != "btrfs" {
        return None;
    }
    let options = tail.get(2).copied().unwrap_or_default();
    let mount_options = head.get(5).copied().unwrap_or_default();
    let subvolume_id = option_value(options, "subvolid").and_then(|value| value.parse().ok());
    let subvolume = option_value(options, "subvol").map(Arc::from);
    Some(BtrfsMount {
        mount_id: Arc::from(*head.first()?),
        device: Arc::from(*head.get(2)?),
        root: Arc::from(unescape(head.get(3).copied().unwrap_or("/")).as_str()),
        mount_point: Arc::from(unescape(head.get(4).copied().unwrap_or("/")).as_str()),
        source: Arc::from(*tail.get(1)?),
        subvolume_id,
        subvolume,
        read_only: has_flag(mount_options, "ro") || has_flag(options, "ro"),
        filesystem_uuid: None,
    })
}

/// The value of a `key=value` mount option.
fn option_value<'a>(options: &'a str, key: &str) -> Option<&'a str> {
    options
        .split(',')
        .find_map(|option| option.strip_prefix(key)?.strip_prefix('='))
}

/// Whether a bare flag is present among comma-separated mount options.
fn has_flag(options: &str, flag: &str) -> bool {
    options.split(',').any(|option| option == flag)
}

/// Decodes the octal escapes `mountinfo(5)` uses for spaces, tabs and backslashes.
fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut characters = field.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        let digits: String = characters.clone().take(3).collect();
        match u8::from_str_radix(&digits, 8) {
            Ok(byte) if digits.len() == 3 => {
                out.push(char::from(byte));
                for _ in 0..3 {
                    let _ = characters.next();
                }
            }
            _ => out.push('\\'),
        }
    }
    out
}
