//! Appendix B's resolution pipeline, walked over the kernel's own mount table.
//!
//! Appendix B.1 fixes the pipeline — `path -> namespace-visible mount -> mount ID -> filesystem
//! type -> filesystem root -> backing persistence object -> snapshot boundaries -> provider
//! candidates` — and this module walks the first six steps. The vocabulary it produces
//! ([`PersistenceDomain`], [`ResolvedMount`], [`NonPersistentReason`]) belongs to
//! `ono-change-core`; what lives here is the reading of `/proc/self/mountinfo` that §50.1 keeps
//! out of the core, and the rules that decide what a mount *means*.
//!
//! Four of those rules are refusals, and they are the reason the module exists at all:
//!
//! - a network filesystem resolves to [`NonPersistentReason::Remote`], because Appendix B.6 says
//!   a local snapshot provider MUST NOT claim NFS, SMB or CephFS;
//! - procfs, sysfs, devtmpfs, cgroupfs, tracefs and debugfs resolve to
//!   [`NonPersistentReason::Pseudo`] and a tmpfs to [`NonPersistentReason::Volatile`], whatever
//!   their mountpoint looks like — §32.4's case is exactly a runtime tmpfs *beneath* a
//!   snapshotted `/`;
//! - an overlay whose writable upper layer this namespace can follow resolves to that layer's
//!   domain, and to [`NonPersistentReason::UpperLayerElsewhere`] when that layer is not
//!   protectable (Appendix B.4). An overlay whose upper layer cannot be followed at all — the
//!   container case, where `upperdir` names a host path — resolves to a domain of
//!   [`DomainReach::CopyOnly`]: B.4 forbids claiming that *snapshotting* the merged view
//!   protects it, and a copy of the visible bytes written back through the same view is not that
//!   claim;
//! - a filesystem Ono cannot classify resolves to [`NonPersistentReason::Unresolved`] rather than
//!   to a guess (§56.3).
//!
//! The two positive rules are just as narrow. A ZFS dataset comes from the mount's `source`
//! field, never from the shape of the path (Appendix B.8), and a Btrfs subvolume comes from the
//! `subvol=` and `subvolid=` mount options, so a directory *named* `@home` inside the `@home`
//! subvolume is still the `@home` subvolume (Appendix B.9).
//!
//! # Two readings of one line
//!
//! `MountInfo` is `ono-provider-linux`'s decoding of `mountinfo(5)`, and it deliberately drops
//! the kernel's per-mount id and the mount root. The identity Appendix B.3 asks a bind mount to
//! be traced to is the *filesystem's*, and that is what survives: the superblock's `major:minor`,
//! the source, and the subvolume options. [`ResolvedMount::mount_id`] therefore carries the
//! superblock device number, which is the identity two mounts of one filesystem share and two
//! filesystems never do.

use std::path::Path;
use std::sync::Arc;

use ono_change_core::{FilesystemKind, NonPersistentReason, PersistenceDomain, ResolvedMount};
use ono_core::ErrorCode;
use ono_value::{ByteSize, ErrorValue, Value};

pub use ono_provider_linux::decoders::{MountInfo, parse_mountinfo};

/// The kernel mount table Appendix B.1 resolves against (§23.5).
pub const MOUNTINFO: &str = "/proc/self/mountinfo";

/// The link whose target names the mount namespace a reading was taken in (Appendix B.2).
pub const MOUNT_NAMESPACE: &str = "/proc/self/ns/mnt";

/// How far the upper layer of an overlay is followed before the resolution is abandoned.
///
/// An overlay whose upper layer is itself an overlay is legal; a cycle is not, and Appendix B.4's
/// answer to "I cannot tell where the writable layer is" is a refusal rather than a loop.
const MAX_OVERLAY_DEPTH: usize = 4;

/// The mount table a resolution is performed against (Appendix B.1).
///
/// The table is a value so that a test can state the exact layout it means — Appendix G.2 asks
/// for deliberately misleading ones — and so a resolution performed for a container names the
/// namespace it was read in (Appendix B.2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MountTable {
    mounts: Vec<MountInfo>,
    namespace: Option<Arc<str>>,
}

