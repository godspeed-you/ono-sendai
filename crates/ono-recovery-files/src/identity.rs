//! What a path is, what filesystem it lives on, and which object it was (spec v0.6 §43.5, §15.2).
//!
//! §43.5 is the reason this module exists rather than a handful of `metadata()` calls spread
//! through the provider: *"Paths, symlinks and identities MUST be revalidated using safe
//! filesystem APIs. Protection of one object followed by mutation of a replaced symlink target is
//! unacceptable."* Every question here is asked with `lstat`, `statx` and `statfs` against the
//! path as given, and none of them follows a symlink in the final component. What protection
//! recorded, restore re-asks, and [`ObjectIdentity::is_still`] is where the two meet.
//!
//! The filesystem classification comes from `statfs`'s magic number rather than from
//! `/proc/self/mountinfo`. Appendix B.1's full pipeline belongs to the protection resolver; what a
//! file-copy provider needs is narrower and answerable from the path itself: whether the bytes
//! survive a reboot (Appendix B.7), and whether the tree it is about to walk stays on one mount.

use std::fs::Metadata;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::Path;

use ono_change_core::FilesystemKind;
use ono_change_core::error::{target_changed, target_unresolved};
use ono_value::ErrorValue;

/// What kind of object a path names (§15.1, §15.2).
///
/// The three protectable kinds are §15.1's list. The other four are §15.2's: a socket, a FIFO or
/// a device node has no content a copy could return, and archiving one would produce an asset
/// that claims to hold state nobody can restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    /// A regular file. §15.1's first case.
    RegularFile,
    /// A symlink, protected *as a symlink* and never followed (§15.1, §43.5).
    Symlink,
    /// A directory, protected as part of a small tree within the configured limits (§15.1).
    Directory,
    /// A named pipe. §15.2 excludes it: its content is a transfer, not a state.
    Fifo,
    /// A unix socket. §15.2 excludes it for the same reason.
    Socket,
    /// A block device node. §15.2 excludes it: the bytes behind it are not the file.
    BlockDevice,
    /// A character device node. §15.2 excludes it.
    CharacterDevice,
}

impl ObjectKind {
    /// The kind's name, as the manifest and the refusals spell it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ObjectKind::RegularFile => "file",
            ObjectKind::Symlink => "symlink",
            ObjectKind::Directory => "directory",
            ObjectKind::Fifo => "fifo",
            ObjectKind::Socket => "socket",
            ObjectKind::BlockDevice => "block-device",
            ObjectKind::CharacterDevice => "character-device",
        }
    }

    /// Reads a kind back out of a manifest.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [
            ObjectKind::RegularFile,
            ObjectKind::Symlink,
            ObjectKind::Directory,
            ObjectKind::Fifo,
            ObjectKind::Socket,
            ObjectKind::BlockDevice,
            ObjectKind::CharacterDevice,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == text)
    }

    /// Whether §15.1 lets this provider hold the object at all.
    #[must_use]
    pub const fn is_protectable(self) -> bool {
        matches!(
            self,
            ObjectKind::RegularFile | ObjectKind::Symlink | ObjectKind::Directory
        )
    }

    /// Why §15.2 excludes this kind, where it does.
    #[must_use]
    pub const fn exclusion(self) -> Option<&'static str> {
        match self {
            ObjectKind::RegularFile | ObjectKind::Symlink | ObjectKind::Directory => None,
            ObjectKind::Fifo => Some("a FIFO carries a transfer rather than a state to restore"),
            ObjectKind::Socket => Some("a socket is an endpoint rather than a state to restore"),
            ObjectKind::BlockDevice | ObjectKind::CharacterDevice => {
                Some("a device node names a driver, and copying it copies nothing behind it")
            }
        }
    }

    /// Classifies what `metadata` describes. `metadata` is always an `lstat` (§43.5).
    #[must_use]
    pub fn of(metadata: &Metadata) -> Self {
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            ObjectKind::Symlink
        } else if file_type.is_dir() {
            ObjectKind::Directory
        } else if file_type.is_file() {
            ObjectKind::RegularFile
        } else if file_type.is_fifo() {
            ObjectKind::Fifo
        } else if file_type.is_socket() {
            ObjectKind::Socket
        } else if file_type.is_block_device() {
            ObjectKind::BlockDevice
        } else {
            ObjectKind::CharacterDevice
        }
    }
}

