//! What the suites need from outside: recorded mount tables and a recovery provider under test
//! control.
//!
//! AGENTS.md section 16 permits faking the outside world and forbids mocking an internal layer.
//! Both fixtures here are the outside world: `mountinfo(5)` text as a kernel actually prints it,
//! and a [`TestProvider`] standing in for ZFS, Btrfs or a file archive so the coverage algorithm
//! can be exercised without a pool, a subvolume or root.

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a shared test fixture is used by some suites and not by others (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ChangePlan, ConsistencyClass, EffectKind, PlanAction, ProtectionAction, ProtectionMode,
    ProviderAvailability, ProviderCapabilities, RecoveryAsset, RecoveryAssetType,
    RecoveryCandidate, RecoveryCapability, RecoveryCost, RecoveryGoal, RecoveryObjective,
    RecoveryPlanFragment, RecoveryProvider, RecoveryScope, RecoveryValidation, RestoreMethod,
};
use ono_change_core::{EffectDomain, PersistenceDomain};
use ono_change_protection::coverage::MutationDomain;
use ono_value::{ByteSize, ErrorValue};

/// The instant every fixture is dated at, so nothing depends on a wall clock.
pub const NOW: Timestamp = Timestamp::UNIX_EPOCH;

/// A ZFS root pool with separate `/var` and `/home` datasets, a bind mount of `/etc`, an NFS
/// export beneath `/var`, a runtime tmpfs and a read-only `/boot`.
///
/// Recorded from a Debian installation on `rpool`, with the host names replaced.
pub const ZFS_ROOT: &str = "\
21 27 0:20 / /proc rw,nosuid,nodev,noexec,relatime shared:5 - proc proc rw
22 27 0:21 / /sys rw,nosuid,nodev,noexec,relatime shared:6 - sysfs sysfs rw
23 27 0:5 / /dev rw,nosuid,relatime shared:2 - devtmpfs udev rw,size=8127816k,nr_inodes=2031954,mode=755
24 22 0:22 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime shared:7 - cgroup2 cgroup2 rw,nsdelegate,memory_recursiveprot
25 22 0:28 / /sys/kernel/debug rw,nosuid,nodev,noexec,relatime shared:8 - debugfs debugfs rw
26 22 0:29 / /sys/kernel/tracing rw,nosuid,nodev,noexec,relatime shared:9 - tracefs tracefs rw
27 1 0:23 / / rw,relatime shared:1 - zfs rpool/ROOT/debian rw,xattr,posixacl
28 27 0:30 / /run rw,nosuid,nodev shared:10 - tmpfs tmpfs rw,size=1626040k,mode=755
29 27 0:26 / /var rw,relatime shared:22 - zfs rpool/var rw,xattr,posixacl
30 27 0:27 / /home rw,relatime shared:23 - zfs rpool/home rw,xattr,posixacl
31 27 0:23 /etc /mnt/etc rw,relatime shared:24 - zfs rpool/ROOT/debian rw,xattr,posixacl
32 29 0:41 / /var/lib/nfs-data rw,relatime - nfs4 nas01:/export/data rw,vers=4.2,rsize=1048576,addr=10.0.0.4
33 27 8:1 / /boot ro,relatime - ext4 /dev/sda1 ro,errors=remount-ro
";

/// A Btrfs root with the `@`, `@home` and `@var` subvolume layout, a nested `@var/lib/machines`
/// subvolume, and a bind mount of a directory inside `@home`.
///
/// Recorded from an Arch installation on `/dev/nvme0n1p2`.
pub const BTRFS_ROOT: &str = "\
23 1 0:24 /@ / rw,relatime shared:1 - btrfs /dev/nvme0n1p2 rw,ssd,space_cache=v2,subvolid=256,subvol=/@
24 23 0:19 / /proc rw,nosuid,nodev,noexec,relatime shared:5 - proc proc rw
25 23 0:20 / /run rw,nosuid,nodev shared:6 - tmpfs tmpfs rw,size=3252080k,mode=755
26 23 0:24 /@home /home rw,relatime shared:29 - btrfs /dev/nvme0n1p2 rw,ssd,space_cache=v2,subvolid=257,subvol=/@home
27 23 0:24 /@var /var rw,relatime shared:30 - btrfs /dev/nvme0n1p2 rw,ssd,space_cache=v2,subvolid=258,subvol=/@var
28 27 0:24 /@var/lib/machines /var/lib/machines rw,relatime shared:31 - btrfs /dev/nvme0n1p2 rw,ssd,space_cache=v2,subvolid=942,subvol=/@var/lib/machines
29 23 0:24 /@home/erin/work /srv/work rw,relatime shared:32 - btrfs /dev/nvme0n1p2 rw,ssd,space_cache=v2,subvolid=257,subvol=/@home
30 23 0:24 /@snapshots /.snapshots ro,relatime shared:33 - btrfs /dev/nvme0n1p2 ro,ssd,space_cache=v2,subvolid=259,subvol=/@snapshots
";

