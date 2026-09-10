//! A recursive mutation touches every filesystem mounted beneath its target (v0.6 §13.4, §14.3,
//! §32.2, §32.3).
//!
//! §32.3: *"Recursive path operations MUST NOT assume mounted filesystems or nested subvolumes
//! belong to the same recovery scope."* Each mount beneath a removed directory is a persistence
//! domain of its own, and a plan is protected only where something captures each one.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;

use ono_change_core::{
    ConsistencyClass, EffectDomain, EffectKind, NonPersistentReason, ProtectionLevel,
    RecoveryCandidate, RecoveryObjective, RestoreMethod,
};
use ono_change_protection::coverage::{CoverageRequest, MutationDomain, analyse};
use ono_change_protection::policy::ProtectionPolicy;
use ono_change_protection::{MountTable, ProviderRegistry};
use ono_value::ByteSize;

mod support;

use support::{
    BTRFS_ROOT, EXT4_ROOT, SRV_TREE, TestProvider, archive_cost, candidate, snapshot_cost,
};

/// `/srv` on `tank/srv` with only the child dataset `tank/srv/data` beneath it.
const SRV_WITH_CHILD_DATASET: &str = "\
27 1 0:23 / / rw,relatime shared:1 - zfs rpool/ROOT/debian rw,xattr,posixacl
40 27 0:44 / /srv rw,relatime shared:40 - zfs tank/srv rw,xattr,posixacl
41 40 0:45 / /srv/data rw,relatime shared:41 - zfs tank/srv/data rw,xattr,posixacl
";

fn zfs(object: &str, covers: &[&str]) -> RecoveryCandidate {
    candidate(
        "ono.recovery.zfs",
        "zfs-dataset",
        object,
        covers,
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .costing(snapshot_cost())
}

fn remove_tree(path: &str) -> MutationDomain {
    MutationDomain::new(
        EffectDomain::FilesystemPersistent,
        EffectKind::Remove,
        path,
        "the directory and everything beneath it are removed",
    )
}

fn excluded_subjects(analysis: &ono_change_protection::CoverageAnalysis) -> Vec<String> {
    analysis
        .summary()
        .exclusions()
        .iter()
        .map(|exclusion| exclusion.subject().to_owned())
        .collect()
}

#[test]
fn should_list_every_filesystem_mounted_beneath_a_directory() {
    let table = MountTable::from_text(SRV_TREE);
    let boundaries = table.boundaries_beneath(Path::new("/srv"));
    let points: Vec<&str> = boundaries
        .iter()
        .map(|boundary| boundary.mount_point())
        .collect();

    assert_eq!(
        points,
        vec!["/srv/data", "/srv/legacy", "/srv/scratch", "/srv/share"],
        "§32.2: every persistence boundary beneath the directory, in path order, and not \
         `/srvx`, which only shares a prefix"
    );
    assert_eq!(boundaries[0].domain().object(), Some("tank/srv/data"));
    assert_eq!(boundaries[1].domain().object(), Some("/dev/sdb1"));
    assert_eq!(
        boundaries[2].domain().refusal(),
        Some(NonPersistentReason::Volatile)
    );
    assert_eq!(
        boundaries[3].domain().refusal(),
        Some(NonPersistentReason::Remote)
    );
    assert!(
        boundaries.iter().all(|boundary| boundary.size().is_none()),
        "the mount table states no sizes, and an unknown size is null rather than zero"
    );
    let sized = boundaries[1]
        .clone()
        .sized(ByteSize::from_bytes(4 * 1024 * 1024));
    assert_eq!(sized.size(), Some(ByteSize::from_bytes(4 * 1024 * 1024)));
}

#[test]
fn should_list_no_boundary_beneath_a_directory_nothing_is_mounted_in() {
    let table = MountTable::from_text(SRV_TREE);
    assert!(
        table
            .boundaries_beneath(Path::new("/srv/data/customers"))
            .is_empty()
    );
}

#[test]
fn should_not_list_a_mount_hidden_by_a_later_mount_over_its_parent() {
    let table = MountTable::from_text(
        "\
27 1 0:23 / / rw,relatime shared:1 - zfs rpool/ROOT/debian rw
40 27 0:44 / /srv rw,relatime - zfs tank/srv rw
41 40 0:45 / /srv/data/old rw,relatime - zfs tank/srv/old rw
42 40 0:46 / /srv/data rw,relatime - ext4 /dev/sdd1 rw
",
    );
    let points: Vec<String> = table
        .boundaries_beneath(Path::new("/srv"))
        .iter()
        .map(|boundary| boundary.mount_point().to_owned())
        .collect();
    assert_eq!(
        points,
        vec!["/srv/data".to_owned()],
        "a mount made beneath `/srv/data` before `/srv/data` itself was mounted over is not \
         visible, and a recursive removal cannot reach it"
    );
}

#[test]
fn should_not_call_a_recursive_removal_protected_when_the_snapshot_holds_only_the_top_dataset() {
    let table = MountTable::from_text(SRV_TREE);
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(zfs("tank/srv", &["tank/srv", "/srv"]))
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .within(&table)
            .over(table.resolve(Path::new("/srv")))
            .mutating(remove_tree("/srv")),
    );

    assert_ne!(
        analysis.level(),
        ProtectionLevel::Protected,
        "§13.4 and §32.3: a snapshot of tank/srv holds nothing of the child dataset, the ext4 \
         disk or the NFS export the removal also deletes. Rows: {:?}",
        analysis.summary().rows()
    );
    let excluded = excluded_subjects(&analysis);
    for subject in ["/srv/data", "/srv/legacy", "/srv/scratch", "/srv/share"] {
        assert!(
            excluded.iter().any(|excluded| excluded == subject),
            "§32.2: {subject} is a boundary the removal crosses and nothing captures, so it is \
             named: {excluded:?}"
        );
    }
    let shortfall: Vec<&str> = analysis.shortfall().iter().map(|row| row.note()).collect();
    for persistent in ["/srv/data", "/srv/legacy", "/srv/share"] {
        assert!(
            shortfall.iter().any(|note| note.contains(persistent)),
            "§10.3: the persistent boundary {persistent} is a row in the shortfall: {shortfall:?}"
        );
    }
    assert!(
        !shortfall.iter().any(|note| note.contains("/srv/scratch")),
        "Appendix B.7: a tmpfs holds no persistent state, so it is an exclusion rather than a \
         shortfall: {shortfall:?}"
    );
}

