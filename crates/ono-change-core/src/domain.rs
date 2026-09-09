//! Persistence-domain resolution, as a vocabulary (spec v0.6 Appendix B).
//!
//! Appendix B is normative for first-party Linux support and its pipeline is
//! `path -> mount -> filesystem type -> filesystem root -> backing persistence object ->
//! snapshot boundaries -> provider candidates`. The types live here; the resolver that walks
//! `/proc/self/mountinfo` lives in `ono-change-protection`, because §50.1 keeps I/O out of the
//! core.
//!
//! The one rule the vocabulary itself enforces is Appendix B.7's: procfs, sysfs, devtmpfs, tmpfs,
//! cgroupfs, tracefs and debugfs *"are never automatically considered persistent recovery
//! domains"*. [`FilesystemKind::is_persistent`] answers `false` for each of them, so §32.4's
//! refusal — a path under `/` that lives on a tmpfs is not snapshot-protected — is a property of
//! the classification rather than a check somebody has to remember to write.

use std::sync::Arc;

use crate::vocab::vocabulary;

vocabulary! {
    /// What kind of filesystem a mount is, as far as recovery is concerned (Appendix B).
    FilesystemKind {
        Zfs => "zfs", "§13: a ZFS dataset. Its snapshot boundary is the dataset (§13.4).";
        Btrfs => "btrfs", "§14: a Btrfs subvolume. Its snapshot boundary is the subvolume, and nested subvolumes are separate boundaries (§14.3).";
        Ext4 => "ext4", "A conventional filesystem with no snapshot mechanism of its own.";
        Xfs => "xfs", "A conventional filesystem with no snapshot mechanism of its own.";
        Lvm => "lvm", "§16.1: a filesystem on a logical volume, where an LVM snapshot may be possible.";
        Overlay => "overlay", "Appendix B.4: a union mount. The writable upper layer is what a mutation lands in, and snapshotting the merged view protects nothing.";
        Network => "network", "Appendix B.6: NFS, SMB, CephFS and friends. A local snapshot provider MUST NOT claim protection.";
        Pseudo => "pseudo", "Appendix B.7: procfs, sysfs, devtmpfs, cgroupfs, tracefs, debugfs. Runtime, not persistence.";
        Volatile => "volatile", "Appendix B.7: tmpfs and ramfs. Persistent-looking paths whose contents do not survive a reboot.";
        Other => "other", "A filesystem Ono has classified as persistent but has no recovery provider for.";
        Unknown => "unknown", "A filesystem Ono could not classify, which §56.3 makes a reason to refuse rather than assume.";
    }
}

impl FilesystemKind {
    /// Whether state on this filesystem survives a reboot (Appendix B.7).
    #[must_use]
    pub const fn is_persistent(self) -> bool {
        matches!(
            self,
            FilesystemKind::Zfs
                | FilesystemKind::Btrfs
                | FilesystemKind::Ext4
                | FilesystemKind::Xfs
                | FilesystemKind::Lvm
                | FilesystemKind::Other
                | FilesystemKind::Overlay
                | FilesystemKind::Network
        )
    }

    /// Whether a local snapshot provider may claim to protect this filesystem (Appendix B.6).
    #[must_use]
    pub const fn admits_local_snapshot(self) -> bool {
        matches!(
            self,
            FilesystemKind::Zfs | FilesystemKind::Btrfs | FilesystemKind::Lvm
        )
    }

    /// Whether the persistent state actually lives somewhere other than this mount.
    ///
    /// Appendix B.4 and B.5: for an overlay, the writable upper layer is the thing that holds a
    /// mutation, and a snapshot of the merged mount protects nothing at all.
    #[must_use]
    pub const fn state_lives_elsewhere(self) -> bool {
        matches!(self, FilesystemKind::Overlay | FilesystemKind::Network)
    }