/// An ext4 root with an NFS export, a runtime tmpfs, a container overlay whose upper layer is on
/// the root filesystem, and a second overlay whose upper layer is on a tmpfs.
///
/// Recorded from a Docker host, with the layer hashes shortened.
pub const EXT4_ROOT: &str = "\
24 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw,errors=remount-ro
25 24 0:19 / /proc rw,nosuid,nodev,noexec,relatime shared:5 - proc proc rw
26 24 0:20 / /run rw,nosuid,nodev shared:6 - tmpfs tmpfs rw,size=1626040k,mode=755
27 24 8:1 / /boot ro,relatime - ext4 /dev/sda1 ro,errors=remount-ro
28 24 0:41 / /data rw,relatime - nfs4 nas01:/export/data rw,vers=4.2,rsize=1048576,addr=10.0.0.4
29 24 8:2 /srv/exports/config /etc/app-config rw,relatime - ext4 /dev/sda2 rw,errors=remount-ro
30 24 0:52 / /var/lib/docker/overlay2/9f3a/merged rw,relatime - overlay overlay rw,lowerdir=/var/lib/docker/overlay2/l/ABCDEF:/var/lib/docker/overlay2/l/GHIJKL,upperdir=/var/lib/docker/overlay2/9f3a/diff,workdir=/var/lib/docker/overlay2/9f3a/work
31 26 0:60 / /run/containers/storage/overlay/1b2c/merged rw,relatime - overlay overlay rw,lowerdir=/usr/lib/containers/l/MNOPQR,upperdir=/run/containers/storage/overlay/1b2c/diff,workdir=/run/containers/storage/overlay/1b2c/work
";

/// A recovery provider whose answers the test states (§12.1).
///
/// It fakes the mechanism, not the algorithm: what it declares, what it offers and whether it is
/// available are exactly what a ZFS or Btrfs provider would report, and everything the suites
/// assert about coverage is computed from those answers by the code under test.
#[derive(Debug, Clone)]
pub struct TestProvider {
    id: Arc<str>,
    capabilities: ProviderCapabilities,
    availability: ProviderAvailability,
    candidates: Vec<RecoveryCandidate>,
    discovery_error: Option<ErrorValue>,
    asset_type: RecoveryAssetType,
}

impl TestProvider {
    /// A fully capable, available provider that offers nothing until it is told to.
    #[must_use]
    pub fn new(id: &str) -> Self {
        let mut capabilities = ProviderCapabilities::new(id);
        for capability in RecoveryCapability::REQUIRED {
            capabilities = capabilities.recovering(*capability);
        }
        Self {
            id: Arc::from(id),
            capabilities,
            availability: ProviderAvailability::Available {
                version: Arc::from("2.2.2"),
            },
            candidates: Vec::new(),
            discovery_error: None,
            asset_type: RecoveryAssetType::ZfsSnapshot,
        }
    }

    /// A provider that declares everything except `capability` (§12.2).
    #[must_use]
    pub fn without(mut self, capability: RecoveryCapability) -> Self {
        let mut capabilities = ProviderCapabilities::new(&*self.id);
        for declared in RecoveryCapability::REQUIRED {
            if *declared != capability {
                capabilities = capabilities.recovering(*declared);
            }
        }
        self.capabilities = capabilities;
        self
    }

    /// A provider that cannot run here (§12.2, Appendix G.4).
    #[must_use]
    pub fn unavailable(mut self, reason: &str) -> Self {
        self.availability = ProviderAvailability::Unavailable {
            reason: Arc::from(reason),
        };
        self
    }

    /// What kind of asset its protection actions propose.
    #[must_use]
    pub const fn producing(mut self, asset_type: RecoveryAssetType) -> Self {
        self.asset_type = asset_type;
        self
    }

    /// Adds a candidate the provider offers for the persistence object in its scope.
    #[must_use]
    pub fn offering(mut self, candidate: RecoveryCandidate) -> Self {
        self.candidates.push(candidate);
        self
    }

    /// A provider whose discovery itself fails (Appendix A.3).
    #[must_use]
    pub fn failing_discovery(mut self, error: ErrorValue) -> Self {
        self.discovery_error = Some(error);
        self
    }

    /// The provider behind an `Arc`, as the registry holds it.
    #[must_use]
    pub fn shared(self) -> Arc<dyn RecoveryProvider> {
        Arc::new(self)
    }
}

