//! Appendix B's resolution pipeline, over mount tables a kernel actually printed.
//!
//! Appendix G.2 asks for the misleading layouts on purpose — "path under ZFS mount but actually
//! separate child dataset", "Btrfs nested subvolume", "bind mount crossing to ext4", "NFS mount
//! below snapshotted root", "read-only filesystem preventing restore" — and every one of them is
//! a case here, because the resolver's whole value is refusing to claim what it cannot.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;

use ono_change_core::{FilesystemKind, NonPersistentReason};
use ono_change_protection::MountTable;
use ono_change_protection::domain::{DomainReach, recorded_domain};

mod support;

use support::{BTRFS_ROOT, CONTAINER, EXT4_ROOT, ZFS_ROOT};

fn zfs() -> MountTable {
    MountTable::from_text(ZFS_ROOT)
}

fn btrfs() -> MountTable {
    MountTable::from_text(BTRFS_ROOT)
}

fn ext4() -> MountTable {
    MountTable::from_text(EXT4_ROOT)
}

#[test]
fn should_resolve_a_path_to_its_deepest_mount_when_several_contain_it() {
    let domain = zfs().resolve(Path::new("/var/lib/app/state.db"));
    assert_eq!(
        domain.object(),
        Some("rpool/var"),
        "Appendix B.1: `/var` is a dataset of its own, and the root dataset does not hold it"
    );
}

#[test]
fn should_refuse_to_treat_a_path_named_like_a_mount_as_being_on_it() {
    let domain = zfs().resolve(Path::new("/variable/state.db"));
    assert_eq!(
        domain.object(),
        Some("rpool/ROOT/debian"),
        "Appendix B.1: `/var` contains `/var/lib`, and contains nothing of `/variable`"
    );
}

#[test]
fn should_resolve_a_normalised_path_when_it_walks_through_a_parent_directory() {
    let domain = zfs().resolve(Path::new("/etc/../var/./lib/app"));
    assert_eq!(
        domain.object(),
        Some("rpool/var"),
        "Appendix B.1 resolves the path the mutation will land on, not the text it was typed as"
    );
}

#[test]
fn should_prefer_the_last_mount_when_two_filesystems_share_a_mountpoint() {
    let shadowed = "\
23 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw
24 23 0:31 / /var/cache rw,relatime shared:8 - ext4 /dev/sdb1 rw
25 23 0:32 / /var/cache rw,relatime shared:9 - tmpfs tmpfs rw,size=64m
";
    let domain = MountTable::from_text(shadowed).resolve(Path::new("/var/cache/build"));
    assert_eq!(
        domain.refusal(),
        Some(NonPersistentReason::Volatile),
        "Appendix B.1: the visible mount is the last one at the point, and it is a tmpfs"
    );
}

#[test]
fn should_trace_a_bind_mount_to_the_dataset_that_holds_it() {
    let table = zfs();
    let bound = table.resolve(Path::new("/mnt/etc/nginx/nginx.conf"));
    let direct = table.resolve(Path::new("/etc/nginx/nginx.conf"));
    assert_eq!(
        bound.object(),
        direct.object(),
        "Appendix B.3: a bind mount does not by itself create a separate persistence domain"
    );
    assert!(
        bound.detail().contains("bind mount"),
        "Appendix B.3: the resolution says which filesystem identity it was traced to: {}",
        bound.detail()
    );
}

#[test]
fn should_trace_a_bind_mount_that_crosses_to_another_filesystem_to_that_filesystem() {
    // Appendix G.2 truth test: bind-mount-crossing.
    let domain = ext4().resolve(Path::new("/etc/app-config/app.toml"));
    assert_eq!(
        domain.object(),
        Some("/dev/sda2"),
        "Appendix G.2: a bind mount crossing to ext4 resolves to the ext4 filesystem"
    );
    assert_eq!(domain.mount().kind(), FilesystemKind::Ext4);
    assert_eq!(
        domain.boundary(),
        None,
        "Appendix B.1: ext4 has no snapshot boundary of its own to name"
    );
}

#[test]
fn should_resolve_an_overlay_to_its_writable_upper_layer_when_that_layer_is_persistent() {
    // Appendix G.2 truth test: container-bind-mount.
    let domain = ext4().resolve(Path::new(
        "/var/lib/docker/overlay2/9f3a/merged/etc/app.conf",
    ));
    assert_eq!(
        domain.object(),
        Some("/dev/sda2"),
        "Appendix B.4: the mutation lands in the writable upper layer, which is on the root ext4"
    );
    assert!(
        domain.detail().contains("upper layer"),
        "Appendix B.4: the resolution names the layer the mutation lands in: {}",
        domain.detail()
    );
}