impl MountTable {
    /// Reads the table the calling process sees, and the namespace it saw it in.
    ///
    /// # Errors
    ///
    /// A structured error when `/proc/self/mountinfo` cannot be read. §56.3 makes an unreadable
    /// mount table a refusal: without it no path has a persistence domain, and inventing one is
    /// how a plan comes to claim protection it does not have.
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
                "v0.6 Appendix B.1: every protection claim rests on mapping a path to the storage \
                 object that holds it. Without the mount table no path can be resolved, and \
                 nothing is assumed in its place",
            )
            .with_metadata("path", Value::string(MOUNTINFO))
        })?;
        let table = Self::from_text(&text);
        Ok(match std::fs::read_link(MOUNT_NAMESPACE) {
            Ok(link) => table.in_namespace(link.to_string_lossy().into_owned()),
            Err(_) => table,
        })
    }

    /// Parses recorded `mountinfo(5)` text (Appendix B.1).
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self {
            mounts: parse_mountinfo(text),
            namespace: None,
        }
    }

    /// Records the mount namespace the reading belongs to (Appendix B.2).
    ///
    /// A path seen inside a container maps differently from the host path of the same name, so
    /// the namespace travels with every domain resolved from this table.
    #[must_use]
    pub fn in_namespace(mut self, namespace: impl Into<Arc<str>>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// The mounts, in the order the kernel listed them.
    #[must_use]
    pub fn mounts(&self) -> &[MountInfo] {
        &self.mounts
    }

    /// The namespace the reading was taken in, where one is known.
    #[must_use]
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Resolves `path` against this table (Appendix B.1).
    #[must_use]
    pub fn resolve(&self, path: &Path) -> PersistenceDomain {
        resolve(path, &self.mounts, self.namespace())
    }

    /// Every filesystem mounted beneath `path`, in path order (§32.2, §32.3).
    ///
    /// §32.3: *"Recursive path operations MUST NOT assume mounted filesystems or nested
    /// subvolumes belong to the same recovery scope."* A removal of `path` reaches each of these,
    /// and each is resolved as a persistence domain of its own — a child dataset, a nested
    /// subvolume, an ext4 disk, a tmpfs, an NFS export — so the caller can show the boundaries
    /// before the plan is sealed (§32.2) and the coverage algorithm can require each one to be
    /// captured. `path` itself is not listed: it is the target, and it resolves the ordinary way.
    ///
    /// A mount is listed once, as the kernel shows it: of several mounts at one point the last
    /// wins, and a mount made beneath a point that was later mounted over is hidden and cannot be
    /// reached, so it is not listed at all. The mount table states no sizes, so every boundary
    /// comes back with [`MountBoundary::size`] unknown; a caller that measures one records it with
    /// [`MountBoundary::sized`].
    #[must_use]
    pub fn boundaries_beneath(&self, path: &Path) -> Vec<MountBoundary> {
        let Some(root) = absolute(path) else {
            return Vec::new();
        };
        let mut points: Vec<String> = Vec::new();
        for (index, info) in self.mounts.iter().enumerate() {
            let Some(point) = absolute(&info.target) else {
                continue;
            };
            if point == root || !contains(&root, &point) || points.contains(&point) {
                continue;
            }
            let hidden = self.mounts[index + 1..].iter().any(|later| {
                absolute(&later.target).is_some_and(|over| {
                    over != point && contains(&over, &point) && contains(&root, &over)
                })
            });
            if !hidden {
                points.push(point);
            }
        }
        points.sort();
        points
            .into_iter()
            .map(|point| MountBoundary {
                domain: self.resolve(Path::new(&point)),
                mount_point: Arc::from(point),
                size: None,
            })
            .collect()
    }

    /// The mount `path` is served by, deepest first (Appendix B.1).
    ///
    /// Deepest wins, and among mounts at the same point the last one wins: that is what the
    /// kernel shows, and a tmpfs mounted over a directory that already had one is the case where
    /// getting it wrong claims protection for the wrong filesystem.
    #[must_use]
    pub fn mount_for(&self, path: &Path) -> Option<&MountInfo> {
        let target = absolute(path)?;
        deepest(&target, &self.mounts).map(|index| &self.mounts[index])
    }
}

