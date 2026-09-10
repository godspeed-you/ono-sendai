#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! §56.2's first fact is "exact filesystem **and** subvolume ID". Subvolume ids start at 256 on
//! every Btrfs filesystem, so an id alone names one subvolume per filesystem on the machine.

mod support;

use std::path::Path;
use std::sync::Arc;

use ono_change_core::{
    EffectDomain, ProtectionMode, RecoveryCandidate, RecoveryObjective, RecoveryProvider,
    ToolOutput, ToolRunner,
};
use ono_recovery_btrfs::parse::parse_filesystem_show;
use ono_recovery_btrfs::{BtrfsMounts, BtrfsProvider, PROVIDER_ID, SubvolumeRef};
use support::{FILESYSTEM, NGINX_CONF, ROOT_ID, VAR_ID, fixture, runner};

/// A second filesystem, built by the test beside the recorded one.
///
/// Its mounts come first, so a lookup that matches on the subvolume id alone finds them before
/// the recorded filesystem's own — which is the mistake the tests here exist to catch. The lines
/// are the recorded ones with another superblock (`0:88`), another device and other mountpoints.
const OTHER_MOUNTS: &str = "\
8001 7619 0:88 /@ /mnt/other rw,relatime - btrfs /dev/loop21 rw,space_cache=v2,subvolid=256,subvol=/@
8002 7619 0:88 / /mnt/othertop rw,relatime - btrfs /dev/loop21 rw,space_cache=v2,subvolid=5,subvol=/
8003 8001 0:88 /@var /mnt/other/var rw,relatime - btrfs /dev/loop21 rw,space_cache=v2,subvolid=258,subvol=/@var
";

/// The other filesystem's UUID.
const OTHER: &str = "7e4a1c52-93b0-4d6f-8a21-0c5d9e3f6b18";

/// `btrfs filesystem show` for both filesystems: the recorded block, and the same block retold for
/// the second filesystem's UUID and device.
fn both_filesystems() -> Vec<ono_recovery_btrfs::FilesystemInfo> {
    let recorded = fixture("fs-show").stdout().to_owned();
    let other = recorded
        .replace(FILESYSTEM, OTHER)
        .replace("/dev/loop20", "/dev/loop21")
        .replace("onotest", "other");
    vec![
        parse_filesystem_show(&recorded).expect("the recorded listing parses"),
        parse_filesystem_show(&other).expect("the retold listing parses"),
    ]
}

fn two_filesystems() -> BtrfsMounts {
    BtrfsMounts::from_mountinfo(&format!("{OTHER_MOUNTS}{}", fixture("mountinfo").stdout()))
        .identified_by(&both_filesystems())
}

fn candidate(filesystem: &str, id: u64, tree_path: &str) -> RecoveryCandidate {
    let scope = ono_change_core::RecoveryScope::new(
        ono_recovery_btrfs::SCOPE_KIND,
        SubvolumeRef::new(filesystem, id, tree_path).reference(),
        "localhost",
    )
    .covering(NGINX_CONF);
    RecoveryCandidate::new(
        PROVIDER_ID,
        scope,
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        "a subvolume the test names",
    )
}