    /// Reads a `/proc/self/mountinfo` filesystem name into a classification (Appendix B).
    ///
    /// The network list matches `ono-provider-linux`'s, and everything unrecognised lands on
    /// [`FilesystemKind::Other`] rather than on a guess: Appendix B.8 and B.9 both forbid
    /// inferring a snapshot mechanism from a name.
    #[must_use]
    pub fn of_mount_type(name: &str) -> Self {
        let bare = name.strip_prefix("fuse.").unwrap_or(name);
        match bare {
            "zfs" => FilesystemKind::Zfs,
            "btrfs" => FilesystemKind::Btrfs,
            "ext2" | "ext3" | "ext4" => FilesystemKind::Ext4,
            "xfs" => FilesystemKind::Xfs,
            "overlay" | "overlayfs" | "aufs" | "unionfs" => FilesystemKind::Overlay,
            "tmpfs" | "ramfs" => FilesystemKind::Volatile,
            "proc" | "procfs" | "sysfs" | "devtmpfs" | "devpts" | "cgroup" | "cgroup2"
            | "tracefs" | "debugfs" | "securityfs" | "pstore" | "bpf" | "configfs" | "fusectl"
            | "hugetlbfs" | "mqueue" | "efivarfs" | "binfmt_misc" | "autofs" | "rpc_pipefs"
            | "nsfs" | "selinuxfs" => FilesystemKind::Pseudo,
            "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "afs" | "ceph" | "glusterfs"
            | "lustre" | "ocfs2" | "9p" | "sshfs" | "davfs" | "beegfs" | "orangefs" | "gfs2"
            | "pvfs2" => FilesystemKind::Network,
            "" => FilesystemKind::Unknown,
            _ => FilesystemKind::Other,
        }
    }
}

vocabulary! {
    /// Why a path is not a persistence domain a local provider can protect (Appendix B).
    NonPersistentReason {
        Pseudo => "pseudo-filesystem", "Appendix B.7 and §32.4: procfs, sysfs and friends are runtime interfaces, whatever their mountpoint looks like.";
        Volatile => "volatile-filesystem", "Appendix B.7: a tmpfs beneath `/` is not protected by a snapshot of `/`.";
        Remote => "remote-filesystem", "Appendix B.6: the bytes live on another machine, and a local snapshot provider MUST NOT claim them.";
        UpperLayerElsewhere => "upper-layer-elsewhere", "Appendix B.4: the writable layer of this overlay is not inside what would be snapshotted.";
        ContainerEphemeral => "container-ephemeral", "Appendix B.5: the change lands in a container writable layer that the host snapshot does not hold.";
        NoProvider => "no-provider", "The filesystem is persistent and no recovery provider covers it.";
        Unresolved => "unresolved", "Appendix B.1's pipeline could not be completed, which §56.3 makes a refusal rather than a default.";
    }
}

/// One mount, as Appendix B.1's pipeline resolved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMount {
    mount_id: Arc<str>,
    mount_point: Arc<str>,
    filesystem: Arc<str>,
    kind: FilesystemKind,
    source: Arc<str>,
    root: Arc<str>,
    options: Vec<Arc<str>>,
    read_only: bool,
    namespace: Option<Arc<str>>,
}

impl ResolvedMount {
    /// Records the mount at `mount_point` of type `filesystem` backed by `source`.
    #[must_use]
    pub fn new(
        mount_id: impl Into<Arc<str>>,
        mount_point: impl Into<Arc<str>>,
        filesystem: impl Into<Arc<str>>,
        source: impl Into<Arc<str>>,
        root: impl Into<Arc<str>>,
    ) -> Self {
        let filesystem = filesystem.into();
        Self {
            mount_id: mount_id.into(),
            mount_point: mount_point.into(),
            kind: FilesystemKind::of_mount_type(&filesystem),
            filesystem,
            source: source.into(),
            root: root.into(),
            options: Vec::new(),
            read_only: false,
            namespace: None,
        }
    }

    /// Records the mount options, which is where `subvol=` and `subvolid=` live (Appendix B.9).
    #[must_use]
    pub fn with_options(mut self, options: Vec<Arc<str>>) -> Self {
        self.read_only = options.iter().any(|option| option.as_ref() == "ro");
        self.options = options;
        self
    }

