//! The rpm provider's `package-source` records from dnf's own configuration, read under a
//! scratch root (issue #17, ADR-0565). dnf has no machine listing of its repositories —
//! `dnf repolist` is a table for people — so the `.repo` files of dnf.conf(5) are what is read,
//! and the metadata cache dnf keeps beside them is where `refreshed` comes from.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{drain, records};
use ono_provider_api::{Action, ObjectId, Provider, Query};
use ono_provider_linux::RpmPackageProvider;
use ono_testkit::{Scratch, scratch};
use ono_value::{ActionStatus, SchemaId, Value};

fn executable(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write the fake tool");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// Fake rpm and dnf on `<scratch>/bin`, and a root with two repositories, one with a cache.
fn fedora_like(directory: &Scratch) -> (PathBuf, PathBuf) {
    let bin = directory.path().join("bin");
    std::fs::create_dir_all(&bin).expect("create the fake PATH");
    executable(
        &bin.join("rpm"),
        "#!/bin/sh\nprintf 'curl\\t8.6.0-8.fc40\\n'\nexit 0\n",
    );
    executable(
        &bin.join("dnf"),
        "#!/bin/sh\necho 'This command has to be run with superuser privileges' >&2\nexit 1\n",
    );
    let root = directory.path().join("root");
    directory.write(
        "root/etc/yum.repos.d/fedora.repo",
        "[fedora]\nname=Fedora 40 - x86_64\nmetalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-40&arch=x86_64\nenabled=1\n\n[fedora-debuginfo]\nname=Fedora 40 - x86_64 - Debug\nmetalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-debug-40&arch=x86_64\nenabled=0\n",
    );
    directory.write(
        "root/var/cache/dnf/fedora-1a2b3c4d5e6f7a8b/repodata/repomd.xml",
        "<repomd/>",
    );
    (bin, root)
}

#[tokio::test]
async fn should_list_dnf_repositories_from_their_repo_files_with_the_cache_time() {
    let directory = scratch();
    let (bin, root) = fedora_like(&directory);
    let provider = RpmPackageProvider::with_path_and_root(Some(bin.into_os_string()), &root);

    let collected = drain(
        provider
            .snapshot(&Query::target("package-source"))
            .expect("a snapshot"),
    )
    .await;
    assert!(
        collected.errors().is_empty(),
        "got {:?}",
        collected.errors()
    );
    let listed = records(&collected);
    assert_eq!(listed.len(), 2, "two sections that are repositories");

    let fedora = &listed[0];
    assert_eq!(fedora.get("id"), Some(&Value::string("fedora")));
    assert_eq!(
        fedora.get("name"),
        Some(&Value::string("Fedora 40 - x86_64"))
    );
    assert_eq!(fedora.get("enabled"), Some(&Value::Bool(true)));
    assert_eq!(fedora.get("provider"), Some(&Value::string("rpm")));
    assert!(
        matches!(fedora.get("refreshed"), Some(Value::Timestamp(_))),
        "the cache dnf keeps for it says when it was written; got {:?}",
        fedora.get("refreshed")
    );
    let debug = &listed[1];
    assert_eq!(debug.get("id"), Some(&Value::string("fedora-debuginfo")));
    assert_eq!(debug.get("enabled"), Some(&Value::Bool(false)));
    assert_eq!(
        debug.get("refreshed"),
        Some(&Value::Null),
        "no cache, no time — null rather than a guess (spec §35.3)"
    );
}

#[tokio::test]
async fn should_say_what_dnf_would_run_when_a_refresh_is_a_dry_run() {
    let directory = scratch();
    let (bin, root) = fedora_like(&directory);
    let provider = RpmPackageProvider::with_path_and_root(Some(bin.into_os_string()), &root);

    let target = ObjectId::new(
        SchemaId::new("ono.package-source", 1),
        [Value::string("rpm"), Value::string("fedora")],
    );
    let action = Action::new("package-source", "refresh", target).as_dry_run();
    let outcome = provider.act(&action).await.expect("an outcome");
    assert_eq!(outcome.status(), ActionStatus::Skipped);
    assert!(!outcome.changed());
    let message = outcome.message().unwrap_or_default().to_owned();
    assert!(
        message.contains("dnf makecache --refresh") && message.contains("fedora"),
        "the dry run names the command and the repository; got {message:?}"
    );
}