/// One filesystem mounted beneath a directory a mutation reaches (§32.2, §32.3).
///
/// Each boundary is a persistence domain of its own, and a recursive mutation of the directory
/// above it is protected only where something captures it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountBoundary {
    mount_point: Arc<str>,
    domain: PersistenceDomain,
    size: Option<ByteSize>,
}

impl MountBoundary {
    /// Where the filesystem is mounted.
    #[must_use]
    pub fn mount_point(&self) -> &str {
        &self.mount_point
    }

    /// The persistence domain the mount resolves to.
    #[must_use]
    pub const fn domain(&self) -> &PersistenceDomain {
        &self.domain
    }

    /// How much the mount holds, where it was measured (§32.2's size before sealing).
    ///
    /// `None` is unknown rather than empty (§35.3).
    #[must_use]
    pub const fn size(&self) -> Option<ByteSize> {
        self.size
    }

    /// Records how much the mount holds.
    #[must_use]
    pub const fn sized(mut self, size: ByteSize) -> Self {
        self.size = Some(size);
        self
    }
}

/// Resolves `path` to the persistence object that holds its state (Appendix B.1).
///
/// `mounts` is passed in rather than read, because Appendix B.2 ties the answer to the namespace
/// the mutation will happen in: the caller decides which table that is. `namespace` is recorded on
/// the result so a resolution taken in a container cannot later be read as a host one.
#[must_use]
pub fn resolve(path: &Path, mounts: &[MountInfo], namespace: Option<&str>) -> PersistenceDomain {
    let subject = path.to_string_lossy().into_owned();
    match absolute(path) {
        Some(target) => resolve_at(&subject, &target, mounts, namespace, 0),
        None => PersistenceDomain::refused(
            subject,
            no_mount(namespace),
            NonPersistentReason::Unresolved,
            "a relative path has no namespace-visible mount, and Appendix B.1 resolves from a \
             mount rather than from a name",
        ),
    }
}

/// Resolves `lookup` and reports the answer as belonging to `subject`.
fn resolve_at(
    subject: &str,
    lookup: &str,
    mounts: &[MountInfo],
    namespace: Option<&str>,
    depth: usize,
) -> PersistenceDomain {
    let Some(index) = deepest(lookup, mounts) else {
        return PersistenceDomain::refused(
            subject,
            no_mount(namespace),
            NonPersistentReason::Unresolved,
            format!("no mount in this namespace contains {lookup}"),
        );
    };
    let info = &mounts[index];
    let mount = resolved_mount(info, namespace);
    let read_only = if mount.is_read_only() {
        ", and the mount is read-only, so a restore could not write to it"
    } else {
        ""
    };
    // Appendix B.3: a bind mount is not a persistence domain of its own, so the answer names the
    // filesystem it was traced to.
    let traced = underlying(index, mounts).map_or_else(String::new, |primary| {
        format!(
            ", traced through a bind mount to the same filesystem at {}",
            primary.target.display()
        )
    });

    match mount.kind() {
        FilesystemKind::Pseudo => PersistenceDomain::refused(
            subject,
            mount,
            NonPersistentReason::Pseudo,
            format!(
                "{} is {}, a runtime interface; §32.4 forbids presenting it as snapshot-protected \
                 because its mountpoint is beneath `/`",
                info.target.display(),
                info.filesystem
            ),
        ),
        FilesystemKind::Volatile => PersistenceDomain::refused(
            subject,
            mount,
            NonPersistentReason::Volatile,
            format!(
                "{} is a {} whose contents do not survive a reboot; a snapshot of the filesystem \
                 above it holds none of this state (Appendix B.7)",
                info.target.display(),
                info.filesystem
            ),
        ),
        FilesystemKind::Network => PersistenceDomain::refused(
            subject,
            mount,
            NonPersistentReason::Remote,
            format!(
                "{} is served by {} over {}; Appendix B.6 forbids a local snapshot provider from \
                 claiming it, and recovery is unknown locally",
                info.target.display(),
                info.source,
                info.filesystem
            ),
        ),
        FilesystemKind::Unknown => PersistenceDomain::refused(
            subject,
            mount,
            NonPersistentReason::Unresolved,
            format!(
                "the kernel names no filesystem type for {}, and Appendix B.8 and B.9 forbid \
                 inferring a recovery mechanism from anything else",
                info.target.display()
            ),
        ),
        FilesystemKind::Overlay => resolve_overlay(subject, index, mount, mounts, namespace, depth),
        FilesystemKind::Zfs => {
            // Appendix B.8: the dataset is what the kernel says is mounted here. `/rpool/data`
            // is a path; `tank/things` is a dataset, and only one of them is evidence.
            let dataset = info.source.clone();
            PersistenceDomain::resolved(
                subject,
                mount,
                "zfs-dataset",
                dataset.clone(),
                format!(
                    "mount {} carries the ZFS dataset {dataset}, taken from mount metadata rather \
                     than from the shape of the path (Appendix B.8){traced}{read_only}",
                    info.target.display()
                ),
            )
            .with_boundary(dataset)
        }
        FilesystemKind::Btrfs => {
            // Appendix B.9: the subvolume is the mount's own, and a nested one is a separate
            // boundary reached through its own mount.
            let subvolume = mount
                .option("subvol")
                .map_or_else(|| Arc::from("/"), Arc::<str>::from);
            let identity = mount.option("subvolid").map_or_else(
                || format!("{subvolume}"),
                |id| format!("{subvolume} (subvolid={id})"),
            );
            PersistenceDomain::resolved(
                subject,
                mount,
                "btrfs-subvolume",
                Arc::clone(&subvolume),
                format!(
                    "mount {} carries the Btrfs subvolume {identity} of {}; a directory merely \
                     named like a subvolume is not one (Appendix B.9){traced}{read_only}",
                    info.target.display(),
                    info.source
                ),
            )
            .with_boundary(subvolume)
        }
        FilesystemKind::Lvm
        | FilesystemKind::Ext4
        | FilesystemKind::Xfs
        | FilesystemKind::Other => PersistenceDomain::resolved(
            subject,
            mount,
            "filesystem",
            info.source.clone(),
            format!(
                "mount {} is {} on {}{traced}. The filesystem has no snapshot mechanism of \
                     its own, so protection depends on a provider that copies state{read_only}",
                info.target.display(),
                info.filesystem,
                info.source
            ),
        ),
    }
}