impl RecoveryProvider for TestProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    fn availability(&self) -> ProviderAvailability {
        self.availability.clone()
    }

    fn resolve_domain(&self, _path: &str) -> Result<Option<PersistenceDomain>, ErrorValue> {
        Ok(None)
    }

    fn discover(
        &self,
        domain: &PersistenceDomain,
        _objective: RecoveryObjective,
    ) -> Result<Vec<RecoveryCandidate>, ErrorValue> {
        if let Some(error) = &self.discovery_error {
            return Err(error.clone());
        }
        Ok(self
            .candidates
            .iter()
            .filter(|candidate| {
                domain.object() == Some(candidate.scope().domain())
                    || candidate.scope().covers_object(domain.path())
            })
            .cloned()
            .collect())
    }

    fn plan_protection(
        &self,
        candidates: &[RecoveryCandidate],
        _mode: ProtectionMode,
    ) -> Result<Vec<ProtectionAction>, ErrorValue> {
        Ok(candidates
            .iter()
            .map(|candidate| {
                let asset = RecoveryAsset::proposed(
                    &*self.id,
                    self.asset_type,
                    format!("{}@ono-test", candidate.scope().domain()),
                    candidate.scope().clone(),
                    NOW,
                )
                .at_consistency(candidate.consistency())
                .restored_by(candidate.restore_method())
                .costing(candidate.cost().clone());
                ProtectionAction::new(
                    &*self.id,
                    format!("protect {}", candidate.scope().domain()),
                    candidate.clone(),
                    asset,
                )
            })
            .collect())
    }

    fn create(&self, action: &ProtectionAction) -> Result<RecoveryAsset, ErrorValue> {
        Ok(action
            .proposed_asset()
            .clone()
            .creating()
            .validated(RecoveryValidation::complete(NOW, "the fixture created it")))
    }

    fn validate(&self, _asset: &RecoveryAsset) -> Result<RecoveryValidation, ErrorValue> {
        Ok(RecoveryValidation::complete(NOW, "the fixture checked it"))
    }

    fn plan_recovery(
        &self,
        asset: &RecoveryAsset,
        _source: Option<&ChangePlan>,
        _goal: RecoveryGoal,
    ) -> Result<RecoveryPlanFragment, ErrorValue> {
        Ok(RecoveryPlanFragment::new(&*self.id, asset.restore_method()))
    }

    fn restore(&self, _action: &PlanAction, _asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        Ok(())
    }

    fn cleanup(&self, _asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        Ok(())
    }

    fn estimate_cost(&self, asset: &RecoveryAsset) -> Result<RecoveryCost, ErrorValue> {
        Ok(asset.cost().clone())
    }
}

/// A candidate over `object` covering exactly `covers`, as a provider would offer it.
#[must_use]
pub fn candidate(
    provider: &str,
    domain_kind: &str,
    object: &str,
    covers: &[&str],
    domain: EffectDomain,
    objective: RecoveryObjective,
) -> RecoveryCandidate {
    let mut scope = RecoveryScope::new(domain_kind, object, "localhost");
    for covered in covers {
        scope = scope.covering(*covered);
    }
    RecoveryCandidate::new(
        provider,
        scope,
        domain,
        objective,
        format!("{provider} would protect {object}"),
    )
}

/// The cost of a copy-on-write snapshot: instant, small, and never free (§38.2).
#[must_use]
pub fn snapshot_cost() -> RecoveryCost {
    RecoveryCost::unknown()
        .with_space(
            Some(ByteSize::from_bytes(4 * 1024)),
            Some(ByteSize::from_bytes(4 * 1024)),
            true,
        )
        .with_latency(std::time::Duration::from_millis(40))
}

/// The cost of copying one configuration file into the recovery store (§15).
#[must_use]
pub fn archive_cost(bytes: u128) -> RecoveryCost {
    RecoveryCost::unknown()
        .with_space(
            Some(ByteSize::from_bytes(bytes)),
            Some(ByteSize::from_bytes(bytes)),
            false,
        )
        .with_latency(std::time::Duration::from_millis(120))
}

/// A file-archive candidate over one configuration file (§15, Appendix A.4).
#[must_use]
pub fn file_archive(provider: &str, path: &str) -> RecoveryCandidate {
    candidate(
        provider,
        "file",
        path,
        &[path],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::ByteConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .costing(archive_cost(2 * 1024))
}

impl TestProvider {
    /// The capabilities the provider declares, for a suite that inspects them directly.
    #[must_use]
    pub fn capabilities_for_test(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }
}

/// A validated, ready ZFS snapshot asset over the root dataset (§11.4).
#[must_use]
pub fn ready_asset(reference: &str) -> RecoveryAsset {
    RecoveryAsset::proposed(
        "ono.recovery.zfs",
        RecoveryAssetType::ZfsSnapshot,
        reference,
        RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost")
            .covering("/etc/nginx/nginx.conf"),
        NOW,
    )
    .costing(snapshot_cost())
    .creating()
    .validated(RecoveryValidation::complete(NOW, "the fixture checked it"))
}

pub fn config_mutation() -> MutationDomain {
    MutationDomain::new(
        EffectDomain::FilesystemPersistent,
        EffectKind::Replace,
        "/etc/nginx/nginx.conf",
        "the configuration file is replaced",
    )
}