#[test]
fn should_snapshot_the_subvolume_on_the_filesystem_the_scope_names_when_ids_collide() {
    let runner = runner(vec![
        fixture("subvolume-create"),
        fixture("snapshot-create"),
        fixture("subvol-show-snapshot"),
        fixture("snapshot-ro-flag"),
    ]);
    let provider = BtrfsProvider::new(Arc::clone(&runner) as Arc<dyn ToolRunner>)
        .with_mounts(two_filesystems())
        .for_plan(support::plan_id());
    let actions = provider
        .plan_protection(
            &[candidate(FILESYSTEM, ROOT_ID, "@")],
            ProtectionMode::Prefer,
        )
        .expect("planning runs");
    assert!(
        actions[0]
            .proposed_asset()
            .reference()
            .starts_with("/mnt/top/@snapshots/"),
        "Appendix D.8: the recovery namespace is the one on the scope's own filesystem, not the \
         first `@snapshots` any Btrfs mount shows: {}",
        actions[0].proposed_asset().reference()
    );
    provider.create(&actions[0]).expect("the snapshot is taken");
    let (_, argv) = runner
        .calls()
        .into_iter()
        .find(|(_, argv)| argv.get(1).is_some_and(|verb| verb == "snapshot"))
        .expect("one snapshot call");
    assert_eq!(
        argv.get(3).map(String::as_str),
        Some("/mnt/root"),
        "§56.2: subvolume 256 of {FILESYSTEM} is mounted at /mnt/root. Subvolume 256 of the other \
         filesystem at /mnt/other is a different subvolume that happens to share the number"
    );
}

#[test]
fn should_refuse_a_scope_whose_filesystem_is_not_mounted_even_when_its_id_is() {
    let runner = runner(Vec::new());
    let provider = BtrfsProvider::new(Arc::clone(&runner) as Arc<dyn ToolRunner>)
        .with_mounts(two_filesystems())
        .for_plan(support::plan_id());
    let unknown = "00000000-1111-2222-3333-444444444444";
    let error = provider
        .plan_protection(&[candidate(unknown, ROOT_ID, "@")], ProtectionMode::Prefer)
        .expect_err("§56.3: a subvolume id on a filesystem nobody mounted is not guessed at");
    assert!(
        error.help().is_some_and(|help| help.contains(unknown)),
        "the refusal names the filesystem it could not find: {error:?}"
    );
    assert!(runner.calls().is_empty(), "and nothing was run on the way");
}

#[test]
fn should_lay_a_filesystem_out_with_its_own_mounts_only() {
    let provider = BtrfsProvider::new(runner(vec![fixture("subvol-list-root")]))
        .with_mounts(two_filesystems());
    let layout = provider
        .layout(Path::new("/mnt/othertop"))
        .expect("the listing of the other filesystem reads");
    let var = layout
        .by_id(VAR_ID)
        .expect("the other filesystem has a 258 too");
    assert_eq!(
        layout.visible_path(var),
        Some(Path::new("/mnt/other/var").to_path_buf()),
        "§14.3: where subvolume 258 of the other filesystem is visible is decided by that \
         filesystem's mounts; /mnt/root/var shows the recorded filesystem's 258"
    );
    assert!(
        layout
            .mounts()
            .mounts()
            .iter()
            .all(|mount| mount.device() == "0:88"),
        "a layout carries the mounts of one superblock"
    );
}

#[test]
fn should_identify_the_mounts_it_is_given_before_acting_on_them() {
    let runner = runner(vec![fixture("fs-show")]);
    let provider = BtrfsProvider::new(Arc::clone(&runner) as Arc<dyn ToolRunner>)
        .with_mounts(support::unidentified_mounts())
        .for_plan(support::plan_id());
    provider
        .plan_protection(
            &[candidate(FILESYSTEM, ROOT_ID, "@")],
            ProtectionMode::Prefer,
        )
        .expect("once the mounts carry their filesystem UUID, planning runs");
    assert_eq!(
        runner.calls().first().map(|(_, argv)| argv.clone()),
        Some(vec!["filesystem".to_owned(), "show".to_owned()]),
        "§56.2: which filesystem a mount belongs to is read from `btrfs filesystem show`, not \
         assumed"
    );
}

#[test]
fn should_attach_the_uuid_to_every_mount_of_the_listed_device() {
    let mounts = support::mounts();
    assert!(
        mounts
            .mounts()
            .iter()
            .all(|mount| mount.filesystem_uuid() == Some(FILESYSTEM)),
        "each of the four recorded mounts is /dev/loop20, which `btrfs filesystem show` lists \
         under {FILESYSTEM}"
    );
    let _ = ToolOutput::ok("");
}
