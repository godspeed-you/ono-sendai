//! Assets created one after another hold no common point in time (v0.6 §11.3, Appendix D.7).
//!
//! Appendix D.7: *"The set MUST record that its member snapshots were created sequentially unless a
//! higher-level mechanism can prove a common atomic point. Ono MUST not invent cross-subvolume
//! atomicity."* One recursive ZFS snapshot is one atomic operation; two Btrfs snapshots are two.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;

use ono_change_core::{
    ConsistencyClass, EffectDomain, EffectKind, ProtectionLevel, RecoveryCandidate,
    RecoveryObjective, RestoreMethod,
};
use ono_change_protection::MountTable;
use ono_change_protection::coverage::{CoverageRequest, MutationDomain, analyse};
use ono_change_protection::policy::ProtectionPolicy;

mod support;

use support::{BTRFS_ROOT, TestProvider, candidate, snapshot_cost};

const SRV_WITH_CHILD_DATASET: &str = "\
27 1 0:23 / / rw,relatime shared:1 - zfs rpool/ROOT/debian rw,xattr,posixacl
40 27 0:44 / /srv rw,relatime shared:40 - zfs tank/srv rw,xattr,posixacl
41 40 0:45 / /srv/data rw,relatime shared:41 - zfs tank/srv/data rw,xattr,posixacl
";

fn snapshot(provider: &str, kind: &str, object: &str, covers: &[&str]) -> RecoveryCandidate {
    candidate(
        provider,
        kind,
        object,
        covers,
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .costing(snapshot_cost())
}

fn modify(path: &str) -> MutationDomain {
    MutationDomain::new(
        EffectDomain::FilesystemPersistent,
        EffectKind::Modify,
        path,
        "the file is rewritten",
    )
}

#[test]
fn should_call_snapshots_of_two_btrfs_subvolumes_crash_consistent_at_best() {
    let table = MountTable::from_text(BTRFS_ROOT);
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.btrfs")
            .offering(snapshot(
                "ono.recovery.btrfs",
                "btrfs-subvolume",
                "/@",
                &["/etc/nginx/nginx.conf"],
            ))
            .offering(snapshot(
                "ono.recovery.btrfs",
                "btrfs-subvolume",
                "/@var",
                &["/var/lib/app/state.db"],
            ))
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .within(&table)
            .over(table.resolve(Path::new("/etc/nginx/nginx.conf")))
            .over(table.resolve(Path::new("/var/lib/app/state.db")))
            .mutating(modify("/etc/nginx/nginx.conf"))
            .mutating(modify("/var/lib/app/state.db")),
    );

    assert_eq!(analysis.level(), ProtectionLevel::Protected);
    assert_eq!(analysis.actions().len(), 2);
    for row in analysis.summary().rows() {
        assert_eq!(
            row.consistency(),
            Some(ConsistencyClass::CrashConsistent),
            "Appendix D.7: two snapshots taken one after the other hold no common point in time"
        );
        assert!(
            row.note().contains("Appendix D.7"),
            "the row says why its consistency is lower than each snapshot's: {}",
            row.note()
        );
    }
}

#[test]
fn should_call_a_separate_snapshot_of_a_child_dataset_crash_consistent_at_best() {
    let table = MountTable::from_text(SRV_WITH_CHILD_DATASET);
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(snapshot(
                "ono.recovery.zfs",
                "zfs-dataset",
                "tank/srv",
                &["tank/srv", "/srv"],
            ))
            .offering(snapshot(
                "ono.recovery.zfs",
                "zfs-dataset",
                "tank/srv/data",
                &["tank/srv/data", "/srv/data"],
            ))
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .within(&table)
            .over(table.resolve(Path::new("/srv")))
            .mutating(MutationDomain::new(
                EffectDomain::FilesystemPersistent,
                EffectKind::Remove,
                "/srv",
                "the tree is removed",
            )),
    );

    assert_eq!(analysis.level(), ProtectionLevel::Protected);
    assert_eq!(
        analysis.actions().len(),
        2,
        "§13.4: the child dataset gets a snapshot of its own"
    );
    assert!(
        analysis
            .summary()
            .rows()
            .iter()
            .all(|row| row.consistency() == Some(ConsistencyClass::CrashConsistent)),
        "two `zfs snapshot` calls are two points in time: {:?}",
        analysis.summary().rows()
    );
}

#[test]
fn should_keep_filesystem_consistency_when_one_recursive_snapshot_captures_every_dataset() {
    let table = MountTable::from_text(SRV_WITH_CHILD_DATASET);
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(snapshot(
                "ono.recovery.zfs",
                "zfs-dataset",
                "tank/srv",
                &["tank/srv", "tank/srv/data", "/srv"],
            ))
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .within(&table)
            .over(table.resolve(Path::new("/srv")))
            .mutating(MutationDomain::new(
                EffectDomain::FilesystemPersistent,
                EffectKind::Remove,
                "/srv",
                "the tree is removed",
            )),
    );

    assert_eq!(analysis.level(), ProtectionLevel::Protected);
    assert_eq!(
        analysis.summary().rows().len(),
        2,
        "the top dataset and the child"
    );
    assert!(
        analysis
            .summary()
            .rows()
            .iter()
            .all(|row| row.consistency() == Some(ConsistencyClass::FilesystemConsistent)),
        "§13.3: `zfs snapshot -r` is one atomic creation, and nothing is invented by saying so: \
         {:?}",
        analysis.summary().rows()
    );
}
