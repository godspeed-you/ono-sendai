//! What `get file` and `get dir` answer, and that a walk cannot be steered out of its tree
//! (spec §23.4, §28.2, ADR-0015 T14).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers shared between the cases below sit outside a `#[test]` function, where a \
              failed precondition should still abort loudly"
)]

mod common;

use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _, symlink};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use common::{FakeAccounts, drain, find, records};
use ono_provider_api::{Provider, Query, Selector};
use ono_provider_linux::FileProvider;
use ono_value::{ByteSize, FieldAccess, Value};

fn provider() -> FileProvider {
    FileProvider::new().with_accounts(Arc::new(
        FakeAccounts::new()
            .with_user(current_uid(), current_gid(), "ada")
            .with_group(current_gid(), "users", &[]),
    ))
}

fn current_uid() -> u32 {
    fs::metadata("/proc/self")
        .map(|meta| meta.uid())
        .unwrap_or(0)
}

fn current_gid() -> u32 {
    fs::metadata("/proc/self")
        .map(|meta| meta.gid())
        .unwrap_or(0)
}

fn path_selector(path: &Path) -> Selector {
    Selector::field("path", Value::Path(Arc::from(path)))
}

fn file_query(path: &Path) -> Query {
    Query::target("file").with(path_selector(path))
}

fn dir_query(path: &Path) -> Query {
    Query::target("dir").with(path_selector(path))
}

#[tokio::test]
async fn should_report_every_declared_field_of_a_regular_file() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let path = scratch.path().join("report.txt");
    fs::write(&path, "twelve bytes").expect("the file");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).expect("the mode");

    let collected = drain(provider().snapshot(&file_query(&path)).expect("a snapshot")).await;
    let records = records(&collected);
    let file = records.first().expect("one entry was asked for");

    assert_eq!(
        file.get("path"),
        Some(&Value::Path(Arc::from(path.clone())))
    );
    assert_eq!(file.get("name"), Some(&Value::string("report.txt")));
    assert_eq!(file.get("kind"), Some(&Value::string("file")));
    assert_eq!(
        file.get("size"),
        Some(&Value::ByteSize(ByteSize::from_bytes(12)))
    );
    assert_eq!(
        file.get("mode"),
        Some(&Value::string("0640")),
        "the mode is four octal digits, which is what a user can compare against"
    );
    assert!(
        matches!(file.get("modified"), Some(Value::Timestamp(_))),
        "a file the test just wrote has a modification time"
    );
    assert!(matches!(file.get("accessed"), Some(Value::Timestamp(_))));
    assert_eq!(
        file.access("created"),
        FieldAccess::Unknown,
        "birth time is out of reach of the openat-relative walk, and unknown is null"
    );
    assert!(matches!(file.get("inode"), Some(Value::Int(inode)) if *inode > 0));
    assert!(
        matches!(file.get("device"), Some(Value::String(device)) if device.contains(':')),
        "the device is half the identity, so it is reported even for a filesystem with no block \
         device behind it"
    );
    assert_eq!(
        file.access("target"),
        FieldAccess::Unknown,
        "only a symlink has a target"
    );

    let owner = file
        .get("owner")
        .and_then(|value| value.as_record().ok())
        .expect("the owner is a user reference");
    assert_eq!(
        owner.get("uid"),
        Some(&Value::Int(i128::from(current_uid())))
    );
    assert_eq!(owner.get("name"), Some(&Value::string("ada")));

    let source = file.provenance().source().expect("a source");
    assert!(source.contains("report.txt"), "provenance names the path");
    assert_eq!(file.provenance().provider(), "linux.fs");
}

#[tokio::test]
async fn should_report_a_symlink_as_a_link_with_its_target_rather_than_following_it() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let target = scratch.path().join("real.txt");
    fs::write(&target, "content").expect("the file");
    let link = scratch.path().join("link.txt");
    symlink(&target, &link).expect("the symlink");

    let collected = drain(provider().snapshot(&file_query(&link)).expect("a snapshot")).await;
    let record = &records(&collected)[0];

    assert_eq!(record.get("kind"), Some(&Value::string("symlink")));
    assert_eq!(
        record.get("target"),
        Some(&Value::Path(Arc::from(target.clone()))),
        "the link and what it points at are two different things and both are reported"
    );
}

#[tokio::test]
async fn should_describe_the_target_when_asked_to_follow_symlinks() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let target = scratch.path().join("real.txt");
    fs::write(&target, "content").expect("the file");
    let link = scratch.path().join("link.txt");
    symlink(&target, &link).expect("the symlink");

    let query = file_query(&link).option("follow-symlinks", Value::Bool(true));
    let collected = drain(provider().snapshot(&query).expect("a snapshot")).await;
    let record = &records(&collected)[0];

    assert_eq!(record.get("kind"), Some(&Value::string("file")));
    assert_eq!(
        record.get("size"),
        Some(&Value::ByteSize(ByteSize::from_bytes(7)))
    );
}

