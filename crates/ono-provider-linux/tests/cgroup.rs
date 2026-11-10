//! `get cgroup`: every control group of the unified hierarchy as one `ono.cgroup/1` record, read
//! from the directories the kernel mounts under `/sys/fs/cgroup` (spec §8.1, v0.4 §16.3,
//! ADR-0971, #206).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers shared between the cases below sit outside a `#[test]` function, where a \
              failed precondition should still abort loudly"
)]

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use common::{drain, records};
use ono_provider_api::{Availability, Provider, Query, Selector};
use ono_provider_linux::CgroupProvider;
use ono_value::Value;

/// A unified hierarchy under `<root>/sys/fs/cgroup` holding `groups`, each a relative path.
fn hierarchy(root: &Path, groups: &[&str]) {
    let mount = root.join("sys/fs/cgroup");
    fs::create_dir_all(&mount).expect("the cgroup mount");
    fs::write(mount.join("cgroup.controllers"), "cpu io memory pids\n").expect("the controllers");
    for group in groups {
        let dir = mount.join(group);
        fs::create_dir_all(&dir).expect("a control group");
        // The files every group carries; a walker must not take them for groups.
        fs::write(dir.join("cgroup.procs"), "").expect("the procs file");
    }
}

fn path(text: &str) -> Value {
    Value::Path(Arc::from(Path::new(text)))
}

fn path_of(record: &ono_value::RecordValue) -> String {
    match record.get("path") {
        Some(Value::Path(path)) => path.to_string_lossy().into_owned(),
        other => panic!("a cgroup record carries its path, got {other:?}"),
    }
}

#[tokio::test]
async fn should_list_every_group_of_the_hierarchy_with_its_name_and_its_parent() {
    let root = tempfile::tempdir().expect("a temporary directory");
    hierarchy(
        root.path(),
        &[
            "system.slice/nginx.service",
            "system.slice/cron.service",
            "user.slice/user-1000.slice",
        ],
    );

    let collected = drain(
        CgroupProvider::rooted(root.path())
            .snapshot(&Query::target("cgroup"))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);
    let paths: Vec<String> = records.iter().map(|record| path_of(record)).collect();
    assert_eq!(
        paths,
        [
            "/",
            "/system.slice",
            "/system.slice/cron.service",
            "/system.slice/nginx.service",
            "/user.slice",
            "/user.slice/user-1000.slice",
        ],
        "every directory is one group, in path order, and no file is one"
    );

    let nginx = &records[3];
    assert_eq!(nginx.get("name"), Some(&Value::string("nginx.service")));
    assert_eq!(
        nginx.get("parent"),
        Some(&path("/system.slice")),
        "the enclosing group, in the same form as the path"
    );
    let top = &records[0];
    assert_eq!(top.get("name"), Some(&Value::string("/")));
    assert_eq!(
        top.get("parent"),
        Some(&Value::Null),
        "the root of the hierarchy has no parent"
    );
    assert_eq!(
        records[1].get("parent"),
        Some(&path("/")),
        "a top-level slice sits in the root group"
    );
}

#[tokio::test]
async fn should_read_the_unified_mount_of_a_hybrid_host() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let unified = root.path().join("sys/fs/cgroup/unified");
    fs::create_dir_all(unified.join("init.scope")).expect("a group");
    fs::write(unified.join("cgroup.controllers"), "").expect("the controllers");
    // A v1 controller mount beside it, which is not part of the unified hierarchy.
    fs::create_dir_all(root.path().join("sys/fs/cgroup/memory/docker")).expect("a v1 group");

    let provider = CgroupProvider::rooted(root.path());
    assert_eq!(provider.availability(), Availability::Available);
    let collected = drain(
        provider
            .snapshot(&Query::target("cgroup"))
            .expect("a snapshot"),
    )
    .await;
    let paths: Vec<String> = records(&collected)
        .iter()
        .map(|record| path_of(record))
        .collect();
    assert_eq!(paths, ["/", "/init.scope"]);
}

#[tokio::test]
async fn should_say_it_is_unavailable_where_no_unified_hierarchy_is_mounted() {
    let root = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir_all(root.path().join("sys/fs/cgroup/memory")).expect("a v1 mount");

    let provider = CgroupProvider::rooted(root.path());
    assert!(
        matches!(provider.availability(), Availability::Unavailable(_)),
        "a host without cgroup v2 has no hierarchy to list, which is not the same as an empty one"
    );
    let collected = drain(
        provider
            .snapshot(&Query::target("cgroup"))
            .expect("a snapshot"),
    )
    .await;
    assert!(records(&collected).is_empty());
    assert_eq!(
        collected.errors().len(),
        1,
        "the stream says why it is empty: {:?}",
        collected.errors()
    );
}

#[tokio::test]
async fn should_resolve_one_group_by_its_path() {
    let root = tempfile::tempdir().expect("a temporary directory");
    hierarchy(root.path(), &["system.slice/nginx.service"]);

    let references = CgroupProvider::rooted(root.path())
        .resolve(&Selector::field(
            "path",
            path("/system.slice/nginx.service"),
        ))
        .await
        .expect("a resolution");
    assert_eq!(references.len(), 1, "{references:?}");
}

/// v0.4.1 §34.4 and §2.17: the orientation of `compute/cgroups` reads a bounded view of the
/// hierarchy, and the count it shows must stay the count that is there (ADR-0576).
#[tokio::test]
async fn should_state_how_many_groups_there_are_when_a_query_bounds_what_it_answers() {
    let root = tempfile::tempdir().expect("a temporary directory");
    hierarchy(
        root.path(),
        &["system.slice/nginx.service", "user.slice/user-1000.slice"],
    );
    let provider = CgroupProvider::rooted(root.path());

    let bounded = drain(
        provider
            .snapshot(&Query::target("cgroup").limit(2))
            .expect("a snapshot"),
    )
    .await;
    assert_eq!(records(&bounded).len(), 2, "the bound is honoured");
    assert_eq!(
        bounded.diagnostics().population(),
        Some(5),
        "the root, two slices and a group in each: the bounded answer says how many it left out"
    );

    let narrowed = drain(
        provider
            .snapshot(
                &Query::target("cgroup")
                    .with(Selector::field("name", Value::string("nginx.service")))
                    .limit(2),
            )
            .expect("a snapshot"),
    )
    .await;
    assert_eq!(
        narrowed.diagnostics().population(),
        None,
        "a query that selects states no population: the figure would be the size of the hierarchy, \
         which is the answer to a different question"
    );
}
