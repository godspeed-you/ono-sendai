//! The same scenarios, against a live pool this suite builds and destroys (§54.4, Appendix G.3).
//!
//! Appendix G.3 is the rule this file exists under: *"Destructive recovery tests MUST run only in
//! disposable loopback/VM/container test environments specifically created for the suite.
//! Production host filesystems MUST never be used for test rollback."* So the harness makes its
//! own pool out of a sparse file, exercises the provider against it, and destroys it — and it
//! refuses to touch a pool whose name does not begin with `onotest`, so a mistyped environment
//! variable cannot point it at anything real.
//!
//! The gate is deliberately two conditions rather than one. `ONO_ZFS_TESTS=1` says the operator
//! meant it; the availability probe says the host can actually do it. A host missing either
//! announces a skip in the house form of v0.4.1 §38.1 — there is no `#[ignore]` here, because an
//! ignored test is a claim of coverage nobody withdrew.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use ono_change_core::{
    ProtectionMode, RecoveryGoal, RecoveryObjective, RecoveryProvider, ToolRunner,
};
use ono_recovery_zfs::{MountTable, ProcessRunner, ZFS, ZPOOL, ZfsProvider};
use ono_testkit::{SkipReason, require};

/// The environment variable that says the operator meant to run these.
const GATE: &str = "ONO_ZFS_TESTS";

/// The prefix Appendix G.3 makes mandatory for anything this suite is allowed to destroy.
const REQUIRED_POOL_PREFIX: &str = "onotest";

/// How large the pool's backing file is. OpenZFS refuses a vdev below 64 MiB.
const IMAGE_BYTES: u64 = 256 * 1024 * 1024;

/// A pool built from nothing for one scenario, and destroyed when the scenario ends.
struct DisposablePool {
    name: String,
    image: PathBuf,
    mountpoint: PathBuf,
    runner: ProcessRunner,
}

impl DisposablePool {
    /// Builds `onotest-<scenario>` on its own sparse file, or announces why it could not.
    fn for_scenario(scenario: &str) -> Option<Self> {
        if require(
            std::env::var(GATE).as_deref() == Ok("1"),
            SkipReason::FixtureNotApplicable,
            "the live-pool suite runs only when ONO_ZFS_TESTS=1 (Appendix G.3)",
        )
        .unmet()
        {
            return None;
        }
        let runner = ProcessRunner::new();
        if require(
            runner.is_available(ZFS) && runner.is_available(ZPOOL),
            SkipReason::ExternalToolUnavailable,
            "this host has no `zfs` and `zpool` to drive",
        )
        .unmet()
        {
            return None;
        }
        if require(
            effective_uid() == Some(0),
            SkipReason::MissingPrivilege,
            "creating and destroying a ZFS pool needs root",
        )
        .unmet()
        {
            return None;
        }

        let name = format!("{REQUIRED_POOL_PREFIX}-{scenario}");
        // Appendix G.3, as a runtime guard rather than a comment: nothing here may run against a
        // pool whose name was not built by this harness.
        assert!(
            name.starts_with(REQUIRED_POOL_PREFIX),
            "Appendix G.3: this suite refuses any pool not named `{REQUIRED_POOL_PREFIX}...`"
        );

        let workdir = std::env::var("ONO_ZFS_WORKDIR")
            .map_or_else(|_| std::env::temp_dir(), PathBuf::from)
            .join(&name);
        let _ = std::fs::remove_dir_all(&workdir);
        std::fs::create_dir_all(&workdir).expect("the harness can make its own working directory");
        let image = workdir.join("pool.img");
        let file = std::fs::File::create(&image).expect("the backing file is creatable");
        file.set_len(IMAGE_BYTES)
            .expect("the backing file can be sized");
        drop(file);
        let mountpoint = workdir.join("mnt");

        let pool = Self {
            name,
            image,
            mountpoint,
            runner,
        };
        let created = pool.zpool(&[
            "create",
            "-f",
            "-m",
            &pool.mountpoint.to_string_lossy(),
            &pool.name,
            &pool.image.to_string_lossy(),
        ]);
        if require(
            created,
            SkipReason::MissingKernelFeature,
            "this host could not create a ZFS pool on a file vdev",
        )
        .unmet()
        {
            return None;
        }
        // The layout the recorded fixtures were taken against, in miniature: a parent dataset,
        // a child dataset inside it, and a sibling that is neither.
        for dataset in ["data", "data/customer", "home"] {
            assert!(
                pool.zfs(&["create", &format!("{}/{dataset}", pool.name)]),
                "the harness can create its own datasets"
            );
        }
        Some(pool)
    }

    fn zfs(&self, argv: &[&str]) -> bool {
        self.runner
            .run(ZFS, argv)
            .is_ok_and(|output| output.succeeded())
    }