/// Which mechanisms may claim a resolved domain (Appendix B.4, B.6, B.7).
///
/// [`PersistenceDomain::is_protectable`] answers whether *anything* local may be asked. Appendix
/// B.4 draws a finer line for an overlay whose writable layer this namespace cannot see: a
/// snapshot of the merged mount would not hold that layer, so no snapshot provider may claim it,
/// while a provider that copies the visible bytes and writes them back through the same merged
/// view protects exactly what it read. The distinction is read off the domain's mount — a domain
/// that is protectable and still resolved *through* an overlay is one whose upper layer could not
/// be followed — so it is a property of the resolution rather than of a sentence in its detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainReach {
    /// No local provider may claim the domain (Appendix B.4, B.6, B.7, §56.3).
    Refused,
    /// Only a copy of the visible bytes, written back through the same view, may claim it
    /// (Appendix B.4).
    CopyOnly,
    /// Any provider whose mechanism the domain's filesystem supports may claim it.
    Any,
}

impl DomainReach {
    /// The reach of `domain`.
    #[must_use]
    pub fn of(domain: &PersistenceDomain) -> Self {
        if !domain.is_protectable() {
            Self::Refused
        } else if domain.mount().kind() == FilesystemKind::Overlay {
            Self::CopyOnly
        } else {
            Self::Any
        }
    }

    /// Whether a snapshot of the domain's filesystem may be claimed to protect it (Appendix B.4).
    #[must_use]
    pub const fn admits_snapshot(self) -> bool {
        matches!(self, Self::Any)
    }

    /// Whether a copy of the visible bytes may be claimed to protect it (§15, Appendix B.4).
    #[must_use]
    pub const fn admits_copy(self) -> bool {
        matches!(self, Self::Any | Self::CopyOnly)
    }
}

