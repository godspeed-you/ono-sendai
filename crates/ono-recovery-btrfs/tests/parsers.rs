#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! Reading what real `btrfs-progs` printed (§14.1, Appendix B.9, Appendix G.4).

mod support;

use ono_recovery_btrfs::parse::{
    ShowOutcome, parse_deleted_subvolume_id, parse_filesystem_show, parse_filesystem_usage,
    parse_get_default, parse_read_only_property, parse_subvolume_list, parse_subvolume_show,
    parse_version, read_subvolume_show, spoken_text,
};
use ono_recovery_btrfs::{BtrfsMounts, FS_TREE_ID};
use support::{FILESYSTEM, NESTED_ID, ROOT_ID, VAR_ID, fixture};

#[test]
fn should_resolve_the_stable_subvolume_id_when_the_path_is_a_subvolume() {
    let show = parse_subvolume_show(fixture("subvol-show-root").stdout())
        .expect("the recorded output of a real `btrfs subvolume show`");
    assert_eq!(
        show.id(),
        ROOT_ID,
        "§14.1: the provider MUST resolve the stable subvolume ID of each protected target"
    );
    assert_eq!(show.tree_path(), "@");
    assert_eq!(show.parent_id(), Some(FS_TREE_ID));
    assert_eq!(show.uuid(), "b08a487d-1079-ac45-9f7e-12d9f15bea63");
}

#[test]
fn should_read_a_nested_subvolume_as_a_child_of_its_parent_when_shown() {
    let show = parse_subvolume_show(fixture("subvol-show-nested").stdout()).expect("readable");
    assert_eq!(show.id(), NESTED_ID);
    assert_eq!(
        show.parent_id(),
        Some(VAR_ID),
        "§14.3: `@var/lib-app` is a subvolume nested inside `@var`, and the parent id says so"
    );
    assert_eq!(show.tree_path(), "@var/lib-app");
}

#[test]
fn should_report_a_plain_directory_as_not_a_subvolume_when_it_only_looks_like_one() {
    let outcome = read_subvolume_show(&fixture("subvol-show-lookslike")).expect("readable");
    assert_eq!(
        outcome,
        ShowOutcome::PlainDirectory,
        "Appendix B.9: a subdirectory named like a subvolume is not sufficient evidence of one, \
         and `btrfs` answers `ERROR: Not a Btrfs subvolume`"
    );
}

#[test]
fn should_tell_a_missing_path_apart_from_a_plain_directory() {
    assert_eq!(
        read_subvolume_show(&fixture("subvol-show-missing")).expect("readable"),
        ShowOutcome::Missing,
        "§56.3: a path that is not there and a path that is an ordinary directory are different \
         facts, and only one of them means the state lives in the containing subvolume"
    );
    assert_eq!(
        read_subvolume_show(&fixture("subvol-show-plaindir")).expect("readable"),
        ShowOutcome::PlainDirectory
    );
}

#[test]
fn should_report_a_refusal_rather_than_an_absence_when_the_search_is_not_permitted() {
    let outcome = read_subvolume_show(&fixture("unprivileged-list")).expect("readable");
    match outcome {
        ShowOutcome::Refused(reason) => assert!(
            reason.contains("Operation not permitted"),
            "§56.3: an unprivileged refusal is a fact that blocks, never an empty answer"
        ),
        other => panic!("an unprivileged search is a refusal, and this was {other:?}"),
    }
}

#[test]
fn should_read_the_read_only_flag_of_a_snapshot_when_it_is_shown() {
    let show = parse_subvolume_show(fixture("subvol-show-snapshot").stdout()).expect("readable");
    assert!(
        show.is_read_only(),
        "§14.5: a retained recovery snapshot carries the readonly flag"
    );
    assert_eq!(
        show.parent_uuid(),
        Some("b08a487d-1079-ac45-9f7e-12d9f15bea63"),
        "§14.2: a snapshot's parent uuid is what ties it to the subvolume it was taken from"
    );
    assert!(
        show.created_at().is_some(),
        "Appendix D.7: each member of a set carries its own creation instant, read from the \
         filesystem rather than from a clock"
    );
}

#[test]
fn should_list_every_subvolume_boundary_including_the_nested_one() {
    let entries =
        parse_subvolume_list(fixture("subvol-list").stdout()).expect("the recorded listing");
    let ids: Vec<u64> = entries.iter().map(|entry| entry.id()).collect();
    assert_eq!(ids, vec![256, 257, 258, 259, 260]);
    let nested = entries
        .iter()
        .find(|entry| entry.id() == NESTED_ID)
        .expect("the recorded listing holds the nested subvolume");
    assert_eq!(
        nested.parent_id(),
        Some(VAR_ID),
        "§14.3: nested subvolumes form snapshot boundaries, and the parent id is the boundary"
    );
    assert_eq!(nested.tree_path(), "@var/lib-app");
}

