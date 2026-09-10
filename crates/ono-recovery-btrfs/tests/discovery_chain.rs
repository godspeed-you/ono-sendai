#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! Appendix A.3 and B.9: a domain goes from resolution into discovery and comes out as the
//! subvolume that really holds the path — including the domain the shell's own resolver hands in.

mod support;

use std::sync::Arc;

use ono_change_core::{PersistenceDomain, RecoveryObjective, RecoveryProvider, ResolvedMount};
use ono_recovery_btrfs::{SCOPE_KIND, SubvolumeRef};
use support::{FILESYSTEM, NESTED_ID, VAR_ID, provider};

/// The domain `ono_change_protection::domain::resolve` builds for a path under `/mnt/root/var`.
///
/// It is built the way that resolver builds it: the superblock's `major:minor` as the mount id,
/// the `subvol=` option as both the mount root and the object, every mount option carried over,
/// and the object kind `btrfs-subvolume`. That object names the *mount's* subvolume and nothing
/// about the filesystem UUID, so it cannot be a subvolume identity on its own (§56.2).
fn core_domain(path: &str, mount_id: &str, object: &str) -> PersistenceDomain {
    let options: Vec<Arc<str>> = [
        "rw",
        "discard=async",
        "space_cache=v2",
        "subvolid=258",
        "subvol=/@var",
    ]
    .into_iter()
    .map(Arc::from)
    .collect();
    let mount = ResolvedMount::new(mount_id, "/mnt/root/var", "btrfs", "/dev/loop20", "/@var")
        .with_options(options);
    PersistenceDomain::resolved(
        path,
        mount,
        "btrfs-subvolume",
        object,
        "mount /mnt/root/var carries the Btrfs subvolume /@var (subvolid=258) of /dev/loop20",
    )
    .with_boundary(object)
}

fn only_candidate_domain(candidates: &[ono_change_core::RecoveryCandidate]) -> SubvolumeRef {
    assert_eq!(
        candidates.len(),
        1,
        "one subvolume holds the path, so one candidate"
    );
    SubvolumeRef::parse(candidates[0].scope().domain()).expect("the scope names a subvolume")
}

#[test]
fn should_discover_the_mounted_subvolume_its_own_resolution_named() {
    let provider = provider(support::discovery_script("subvol-show-plaindir"));
    let domain = provider
        .resolve_domain("/mnt/root/var/plain-dir")
        .expect("the resolution runs")
        .expect("the path is on Btrfs");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs");
    let reference = only_candidate_domain(&candidates);
    assert_eq!(reference.id(), VAR_ID);
    assert_eq!(
        reference.filesystem(),
        FILESYSTEM,
        "Appendix A.3: what `resolve_domain` resolved is what `discover` offers to protect"
    );
}

#[test]
fn should_discover_the_nested_subvolume_for_a_path_inside_it_rather_than_its_mounted_parent() {
    let provider = provider(support::discovery_script("subvol-show-plaindir"));
    let domain = provider
        .resolve_domain("/mnt/root/var/lib-app/state")
        .expect("the resolution runs")
        .expect("the path is on Btrfs");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs");
    let reference = only_candidate_domain(&candidates);
    assert_eq!(
        (reference.id(), reference.tree_path()),
        (NESTED_ID, "@var/lib-app"),
        "Appendix B.9 and §14.3: `lib-app` is not mounted, it is nested inside the mounted `@var`, \
         and a snapshot of `@var` holds an empty directory there — so the candidate is the nested \
         subvolume itself"
    );
}

#[test]
fn should_resolve_the_shell_resolvers_mount_level_domain_to_the_subvolume_holding_the_path() {
    let provider = provider(support::discovery_script("subvol-show-plaindir"));
    let domain = core_domain("/mnt/root/var/lib-app/state", "0:87", "/@var");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("a `subvol=` domain is resolved by the provider rather than dropped");
    let reference = only_candidate_domain(&candidates);
    assert_eq!(
        (reference.filesystem(), reference.id()),
        (FILESYSTEM, NESTED_ID),
        "the object `/@var` is the mount's subvolume; the path lives in the nested `@var/lib-app`, \
         and the provider resolves that itself rather than protecting the parent"
    );
}

#[test]
fn should_refuse_a_mount_level_domain_whose_superblock_is_not_the_one_the_path_is_on() {
    let provider = provider(support::discovery_script("subvol-show-plaindir"));
    let domain = core_domain("/mnt/root/var/lib-app/state", "0:99", "/@var");
    let error = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect_err("§56.3: a domain resolved against another filesystem is not guessed at");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("0:99") && help.contains("0:87")),
        "the refusal names both superblocks, so the mismatch is visible: {error:?}"
    );
}

#[test]
fn should_refuse_rather_than_return_nothing_for_a_btrfs_domain_it_cannot_read() {
    let provider = provider(Vec::new());
    let domain = core_domain("/mnt/root/var/x", "0:87", "not a subvolume reference");
    let error = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect_err("an empty list would read as \"nothing to protect here\" (§55.6 case 29)");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("not a subvolume reference")),
        "and the refusal quotes what it could not read: {error:?}"
    );
}

#[test]
fn should_return_nothing_for_a_domain_that_is_not_btrfs_at_all() {
    let provider = provider(Vec::new());
    let mount = ResolvedMount::new("0:40", "/tank", "zfs", "tank/data", "/");
    let domain = PersistenceDomain::resolved("/tank/x", mount, "zfs-dataset", "tank/data", "zfs");
    assert!(
        provider
            .discover(&domain, RecoveryObjective::PreserveExact)
            .expect("not this provider's business")
            .is_empty(),
        "a ZFS dataset is somebody else's domain, and saying nothing about it is the truth"
    );
    let _ = SCOPE_KIND;
}