/// Which object a path was, so that restore can tell whether it still is (§43.5).
///
/// Device and inode say which object; the creation time says which *incarnation* of it, so a
/// delete-and-recreate that reused the inode number is visible too. `statx` reports a birth time
/// only where the filesystem stores one, and where it does not the identity is the pair — an
/// absent generation is never treated as a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectIdentity {
    device: u64,
    inode: u64,
    generation: Option<i128>,
}

impl ObjectIdentity {
    /// Records the identity `device`, `inode` and `generation` name.
    #[must_use]
    pub const fn new(device: u64, inode: u64, generation: Option<i128>) -> Self {
        Self {
            device,
            inode,
            generation,
        }
    }

    /// The identity `metadata` describes, with the birth time `path` reports where it has one.
    #[must_use]
    pub fn of(path: &Path, metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            generation: birth_time(path),
        }
    }

    /// Reads the identity of `path` without following a symlink in its last component (§43.5).
    ///
    /// # Errors
    ///
    /// `change.target_unresolved` when the path cannot be stated, which §56.3 makes a refusal
    /// rather than an assumption that it is gone.
    pub fn read(path: &Path) -> Result<Self, ErrorValue> {
        let metadata = lstat(path)?;
        Ok(Self::of(path, &metadata))
    }

    /// The device the object lives on.
    #[must_use]
    pub const fn device(&self) -> u64 {
        self.device
    }

    /// The inode number.
    #[must_use]
    pub const fn inode(&self) -> u64 {
        self.inode
    }

    /// The creation time, where the filesystem records one.
    #[must_use]
    pub const fn generation(&self) -> Option<i128> {
        self.generation
    }

    /// Whether `other` is the same object this identity was taken from (§43.5).
    ///
    /// Two identities with different birth times are different objects even when the inode number
    /// was reused, and an identity that has a birth time never matches one that does not: the
    /// answer to "is this still the file I protected" is `false` whenever it cannot be `true`.
    #[must_use]
    pub fn is_still(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.generation == other.generation
    }
}

/// `lstat`, refusing rather than guessing when the path cannot be read (§43.5, §56.3).
pub(crate) fn lstat(path: &Path) -> Result<Metadata, ErrorValue> {
    std::fs::symlink_metadata(path).map_err(|error| {
        target_unresolved(
            &path.display().to_string(),
            &format!("v0.6 §43.5: the path was stated without following it, and: {error}"),
        )
    })
}

/// The refusal a path that is no longer the object that was protected produces (§43.5).
pub(crate) fn identity_changed(path: &Path, detail: &str) -> ErrorValue {
    target_changed(
        &path.display().to_string(),
        &format!(
            "v0.6 §43.5: protection of one object followed by mutation of a replaced symlink \
             target is unacceptable, so the restore refuses rather than writing through what is \
             there now. {detail}"
        ),
    )
}

/// The birth time `statx` reports for `path`, in nanoseconds, where the filesystem stores one.
fn birth_time(path: &Path) -> Option<i128> {
    let statx = rustix::fs::statx(
        rustix::fs::CWD,
        path,
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        rustix::fs::StatxFlags::BTIME,
    )
    .ok()?;
    if statx.stx_mask & rustix::fs::StatxFlags::BTIME.bits() == 0 {
        return None;
    }
    Some(i128::from(statx.stx_btime.tv_sec) * 1_000_000_000 + i128::from(statx.stx_btime.tv_nsec))
}

/// What filesystem holds a path, read from `statfs` (Appendix B.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemFacts {
    magic: i128,
    name: String,
    kind: FilesystemKind,
    device: u64,
    mount_point: String,
}

impl FilesystemFacts {
    /// The `statfs` magic number, as the kernel reports it.
    #[must_use]
    pub const fn magic(&self) -> i128 {
        self.magic
    }

    /// The filesystem's name, as `/proc/self/mountinfo` would spell it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How v0.6 classifies it (Appendix B).
    #[must_use]
    pub const fn kind(&self) -> FilesystemKind {
        self.kind
    }

    /// The device the path's own mount is on.
    #[must_use]
    pub const fn device(&self) -> u64 {
        self.device
    }

