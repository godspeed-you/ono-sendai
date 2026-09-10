#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! §15.4 and §43.5 against a real directory: a restore writes a sibling file and renames it over
//! the live path, and it never writes through a symlink it did not create.

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use ono_recovery_btrfs::{FileStore, SystemFiles};

/// A fresh directory for one test, under the target directory rather than a shared `/tmp`.
fn scratch(name: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("ono-recovery-btrfs-atomic-copy")
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("the scratch directory is created");
    directory
}

fn entries(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .expect("the directory lists")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn should_replace_the_live_file_by_rename_rather_than_truncating_it_in_place() {
    let root = scratch("rename");
    let snapshot = root.join("snapshot.conf");
    let live_dir = root.join("live");
    std::fs::create_dir_all(&live_dir).unwrap();
    let live = live_dir.join("nginx.conf");
    std::fs::write(&snapshot, "worker_processes 4;\n").unwrap();
    std::fs::set_permissions(&snapshot, std::fs::Permissions::from_mode(0o640)).unwrap();
    std::fs::write(&live, "worker_processes 8;\n").unwrap();
    // A second name for the live file's inode: an in-place truncate would change what it reads.
    std::fs::hard_link(&live, root.join("witness")).unwrap();

    SystemFiles
        .copy(&snapshot, &live)
        .expect("the restore runs");

    assert_eq!(
        std::fs::read_to_string(&live).unwrap(),
        "worker_processes 4;\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("witness")).unwrap(),
        "worker_processes 8;\n",
        "§15.4: the live path now names a new file; the old inode was never truncated, so a \
         reader holding it open saw one whole version or the other"
    );
    assert_eq!(
        std::fs::metadata(&live).unwrap().permissions().mode() & 0o7777,
        0o640,
        "Appendix C.7: the mode comes back with the content"
    );
    assert_eq!(
        entries(&live_dir),
        vec!["nginx.conf".to_owned()],
        "and no temporary file is left beside it"
    );
}

#[test]
fn should_refuse_to_write_through_a_symlink_at_the_destination() {
    let root = scratch("symlink-destination");
    let snapshot = root.join("snapshot.conf");
    let precious = root.join("precious");
    let live = root.join("nginx.conf");
    std::fs::write(&snapshot, "restored\n").unwrap();
    std::fs::write(&precious, "somebody else's file\n").unwrap();
    symlink(&precious, &live).unwrap();

    let error = SystemFiles
        .copy(&snapshot, &live)
        .expect_err("§43.5: a symlink at the live path is refused, not followed");

    assert!(
        error.message().contains("symlink"),
        "the refusal says why: {error:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&precious).unwrap(),
        "somebody else's file\n",
        "the file the link points at is untouched"
    );
    assert!(
        std::fs::symlink_metadata(&live)
            .unwrap()
            .file_type()
            .is_symlink(),
        "and the link itself is left as it was found"
    );
}

#[test]
fn should_refuse_when_a_directory_on_the_way_to_the_destination_is_a_symlink() {
    let root = scratch("symlink-component");
    let snapshot = root.join("snapshot.conf");
    let elsewhere = root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(&snapshot, "restored\n").unwrap();
    symlink(&elsewhere, root.join("etc")).unwrap();

    SystemFiles
        .copy(&snapshot, &root.join("etc").join("nginx.conf"))
        .expect_err("§43.5: a replaced directory symlink is how a restore lands somewhere else");

    assert!(
        entries(&elsewhere).is_empty(),
        "nothing was written where the symlink points"
    );
}

#[test]
fn should_restore_a_symlink_as_a_symlink() {
    let root = scratch("symlink-source");
    let snapshot = root.join("snapshot-link");
    let live = root.join("live-link");
    symlink("sites-available/default", &snapshot).unwrap();
    std::fs::write(&live, "a regular file now\n").unwrap();

    SystemFiles
        .copy(&snapshot, &live)
        .expect("the restore runs");

    assert_eq!(
        std::fs::read_link(&live).unwrap(),
        PathBuf::from("sites-available/default"),
        "§15.1: a symlink comes back as the link it was, not as the bytes of whatever it names"
    );
}

#[test]
fn should_create_a_missing_parent_directory_it_then_owns() {
    let root = scratch("missing-parent");
    let snapshot = root.join("snapshot.conf");
    std::fs::write(&snapshot, "restored\n").unwrap();
    let live = root.join("etc").join("nginx").join("nginx.conf");

    SystemFiles
        .copy(&snapshot, &live)
        .expect("the restore runs");

    assert_eq!(std::fs::read_to_string(&live).unwrap(), "restored\n");
}

#[test]
fn should_leave_the_live_file_and_no_temporary_behind_when_the_source_cannot_be_read() {
    let root = scratch("missing-source");
    let live = root.join("nginx.conf");
    std::fs::write(&live, "live\n").unwrap();

    SystemFiles
        .copy(&root.join("absent"), &live)
        .expect_err("there is nothing to restore from");

    assert_eq!(std::fs::read_to_string(&live).unwrap(), "live\n");
    assert_eq!(entries(&root), vec!["nginx.conf".to_owned()]);
}

#[test]
fn should_say_whether_anything_is_at_a_path_without_following_a_symlink() {
    let root = scratch("exists");
    symlink(root.join("nowhere"), root.join("dangling")).unwrap();
    assert!(
        SystemFiles.exists(&root.join("dangling")).unwrap(),
        "a dangling symlink is something at the path, and a rename would replace it"
    );
    assert!(!SystemFiles.exists(&root.join("nothing")).unwrap());
}