#[test]
fn should_refuse_an_overlay_whose_writable_layer_is_on_a_volatile_filesystem() {
    let domain = ext4().resolve(Path::new(
        "/run/containers/storage/overlay/1b2c/merged/var/lib/app",
    ));
    assert_eq!(
        domain.refusal(),
        Some(NonPersistentReason::UpperLayerElsewhere),
        "Appendix B.4: snapshotting the merged mount protects nothing when the upper layer is a tmpfs"
    );
    assert!(!domain.is_protectable());
}

#[test]
fn should_refuse_an_overlay_that_declares_no_writable_layer() {
    let read_only_overlay = "\
23 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw
24 23 0:52 / /opt/image rw,relatime - overlay overlay rw,lowerdir=/opt/lower1:/opt/lower2
";
    let domain = MountTable::from_text(read_only_overlay).resolve(Path::new("/opt/image/bin/app"));
    assert_eq!(
        domain.refusal(),
        Some(NonPersistentReason::UpperLayerElsewhere),
        "Appendix B.4: without a writable upper layer there is nothing to protect and nothing to claim"
    );
}

#[test]
fn should_refuse_a_network_filesystem_so_no_local_provider_claims_it() {
    for (table, path) in [
        (zfs(), "/var/lib/nfs-data/customers.csv"),
        (ext4(), "/data/customers.csv"),
    ] {
        let domain = table.resolve(Path::new(path));
        assert_eq!(
            domain.refusal(),
            Some(NonPersistentReason::Remote),
            "Appendix B.6: local snapshot providers MUST NOT claim protection for {path}"
        );
        assert!(
            domain.detail().contains("nas01"),
            "Appendix B.6: the map shows the server the bytes actually live on"
        );
    }
}

#[test]
fn should_refuse_an_nfs_mount_beneath_a_snapshotted_dataset() {
    // Appendix G.2 truth test: nfs-below-snapshotted-root.
    let table = zfs();
    let parent = table.resolve(Path::new("/var/lib/app/state.db"));
    let nfs = table.resolve(Path::new("/var/lib/nfs-data/customers.csv"));
    assert_eq!(parent.object(), Some("rpool/var"));
    assert!(
        !nfs.is_protectable(),
        "Appendix G.2: an NFS mount below a snapshotted root is not covered by that snapshot"
    );
}

#[test]
fn should_refuse_every_pseudo_filesystem_whatever_its_mountpoint_is() {
    let table = zfs();
    for path in [
        "/proc/sys/vm/swappiness",
        "/sys/class/net/eth0/mtu",
        "/dev/null",
        "/sys/fs/cgroup/system.slice",
        "/sys/kernel/tracing/events",
        "/sys/kernel/debug/sched",
    ] {
        let domain = table.resolve(Path::new(path));
        assert_eq!(
            domain.refusal(),
            Some(NonPersistentReason::Pseudo),
            "§32.4: {path} MUST never be presented as snapshot-protected because it sits beneath `/`"
        );
    }
}

#[test]
fn should_refuse_a_runtime_tmpfs_mounted_under_a_snapshotted_root() {
    let table = zfs();
    let root = table.resolve(Path::new("/etc/nginx/nginx.conf"));
    let volatile = table.resolve(Path::new("/run/nginx.pid"));
    assert!(
        root.is_protectable(),
        "the root dataset is protectable, which is what makes the next assertion matter"
    );
    assert_eq!(
        volatile.refusal(),
        Some(NonPersistentReason::Volatile),
        "§32.4 and Appendix B.7: a runtime tmpfs is not protected by a snapshot of `/`"
    );
}

#[test]
fn should_take_a_zfs_dataset_from_mount_metadata_rather_than_from_the_path() {
    let table = zfs();
    let looks_like_a_dataset = table.resolve(Path::new("/rpool/home/erin/notes.md"));
    assert_eq!(
        looks_like_a_dataset.object(),
        Some("rpool/ROOT/debian"),
        "Appendix B.8: a path that reads like a dataset name is not evidence of one"
    );
    let real = table.resolve(Path::new("/home/erin/notes.md"));
    assert_eq!(real.object(), Some("rpool/home"));
}

#[test]
fn should_name_the_dataset_as_the_snapshot_boundary_of_a_zfs_path() {
    let domain = zfs().resolve(Path::new("/etc/nginx/nginx.conf"));
    assert_eq!(domain.mount().kind(), FilesystemKind::Zfs);
    assert_eq!(
        domain.boundary(),
        Some("rpool/ROOT/debian"),
        "§13.4: the ZFS snapshot boundary is the dataset the path resolves to"
    );
}

