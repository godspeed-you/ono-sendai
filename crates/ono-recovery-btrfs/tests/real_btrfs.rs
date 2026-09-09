#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! The same scenarios against a real Btrfs filesystem the harness builds and tears down.
//!
//! §54.4 asks for real ZFS and Btrfs where the environment permits, and Appendix G.3 fixes the
//! terms: *"Destructive recovery tests MUST run only in disposable loopback/VM/container test
//! environments specifically created for the suite. Production host filesystems MUST never be
//! used for test rollback."*
//!
//! So the harness makes its own filesystem out of a file, labels it `onotest`, and every
//! destructive step checks that label first — [`is_disposable`] is the guard, and it is exercised
//! by an ordinary test that runs everywhere, because a guard nobody checks is a guard nobody has.
//!
//! Running the suite needs three things: `ONO_BTRFS_TESTS=1`, a `btrfs` this provider has
//! validated, and the privilege to make and mount a loop device. Where one of them is missing the
//! test announces a skip through [`ono_testkit::require`] and returns, so the run reports the
//! coverage it had rather than the coverage it wanted (§38.1 of v0.4.1).

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use ono_change_core::{ProtectionMode, RecoveryGoal, RecoveryObjective, RecoveryProvider};
use ono_recovery_btrfs::{BtrfsMounts, BtrfsProvider, ProcessRunner, SubvolumeRef, SystemFiles};
use ono_testkit::{SkipReason, require};

/// The label the harness gives the filesystem it owns (Appendix G.3).
pub const TEST_LABEL: &str = "onotest";

/// The environment variable that turns the suite on.
pub const GATE: &str = "ONO_BTRFS_TESTS";

/// Whether a filesystem carrying `label` is one this suite may write to (Appendix G.3).
///
/// The answer is `true` for exactly one label, and an unlabelled filesystem is never it. A
/// production root is far more likely to be unlabelled than to be labelled `onotest`, so the
/// permissive reading of "no label" is the one that would eventually reformat somebody's laptop.
#[must_use]
pub fn is_disposable(label: Option<&str>) -> bool {
    label == Some(TEST_LABEL)
}

#[test]
fn should_refuse_to_touch_a_filesystem_that_is_not_the_harness_own() {
    assert!(is_disposable(Some(TEST_LABEL)));
    assert!(
        !is_disposable(None),
        "Appendix G.3: an unlabelled filesystem is not a disposable one. A production root \
         usually has no label at all, and reading that as consent is how a test suite destroys a \
         machine"
    );
    assert!(!is_disposable(Some("rpool")));
    assert!(!is_disposable(Some("ONOTEST")));
}

/// A Btrfs filesystem built out of a file, mounted, and removed again on drop.
struct LoopFilesystem {
    directory: PathBuf,
    device: String,
    mounted: Vec<PathBuf>,
}

impl LoopFilesystem {
    /// The top level of the filesystem.
    fn top(&self) -> PathBuf {
        self.directory.join("top")
    }

    /// Where `@` is mounted.
    fn root(&self) -> PathBuf {
        self.directory.join("root")
    }

    /// The provider under test, driving the real `btrfs` through the real runner.
    fn provider(&self) -> BtrfsProvider {
        BtrfsProvider::new(Arc::new(ProcessRunner::new()))
            .with_files(Arc::new(SystemFiles))
            .with_mounts(BtrfsMounts::from_proc().expect("the kernel mount table is readable"))
            .for_plan(support::plan_id())
    }
}