    fn zpool(&self, argv: &[&str]) -> bool {
        self.runner
            .run(ZPOOL, argv)
            .is_ok_and(|output| output.succeeded())
    }

    /// The provider under test, reading the kernel's real mount table.
    fn provider(&self) -> ZfsProvider {
        let runner: Arc<dyn ToolRunner> = Arc::new(ProcessRunner::new());
        let table = std::fs::read_to_string(ono_recovery_zfs::MOUNTINFO)
            .map(|text| MountTable::from_text(&text))
            .expect("the kernel mount table is readable");
        ZfsProvider::new(runner)
            .reading_mounts(table)
            .at_instant(support::instant())
            .for_plan(support::plan())
    }

    fn path(&self, relative: &str) -> String {
        self.mountpoint
            .join(relative)
            .to_string_lossy()
            .into_owned()
    }

    fn dataset(&self, relative: &str) -> String {
        format!("{}/{relative}", self.name)
    }
}

impl Drop for DisposablePool {
    fn drop(&mut self) {
        // Appendix G.3 again: the pool exists for one scenario and does not outlive it, whether
        // the scenario passed, failed or panicked.
        let _ = self.zpool(&["destroy", "-f", &self.name]);
        if let Some(workdir) = self.image.parent() {
            let _ = std::fs::remove_dir_all(workdir);
        }
    }
}

/// This process's effective user id, read from procfs so the crate needs no libc of its own.
fn effective_uid() -> Option<u32> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|line| line.split_whitespace().nth(1).map(str::to_owned))
        .and_then(|uid| uid.parse().ok())
}

#[test]
fn should_resolve_a_file_in_a_child_dataset_to_the_child_on_a_live_pool() {
    let Some(pool) = DisposablePool::for_scenario("resolve") else {
        return;
    };
    let target = pool.path("data/customer/db.sqlite");
    std::fs::write(&target, b"before\n").expect("the live dataset is writable");
    let domain = pool
        .provider()
        .resolve_domain(&target)
        .expect("a live pool resolves")
        .expect("the path lies on a ZFS dataset");
    assert_eq!(
        domain.object(),
        Some(pool.dataset("data/customer").as_str()),
        "§13.4 and Appendix B.8, against a real pool: the child dataset is the boundary"
    );
}

#[test]
fn should_not_offer_the_parent_dataset_as_covering_a_child_dataset_on_a_live_pool() {
    let Some(pool) = DisposablePool::for_scenario("boundary") else {
        return;
    };
    let child = pool.path("data/customer/db.sqlite");
    std::fs::write(&child, b"before\n").expect("the live dataset is writable");
    let parent_target = pool.path("data/notes.txt");
    std::fs::write(&parent_target, b"notes\n").expect("the live dataset is writable");
    let provider = pool.provider();
    let domain = provider
        .resolve_domain(&parent_target)
        .expect("a live pool resolves")
        .expect("the path lies on a ZFS dataset");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("a live pool is discoverable");
    for candidate in &candidates {
        assert!(
            !candidate.scope().covers_object(&child),
            "§13.4: a snapshot of {} does not protect {child}",
            pool.dataset("data")
        );
    }
}

#[test]
fn should_create_validate_and_remove_a_snapshot_on_a_live_pool() {
    let Some(pool) = DisposablePool::for_scenario("lifecycle") else {
        return;
    };
    let target = pool.path("data/customer/db.sqlite");
    std::fs::write(&target, b"before\n").expect("the live dataset is writable");
    let provider = pool.provider();
    let domain = provider
        .resolve_domain(&target)
        .expect("a live pool resolves")
        .expect("the path lies on a ZFS dataset");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("a live pool is discoverable");
    let actions = provider
        .plan_protection(&candidates, ProtectionMode::Prefer)
        .expect("a live pool has room");
    let action = actions.first().expect("one dataset, one action");
    let asset = provider.create(action).expect("the snapshot is created");
    let validation = provider.validate(&asset).expect("validation runs");
    assert!(
        validation.is_complete(),
        "§11.4 against a real pool: {:?}",
        validation.failures()
    );
    let cost = provider.estimate_cost(&asset).expect("cost is measurable");
    assert!(
        cost.is_estimated(),
        "§37.5: the figure is labelled estimated"
    );
    provider.cleanup(&asset).expect("the snapshot is removed");
    assert!(
        provider
            .validate(&asset)
            .expect("validation runs")
            .failures()
            .contains(&"the asset does not exist"),
        "§37: what cleanup removed is gone"
    );
}