    /// Records the mount namespace this reading was taken in (Appendix B.2).
    #[must_use]
    pub fn in_namespace(mut self, namespace: impl Into<Arc<str>>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// The kernel's mount id.
    #[must_use]
    pub fn mount_id(&self) -> &str {
        &self.mount_id
    }

    /// Where it is mounted.
    #[must_use]
    pub fn mount_point(&self) -> &str {
        &self.mount_point
    }

    /// The filesystem type as the kernel spells it.
    #[must_use]
    pub fn filesystem(&self) -> &str {
        &self.filesystem
    }

    /// How Ono classified it.
    #[must_use]
    pub const fn kind(&self) -> FilesystemKind {
        self.kind
    }

    /// The mount source — a device, a dataset name, a server export.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The mount's root within its filesystem, which is what tells a Btrfs subvolume apart.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// The mount options.
    #[must_use]
    pub fn options(&self) -> &[Arc<str>] {
        &self.options
    }

    /// The value of a `key=value` mount option, where it is present.
    #[must_use]
    pub fn option(&self, key: &str) -> Option<&str> {
        let prefix = format!("{key}=");
        self.options
            .iter()
            .find_map(|option| option.strip_prefix(&prefix))
    }

    /// Whether the mount is read-only, which blocks a restore into it (Appendix G.2).
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// The mount namespace the reading was taken in (Appendix B.2).
    #[must_use]
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }
}

/// The persistence object that actually holds a target's state (Appendix B.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistenceDomain {
    path: Arc<str>,
    mount: ResolvedMount,
    object: Option<Arc<str>>,
    object_kind: Arc<str>,
    boundary: Option<Arc<str>>,
    refusal: Option<NonPersistentReason>,
    detail: Arc<str>,
}