/// The persistence domain a frozen target records beside its identity (§7.1, Appendix B).
///
/// The snapshot boundary where the mechanism has one, otherwise the backing object. A path the
/// resolution refused — a tmpfs, procfs, a network export, an overlay whose writable layer is on a
/// volatile filesystem — records none at all: Appendix B.7 says such a filesystem is never a
/// persistence domain, and its mount source (`shm`, `proc`, `tmpfs`) is a label rather than one.
#[must_use]
pub fn recorded_domain(domain: &PersistenceDomain) -> Option<&str> {
    if !domain.is_protectable() {
        return None;
    }
    domain.boundary().or_else(|| domain.object())
}

/// Appendix B.4: a mutation of an overlay lands in its writable upper layer.
///
/// Three answers, and the order matters. An overlay with no upper layer has nowhere a write could
/// land, and is refused. An upper layer this namespace can see is followed, and its domain is the
/// answer — refused if that layer is itself not protectable. An upper layer this namespace cannot
/// see (no mount here contains it, or the only one that does is this overlay) cannot be followed,
/// and the merged view becomes a [`DomainReach::CopyOnly`] domain.
fn resolve_overlay(
    subject: &str,
    index: usize,
    mount: ResolvedMount,
    mounts: &[MountInfo],
    namespace: Option<&str>,
    depth: usize,
) -> PersistenceDomain {
    let info = &mounts[index];
    let Some(upper) = mount.option("upperdir").map(str::to_owned) else {
        return PersistenceDomain::refused(
            subject,
            mount,
            NonPersistentReason::UpperLayerElsewhere,
            format!(
                "the overlay at {} declares no writable upper layer, so a mutation has nowhere \
                 recoverable to land (Appendix B.4)",
                info.target.display()
            ),
        );
    };
    if depth >= MAX_OVERLAY_DEPTH {
        return PersistenceDomain::refused(
            subject,
            mount,
            NonPersistentReason::UpperLayerElsewhere,
            format!(
                "the writable layer of the overlay at {} could not be followed to a filesystem \
                 within {MAX_OVERLAY_DEPTH} steps (Appendix B.4)",
                info.target.display()
            ),
        );
    }
    let hidden = match absolute(Path::new(&upper)) {
        Some(lookup) => deepest(&lookup, mounts).is_none_or(|holder| holder == index),
        None => true,
    };
    if hidden {
        return copy_only(subject, info, mount, &upper);
    }
    let resolved = resolve_at(subject, &upper, mounts, namespace, depth + 1);
    match DomainReach::of(&resolved) {
        DomainReach::Refused => PersistenceDomain::refused(
            subject,
            mount,
            NonPersistentReason::UpperLayerElsewhere,
            format!(
                "the writable upper layer of the overlay at {} is {upper}, which is not a \
                 persistence domain a local provider can protect: {}. Appendix B.4 forbids \
                 claiming that snapshotting the merged mount protects this data",
                info.target.display(),
                resolved.detail()
            ),
        ),
        // The upper layer is itself an overlay whose own writable layer is out of sight: the
        // bytes are still reachable through this merged view, and through nothing else.
        DomainReach::CopyOnly => copy_only(subject, info, mount, &upper),
        DomainReach::Any => {
            let object = resolved
                .object()
                .map_or_else(|| Arc::from(upper.as_str()), Arc::<str>::from);
            let domain = PersistenceDomain::resolved(
                subject,
                resolved.mount().clone(),
                resolved.object_kind().to_owned(),
                Arc::clone(&object),
                format!(
                    "the mutation lands in the writable upper layer {upper} of the overlay at {}, \
                     which resolves to {object}; the merged mount itself holds nothing \
                     (Appendix B.4)",
                    info.target.display()
                ),
            );
            match resolved.boundary() {
                Some(boundary) => domain.with_boundary(boundary.to_owned()),
                None => domain,
            }
        }
    }
}

/// The merged view of an overlay whose writable layer cannot be followed (Appendix B.4, B.5).
///
/// The domain stays on the overlay's own mount, which is what [`DomainReach::of`] reads, and it
/// names the upper layer as its object because that is where the bytes a mutation writes land.
/// It carries no snapshot boundary: there is none a provider here could take.
fn copy_only(
    subject: &str,
    info: &MountInfo,
    mount: ResolvedMount,
    upper: &str,
) -> PersistenceDomain {
    PersistenceDomain::resolved(
        subject,
        mount,
        "overlay-writable-layer",
        upper,
        format!(
            "the writable upper layer {upper} of the overlay at {} is not visible in this mount \
             namespace, so it cannot be followed to a filesystem. Appendix B.4 forbids claiming \
             that snapshotting the merged mount protects this data; only a copy of the visible \
             bytes, written back through the same merged view, may protect it. A container's \
             writable layer may be ephemeral (Appendix B.5), and a copy kept on the same overlay \
             shares its failure domain (§11.5)",
            info.target.display()
        ),
    )
}