#[test]
fn should_read_the_same_subvolume_from_both_spellings_of_a_listing() {
    let top = parse_subvolume_list(fixture("subvol-list").stdout()).expect("readable");
    let root = parse_subvolume_list(fixture("subvol-list-root").stdout()).expect("readable");
    let path_of = |entries: &[ono_recovery_btrfs::SubvolumeEntry], id: u64| {
        entries
            .iter()
            .find(|entry| entry.id() == id)
            .map(|entry| entry.tree_path().to_owned())
    };
    assert_eq!(
        path_of(&top, VAR_ID),
        path_of(&root, VAR_ID),
        "`<FS_TREE>/@var` and `@var` name one subvolume, and a boundary comparison that read them \
         as two would miss the boundary §14.3 is about"
    );
}

#[test]
fn should_read_the_snapshot_listing_that_carries_creation_times() {
    let entries = parse_subvolume_list(fixture("subvol-list-after").stdout()).expect("readable");
    assert_eq!(entries.len(), 3);
    assert!(
        entries.iter().all(|entry| entry.parent_uuid().is_some()),
        "§14.2: each of these is a snapshot of something, and says of what — and `-s`'s extra \
         `cgen` and `otime` columns must not shift the fields around them"
    );
    assert_eq!(
        entries
            .iter()
            .map(ono_recovery_btrfs::SubvolumeEntry::tree_path)
            .collect::<Vec<_>>(),
        vec![
            "@snapshots/ono-a82f-root",
            "@snapshots/ono-a82f-var",
            "@snapshots/ono-a82f-home"
        ]
    );
}

#[test]
fn should_read_the_filesystem_uuid_and_label_when_shown() {
    let info = parse_filesystem_show(fixture("fs-show-mount").stdout()).expect("readable");
    assert_eq!(
        info.uuid(),
        FILESYSTEM,
        "§56.2: the exact filesystem is half of the identity a recovery must prove"
    );
    assert_eq!(info.label(), Some("onotest"));
    assert_eq!(info.devices(), &[std::sync::Arc::from("/dev/loop20")]);
}

#[test]
fn should_read_the_space_figures_of_a_filesystem_when_asked_for_usage() {
    let usage = parse_filesystem_usage(fixture("fs-usage").stdout()).expect("readable");
    assert_eq!(
        usage.device_size().map(ono_value::ByteSize::bytes),
        Some(2 * 1024 * 1024 * 1024)
    );
    assert!(
        usage.free_estimated().is_some(),
        "§37.5: the free figure travels labelled as the estimate Btrfs says it is"
    );
}

#[test]
fn should_read_the_default_subvolume_when_it_is_the_filesystem_tree() {
    let default = parse_get_default(fixture("get-default").stdout()).expect("readable");
    assert_eq!(default.id(), FS_TREE_ID);
    assert!(
        default.is_filesystem_tree(),
        "§56.2: what boots is a fact about the default subvolume, and the top level is a distinct \
         answer from a named subvolume"
    );
}

#[test]
fn should_read_the_read_only_property_of_a_snapshot() {
    assert!(
        parse_read_only_property(fixture("snapshot-ro-flag").stdout()).expect("readable"),
        "§14.5: the flag is read rather than assumed"
    );
    assert!(!parse_read_only_property("ro=false\n").expect("readable"));
    assert!(
        parse_read_only_property("").is_err(),
        "§56.3: an absent answer is not `true`"
    );
}

#[test]
fn should_read_the_version_of_the_tool_in_use() {
    let version = parse_version(fixture("version").stdout()).expect("readable");
    assert_eq!(version.series(), "6.17");
    assert_eq!(version.raw(), "v6.17.1");
}

#[test]
fn should_read_the_subvolume_id_a_deletion_reported() {
    assert_eq!(
        parse_deleted_subvolume_id(fixture("delete-readonly").stdout()),
        Some(261),
        "§37: cleanup can say which object it removed rather than which path it aimed at"
    );
}

#[test]
fn should_read_the_error_stream_when_a_command_failed() {
    let failed = fixture("snapshot-exists");
    assert!(!failed.succeeded());
    assert!(
        spoken_text(&failed).contains("Read-only file system"),
        "`btrfs` says why on standard error, and a reader that looked only at standard output \
         would read a failure as an empty success"
    );
}

#[test]
fn should_refuse_output_that_carries_no_subvolume_id() {
    assert!(
        parse_subvolume_show("@\n\tName: \t\t@\n").is_err(),
        "§14.1: without a subvolume ID there is no stable identity, and a successful command \
         whose output cannot be read is worse than a failed one"
    );
}

#[test]
fn should_read_the_subvolume_of_each_btrfs_mount_from_its_options() {
    let mounts = BtrfsMounts::from_mountinfo(fixture("mountinfo").stdout());
    assert_eq!(
        mounts.mounts().len(),
        4,
        "the recorded table holds four Btrfs mounts and one ext4 bind mount, and only the Btrfs \
         ones are this provider's business"
    );
    let root = mounts
        .for_subvolume_id(ROOT_ID)
        .expect("the root subvolume is mounted");
    assert_eq!(root.mount_point(), "/mnt/root");
    assert_eq!(
        root.root(),
        "/@",
        "Appendix B.9: the mount root is the other half of the identity"
    );
    assert_eq!(root.subvolume(), Some("/@"));
}