impl PersistenceDomain {
    /// Records that `path` resolves to `object` on `mount`.
    #[must_use]
    pub fn resolved(
        path: impl Into<Arc<str>>,
        mount: ResolvedMount,
        object_kind: impl Into<Arc<str>>,
        object: impl Into<Arc<str>>,
        detail: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            path: path.into(),
            mount,
            object: Some(object.into()),
            object_kind: object_kind.into(),
            boundary: None,
            refusal: None,
            detail: detail.into(),
        }
    }

    /// Records that `path` has no persistence domain a local provider can protect.
    #[must_use]
    pub fn refused(
        path: impl Into<Arc<str>>,
        mount: ResolvedMount,
        reason: NonPersistentReason,
        detail: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            path: path.into(),
            mount,
            object: None,
            object_kind: Arc::from("none"),
            boundary: None,
            refusal: Some(reason),
            detail: detail.into(),
        }
    }

    /// Records the snapshot boundary the object sits inside (§13.4, §14.3).
    #[must_use]
    pub fn with_boundary(mut self, boundary: impl Into<Arc<str>>) -> Self {
        self.boundary = Some(boundary.into());
        self
    }

    /// The path that was resolved.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The mount it resolved through.
    #[must_use]
    pub const fn mount(&self) -> &ResolvedMount {
        &self.mount
    }

    /// The backing persistence object — a dataset name, a subvolume id, a logical volume.
    #[must_use]
    pub fn object(&self) -> Option<&str> {
        self.object.as_deref()
    }

    /// What kind of object it is.
    #[must_use]
    pub fn object_kind(&self) -> &str {
        &self.object_kind
    }

    /// The snapshot boundary, where the mechanism has one.
    #[must_use]
    pub fn boundary(&self) -> Option<&str> {
        self.boundary.as_deref()
    }

    /// Why the path has no protectable domain, where it does not.
    #[must_use]
    pub const fn refusal(&self) -> Option<NonPersistentReason> {
        self.refusal
    }

    /// The sentence `inspect plan` shows beside the resolution (Appendix B.10).
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Whether a local recovery provider may be asked to protect this path.
    #[must_use]
    pub fn is_protectable(&self) -> bool {
        self.refusal.is_none() && self.object.is_some() && self.mount.kind().is_persistent()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    fn mount(kind: &str) -> ResolvedMount {
        ResolvedMount::new("42", "/", kind, "source", "/")
    }

    #[test]
    fn should_never_treat_a_pseudo_filesystem_as_persistent() {
        for name in ["proc", "sysfs", "devtmpfs", "cgroup2", "tracefs", "debugfs"] {
            let kind = FilesystemKind::of_mount_type(name);
            assert!(
                !kind.is_persistent(),
                "Appendix B.7 and §32.4: `{name}` is never a persistent recovery domain"
            );
        }
    }

    #[test]
    fn should_never_treat_a_tmpfs_beneath_the_root_as_snapshot_protected() {
        let volatile = FilesystemKind::of_mount_type("tmpfs");
        assert_eq!(volatile, FilesystemKind::Volatile);
        assert!(
            !volatile.is_persistent(),
            "§32.4: a runtime tmpfs is not protected because its path is beneath `/`"
        );
    }

    #[test]
    fn should_refuse_a_local_snapshot_claim_over_a_network_filesystem() {
        for name in ["nfs4", "cifs", "ceph", "fuse.sshfs"] {
            let kind = FilesystemKind::of_mount_type(name);
            assert_eq!(kind, FilesystemKind::Network, "`{name}` is a network mount");
            assert!(
                !kind.admits_local_snapshot(),
                "Appendix B.6: a local snapshot provider MUST NOT claim `{name}`"
            );
        }
    }

    #[test]
    fn should_say_that_an_overlay_keeps_its_writable_state_elsewhere() {
        let overlay = FilesystemKind::of_mount_type("overlay");
        assert!(
            overlay.state_lives_elsewhere(),
            "Appendix B.4: snapshotting a merged mount does not protect the upper layer"
        );
    }

    #[test]
    fn should_classify_an_unrecognised_filesystem_without_inventing_a_mechanism() {
        let other = FilesystemKind::of_mount_type("bcachefs");
        assert_eq!(other, FilesystemKind::Other);
        assert!(
            !other.admits_local_snapshot(),
            "Appendix B.8 and B.9: a mechanism is never inferred from a name"
        );
    }

    #[test]
    fn should_keep_the_subvolume_options_a_btrfs_mount_carries() {
        let btrfs = mount("btrfs").with_options(vec![
            Arc::from("rw"),
            Arc::from("subvol=/@home"),
            Arc::from("subvolid=257"),
        ]);
        assert_eq!(btrfs.option("subvol"), Some("/@home"));
        assert_eq!(btrfs.option("subvolid"), Some("257"));
        assert!(!btrfs.is_read_only());
    }

    #[test]
    fn should_notice_a_read_only_mount_a_restore_could_not_write_to() {
        let read_only = mount("ext4").with_options(vec![Arc::from("ro")]);
        assert!(
            read_only.is_read_only(),
            "Appendix G.2: a read-only filesystem preventing restore is a truth test"
        );
    }

    #[test]
    fn should_refuse_to_call_an_unresolved_path_protectable() {
        let refused = PersistenceDomain::refused(
            "/proc/1/status",
            mount("proc"),
            NonPersistentReason::Pseudo,
            "procfs is a runtime interface",
        );
        assert!(!refused.is_protectable());
        assert_eq!(refused.refusal(), Some(NonPersistentReason::Pseudo));
    }

    #[test]
    fn should_call_a_resolved_dataset_protectable() {
        let resolved = PersistenceDomain::resolved(
            "/etc/nginx/nginx.conf",
            mount("zfs"),
            "zfs-dataset",
            "rpool/ROOT/debian",
            "the path resolves to the root dataset",
        )
        .with_boundary("rpool/ROOT/debian");
        assert!(resolved.is_protectable());
        assert_eq!(resolved.boundary(), Some("rpool/ROOT/debian"));
    }
}