#[tokio::test]
async fn should_report_a_socket_and_a_directory_by_their_own_kinds() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir(scratch.path().join("nested")).expect("the directory");
    let socket = scratch.path().join("s.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("the socket");
    let fifo = scratch.path().join("pipe");
    nix::unistd::mkfifo(
        &fifo,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .expect("the fifo");

    let collected = drain(
        provider()
            .snapshot(&dir_query(scratch.path()))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);

    assert_eq!(
        find(&records, "name", "nested").and_then(|record| record.get("kind")),
        Some(&Value::string("dir"))
    );
    assert_eq!(
        find(&records, "name", "s.sock").and_then(|record| record.get("kind")),
        Some(&Value::string("socket"))
    );
    assert_eq!(
        find(&records, "name", "pipe").and_then(|record| record.get("kind")),
        Some(&Value::string("fifo"))
    );
    assert_eq!(
        find(&records, "name", "pipe").map(|record| record.access("size")),
        Some(FieldAccess::Unknown),
        "a fifo's st_size describes nothing a user would call a size"
    );
}

#[tokio::test]
async fn should_hide_dot_entries_from_a_directory_listing_unless_all_was_asked_for() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    fs::write(scratch.path().join("visible"), "").expect("a file");
    fs::write(scratch.path().join(".hidden"), "").expect("a dot file");

    let plain = drain(
        provider()
            .snapshot(&dir_query(scratch.path()))
            .expect("a snapshot"),
    )
    .await;
    assert!(find(&records(&plain), "name", ".hidden").is_none());
    assert!(find(&records(&plain), "name", "visible").is_some());

    let all = drain(
        provider()
            .snapshot(&dir_query(scratch.path()).option("all", Value::Bool(true)))
            .expect("a snapshot"),
    )
    .await;
    assert!(find(&records(&all), "name", ".hidden").is_some());
}

#[tokio::test]
async fn should_descend_only_as_far_as_the_depth_allows() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir_all(scratch.path().join("a/b/c")).expect("the tree");
    fs::write(scratch.path().join("a/b/c/deep.txt"), "").expect("the deep file");

    let shallow = drain(
        provider()
            .snapshot(
                &dir_query(scratch.path())
                    .option("recursive", Value::Bool(true))
                    .option("depth", Value::Int(1)),
            )
            .expect("a snapshot"),
    )
    .await;
    assert!(find(&records(&shallow), "name", "deep.txt").is_none());

    let deep = drain(
        provider()
            .snapshot(&dir_query(scratch.path()).option("recursive", Value::Bool(true)))
            .expect("a snapshot"),
    )
    .await;
    assert!(find(&records(&deep), "name", "deep.txt").is_some());
}

#[tokio::test]
async fn should_not_descend_into_a_symlinked_directory_while_walking() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let inside = scratch.path().join("inside");
    let outside = scratch.path().join("outside");
    fs::create_dir_all(&inside).expect("the tree");
    fs::create_dir_all(&outside).expect("the tree");
    fs::write(outside.join("secret.txt"), "").expect("the secret");
    symlink(&outside, inside.join("escape")).expect("the escape hatch");

    let collected = drain(
        provider()
            .snapshot(&dir_query(&inside).option("recursive", Value::Bool(true)))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);

    assert_eq!(
        find(&records, "name", "escape").and_then(|record| record.get("kind")),
        Some(&Value::string("symlink")),
        "the link is reported as what it is"
    );
    assert!(
        find(&records, "name", "secret.txt").is_none(),
        "a walk descends with O_NOFOLLOW, so a symlink cannot carry it out of the tree"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn should_stay_inside_the_tree_when_a_directory_is_swapped_for_a_symlink_mid_walk() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let root = scratch.path().join("root");
    let outside = scratch.path().join("outside");
    fs::create_dir_all(&outside).expect("the outside tree");
    fs::write(outside.join("secret.txt"), "stolen").expect("the secret");

    // Enough entries that the walk is still running when the swap lands. The assertion below
    // holds under every interleaving, so the test is deterministic whatever the timing does:
    // the walk descends through descriptors it already holds and never re-resolves a path.
    fs::create_dir_all(&root).expect("the tree");
    for index in 0..400 {
        let branch = root.join(format!("branch-{index:03}"));
        fs::create_dir_all(&branch).expect("a branch");
        fs::write(branch.join("leaf.txt"), "").expect("a leaf");
    }
    let victim = root.join("zz-victim");
    fs::create_dir_all(&victim).expect("the victim");
    fs::write(victim.join("inside.txt"), "").expect("a file inside the victim");

    let stream = provider()
        .snapshot(&dir_query(&root).option("recursive", Value::Bool(true)))
        .expect("a snapshot");

    let swapper = tokio::spawn({
        let victim = victim.clone();
        let outside = outside.clone();
        async move {
            tokio::time::sleep(Duration::from_micros(200)).await;
            let _ = fs::remove_dir_all(&victim);
            let _ = symlink(&outside, &victim);
        }
    });

    let collected = drain(stream).await;
    swapper.await.expect("the swap task finished");

    for record in records(&collected) {
        let path = record
            .get("path")
            .and_then(|value| value.as_path().ok())
            .map(Path::to_path_buf)
            .expect("every record carries the path it was reached by");
        assert!(
            path.starts_with(&root),
            "the walk reported {} which is outside the tree it was given",
            path.display()
        );
        assert_ne!(
            record.get("name"),
            Some(&Value::string("secret.txt")),
            "the swapped symlink redirected the walk out of its tree"
        );
    }
}