#[test]
fn should_take_a_btrfs_subvolume_from_the_mount_options() {
    let domain = btrfs().resolve(Path::new("/home/erin/notes.md"));
    assert_eq!(
        domain.object(),
        Some("/@home"),
        "Appendix B.9: the subvolume is the one the kernel says is mounted here"
    );
    assert!(
        domain.detail().contains("subvolid=257"),
        "Appendix B.10: the resolution shows the subvolume id: {}",
        domain.detail()
    );
}

#[test]
fn should_treat_a_nested_btrfs_subvolume_as_a_boundary_of_its_own() {
    let table = btrfs();
    let parent = table.resolve(Path::new("/var/lib/app/state.db"));
    let nested = table.resolve(Path::new("/var/lib/machines/web/etc/hosts"));
    assert_eq!(parent.object(), Some("/@var"));
    assert_eq!(
        nested.object(),
        Some("/@var/lib/machines"),
        "§14.3 and §32.3: a recursive operation MUST NOT assume a nested subvolume shares the scope"
    );
    assert_ne!(parent.boundary(), nested.boundary());
}

#[test]
fn should_refuse_to_read_a_subvolume_out_of_a_directory_that_is_merely_named_like_one() {
    let domain = btrfs().resolve(Path::new("/home/@backup/2026-09/notes.md"));
    assert_eq!(
        domain.object(),
        Some("/@home"),
        "Appendix B.9: a subdirectory named like a subvolume is not sufficient evidence of one"
    );
}

#[test]
fn should_trace_a_bind_mount_of_a_directory_inside_a_subvolume_to_that_subvolume() {
    let table = btrfs();
    let bound = table.resolve(Path::new("/srv/work/report.odt"));
    assert_eq!(
        bound.object(),
        Some("/@home"),
        "Appendix B.3: the bind mount is the same subvolume, reached by another name"
    );
    assert!(bound.detail().contains("bind mount"));
}

#[test]
fn should_record_the_mount_namespace_the_resolution_was_taken_in() {
    let namespace = "mnt:[4026532567]";
    let domain = MountTable::from_text(ZFS_ROOT)
        .in_namespace(namespace)
        .resolve(Path::new("/etc/nginx/nginx.conf"));
    assert_eq!(
        domain.mount().namespace(),
        Some(namespace),
        "Appendix B.2: the result MUST be tied to the namespace the mutation will occur in"
    );
}

#[test]
fn should_flag_a_read_only_mount_a_restore_could_not_write_to() {
    let domain = zfs().resolve(Path::new("/boot/vmlinuz-6.1.0"));
    assert!(
        domain.mount().is_read_only(),
        "Appendix G.2: a read-only filesystem prevents a restore, and the resolution says so"
    );
    assert!(
        domain.detail().contains("read-only"),
        "the sentence `inspect plan` shows names the obstacle: {}",
        domain.detail()
    );
}

#[test]
fn should_refuse_a_relative_path_rather_than_guessing_a_mount_for_it() {
    let domain = zfs().resolve(Path::new("etc/nginx/nginx.conf"));
    assert_eq!(
        domain.refusal(),
        Some(NonPersistentReason::Unresolved),
        "Appendix B.1 resolves from a namespace-visible mount, and a relative path names none"
    );
}

#[test]
fn should_refuse_a_path_no_mount_in_the_table_contains() {
    let domain = MountTable::from_text("").resolve(Path::new("/etc/nginx/nginx.conf"));
    assert_eq!(
        domain.refusal(),
        Some(NonPersistentReason::Unresolved),
        "§56.3: an incomplete resolution is a refusal rather than an assumption"
    );
}

#[test]
fn should_classify_an_unfamiliar_filesystem_without_inventing_a_snapshot_mechanism() {
    let exotic = "\
23 1 0:44 / / rw,relatime shared:1 - bcachefs /dev/sda2:/dev/sdb2 rw
";
    let domain = MountTable::from_text(exotic).resolve(Path::new("/etc/hosts"));
    assert_eq!(domain.mount().kind(), FilesystemKind::Other);
    assert!(
        !domain.mount().kind().admits_local_snapshot(),
        "Appendix B.8 and B.9: no mechanism is inferred from a filesystem's name"
    );
    assert!(
        domain.is_protectable(),
        "the state is persistent, so a provider that copies files may still protect it"
    );
}

#[test]
fn should_expose_the_mount_a_path_is_served_by() {
    let table = zfs();
    let mount = table
        .mount_for(Path::new("/home/erin/notes.md"))
        .expect("the fixture mounts /home");
    assert_eq!(mount.source, "rpool/home");
    assert_eq!(table.mounts().len(), 13);
    assert_eq!(table.namespace(), None);
}