/// Turns one `mountinfo(5)` line into the vocabulary's mount (Appendix B.1).
fn resolved_mount(info: &MountInfo, namespace: Option<&str>) -> ResolvedMount {
    let mount = ResolvedMount::new(
        info.device_number.clone(),
        info.target.to_string_lossy().into_owned(),
        info.filesystem.clone(),
        info.source.clone(),
        info.options
            .iter()
            .find_map(|option| option.strip_prefix("subvol="))
            .unwrap_or("/"),
    )
    .with_options(
        info.options
            .iter()
            .map(|option| Arc::from(option.as_str()))
            .collect(),
    );
    match namespace {
        Some(namespace) => mount.in_namespace(namespace),
        None => mount,
    }
}

/// The empty mount a refusal is hung on when Appendix B.1's pipeline found none at all.
///
/// A refusal still has to name where it was looking, and `PersistenceDomain::refused` takes a
/// mount because every other refusal has one. This is the honest answer for the two cases that do
/// not — a relative path, and a path no mount in the namespace contains.
fn no_mount(namespace: Option<&str>) -> ResolvedMount {
    let mount = ResolvedMount::new("0:0", "", "", "", "/");
    match namespace {
        Some(namespace) => mount.in_namespace(namespace),
        None => mount,
    }
}

/// The index of the mount serving `path`, deepest and latest first (Appendix B.1).
fn deepest(path: &str, mounts: &[MountInfo]) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (index, info) in mounts.iter().enumerate() {
        let target = info.target.to_string_lossy();
        if !contains(&target, path) {
            continue;
        }
        let depth = target.trim_end_matches('/').len();
        if best.is_none_or(|(_, previous)| depth >= previous) {
            best = Some((index, depth));
        }
    }
    best.map(|(index, _)| index)
}

/// Appendix B.3: the mount of the same filesystem this one is a bind or later mount of.
///
/// Sameness is the superblock plus the source plus the subvolume: two Btrfs subvolumes share a
/// superblock and are separate persistence domains, which is why the option is part of the test.
fn underlying(index: usize, mounts: &[MountInfo]) -> Option<&MountInfo> {
    let info = mounts.get(index)?;
    let subvolume = subvolume_of(info);
    let mut best: Option<&MountInfo> = None;
    for (position, other) in mounts.iter().enumerate() {
        if position == index
            || other.device_number != info.device_number
            || other.source != info.source
            || subvolume_of(other) != subvolume
        {
            continue;
        }
        let shorter = best.is_none_or(|current| {
            other.target.as_os_str().len() < current.target.as_os_str().len()
        });
        if shorter {
            best = Some(other);
        }
    }
    best.filter(|primary| primary.target.as_os_str().len() < info.target.as_os_str().len())
}

fn subvolume_of(info: &MountInfo) -> Option<&str> {
    info.options
        .iter()
        .find_map(|option| option.strip_prefix("subvol="))
}

/// Whether the mount at `target` serves `path`, by whole path components.
///
/// `/var` serves `/var/lib/app` and does not serve `/variable`: a prefix test on the raw text
/// would claim the second, and claim protection for a filesystem that holds nothing of it.
pub(crate) fn contains(target: &str, path: &str) -> bool {
    let target = target.trim_end_matches('/');
    if target.is_empty() {
        return path.starts_with('/');
    }
    path == target || path.starts_with(&format!("{target}/"))
}

/// Normalises `path` lexically, without touching the filesystem.
///
/// §2.1 keeps planning side-effect free and the brief keeps tests off the developer's own disk,
/// so `.` and `..` are resolved by text. A path that is not absolute has no answer here at all.
fn absolute(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    if !text.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for component in text.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    Some(format!("/{}", parts.join("/")))
}