impl Drop for LoopFilesystem {
    fn drop(&mut self) {
        for mounted in self.mounted.iter().rev() {
            let _ = run("umount", &[&mounted.to_string_lossy()]);
        }
        let _ = run("losetup", &["-d", &self.device]);
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Runs one program, returning whether it succeeded and what it said.
fn run(program: &str, argv: &[&str]) -> (bool, String) {
    match Command::new(program).args(argv).output() {
        Ok(output) => (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ),
        Err(error) => (false, error.to_string()),
    }
}

/// Builds the filesystem, or announces why this host cannot present one.
fn harness() -> Option<LoopFilesystem> {
    if require(
        std::env::var(GATE).is_ok_and(|value| value == "1"),
        SkipReason::FixtureNotApplicable,
        "the real-filesystem suite runs only under ONO_BTRFS_TESTS=1, because it makes and mounts \
         a loop device",
    )
    .unmet()
    {
        return None;
    }
    let probe = BtrfsProvider::new(Arc::new(ProcessRunner::new()));
    if require(
        probe.availability().is_available(),
        SkipReason::ExternalToolUnavailable,
        &format!(
            "this host has no btrfs-progs this provider has validated: {}",
            probe.availability().reason().unwrap_or("no reason given")
        ),
    )
    .unmet()
    {
        return None;
    }
    if require(
        run("id", &["-u"]).1.trim() == "0",
        SkipReason::MissingPrivilege,
        "making a loop device and mounting it needs root, and Appendix G.3 forbids using a \
         filesystem the suite did not create",
    )
    .unmet()
    {
        return None;
    }

    let directory = std::env::temp_dir().join(format!("ono-btrfs-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("top")).ok()?;
    std::fs::create_dir_all(directory.join("root")).ok()?;
    let image = directory.join("image");
    let image_text = image.to_string_lossy().into_owned();
    if !run("truncate", &["-s", "512M", &image_text]).0 {
        let _ = std::fs::remove_dir_all(&directory);
        return None;
    }
    let (made, device) = run("losetup", &["--find", "--show", &image_text]);
    let device = device.trim().to_owned();
    if !made || device.is_empty() {
        let _ = std::fs::remove_dir_all(&directory);
        return None;
    }
    let mut filesystem = LoopFilesystem {
        directory: directory.clone(),
        device: device.clone(),
        mounted: Vec::new(),
    };
    assert!(
        run("mkfs.btrfs", &["-q", "-L", TEST_LABEL, &device]).0,
        "the harness makes its own filesystem, labelled {TEST_LABEL} (Appendix G.3)"
    );
    let top = filesystem.top().to_string_lossy().into_owned();
    assert!(run("mount", &[&device, &top]).0, "the top level mounts");
    filesystem.mounted.push(filesystem.top());

    for subvolume in ["@", "@home", "@var", "@snapshots"] {
        assert!(
            run(
                "btrfs",
                &[
                    "subvolume",
                    "create",
                    &filesystem.top().join(subvolume).to_string_lossy(),
                ],
            )
            .0,
            "the harness creates {subvolume}"
        );
    }
    assert!(
        run(
            "btrfs",
            &[
                "subvolume",
                "create",
                &filesystem.top().join("@var/lib-app").to_string_lossy(),
            ],
        )
        .0,
        "§14.3's nested subvolume, inside @var"
    );
    std::fs::create_dir_all(filesystem.top().join("@var/lib-app/state")).ok()?;
    std::fs::write(
        filesystem.top().join("@var/lib-app/state/db"),
        b"live nested state\n",
    )
    .ok()?;
    std::fs::create_dir_all(filesystem.top().join("@/etc/nginx")).ok()?;
    std::fs::write(
        filesystem.top().join("@/etc/nginx/nginx.conf"),
        b"worker_processes 4;\n",
    )
    .ok()?;
    std::fs::create_dir_all(filesystem.top().join("@/looks-like-a-subvol")).ok()?;

    let root = filesystem.root().to_string_lossy().into_owned();
    assert!(
        run("mount", &["-o", "subvol=@", &device, &root]).0,
        "the root subvolume mounts where a running system would have it"
    );
    filesystem.mounted.push(filesystem.root());
    std::fs::create_dir_all(filesystem.root().join("var")).ok()?;
    std::fs::create_dir_all(filesystem.root().join("home")).ok()?;
    let var = filesystem.root().join("var").to_string_lossy().into_owned();
    assert!(run("mount", &["-o", "subvol=@var", &device, &var]).0);
    filesystem.mounted.push(filesystem.root().join("var"));
    let home = filesystem
        .root()
        .join("home")
        .to_string_lossy()
        .into_owned();
    assert!(run("mount", &["-o", "subvol=@home", &device, &home]).0);
    filesystem.mounted.push(filesystem.root().join("home"));

    let provider = filesystem.provider();
    let info = provider
        .filesystem_info(&filesystem.top())
        .expect("the filesystem answers");
    assert!(
        is_disposable(info.label()),
        "Appendix G.3: this suite refuses to run against any filesystem but the one it made"
    );
    Some(filesystem)
}

#[test]
fn should_resolve_a_real_subvolume_identity_from_real_metadata() {
    let Some(filesystem) = harness() else { return };
    let provider = filesystem.provider();
    let conf = filesystem.root().join("etc/nginx/nginx.conf");
    let domain = provider
        .resolve_domain(&conf.to_string_lossy())
        .expect("the resolution runs")
        .expect("the path is on Btrfs");
    let reference =
        SubvolumeRef::parse(domain.object().expect("a resolved object")).expect("a subvolume");
    assert_eq!(reference.tree_path(), "@");
    assert!(reference.id() >= 256, "§14.1: a real, stable subvolume id");

    let looks_like = filesystem.root().join("looks-like-a-subvol");
    let plain = provider
        .resolve_domain(&looks_like.to_string_lossy())
        .expect("the resolution runs")
        .expect("the path is on Btrfs");
    assert_eq!(
        SubvolumeRef::parse(plain.object().expect("resolved"))
            .expect("a subvolume")
            .id(),
        reference.id(),
        "Appendix B.9, against a real filesystem: a directory named like a subvolume resolves to \
         the subvolume containing it"
    );
}

#[test]
fn should_find_the_real_nested_subvolume_boundary() {
    let Some(filesystem) = harness() else { return };
    let provider = filesystem.provider();
    let layout = provider
        .layout(&filesystem.top())
        .expect("the subvolumes list");
    let var = layout.by_tree_path("@var").expect("@var exists");
    let nested: Vec<&str> = layout
        .nested_within(var)
        .iter()
        .map(|boundary| boundary.tree_path())
        .collect();
    assert_eq!(
        nested,
        vec!["@var/lib-app"],
        "§14.3: the boundary is real, and the provider finds it in real metadata"
    );
}

#[test]
fn should_show_an_empty_directory_where_a_nested_subvolume_was_in_a_real_snapshot() {
    let Some(filesystem) = harness() else { return };
    let provider = filesystem.provider();
    let domain = provider
        .resolve_domain(&filesystem.root().join("var/log").to_string_lossy())
        .expect("the resolution runs")
        .expect("on Btrfs");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs");
    let actions = provider
        .plan_protection(&candidates, ProtectionMode::Prefer)
        .expect("planning runs");
    let asset = provider.create(&actions[0]).expect("the snapshot is taken");

    let inside = Path::new(asset.reference()).join("lib-app");
    let entries: Vec<_> = std::fs::read_dir(&inside)
        .expect("the snapshot holds the mountpoint directory")
        .collect();
    assert!(
        entries.is_empty(),
        "§14.3, proven on a live filesystem: the nested subvolume's contents are not in the \
         parent's snapshot. {} holds {} entries, and the live path holds its `state` directory",
        inside.display(),
        entries.len()
    );
    assert!(
        filesystem.root().join("var/lib-app/state/db").exists(),
        "while the live nested subvolume still has everything in it"
    );
    provider
        .cleanup(&asset)
        .expect("§37: and it is removed again");
}

#[test]
fn should_create_a_real_read_only_snapshot_and_verify_the_flag() {
    let Some(filesystem) = harness() else { return };
    let provider = filesystem.provider();
    let conf = filesystem.root().join("etc/nginx/nginx.conf");
    let domain = provider
        .resolve_domain(&conf.to_string_lossy())
        .expect("resolution runs")
        .expect("on Btrfs");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs");
    let actions = provider
        .plan_protection(&candidates, ProtectionMode::Prefer)
        .expect("planning runs");
    let asset = provider.create(&actions[0]).expect("the snapshot is taken");
    assert!(
        provider
            .is_read_only(Path::new(asset.reference()))
            .expect("the flag is readable"),
        "§14.5: the snapshot really is read-only, and the provider read the flag rather than \
         assuming it"
    );
    let validation = provider.validate(&asset).expect("validation runs");
    assert!(
        validation.is_complete(),
        "§11.4: it exists, it is a snapshot of the subvolume the plan named, and a restore path \
         is available: {:?}",
        validation.failures()
    );
    provider.cleanup(&asset).expect("§37: cleanup removes it");
    assert!(
        !Path::new(asset.reference()).exists(),
        "and the snapshot is gone from the filesystem"
    );
}

#[test]
fn should_restore_one_real_file_out_of_a_real_snapshot() {
    let Some(filesystem) = harness() else { return };
    let provider = filesystem.provider();
    let conf = filesystem.root().join("etc/nginx/nginx.conf");
    let domain = provider
        .resolve_domain(&conf.to_string_lossy())
        .expect("resolution runs")
        .expect("on Btrfs");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs");
    let actions = provider
        .plan_protection(&candidates, ProtectionMode::Prefer)
        .expect("planning runs");
    let asset = provider.create(&actions[0]).expect("the snapshot is taken");

    std::fs::write(&conf, b"worker_processes 8;\n").expect("the plan changes the file");
    let fragment = provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("§56.2's ten facts are established against a real filesystem");
    let conflicts = fragment
        .newer_state()
        .items()
        .iter()
        .filter(|item| item.class() == ono_change_core::NewerStateClass::Conflicting)
        .count();
    assert_eq!(
        conflicts, 1,
        "Appendix C.4: the live file differs from the snapshot, so restoring it discards the \
         later edit"
    );
    for action in fragment.actions() {
        if action.role() == ono_change_core::ActionRole::Recover {
            provider.restore(action, &asset).expect("the restore runs");
        }
    }
    assert_eq!(
        std::fs::read_to_string(&conf).expect("the file is readable"),
        "worker_processes 4;\n",
        "§13.5: the snapshot's bytes came back"
    );
    assert!(
        filesystem.root().join("home").exists(),
        "§59.6: and nothing else was touched"
    );
    provider
        .cleanup(&asset)
        .expect("§37: cleanup removes the asset");
}

#[test]
fn should_report_a_real_cost_that_is_estimated_and_never_zero() {
    let Some(filesystem) = harness() else { return };
    let provider = filesystem.provider();
    let domain = provider
        .resolve_domain(&filesystem.root().join("etc").to_string_lossy())
        .expect("resolution runs")
        .expect("on Btrfs");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs");
    let actions = provider
        .plan_protection(&candidates, ProtectionMode::Prefer)
        .expect("planning runs");
    let asset = provider.create(&actions[0]).expect("the snapshot is taken");
    let cost = provider
        .estimate_cost(&asset)
        .expect("the cost is reported");
    assert!(cost.is_estimated(), "§37.5 against a real filesystem");
    assert_ne!(
        cost.retained_bytes(),
        Some(ono_value::ByteSize::ZERO),
        "§38.2: never free"
    );
    provider
        .cleanup(&asset)
        .expect("§37: cleanup removes the asset");
}