#[test]
fn should_recover_one_changed_file_without_reverting_unrelated_state_on_a_live_pool() {
    let Some(pool) = DisposablePool::for_scenario("selective") else {
        return;
    };
    let target = pool.path("data/customer/db.sqlite");
    let bystander = pool.path("data/customer/unrelated.txt");
    std::fs::write(&target, b"before\n").expect("the live dataset is writable");
    let provider = pool.provider();
    let domain = provider
        .resolve_domain(&target)
        .expect("a live pool resolves")
        .expect("the path lies on a ZFS dataset");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("a live pool is discoverable");
    let actions = provider
        .plan_protection(&candidates, ProtectionMode::Prefer)
        .expect("a live pool has room");
    let asset = provider
        .create(actions.first().expect("one action"))
        .expect("the snapshot is created");

    std::fs::write(&target, b"after\n").expect("the plan changes the file");
    std::fs::write(&bystander, b"written later\n").expect("something unrelated changes too");

    let fragment = provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("§56.1's facts hold for a pool this suite just built");
    assert_eq!(
        fragment.method(),
        ono_change_core::RestoreMethod::SelectiveFileRestore,
        "§13.5: prefer the method that minimises unrelated rollback damage"
    );
    for action in fragment.actions() {
        provider
            .restore(action, &asset)
            .expect("the selective restore runs");
    }
    assert_eq!(
        std::fs::read_to_string(&target).expect("the file is readable"),
        "before\n",
        "§55.3 case 14: the changed file comes back"
    );
    assert_eq!(
        std::fs::read_to_string(&bystander).expect("the file is readable"),
        "written later\n",
        "§55.3 case 14: without reverting unrelated later files"
    );
}

#[test]
fn should_block_a_full_rollback_that_would_destroy_a_newer_snapshot_on_a_live_pool() {
    let Some(pool) = DisposablePool::for_scenario("rollback") else {
        return;
    };
    let target = pool.path("data/customer/db.sqlite");
    std::fs::write(&target, b"before\n").expect("the live dataset is writable");
    let provider = pool.provider();
    let domain = provider
        .resolve_domain(&target)
        .expect("a live pool resolves")
        .expect("the path lies on a ZFS dataset");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("a live pool is discoverable");
    let actions = provider
        .plan_protection(&candidates, ProtectionMode::Prefer)
        .expect("a live pool has room");
    let asset = provider
        .create(actions.first().expect("one action"))
        .expect("the snapshot is created");

    std::fs::write(&target, b"after\n").expect("the plan changes the file");
    assert!(
        pool.zfs(&[
            "snapshot",
            &format!("{}@later-1", pool.dataset("data/customer"))
        ]),
        "something else took a snapshot afterwards"
    );

    let fragment = provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreDomain)
        .expect("§56.1's facts hold for a pool this suite just built");
    assert!(
        fragment
            .newer_state()
            .destroyed_assets()
            .iter()
            .any(|object| object.ends_with("@later-1")),
        "§13.6 against a real pool: the newer snapshot is enumerated"
    );
    let rollback = fragment
        .actions()
        .iter()
        .find(|action| match action.execution() {
            ono_change_core::Execution::Program { argv, .. } => {
                argv.first().is_some_and(|word| word.as_ref() == "rollback")
            }
            _ => false,
        })
        .expect("the fragment plans the rollback");
    let error = provider
        .restore(rollback, &asset)
        .expect_err("§55.3 case 15: blocked without acceptance");
    assert_eq!(
        error.code().name(),
        "recovery.destructive_history_not_accepted"
    );
    assert!(
        std::fs::read_to_string(&target).expect("the file is readable") == "after\n",
        "§2.3: a refusal changed nothing"
    );
}

#[test]
fn should_record_each_dataset_of_a_recursive_creation_individually_on_a_live_pool() {
    let Some(pool) = DisposablePool::for_scenario("recursive") else {
        return;
    };
    std::fs::write(pool.path("data/customer/db.sqlite"), b"before\n")
        .expect("the live dataset is writable");
    let provider = pool.provider();
    let tree = pool.path("data");
    let domain = provider
        .resolve_domain(&tree)
        .expect("a live pool resolves")
        .expect("the path lies on a ZFS dataset");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("a live pool is discoverable");
    let recursive = candidates
        .into_iter()
        .find(|candidate| candidate.detail().contains("snapshot -r"))
        .expect("§13.3: the child dataset lies inside the tree");
    let actions = provider
        .plan_protection(&[recursive], ProtectionMode::Prefer)
        .expect("a live pool has room");
    assert_eq!(
        actions.len(),
        2,
        "Appendix D.1: one concrete snapshot reference per dataset covered"
    );
    let mut created = Vec::new();
    for action in &actions {
        created.push(provider.create(action).expect("the snapshot is created"));
    }
    for asset in &created {
        let validation = provider.validate(asset).expect("validation runs");
        assert!(
            validation.is_complete(),
            "§13.3: each dataset's snapshot is validated on its own, {:?}",
            validation.failures()
        );
    }
    for asset in &created {
        provider
            .cleanup(asset)
            .expect("each snapshot is removed by name");
    }
}