#[tokio::test]
async fn should_report_an_unreadable_directory_without_ending_the_walk() {
    if fs::metadata("/proc/self").is_ok_and(|meta| meta.uid() == 0) {
        // Mode bits do not restrain root, so this fixture would assert nothing.
        return;
    }
    let scratch = tempfile::tempdir().expect("a temporary directory");
    fs::write(scratch.path().join("readable.txt"), "").expect("a file");
    let closed = scratch.path().join("closed");
    fs::create_dir(&closed).expect("the directory");
    fs::write(closed.join("unreachable.txt"), "").expect("a file inside");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).expect("the mode");

    let collected = drain(
        provider()
            .snapshot(&dir_query(scratch.path()).option("recursive", Value::Bool(true)))
            .expect("a snapshot"),
    )
    .await;

    assert!(
        find(&records(&collected), "name", "readable.txt").is_some(),
        "one closed directory must not cost the entries that could be read (spec §16.5)"
    );
    assert!(
        collected
            .errors()
            .iter()
            .any(|error| error.code() == ono_core::ErrorCode::IoPermissionDenied),
        "the closed directory is reported on the error channel rather than silently skipped"
    );

    fs::set_permissions(&closed, fs::Permissions::from_mode(0o700)).expect("restore the mode");
}

#[tokio::test]
async fn should_yield_the_first_entry_of_a_large_tree_within_the_latency_budget() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    for index in 0..3000 {
        fs::write(scratch.path().join(format!("entry-{index:04}")), "").expect("an entry");
    }

    let mut stream = provider()
        .snapshot(&dir_query(scratch.path()).option("recursive", Value::Bool(true)))
        .expect("a snapshot");
    let first = tokio::time::timeout(Duration::from_millis(50), stream.recv())
        .await
        .expect("spec §34 budgets the first row of a listing well under 50 ms");
    assert!(first.is_some(), "a large tree still produces a first row");
}

#[tokio::test]
async fn should_stop_walking_when_the_consumer_asks_for_only_the_first_entries() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    for index in 0..500 {
        fs::write(scratch.path().join(format!("entry-{index:03}")), "").expect("an entry");
    }

    let collected = drain(
        provider()
            .snapshot(
                &dir_query(scratch.path())
                    .option("recursive", Value::Bool(true))
                    .limit(3),
            )
            .expect("a snapshot"),
    )
    .await;
    assert_eq!(records(&collected).len(), 3);
}

#[tokio::test]
async fn should_report_a_path_that_does_not_exist_as_a_failure_rather_than_as_an_empty_result() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let missing = scratch.path().join("nowhere");

    let collected = drain(
        provider()
            .snapshot(&file_query(&missing))
            .expect("a stream"),
    )
    .await;
    assert!(records(&collected).is_empty());
    assert_eq!(
        collected.errors().first().map(ono_value::ErrorValue::code),
        Some(ono_core::ErrorCode::IoNotFound),
        "nothing there and could not look are different answers"
    );
}

#[tokio::test]
async fn should_identify_a_file_by_its_device_and_inode() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let path = scratch.path().join("one.txt");
    fs::write(&path, "x").expect("the file");
    let link = scratch.path().join("hard.txt");
    fs::hard_link(&path, &link).expect("the hard link");

    let first = drain(provider().snapshot(&file_query(&path)).expect("a snapshot")).await;
    let second = drain(provider().snapshot(&file_query(&link)).expect("a snapshot")).await;

    let left = ono_provider_api::ObjectId::of(&records(&first)[0]).expect("an identity");
    let right = ono_provider_api::ObjectId::of(&records(&second)[0]).expect("an identity");
    assert_eq!(
        left, right,
        "two hard links are two paths to one object, which is why path is not the identity"
    );
    assert_ne!(
        records(&first)[0].get("path"),
        records(&second)[0].get("path"),
        "the path each was reached by is still reported"
    );
}

#[tokio::test]
async fn should_resolve_a_path_to_one_object_reference() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let path = scratch.path().join("resolve.txt");
    fs::write(&path, "x").expect("the file");

    let found = provider()
        .resolve(&path_selector(&path))
        .await
        .expect("the path resolves");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].label(), "resolve.txt");
}

#[test]
fn should_claim_the_file_and_dir_targets_with_the_registry_capability_ids() {
    let provider = FileProvider::new();
    assert_eq!(provider.targets(), ["file", "dir"]);
    let ids: Vec<String> = provider
        .capabilities()
        .iter()
        .map(|capability| capability.id().to_owned())
        .collect();
    assert!(ids.contains(&"file.list".to_owned()));
    assert!(ids.contains(&"dir.list".to_owned()));
    assert!(ids.contains(&"file.find".to_owned()));
}