    /// The mount point, found by walking up until the device number changes (Appendix B.1).
    #[must_use]
    pub fn mount_point(&self) -> &str {
        &self.mount_point
    }
}

/// Reads which filesystem holds `path`.
///
/// # Errors
///
/// `change.target_unresolved` when `statfs` or `lstat` could not answer, which Appendix B.1 and
/// §56.3 both make a refusal: an unresolved path has no persistence domain, and a provider that
/// guessed would claim protection over something it never located.
pub fn filesystem_of(path: &Path) -> Result<FilesystemFacts, ErrorValue> {
    let statfs = rustix::fs::statfs(path).map_err(|error| {
        target_unresolved(
            &path.display().to_string(),
            &format!(
                "v0.6 Appendix B.1: the filesystem holding the path could not be read: {error}"
            ),
        )
    })?;
    let magic = i128::from(statfs.f_type);
    let name = filesystem_name(magic);
    let metadata = lstat(path)?;
    let device = metadata.dev();
    Ok(FilesystemFacts {
        magic,
        kind: FilesystemKind::of_mount_type(&name),
        name,
        device,
        mount_point: mount_point_of(path, device),
    })
}

/// The filesystem name a `statfs` magic number belongs to.
///
/// The table carries every pseudo, volatile and network filesystem Appendix B.6 and B.7 name,
/// because those are the ones where the classification changes the answer. A magic that is not in
/// it is reported as an unrecognised persistent filesystem: a file copy needs bytes that can be
/// read and written back and nothing else, and [`FilesystemKind::of_mount_type`] lands it on
/// `other`, which claims no snapshot mechanism.
fn filesystem_name(magic: i128) -> String {
    const MAGICS: &[(i128, &str)] = &[
        (0x0000_EF53, "ext4"),
        (0x9123_683E, "btrfs"),
        (0x5846_5342, "xfs"),
        (0x2FC1_2FC1, "zfs"),
        (0xF2F5_2010, "f2fs"),
        (0x0102_1994, "tmpfs"),
        (0x8584_58F6, "ramfs"),
        (0x0000_9FA0, "proc"),
        (0x6265_6572, "sysfs"),
        (0x6462_6720, "debugfs"),
        (0x7472_6163, "tracefs"),
        (0x0027_E0EB, "cgroup"),
        (0x6367_7270, "cgroup2"),
        (0x0000_1CD1, "devpts"),
        (0x7363_6673, "securityfs"),
        (0xF97C_FF8C, "selinuxfs"),
        (0xCAFE_4A11, "bpf"),
        (0x6165_676C, "pstore"),
        (0x6E73_6673, "nsfs"),
        (0xDE5E_81E4, "efivarfs"),
        (0x1980_0202, "mqueue"),
        (0x9584_58F6, "hugetlbfs"),
        (0x6265_6570, "configfs"),
        (0x4249_4E4D, "binfmt_misc"),
        (0x0000_0187, "autofs"),
        (0x6573_7543, "fusectl"),
        (0x6573_7546, "fuse"),
        (0x794C_7630, "overlay"),
        (0x0000_6969, "nfs"),
        (0xFF53_4D42, "cifs"),
        (0xFE53_4D42, "smb3"),
        (0x00C3_6400, "ceph"),
        (0x0102_1997, "9p"),
        (0x0116_1970, "gfs2"),
        (0x7461_636F, "ocfs2"),
        (0x0BD0_0BD0, "lustre"),
        (0x5346_414F, "afs"),
    ];
    MAGICS
        .iter()
        .find(|(candidate, _)| *candidate == magic)
        .map_or_else(
            || format!("unrecognised-0x{magic:x}"),
            |(_, name)| (*name).to_owned(),
        )
}

/// The mount point `path` sits in: the last ancestor still on `device` (Appendix B.1).
fn mount_point_of(path: &Path, device: u64) -> String {
    let mut mount_point = path.to_path_buf();
    let mut cursor = path;
    while let Some(parent) = cursor.parent() {
        match std::fs::symlink_metadata(parent) {
            Ok(metadata) if metadata.dev() == device => {
                mount_point = parent.to_path_buf();
                cursor = parent;
            }
            _ => break,
        }
    }
    mount_point.display().to_string()
}