#[test]
fn should_prefer_the_recursive_snapshot_that_captures_the_child_dataset_for_a_tree_removal() {
    let table = MountTable::from_text(SRV_WITH_CHILD_DATASET);
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(zfs("tank/srv", &["tank/srv", "/srv"]))
            .offering(zfs("tank/srv", &["tank/srv", "tank/srv/data", "/srv"]))
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .within(&table)
            .over(table.resolve(Path::new("/srv")))
            .mutating(remove_tree("/srv")),
    );

    assert_eq!(analysis.actions().len(), 1, "{:?}", analysis.actions());
    assert!(
        analysis.actions()[0]
            .candidate()
            .scope()
            .covers_object("tank/srv/data"),
        "Appendix A.4's first key is the objective, and for a tree removal the objective spans \
         the child dataset; the narrower snapshot does not satisfy it"
    );
    assert_eq!(analysis.level(), ProtectionLevel::Protected);
    assert!(
        analysis
            .summary()
            .rows()
            .iter()
            .any(|row| row.note().contains("/srv/data") && row.is_satisfied()),
        "§32.2: the child dataset is shown as a boundary of its own, covered by the recursive \
         snapshot: {:?}",
        analysis.summary().rows()
    );
}

#[test]
fn should_not_call_a_tree_removal_protected_when_a_file_archive_stops_at_a_mount() {
    let table = MountTable::from_text(
        "\
24 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw,errors=remount-ro
50 24 8:33 / /srv/site/uploads rw,relatime - ext4 /dev/sdc1 rw
",
    );
    let archive = candidate(
        "ono.recovery.files",
        "directory",
        "/srv/site",
        &["/srv/site", "/srv/site/index.html"],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::ByteConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .costing(archive_cost(64 * 1024));
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.files")
            .offering(archive)
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .within(&table)
            .over(table.resolve(Path::new("/srv/site")))
            .mutating(remove_tree("/srv/site")),
    );

    assert_ne!(
        analysis.level(),
        ProtectionLevel::Protected,
        "§32.3: the archive stops at the mount, so nothing inside the filesystem mounted at \
         /srv/site/uploads is captured"
    );
    assert!(excluded_subjects(&analysis).contains(&"/srv/site/uploads".to_owned()));
}

#[test]
fn should_not_call_a_tree_removal_protected_when_a_nested_subvolume_has_no_snapshot() {
    let table = MountTable::from_text(BTRFS_ROOT);
    let registry = support::registry_with(vec![
        TestProvider::new("ono.recovery.btrfs")
            .offering(
                candidate(
                    "ono.recovery.btrfs",
                    "btrfs-subvolume",
                    "/@var",
                    &["/var/lib"],
                    EffectDomain::FilesystemPersistent,
                    RecoveryObjective::PreserveExact,
                )
                .at_consistency(ConsistencyClass::FilesystemConsistent)
                .restored_by(RestoreMethod::SelectiveFileRestore)
                .costing(snapshot_cost()),
            )
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .within(&table)
            .over(table.resolve(Path::new("/var/lib")))
            .mutating(remove_tree("/var/lib")),
    );

    assert_eq!(
        analysis.level(),
        ProtectionLevel::PartiallyProtected,
        "§14.3: a snapshot of @var does not contain the nested @var/lib/machines"
    );
    assert!(excluded_subjects(&analysis).contains(&"/var/lib/machines".to_owned()));
}

#[test]
fn should_resolve_a_subject_through_a_mount_boundary_beneath_a_resolved_directory() {
    let table = MountTable::from_text(SRV_TREE);
    let registry = ProviderRegistry::new();
    let policy = ProtectionPolicy::default();

    let within = CoverageRequest::new(&registry, &policy)
        .within(&table)
        .over(table.resolve(Path::new("/srv")));
    let found = within
        .persistence_for("/srv/data/customer.db")
        .expect("§11.2: the path resolves through the table");
    assert_eq!(
        found.object(),
        Some("tank/srv/data"),
        "§13.4: the file is on the child dataset, not on the directory's dataset"
    );

    let without = CoverageRequest::new(&registry, &policy)
        .over(table.resolve(Path::new("/srv")))
        .over(table.resolve(Path::new("/srv/share/y")));
    assert!(
        without.persistence_for("/srv/share/z").is_none(),
        "a known mount at /srv/share lies between /srv and the subject, so the dataset of /srv \
         does not hold it"
    );
    assert_eq!(
        without
            .persistence_for("/srv/app.conf")
            .and_then(|domain| domain.object().map(str::to_owned)),
        Some("tank/srv".to_owned())
    );
}

#[test]
fn should_leave_a_tree_removal_on_an_ext4_root_without_child_rows_when_nothing_is_mounted_beneath()
{
    let table = MountTable::from_text(EXT4_ROOT);
    let registry = ProviderRegistry::new();
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .within(&table)
            .over(table.resolve(Path::new("/srv/www")))
            .mutating(remove_tree("/srv/www")),
    );
    assert_eq!(analysis.summary().rows().len(), 1);
}