#[test]
fn should_keep_the_read_only_option_of_a_snapshot_subvolume() {
    let domain = btrfs().resolve(Path::new("/.snapshots/1/snapshot/etc/hosts"));
    assert!(
        domain.mount().is_read_only(),
        "§14.2: a read-only snapshot subvolume is where a restore reads from, never writes to"
    );
    assert_eq!(domain.object(), Some("/@snapshots"));
}

/// The same two mounts as a host kernel prints them, read off a developer machine.
const HOST_VOLATILE_AND_PSEUDO: &str = "\
44 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw
43 41 0:27 / /dev/shm rw,nosuid,nodev shared:3 - tmpfs tmpfs rw,inode64,usrquota
53 44 0:25 / /proc rw,nosuid,nodev,noexec,relatime shared:12 - proc proc rw
";

#[test]
fn should_resolve_an_overlay_whose_writable_layer_is_hidden_to_a_copy_only_domain() {
    let domain = MountTable::from_text(CONTAINER).resolve(Path::new("/home/ono/etc/source"));
    assert!(
        domain.is_protectable(),
        "Appendix B.4 as decided: the visible bytes can still be copied and written back through \
         the merged view: {}",
        domain.detail()
    );
    assert_eq!(
        DomainReach::of(&domain),
        DomainReach::CopyOnly,
        "Appendix B.4: a snapshot of the merged mount would not hold the hidden writable layer"
    );
    assert!(!DomainReach::of(&domain).admits_snapshot());
    assert!(DomainReach::of(&domain).admits_copy());
    assert!(
        domain.detail().contains("Appendix B.4"),
        "the resolution says why only a copy may protect it: {}",
        domain.detail()
    );
}

#[test]
fn should_still_follow_a_visible_writable_layer_rather_than_calling_the_overlay_copy_only() {
    let domain = ext4().resolve(Path::new(
        "/var/lib/docker/overlay2/9f3a/merged/etc/app.conf",
    ));
    assert_eq!(
        DomainReach::of(&domain),
        DomainReach::Any,
        "Appendix B.4: an upper layer this namespace can see is the domain, and it is the ext4 \
         root's to protect however that filesystem allows"
    );
    assert_eq!(domain.mount().kind(), FilesystemKind::Ext4);
}

#[test]
fn should_keep_refusing_an_overlay_whose_visible_writable_layer_is_volatile() {
    let domain = ext4().resolve(Path::new(
        "/run/containers/storage/overlay/1b2c/merged/var/lib/app",
    ));
    assert_eq!(
        DomainReach::of(&domain),
        DomainReach::Refused,
        "Appendix B.4 and B.7: a writable layer on a tmpfs is followed, and it holds nothing"
    );
}

#[test]
fn should_record_no_persistence_domain_for_a_container_tmpfs_or_procfs_path() {
    let table = MountTable::from_text(CONTAINER);
    for (path, reason) in [
        ("/dev/shm/ono-volatile", NonPersistentReason::Volatile),
        ("/proc/self/comm", NonPersistentReason::Pseudo),
    ] {
        let domain = table.resolve(Path::new(path));
        assert_eq!(domain.refusal(), Some(reason), "Appendix B.7: {path}");
        assert_eq!(
            recorded_domain(&domain),
            None,
            "Appendix B.7: {path} has no persistence domain, and its mount source `{}` is not one",
            domain.mount().source()
        );
    }
}

#[test]
fn should_record_no_persistence_domain_for_a_host_tmpfs_or_procfs_path() {
    let table = MountTable::from_text(HOST_VOLATILE_AND_PSEUDO);
    for path in ["/dev/shm/ono-truth", "/proc/self/comm"] {
        let domain = table.resolve(Path::new(path));
        assert!(!domain.is_protectable(), "Appendix B.7: {path}");
        assert_eq!(recorded_domain(&domain), None, "Appendix B.7: {path}");
    }
}

#[test]
fn should_record_the_persistence_object_of_a_protectable_path() {
    assert_eq!(
        recorded_domain(&zfs().resolve(Path::new("/etc/nginx/nginx.conf"))),
        Some("rpool/ROOT/debian"),
        "Appendix B.8: a ZFS path records its dataset"
    );
    assert_eq!(
        recorded_domain(&ext4().resolve(Path::new("/srv/app.conf"))),
        Some("/dev/sda2"),
        "a filesystem without snapshots records the device it lives on"
    );
    let container = MountTable::from_text(CONTAINER).resolve(Path::new("/home/ono/etc/source"));
    assert_eq!(
        recorded_domain(&container),
        Some("/var/lib/docker/overlay2/5e1c/diff"),
        "Appendix B.4: a copy-only overlay records the writable layer its bytes land in"
    );
}
