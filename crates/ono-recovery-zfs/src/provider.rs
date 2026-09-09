//! The ZFS recovery provider (spec v0.6 §13, Appendix D.1-D.5, §56.1).
//!
//! §13 opens by saying why ZFS is a first-party reference provider: its snapshot semantics map
//! naturally onto protected change. The rest of §13 is about the places where that map is
//! misleading, and this module is mostly those places.
//!
//! - §13.4: the dataset is the snapshot boundary. A snapshot of `tank/data` does not protect
//!   `tank/data/customer`, and [`Layout::dataset_of_path`] resolves through mount metadata so a
//!   directory *named* like a dataset never becomes one (Appendix B.8).
//! - §13.3: one `zfs snapshot -r` may create several snapshots, and Appendix D.1 still wants one
//!   concrete reference per dataset. [`ZfsProvider::plan_protection`] emits one protection action
//!   per dataset, so a recursive creation produces a list of assets rather than a single entry
//!   that quietly stands for several.
//! - §13.6: rollback can require destroying newer snapshots, bookmarks and clones, and Ono MUST
//!   NEVER silently add the flag that does it. No argument vector this provider builds contains
//!   `-R`, and the only `-r` it ever emits is §13.3's recursive *creation*.
//! - §13.7: rollback may need an unmount, a reboot or a boot-environment switch, and the plan
//!   says so before apply rather than promising online rollback because a snapshot exists.
//! - §56.1 and §56.3: twelve facts are proven before a destructive path is enabled, and a fact
//!   that could not be established blocks rather than being guessed.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::error::{
    asset_create_failed, destructive_history_not_accepted, privilege_required,
    provider_unavailable, recovery_apply_failed, recovery_plan_incomplete, requires_offline,
    requires_reboot, storage_pressure, target_unresolved, tool_failed,
};
use ono_change_core::{
    ActionRole, ChangePlan, ConsistencyClass, EffectDomain, Execution, Idempotency,
    MetadataCoverage, NewerStateClass, NewerStateImpact, NewerStateItem, PersistenceDomain,
    PlanAction, PlanId, Precondition, PreconditionKind, ProtectionAction, ProtectionMode,
    ProviderAvailability, ProviderCapabilities, RecoveryAsset, RecoveryAssetType,
    RecoveryCandidate, RecoveryCapability, RecoveryCost, RecoveryExclusion, RecoveryGoal,
    RecoveryObjective, RecoveryPlanFragment, RecoveryProvider, RecoveryScope, RecoveryValidation,
    RestoreMethod, ToolOutput, ToolRunner, UnrecoverableEffect, VerificationClass,
    VerificationContract, choose_method,
};
use ono_value::{ByteSize, ErrorValue, Value};

use crate::checklist::{SafetyChecklist, ZfsFact};
use crate::layout::{
    Dataset, Layout, MountState, MountTable, Snapshot, is_beneath, is_descendant, mount_state,
    with_status,
};
use crate::naming::{full_name, snapshot_part};
use crate::parse;

/// The provider id §12.1's example gives this provider.
pub const PROVIDER_ID: &str = "ono.recovery.zfs";

/// The `zfs` binary, as an absolute path because §12.3 resolves the program rather than a name.
pub const ZFS: &str = "/usr/sbin/zfs";

/// The `zpool` binary.
pub const ZPOOL: &str = "/usr/sbin/zpool";

/// The program a file restore copies through.
///
/// §12.3 asks for a direct process API and an argument vector, which `execve` of `cp` is; the
/// alternative — building a copy in this crate — would have to re-implement the mode, owner,
/// ACL and extended-attribute preservation that `--preserve=all` already performs, and
/// Appendix C.7 makes losing any of those a restore that did not return the file.
pub const CP: &str = "/bin/cp";

/// The OpenZFS releases this provider has been validated against (Appendix G.4).
///
/// The fixtures under `tests/fixtures/` are the verbatim output of the tools at this version,
/// run against a real pool, and the deterministic suite replays them. A version outside this list
/// makes [`ZfsProvider::availability`] answer [`ProviderAvailability::Unsupported`], because
/// Appendix G.4 requires a provider to degrade rather than execute semantics it has not tested.
pub const VALIDATED_VERSIONS: &[&str] = &["2.4.1"];

/// The fingerprint prefix under which a snapshot's GUID is recorded on its asset (§18.3, §56.1).
///
/// §11.4 asks validation to check that the asset's identity matches the planned source, and for
/// ZFS the identity is the GUID rather than the name: a snapshot destroyed and recreated under
/// the same name is a different snapshot, and only the GUID says so.
pub const GUID_FINGERPRINT: &str = "zfs-guid:";

/// The `mountinfo(5)` table Appendix B.1 resolves paths against.
pub const MOUNTINFO: &str = "/proc/self/mountinfo";

/// Where the mount table a resolution uses comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mounts {
    /// Read from the kernel when a resolution needs it.
    Proc,
    /// Supplied by the caller — the recorded fixture, or a harness's own reading.
    Recorded(MountTable),
}

/// Whether a dataset carries a running or bootable system (§13.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootDatasetCase {
    /// The dataset is mounted at `/`. Rolling it back cannot happen under the running system.
    RunningRoot,
    /// The dataset is a boot environment beneath the pool's `ROOT` container.
    BootEnvironment,
}

impl RootDatasetCase {
    /// The sentence the plan shows for this case.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            RootDatasetCase::RunningRoot => {
                "the dataset is mounted at `/`, so rolling it back is offline recovery rather \
                 than an in-place operation"
            }
            RootDatasetCase::BootEnvironment => {
                "the dataset is a boot environment beneath the pool's ROOT container, so recovery \
                 is a boot-environment switch and takes effect on the next boot"
            }
        }
    }
}

/// The ZFS recovery provider of §13.
#[derive(Debug, Clone)]
pub struct ZfsProvider {
    runner: Arc<dyn ToolRunner>,
    mounts: Mounts,
    host: Arc<str>,
    plan: Option<PlanId>,
    now: Option<Timestamp>,
    free_space_floor: ByteSize,
    accepted_history_destruction: bool,
}

impl ZfsProvider {
    /// A provider that runs its commands through `runner` (§12.3).
    #[must_use]
    pub fn new(runner: Arc<dyn ToolRunner>) -> Self {
        Self {
            runner,
            mounts: Mounts::Proc,
            host: Arc::from("localhost"),
            plan: None,
            now: None,
            free_space_floor: ByteSize::ZERO,
            accepted_history_destruction: false,
        }
    }

    /// Resolves paths against `table` rather than against `/proc/self/mountinfo`.
    ///
    /// Appendix G.2 asks for deliberately misleading layouts, and a layout is only statable if
    /// the mount table is a value. It is also what the gated real-filesystem harness uses to name
    /// the reading it took at the moment it took it.
    #[must_use]
    pub fn reading_mounts(mut self, table: MountTable) -> Self {
        self.mounts = Mounts::Recorded(table);
        self
    }

    /// Names the host the scopes this provider produces belong to (§11.2).
    #[must_use]
    pub fn on_host(mut self, host: impl Into<Arc<str>>) -> Self {
        self.host = host.into();
        self
    }

    /// Attributes the assets this provider creates to `plan` (§11.1, Appendix D.2).
    #[must_use]
    pub fn for_plan(mut self, plan: PlanId) -> Self {
        self.plan = Some(plan);
        self
    }

    /// Fixes the instant assets and validations are stamped with.
    ///
    /// A provider left to read the clock produces a different snapshot name every run, which is
    /// correct in production and useless in a test. Taking the instant as a parameter is what
    /// keeps the deterministic suite deterministic.
    #[must_use]
    pub const fn at_instant(mut self, now: Timestamp) -> Self {
        self.now = Some(now);
        self
    }

    /// Sets Appendix D.3's free-space floor, below which automatic protection fails closed.
    ///
    /// There is no default floor. Appendix D.3 speaks of a *configured* one, and a provider that
    /// invented a number would refuse to protect a small pool whose operator is content with it.
    #[must_use]
    pub const fn with_free_space_floor(mut self, floor: ByteSize) -> Self {
        self.free_space_floor = floor;
        self
    }

    /// Records that §24.5's gate was passed and history destruction was explicitly accepted.
    ///
    /// §13.6 requires the plan to enumerate every newer snapshot, bookmark and clone a rollback
    /// would destroy and to require explicit acceptance. The acceptance itself belongs to the
    /// operator and reaches the provider only here: an executor constructs the provider with this
    /// after the gate, and [`ZfsProvider::restore`] refuses a destructive rollback without it.
    #[must_use]
    pub const fn with_accepted_history_destruction(mut self) -> Self {
        self.accepted_history_destruction = true;
        self
    }

    /// The instant this provider stamps its work with.
    fn now(&self) -> Timestamp {
        self.now.unwrap_or_else(Timestamp::now)
    }

    /// The mount table this resolution uses (Appendix B.1).
    fn mount_table(&self) -> Result<MountTable, ErrorValue> {
        match &self.mounts {
            Mounts::Recorded(table) => Ok(table.clone()),
            Mounts::Proc => std::fs::read_to_string(MOUNTINFO)
                .map(|text| MountTable::from_text(&text))
                .map_err(|error| {
                    tool_failed(
                        MOUNTINFO,
                        &format!(
                            "v0.6 Appendix B.1 and B.8: the ZFS provider takes dataset identity \
                             from mount metadata, and without the kernel mount table there is \
                             none to take. Nothing is assumed in its place: {error}"
                        ),
                    )
                }),
        }
    }

    /// Refuses when the tool is not on this host at all (§54.4).
    fn require_tools(&self) -> Result<(), ErrorValue> {
        if self.runner.is_available(ZFS) {
            Ok(())
        } else {
            Err(provider_unavailable(
                PROVIDER_ID,
                &format!("`{ZFS}` is not present on this host"),
            ))
        }
    }

    /// Runs one ZFS command, returning what it said whatever its status (§12.3).
    fn zfs(&self, argv: &[&str]) -> Result<ToolOutput, ErrorValue> {
        self.runner.run(ZFS, argv)
    }

    /// Runs one `zpool` command.
    fn zpool(&self, argv: &[&str]) -> Result<ToolOutput, ErrorValue> {
        self.runner.run(ZPOOL, argv)
    }

    /// Reads everything §13.1 asks discovery to resolve, at one instant.
    ///
    /// The six questions are asked in a fixed order and always all of them, so every answer a
    /// plan is built from describes the same reading of the pool. A query that failed leaves its
    /// part of the layout empty rather than aborting: §56.1's checklist is what turns an empty
    /// part into a refusal, and it can only do that if it gets to see which part is empty.
    ///
    /// # Errors
    ///
    /// A structured error when a program could not be run at all, or when its output was not in
    /// the machine form this provider validated against (Appendix G.4).
    pub fn survey(&self) -> Result<Layout, ErrorValue> {
        Ok(self.survey_with_status()?.0)
    }

    /// The survey, and which of its six questions the tools actually answered.
    ///
    /// §56.1 asks the implementation to *prove* a fact, and a query that failed proves nothing —
    /// so which query failed has to survive into the checklist rather than being flattened into
    /// an empty list that looks like "there are none".
    fn survey_with_status(&self) -> Result<(Layout, SurveyStatus), ErrorValue> {
        self.require_tools()?;
        let filesystems = self.zfs(&[
            "list",
            "-H",
            "-p",
            "-t",
            "filesystem",
            "-o",
            "name,mountpoint,mounted,type,used,available,referenced,origin,canmount",
        ])?;
        let snapshots = self.zfs(&[
            "list",
            "-H",
            "-p",
            "-t",
            "snapshot",
            "-o",
            "name,creation,used,referenced,guid,defer_destroy",
        ])?;
        let order = self.zfs(&[
            "list",
            "-H",
            "-p",
            "-t",
            "snapshot",
            "-o",
            "name,creation",
            "-s",
            "creation",
        ])?;
        let bookmarks = self.zfs(&[
            "list",
            "-H",
            "-p",
            "-t",
            "bookmark",
            "-o",
            "name,creation,guid",
        ])?;
        let origins = self.zfs(&["list", "-H", "-p", "-t", "filesystem", "-o", "name,origin"])?;
        let listed = self.zpool(&[
            "list",
            "-H",
            "-p",
            "-o",
            "name,size,alloc,free,capacity,fragmentation,health",
        ])?;
        let pool_status = self.zpool(&["status"])?;

        let status = SurveyStatus {
            filesystems: filesystems.succeeded(),
            snapshots: snapshots.succeeded(),
            order: order.succeeded(),
            bookmarks: bookmarks.succeeded(),
            origins: origins.succeeded(),
        };
        let mut datasets = if filesystems.succeeded() {
            crate::layout::datasets(ZFS, filesystems.stdout())?
        } else {
            Vec::new()
        };
        if origins.succeeded() {
            for (name, origin) in crate::layout::origins(ZFS, origins.stdout())? {
                match datasets.iter_mut().find(|dataset| dataset.name == name) {
                    Some(dataset) => dataset.origin = origin,
                    // A dataset the origin listing knows and the filesystem listing did not is
                    // still a dataset, and where it is a clone §13.6 needs it visible. Its mount
                    // metadata stays unknown rather than being invented (§56.3).
                    None => datasets.push(Dataset {
                        name,
                        mountpoint: Arc::from("-"),
                        mounted: false,
                        used: None,
                        available: None,
                        referenced: None,
                        origin,
                        canmount: Arc::from("-"),
                    }),
                }
            }
        }
        let snapshots = if snapshots.succeeded() {
            crate::layout::snapshots(ZFS, snapshots.stdout())?
        } else {
            Vec::new()
        };
        let order = if order.succeeded() {
            crate::layout::creation_order(ZFS, order.stdout())?
        } else {
            Vec::new()
        };
        let bookmarks = if bookmarks.succeeded() {
            crate::layout::bookmarks(ZFS, bookmarks.stdout())?
        } else {
            Vec::new()
        };
        let pools = if listed.succeeded() {
            with_status(
                crate::layout::pools(ZPOOL, listed.stdout())?,
                pool_status.stdout(),
            )
        } else {
            Vec::new()
        };
        Ok((
            Layout::new(
                datasets,
                snapshots,
                order,
                bookmarks,
                pools,
                self.mount_table()?,
            ),
            status,
        ))
    }

    /// Whether any of these readings was refused for want of privilege (§11.4, §43.4).
    fn refused_for_privilege(outputs: &[&ToolOutput]) -> bool {
        outputs.iter().any(|output| {
            !output.succeeded()
                && (parse::is_permission_refusal(output.stderr())
                    || parse::is_permission_refusal(output.stdout()))
        })
    }

    /// The free-space guard of Appendix D.3.
    fn space_guard(&self, layout: &Layout, dataset: &str) -> Result<(), ErrorValue> {
        if self.free_space_floor == ByteSize::ZERO {
            return Ok(());
        }
        let pool_name = dataset.split('/').next().unwrap_or(dataset);
        let Some(pool) = layout.pool(pool_name) else {
            return Ok(());
        };
        let Some(free) = pool.free else {
            return Ok(());
        };
        if ByteSize::from_bytes(free) < self.free_space_floor {
            return Err(storage_pressure(
                pool_name,
                &ByteSize::from_bytes(free).to_string(),
                &self.free_space_floor.to_string(),
            ));
        }
        Ok(())
    }

    /// Reads only the pools, for the guard that runs before a protection action is planned.
    fn pool_reading(&self) -> Result<Layout, ErrorValue> {
        self.require_tools()?;
        let listed = self.zpool(&[
            "list",
            "-H",
            "-p",
            "-o",
            "name,size,alloc,free,capacity,fragmentation,health",
        ])?;
        let pools = if listed.succeeded() {
            crate::layout::pools(ZPOOL, listed.stdout())?
        } else {
            Vec::new()
        };
        Ok(Layout::new(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            pools,
            MountTable::default(),
        ))
    }

    /// Which dataset case §13.7 puts this dataset in, where it is one of them.
    #[must_use]
    fn root_case(layout: &Layout, dataset: &str, mount: &MountState) -> Option<RootDatasetCase> {
        let mounted_at_root = mount
            .mountpoint
            .as_deref()
            .is_some_and(|mountpoint| mountpoint == "/")
            || layout
                .mounts()
                .of_dataset(dataset)
                .is_some_and(|entry| entry.mount_point.as_ref() == "/");
        if mounted_at_root {
            return Some(RootDatasetCase::RunningRoot);
        }
        let pool = dataset.split('/').next().unwrap_or(dataset);
        let container = format!("{pool}/ROOT");
        (dataset != container
            && is_descendant(dataset, &container)
            && layout.dataset(&container).is_some())
        .then_some(RootDatasetCase::BootEnvironment)
    }

    /// What this provider restores of a file's metadata (Appendix C.7).
    ///
    /// The claim is what `cp --preserve=all` actually does and no more. File capabilities and
    /// SELinux labels ride on extended attributes and usually survive, and "usually" is not a
    /// claim Appendix C.7 permits, so they are named as gaps and the plan shows them.
    #[must_use]
    pub const fn metadata_coverage(method: RestoreMethod) -> MetadataCoverage {
        match method {
            RestoreMethod::SelectiveFileRestore | RestoreMethod::CloneAndCopy => MetadataCoverage {
                content: true,
                mode: true,
                owner: true,
                acl: true,
                xattrs: true,
                capabilities: false,
                selinux: false,
                hardlinks: false,
            },
            // A rollback returns the dataset to the exact recorded state, so every piece of
            // metadata comes back with it — including the hard-link relationships a file copy
            // cannot reconstruct.
            RestoreMethod::DatasetRollback | RestoreMethod::OfflineRootRecovery => {
                MetadataCoverage {
                    content: true,
                    mode: true,
                    owner: true,
                    acl: true,
                    xattrs: true,
                    capabilities: true,
                    selinux: true,
                    hardlinks: true,
                }
            }
            _ => MetadataCoverage::none(),
        }
    }

    /// The path a file inside `snapshot` is reached through (§13.5).
    ///
    /// `<mountpoint>/.zfs/snapshot/<name>/<path relative to the dataset>`. The `.zfs` directory
    /// is reachable by explicit path whether `snapdir` is `visible` or `hidden`; the property
    /// decides only whether it appears in a directory listing, and the plan states which of the
    /// two it found rather than leaving the operator to guess why `ls` shows nothing.
    #[must_use]
    pub fn snapshot_path(mountpoint: &str, snapshot_short: &str, path: &str) -> Option<String> {
        if !mountpoint.starts_with('/') || !is_beneath(path, mountpoint) {
            return None;
        }
        let relative = path
            .get(mountpoint.len()..)
            .unwrap_or("")
            .trim_start_matches('/');
        let base = mountpoint.trim_end_matches('/');
        Some(if relative.is_empty() {
            format!("{base}/.zfs/snapshot/{snapshot_short}")
        } else {
            format!("{base}/.zfs/snapshot/{snapshot_short}/{relative}")
        })
    }

    /// §13.4's worked example for `path`, built from a reading of the pool.
    ///
    /// The `NOT PROTECTED BY` list is what makes the block worth printing: it names the snapshots
    /// that exist under the same name on other datasets, which are exactly the ones an operator
    /// would otherwise assume covered the target.
    ///
    /// # Errors
    ///
    /// A structured error when the pool could not be read (§13.1).
    pub fn boundary_report(
        &self,
        path: &str,
        snapshot_part: &str,
    ) -> Result<Option<crate::render::BoundaryReport>, ErrorValue> {
        Ok(crate::render::BoundaryReport::of(
            &self.survey()?,
            path,
            snapshot_part,
        ))
    }

    /// Proves §56.1's twelve facts about `asset`, or records which of them it could not.
    ///
    /// # Errors
    ///
    /// A structured error when a program could not be run at all. A program that ran and refused
    /// is a fact this provider failed to establish, which is a checklist entry rather than an
    /// error: §56.3's block is raised by the caller that wanted a destructive path.
    pub fn safety_checklist(&self, asset: &RecoveryAsset) -> Result<SafetyChecklist, ErrorValue> {
        Ok(self.examine(asset)?.checklist)
    }

    /// Reads everything a recovery plan over `asset` needs, and scores §56.1's checklist.
    fn examine(&self, asset: &RecoveryAsset) -> Result<Examination, ErrorValue> {
        self.require_tools()?;
        let reference = asset.reference().to_owned();
        let dataset_name = reference.split_once('@').map_or_else(
            || asset.scope().domain().to_owned(),
            |(head, _)| head.to_owned(),
        );

        let (layout, status) = self.survey_with_status()?;
        let clones = self.zfs(&[
            "get",
            "-H",
            "-p",
            "-o",
            "name,property,value",
            "clones",
            &reference,
        ])?;
        let written = self.zfs(&[
            "get",
            "-H",
            "-p",
            "-o",
            "name,property,value",
            "written",
            &dataset_name,
        ])?;
        let space = self.zfs(&[
            "get",
            "-H",
            "-p",
            "-o",
            "name,property,value",
            "usedbysnapshots,usedbydataset",
            &dataset_name,
        ])?;
        let placement = self.zfs(&[
            "get",
            "-H",
            "-p",
            "-o",
            "name,property,value",
            "mounted,mountpoint,canmount,readonly,origin,snapdir",
            &dataset_name,
        ])?;

        let privileged = !Self::refused_for_privilege(&[&clones, &written, &space, &placement]);
        let dataset = layout.dataset(&dataset_name).cloned();
        let snapshot = layout.snapshot(&reference).cloned();
        let short = snapshot
            .as_ref()
            .map_or_else(|| Arc::from(""), |found| Arc::clone(&found.short));

        let clone_names: Option<Vec<Arc<str>>> = clones.succeeded().then_some(()).and_then(|()| {
            let listed = parse::properties(ZFS, clones.stdout()).ok()?;
            let entry = parse::property_of(&listed, &reference, "clones")?;
            Some(if entry.is_absent() {
                Vec::new()
            } else {
                entry
                    .value
                    .split(',')
                    .filter(|name| !name.is_empty())
                    .map(Arc::from)
                    .collect()
            })
        });
        let written_bytes = written.succeeded().then_some(()).and_then(|()| {
            let listed = parse::properties(ZFS, written.stdout()).ok()?;
            parse::number(&parse::property_of(&listed, &dataset_name, "written")?.value)
        });
        let used_by_snapshots = space.succeeded().then_some(()).and_then(|()| {
            let listed = parse::properties(ZFS, space.stdout()).ok()?;
            parse::number(&parse::property_of(&listed, &dataset_name, "usedbysnapshots")?.value)
        });
        let placement_rows = if placement.succeeded() {
            parse::properties(ZFS, placement.stdout()).unwrap_or_default()
        } else {
            Vec::new()
        };
        let mount = mount_state(&placement_rows, &dataset_name);
        let root_case = Self::root_case(&layout, &dataset_name, &mount);

        let newer_snapshots: Vec<Arc<str>> = layout
            .newer_snapshots(&reference)
            .into_iter()
            .map(|snapshot| Arc::clone(&snapshot.name))
            .collect();
        let affected_bookmarks = affected_bookmarks(&layout, &dataset_name, &reference);
        let children: Vec<Arc<str>> = layout
            .descendants_of(&dataset_name)
            .into_iter()
            .map(|child| Arc::clone(&child.name))
            .collect();

        let recorded_guid = asset
            .captured_state()
            .and_then(|fingerprint| fingerprint.strip_prefix(GUID_FINGERPRINT))
            .map(str::to_owned);
        let identity_agrees = match (&snapshot, &recorded_guid) {
            (Some(found), Some(recorded)) => found.guid.as_ref() == recorded,
            (Some(_), None) => true,
            _ => false,
        };

        let mut checklist = SafetyChecklist::unproven(reference.clone())
            .establishing(
                ZfsFact::DatasetIdentity,
                dataset
                    .as_ref()
                    .filter(|_| status.filesystems)
                    .map(|found| format!("ZFS reports the dataset `{}`", found.name)),
                "ZFS did not report a dataset of this name",
            )
            .establishing(
                ZfsFact::SnapshotExists,
                snapshot
                    .as_ref()
                    .filter(|_| status.snapshots)
                    .map(|found| format!("`{}` is present in `zfs list -t snapshot`", found.name)),
                "the snapshot is not present in the snapshot listing",
            )
            .establishing(
                ZfsFact::LatestRelevantSnapshot,
                (status.order && layout.has_creation_order() && snapshot.is_some()).then(|| {
                    if layout.is_latest_snapshot(&reference) {
                        format!("`{reference}` is the newest snapshot its dataset holds")
                    } else {
                        format!(
                            "`{reference}` is followed by {} newer snapshot(s)",
                            newer_snapshots.len()
                        )
                    }
                }),
                "ZFS creation order could not be read, so `newer` could not be decided",
            )
            .establishing(
                ZfsFact::ChildDatasetBoundaries,
                status.filesystems.then(|| {
                    format!(
                        "`{dataset_name}` has {} descendant dataset(s), each its own snapshot \
                         boundary",
                        children.len()
                    )
                }),
                "the filesystem listing could not be read, so child boundaries are unknown",
            )
            .establishing(
                ZfsFact::MountRequirement,
                mount.is_established().then(|| {
                    format!(
                        "ZFS reports `{dataset_name}` {} at `{}`",
                        if mount.mounted == Some(true) {
                            "mounted"
                        } else {
                            "not mounted"
                        },
                        mount.mountpoint.as_deref().unwrap_or("-")
                    )
                }),
                "the `mounted` and `mountpoint` properties could not be read",
            )
            .establishing(
                ZfsFact::DiscardedLiveData,
                written_bytes.map(|bytes| {
                    format!("`zfs get written` reports {bytes} bytes written since the snapshot")
                }),
                "`zfs get written` did not report a value, so the discarded live data is unknown",
            );

        checklist = if identity_agrees {
            let detail = snapshot.as_ref().map_or_else(
                || "the snapshot's GUID was not read".to_owned(),
                |found| format!("the snapshot's GUID is {}", found.guid),
            );
            checklist.established(ZfsFact::SnapshotIdentity, detail)
        } else {
            checklist.missing(
                ZfsFact::SnapshotIdentity,
                match (&snapshot, &recorded_guid) {
                    (Some(found), Some(recorded)) => format!(
                        "the snapshot now carries GUID {} and the asset recorded {recorded}, so \
                         this is a different snapshot under the same name",
                        found.guid
                    ),
                    _ => "the snapshot listing gave no GUID for this name".to_owned(),
                },
            )
        };

        checklist = match clone_names.as_ref().filter(|_| status.origins) {
            Some(names) if names.is_empty() => checklist.established(
                ZfsFact::AffectedClones,
                format!("`zfs get clones {reference}` reports no clone"),
            ),
            Some(names) => checklist.established(
                ZfsFact::AffectedClones,
                format!("`{reference}` has the clone(s) {}", names.join(", ")),
            ),
            None => checklist.missing(
                ZfsFact::AffectedClones,
                "`zfs get clones` did not report the property for this snapshot",
            ),
        };

        checklist = if status.order
            && status.bookmarks
            && layout.has_creation_order()
            && snapshot.is_some()
        {
            checklist.established(
                ZfsFact::NewerSnapshotsAndBookmarks,
                format!(
                    "{} newer snapshot(s) and {} affected bookmark(s) were enumerated",
                    newer_snapshots.len(),
                    affected_bookmarks.len()
                ),
            )
        } else {
            checklist.missing(
                ZfsFact::NewerSnapshotsAndBookmarks,
                "the snapshot and bookmark listings did not together establish what rollback \
                 would destroy",
            )
        };

        checklist = if layout.mounts().is_empty() || !mount.is_established() {
            checklist.missing(
                ZfsFact::RebootOrOfflineRequirement,
                "without both the kernel mount table and the dataset's mount properties, whether \
                 recovery needs a reboot or the filesystem offline could not be established",
            )
        } else {
            checklist.established(
                ZfsFact::RebootOrOfflineRequirement,
                root_case.map_or_else(
                    || {
                        format!(
                            "`{dataset_name}` carries no running or bootable system, so recovery \
                             needs no reboot"
                        )
                    },
                    |case| case.detail().to_owned(),
                ),
            )
        };

        checklist = if privileged {
            checklist.established(
                ZfsFact::SufficientPrivilege,
                "every ZFS query this plan rests on ran without a permission refusal",
            )
        } else {
            checklist.missing(
                ZfsFact::SufficientPrivilege,
                "ZFS refused a query for want of privilege: the utilities must be run as root",
            )
        };

        let enumeration_complete = checklist.is_established(ZfsFact::LatestRelevantSnapshot)
            && checklist.is_established(ZfsFact::NewerSnapshotsAndBookmarks)
            && checklist.is_established(ZfsFact::AffectedClones);
        let destroyed = destroyed_objects(
            &newer_snapshots,
            &affected_bookmarks,
            clone_names.as_deref().unwrap_or(&[]),
        );
        checklist = if enumeration_complete {
            checklist.established(
                ZfsFact::HistoryDestructionAccepted,
                format!(
                    "{} object(s) would be destroyed, and the plan requires explicit acceptance \
                     of each",
                    destroyed.len()
                ),
            )
        } else {
            checklist.missing(
                ZfsFact::HistoryDestructionAccepted,
                "what acceptance would cover could not be enumerated, so it cannot be explicit",
            )
        };

        Ok(Examination {
            reference: Arc::from(reference.as_str()),
            dataset_name: Arc::from(dataset_name.as_str()),
            dataset,
            snapshot,
            short,
            newer_snapshots,
            affected_bookmarks,
            clones: clone_names.unwrap_or_default(),
            children,
            mount,
            root_case,
            written: written_bytes,
            used_by_snapshots,
            checklist,
        })
    }
}

/// Which of §13.1's six questions the tools answered (§56.1, §56.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurveyStatus {
    filesystems: bool,
    snapshots: bool,
    order: bool,
    bookmarks: bool,
    origins: bool,
}

/// Everything one recovery plan rests on, read at one instant.
#[derive(Debug, Clone)]
struct Examination {
    reference: Arc<str>,
    dataset_name: Arc<str>,
    dataset: Option<Dataset>,
    snapshot: Option<Snapshot>,
    short: Arc<str>,
    newer_snapshots: Vec<Arc<str>>,
    affected_bookmarks: Vec<Arc<str>>,
    clones: Vec<Arc<str>>,
    children: Vec<Arc<str>>,
    mount: MountState,
    root_case: Option<RootDatasetCase>,
    written: Option<u128>,
    used_by_snapshots: Option<u128>,
    checklist: SafetyChecklist,
}

impl Examination {
    /// Everything a full rollback would destroy, in the order §13.6 enumerates them.
    fn destroyed(&self) -> Vec<Arc<str>> {
        destroyed_objects(
            &self.newer_snapshots,
            &self.affected_bookmarks,
            &self.clones,
        )
    }

    /// Whether a selective file restore can reach into the snapshot at all (§13.5).
    fn selective_is_possible(&self) -> bool {
        self.dataset
            .as_ref()
            .is_some_and(Dataset::has_placed_mountpoint)
            && self.mount.mounted == Some(true)
            && self.mount.read_only != Some(true)
            && self
                .mount
                .mountpoint
                .as_deref()
                .is_some_and(|mountpoint| mountpoint.starts_with('/'))
    }

    /// The `snapdir` word the plan states (§13.5).
    fn snapdir(&self) -> &str {
        self.mount.snapdir.as_deref().unwrap_or("hidden")
    }
}

/// The bookmarks a rollback past `reference` would take with it (Appendix D.5).
///
/// A bookmark records the GUID of the snapshot it was taken from, so a bookmark whose GUID
/// belongs to a snapshot at or before the target is unaffected and one whose GUID belongs to a
/// newer snapshot is destroyed. A bookmark whose source snapshot no longer exists cannot be
/// placed either way, and §56.3's direction is to include it: over-enumerating adds an acceptance
/// the operator can give, and under-enumerating destroys something nobody was shown.
fn affected_bookmarks(layout: &Layout, dataset: &str, reference: &str) -> Vec<Arc<str>> {
    let newer: Vec<&str> = layout
        .newer_snapshots(reference)
        .into_iter()
        .map(|snapshot| snapshot.guid.as_ref())
        .collect();
    let at_or_before: Vec<&str> = layout
        .snapshots_of(dataset)
        .into_iter()
        .filter(|snapshot| !newer.contains(&snapshot.guid.as_ref()))
        .map(|snapshot| snapshot.guid.as_ref())
        .collect();
    layout
        .bookmarks_of(dataset)
        .into_iter()
        .filter(|bookmark| !at_or_before.contains(&bookmark.guid.as_ref()))
        .map(|bookmark| Arc::clone(&bookmark.name))
        .collect()
}

/// Newer snapshots, then bookmarks, then clones — §13.6's enumeration, without duplicates.
fn destroyed_objects(
    snapshots: &[Arc<str>],
    bookmarks: &[Arc<str>],
    clones: &[Arc<str>],
) -> Vec<Arc<str>> {
    let mut objects: Vec<Arc<str>> = Vec::new();
    for name in snapshots.iter().chain(bookmarks).chain(clones) {
        if !objects.iter().any(|seen| seen == name) {
            objects.push(Arc::clone(name));
        }
    }
    objects
}

/// A byte figure that is `None` where ZFS reported nothing to charge for (§38.2).
///
/// §38.2 forbids displaying "free" for a copy-on-write snapshot, and zero rendered in a cost
/// column is that word spelled differently. A snapshot that shares every block with its dataset
/// genuinely uses no space *of its own*, and what it will cost is a function of what the dataset
/// writes next — so the honest figure is unknown rather than nought.
fn chargeable(bytes: Option<u128>) -> Option<ByteSize> {
    bytes.filter(|count| *count > 0).map(ByteSize::from_bytes)
}

/// Where a clone materialised for a `CLONE_AND_COPY` recovery is placed (Appendix D.4).
///
/// The mountpoint is set explicitly on the clone rather than inherited, so the path the copy
/// reads from is a fact of the plan rather than something ZFS decides at apply time.
pub const CLONE_MOUNT_ROOT: &str = "/var/lib/ono/recovery";

/// The dataset names one candidate's scope covers, in the order discovery listed them.
///
/// A ZFS dataset name never begins with `/` and a filesystem path always does, which is what
/// lets one scope carry both the datasets an asset holds and the concrete paths inside them —
/// §13.4's coverage question can then be asked with either.
#[must_use]
fn covered_datasets(candidate: &RecoveryCandidate) -> Vec<Arc<str>> {
    let named: Vec<Arc<str>> = candidate
        .scope()
        .covers()
        .iter()
        .filter(|covered| !covered.starts_with('/'))
        .map(Arc::clone)
        .collect();
    if named.is_empty() {
        vec![Arc::from(candidate.scope().domain())]
    } else {
        named
    }
}

/// The dataset in `datasets` that is an ancestor of every other one, where there is one.
///
/// §13.3's recursive creation happens at exactly one point in the tree, and this is that point.
#[must_use]
fn top_dataset(datasets: &[Arc<str>]) -> Option<Arc<str>> {
    datasets
        .iter()
        .find(|candidate| {
            datasets
                .iter()
                .all(|other| is_descendant(other, candidate.as_ref()))
        })
        .map(Arc::clone)
}

/// The concrete paths a scope covers.
#[must_use]
fn covered_paths(scope: &RecoveryScope) -> Vec<Arc<str>> {
    scope
        .covers()
        .iter()
        .filter(|covered| covered.starts_with('/'))
        .map(Arc::clone)
        .collect()
}

impl RecoveryProvider for ZfsProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        // §12.2's five required capabilities and nothing beyond them: ZFS offers no quiesce of
        // its own (§39.2 leaves application consistency to the application) and no transaction
        // boundary (§27.1 reserves the word for a provider that can state atomicity guarantees).
        RecoveryCapability::REQUIRED
            .iter()
            .fold(
                ProviderCapabilities::new(PROVIDER_ID),
                |carry, capability| carry.recovering(*capability),
            )
            .tested_against("zfs", VALIDATED_VERSIONS.join(", "))
    }

    fn availability(&self) -> ProviderAvailability {
        if !self.runner.is_available(ZFS) {
            return ProviderAvailability::Unavailable {
                reason: Arc::from(format!("`{ZFS}` is not present on this host")),
            };
        }
        let Ok(output) = self.zfs(&["version"]) else {
            return ProviderAvailability::Unavailable {
                reason: Arc::from(format!("`{ZFS} version` could not be run")),
            };
        };
        if !output.succeeded() {
            return ProviderAvailability::Unavailable {
                reason: Arc::from(format!(
                    "`{ZFS} version` exited {} rather than reporting a version",
                    output.status()
                )),
            };
        }
        let Some(version) = parse::version(output.stdout()) else {
            return ProviderAvailability::Unavailable {
                reason: Arc::from("`zfs version` did not print a `zfs-<version>` line"),
            };
        };
        let release = version.split('-').next().unwrap_or(&version);
        if VALIDATED_VERSIONS.contains(&release) {
            ProviderAvailability::Available { version }
        } else {
            ProviderAvailability::Unsupported {
                reason: Arc::from(format!(
                    "v0.6 Appendix G.4: this provider has been validated against OpenZFS {} and \
                     degrades rather than executing semantics it has not tested",
                    VALIDATED_VERSIONS.join(", ")
                )),
                version,
            }
        }
    }

    fn resolve_domain(&self, path: &str) -> Result<Option<PersistenceDomain>, ErrorValue> {
        self.require_tools()?;
        let table = self.mount_table()?;
        if table.is_empty() {
            return Err(target_unresolved(
                path,
                "v0.6 Appendix B.1 and §56.3: the kernel mount table held no mounts, so no path \
                 has a persistence domain and none is assumed",
            ));
        }
        let Some(mount) = table.covering(path) else {
            return Ok(None);
        };
        if mount.kind() != ono_change_core::FilesystemKind::Zfs {
            // Appendix B.8: a directory named like a dataset, or an ext4 filesystem mounted at a
            // pool-shaped path, is not this provider's business however much it looks like it.
            return Ok(None);
        }
        let listed = self.zfs(&[
            "list",
            "-H",
            "-p",
            "-t",
            "filesystem",
            "-o",
            "name,mountpoint,mounted,type,used,available,referenced,origin,canmount",
        ])?;
        if !listed.succeeded() {
            let refusal = refusal_text(&listed);
            if parse::is_permission_refusal(&refusal) {
                return Err(privilege_required(
                    path,
                    "root, or a `zfs allow` delegation that permits listing datasets",
                    false,
                ));
            }
            return Err(target_unresolved(path, &refusal));
        }
        let datasets = crate::layout::datasets(ZFS, listed.stdout())?;
        let Some(dataset) = datasets.iter().find(|dataset| dataset.name == mount.source) else {
            return Err(target_unresolved(
                path,
                &format!(
                    "v0.6 Appendix B.8: the mount table names `{}` as the source of the mount at \
                     `{}`, and `zfs list` does not report a dataset of that name. Dataset \
                     identity is never inferred from the shape of the path",
                    mount.source, mount.mount_point
                ),
            ));
        };
        Ok(Some(
            PersistenceDomain::resolved(
                path,
                mount.resolved(),
                "zfs-dataset",
                Arc::clone(&dataset.name),
                format!(
                    "the mount at `{}` is served by the ZFS dataset `{}`, which is the snapshot \
                     boundary §13.4 makes it",
                    mount.mount_point, dataset.name
                ),
            )
            .with_boundary(Arc::clone(&dataset.name)),
        ))
    }

    fn discover(
        &self,
        domain: &PersistenceDomain,
        objective: RecoveryObjective,
    ) -> Result<Vec<RecoveryCandidate>, ErrorValue> {
        self.require_tools()?;
        let layout = self.survey()?;
        let path = domain.path();
        let Some(dataset) = layout.dataset_of_path(path).cloned() else {
            // Not this provider's business: §12.1's `resolve_domain` contract answers "nothing
            // here", which §55.6 case 29 keeps distinct from "there is nothing to protect".
            return Ok(Vec::new());
        };
        if let Some(declared) = domain.object()
            && declared != dataset.name.as_ref()
        {
            return Err(target_unresolved(
                path,
                &format!(
                    "v0.6 Appendix B.8: the caller resolved `{path}` to `{declared}` and the \
                     mount table resolves it to `{}`. A protection claim is not made over a \
                     dataset two readings disagree about",
                    dataset.name
                ),
            ));
        }

        let part = snapshot_part(self.plan.as_ref().map(PlanId::short), self.now());
        let children = layout.descendants_of(&dataset.name);
        let in_tree = layout.descendants_in_tree(&dataset.name, path);
        let pool_note = layout.pool(dataset.pool()).map(|pool| {
            format!(
                "the pool `{}` is {} with {} fragmentation",
                pool.name,
                pool.health,
                pool.fragmentation
                    .map_or_else(|| "unknown".to_owned(), |value| format!("{value}%"))
            )
        });

        let mut candidates = Vec::new();
        let mut exact = RecoveryCandidate::new(
            PROVIDER_ID,
            RecoveryScope::new(
                "zfs-dataset",
                Arc::clone(&dataset.name),
                Arc::clone(&self.host),
            )
            .covering(Arc::clone(&dataset.name))
            .covering(path),
            EffectDomain::FilesystemPersistent,
            objective,
            format!("snapshot {}@{part}", dataset.name),
        )
        .at_consistency(ConsistencyClass::FilesystemConsistent)
        .restored_by(RestoreMethod::SelectiveFileRestore)
        .costing(RecoveryCost::unknown())
        .needing_to_create(
            "root, or a `zfs allow` delegation that permits `snapshot` on this dataset",
        )
        .needing_to_restore(
            "v0.6 §13.5: a selective file restore reads the file out of the dataset's \
             `.zfs/snapshot` directory, which needs the dataset mounted and writable",
        );
        for child in &children {
            exact = exact.excluding(RecoveryExclusion::new(
                Arc::clone(&child.name),
                format!(
                    "v0.6 §13.4: `{}` is a separate dataset, so this snapshot does not protect it",
                    child.name
                ),
            ));
        }
        // §11.5 and §33.1: a dataset snapshot is a filesystem-persistent recovery point and says
        // nothing about the process or network state a change also touches.
        exact = exact
            .excluding(RecoveryExclusion::new(
                "process state",
                "v0.6 §33.1: a filesystem snapshot does not capture process identity or memory",
            ))
            .excluding(RecoveryExclusion::new(
                "network sessions",
                "v0.6 §34: live sessions are runtime state and no snapshot returns them",
            ));
        if let Some(note) = &pool_note {
            exact = exact.needing_to_create(note.clone());
        }
        candidates.push(exact);

        if !in_tree.is_empty() {
            let mut scope = RecoveryScope::new(
                "zfs-dataset",
                Arc::clone(&dataset.name),
                Arc::clone(&self.host),
            )
            .covering(Arc::clone(&dataset.name));
            for child in &in_tree {
                scope = scope.covering(Arc::clone(&child.name));
            }
            let mut recursive = RecoveryCandidate::new(
                PROVIDER_ID,
                scope.covering(path),
                EffectDomain::FilesystemPersistent,
                objective,
                format!(
                    "snapshot -r {}@{part}, covering {} dataset(s) at one point",
                    dataset.name,
                    in_tree.len() + 1
                ),
            )
            .at_consistency(ConsistencyClass::FilesystemConsistent)
            .restored_by(RestoreMethod::SelectiveFileRestore)
            .costing(RecoveryCost::unknown())
            .needing_to_create(
                "v0.6 §13.3: one recursive creation, recorded as one asset per dataset",
            )
            .needing_to_restore(
                "v0.6 §13.3: recovery reasons about each dataset separately; ZFS offers no one \
                 recursive rollback for the whole tree",
            );
            for child in &children {
                if !in_tree.iter().any(|inside| inside.name == child.name) {
                    recursive = recursive.excluding(RecoveryExclusion::new(
                        Arc::clone(&child.name),
                        format!(
                            "v0.6 §13.4: `{}` is a separate dataset outside the target tree",
                            child.name
                        ),
                    ));
                }
            }
            candidates.push(recursive);
        }
        Ok(candidates)
    }

    fn plan_protection(
        &self,
        candidates: &[RecoveryCandidate],
        mode: ProtectionMode,
    ) -> Result<Vec<ProtectionAction>, ErrorValue> {
        if !mode.creates_assets() {
            // §17.2's `off`: create nothing, and let the caller still show what was available.
            return Ok(Vec::new());
        }
        self.require_tools()?;
        let pools = self.pool_reading()?;
        let now = self.now();
        let part = snapshot_part(self.plan.as_ref().map(PlanId::short), now);
        let mut actions = Vec::new();
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.provider() == PROVIDER_ID)
        {
            let datasets = covered_datasets(candidate);
            let top = top_dataset(&datasets);
            let paths = covered_paths(candidate.scope());
            for dataset in &datasets {
                if let Err(pressure) = self.space_guard(&pools, dataset) {
                    // Appendix D.3: `prefer` and `maximize` fail closed here, into a plan the
                    // operator has to decide about explicitly. `require` still plans, and
                    // `create` fails the apply, which is what "MUST fail apply" asks for.
                    if !mode.refuses_shortfall() {
                        return Err(pressure);
                    }
                }
                let reference = full_name(dataset, &part);
                let recursive = datasets.len() > 1 && top.as_deref() == Some(dataset.as_ref());
                let mut scope =
                    RecoveryScope::new("zfs-dataset", Arc::clone(dataset), Arc::clone(&self.host))
                        .covering(Arc::clone(dataset));
                if dataset.as_ref() == candidate.scope().domain() {
                    for path in &paths {
                        scope = scope.covering(Arc::clone(path));
                    }
                }
                let mut asset = RecoveryAsset::proposed(
                    PROVIDER_ID,
                    RecoveryAssetType::ZfsSnapshot,
                    Arc::clone(&reference),
                    scope,
                    now,
                )
                .at_consistency(ConsistencyClass::FilesystemConsistent)
                .restored_by(RestoreMethod::SelectiveFileRestore)
                .costing(RecoveryCost::unknown());
                if let Some(plan) = &self.plan {
                    asset = asset.for_plan(plan.clone());
                }
                for exclusion in candidate.exclusions() {
                    asset = asset.excluding(exclusion.clone());
                }
                let summary = if recursive {
                    format!(
                        "zfs snapshot -r {reference} — one recursive creation over {} datasets, \
                         recorded individually (§13.3, Appendix D.1)",
                        datasets.len()
                    )
                } else {
                    format!("zfs snapshot {reference}")
                };
                let action = ProtectionAction::new(PROVIDER_ID, summary, candidate.clone(), asset);
                actions.push(if matches!(mode, ProtectionMode::Maximize) {
                    action.optional()
                } else {
                    action
                });
            }
        }
        Ok(actions)
    }

    fn create(&self, action: &ProtectionAction) -> Result<RecoveryAsset, ErrorValue> {
        self.require_tools()?;
        let asset = action.proposed_asset();
        let dataset = asset.scope().domain().to_owned();
        let reference = asset.reference().to_owned();
        // §43.6 governs the half of the name this provider generates. The dataset half comes
        // from ZFS's own listing, so it is passed through as one argument and ZFS refuses it if
        // it is impossible; the snapshot half is checked here, because a caller that hand-built
        // a protection action must not be able to route an unsanitised name through `create`.
        let generated = reference.rsplit_once('@').map(|(_, part)| part);
        if !generated.is_some_and(crate::naming::is_valid_snapshot_part) {
            return Err(asset_create_failed(
                PROVIDER_ID,
                &dataset,
                "v0.6 §43.6: the snapshot name is not one ZFS accepts, and this provider does not \
                 hand ZFS a name it has not sanitised",
            ));
        }
        // Appendix D.3: `require` plans through storage pressure and fails here instead.
        let pools = self.pool_reading()?;
        self.space_guard(&pools, &dataset)?;

        let datasets = covered_datasets(action.candidate());
        let recursive =
            datasets.len() > 1 && top_dataset(&datasets).as_deref() == Some(dataset.as_str());
        let argv: Vec<&str> = if recursive {
            vec!["snapshot", "-r", &reference]
        } else {
            vec!["snapshot", &reference]
        };
        let created = self.zfs(&argv)?;
        if !created.succeeded() {
            let refusal = refusal_text(&created);
            if parse::is_permission_refusal(&refusal) {
                return Err(privilege_required(
                    &reference,
                    "root, or a `zfs allow` delegation that permits `snapshot`",
                    false,
                ));
            }
            if !parse::is_already_present(&refusal) {
                return Err(asset_create_failed(PROVIDER_ID, &dataset, &refusal));
            }
            // §13.3: the sibling recursive creation already made this one, and Appendix D.1
            // still wants its identity recorded individually.
        }

        // §2.14: the command exiting zero is not evidence the snapshot exists. The identity is
        // read back, and it is the GUID rather than the name that §56.1 calls exact.
        let listed = self.zfs(&[
            "list",
            "-H",
            "-p",
            "-t",
            "snapshot",
            "-o",
            "name,creation,used,referenced,guid,defer_destroy",
        ])?;
        if !listed.succeeded() {
            return Err(asset_create_failed(
                PROVIDER_ID,
                &dataset,
                &refusal_text(&listed),
            ));
        }
        let snapshots = crate::layout::snapshots(ZFS, listed.stdout())?;
        let Some(found) = snapshots
            .iter()
            .find(|snapshot| snapshot.name.as_ref() == reference)
        else {
            return Err(asset_create_failed(
                PROVIDER_ID,
                &dataset,
                "v0.6 §2.14: the command reported success and `zfs list -t snapshot` does not \
                 show the snapshot, so no protection is claimed",
            ));
        };
        Ok(asset
            .clone()
            .creating()
            .capturing(format!("{GUID_FINGERPRINT}{}", found.guid))
            .costing(RecoveryCost::unknown().with_space(
                chargeable(found.used),
                chargeable(found.used),
                true,
            )))
    }

    fn validate(&self, asset: &RecoveryAsset) -> Result<RecoveryValidation, ErrorValue> {
        self.require_tools()?;
        let now = self.now();
        let reference = asset.reference().to_owned();
        let dataset = reference.split_once('@').map_or_else(
            || asset.scope().domain().to_owned(),
            |(head, _)| head.to_owned(),
        );

        let listed = self.zfs(&[
            "list",
            "-H",
            "-p",
            "-t",
            "snapshot",
            "-o",
            "name,creation,used,referenced,guid,defer_destroy",
        ])?;
        if !listed.succeeded() {
            let refusal = refusal_text(&listed);
            let detail = if parse::is_permission_refusal(&refusal) {
                format!(
                    "v0.6 §11.4 and §43.4: ZFS refused the listing for want of privilege, so \
                     nothing about `{reference}` was checked. {refusal}"
                )
            } else {
                format!("`zfs list -t snapshot` could not be read: {refusal}")
            };
            return Ok(RecoveryValidation::none(now, detail));
        }
        let snapshots = crate::layout::snapshots(ZFS, listed.stdout())?;
        let found = snapshots
            .iter()
            .find(|snapshot| snapshot.name.as_ref() == reference);
        let recorded = asset
            .captured_state()
            .and_then(|fingerprint| fingerprint.strip_prefix(GUID_FINGERPRINT));
        let identity = found
            .is_some_and(|snapshot| recorded.is_none_or(|guid| snapshot.guid.as_ref() == guid));
        let scope_matches =
            found.is_some_and(|snapshot| snapshot.dataset.as_ref() == asset.scope().domain());

        let placement = self.zfs(&[
            "get",
            "-H",
            "-p",
            "-o",
            "name,property,value",
            "mounted,mountpoint,canmount,readonly,origin,snapdir",
            &dataset,
        ])?;
        let permissions =
            !Self::refused_for_privilege(&[&listed, &placement]) && placement.succeeded();
        let mount = if placement.succeeded() {
            mount_state(
                &parse::properties(ZFS, placement.stdout()).unwrap_or_default(),
                &dataset,
            )
        } else {
            MountState::default()
        };
        let restore_available = mount.mounted == Some(true)
            && mount.read_only != Some(true)
            && mount
                .mountpoint
                .as_deref()
                .is_some_and(|mountpoint| mountpoint.starts_with('/'));

        let detail = format!(
            "`{reference}` was checked against `zfs list -t snapshot` and the dataset's mount \
             properties. §11.4: existence {}, identity {}, scope {}, restore {}, privilege {}",
            yes_no(found.is_some()),
            yes_no(identity),
            yes_no(scope_matches),
            yes_no(restore_available),
            yes_no(permissions),
        );
        Ok(RecoveryValidation::complete(now, detail)
            .existing(found.is_some())
            .identity(identity)
            .scope(scope_matches)
            .restore(restore_available)
            .permissions(permissions))
    }

    fn plan_recovery(
        &self,
        asset: &RecoveryAsset,
        source: Option<&ChangePlan>,
        goal: RecoveryGoal,
    ) -> Result<RecoveryPlanFragment, ErrorValue> {
        let examination = self.examine(asset)?;
        // §56.3: the goal decides whether the whole checklist applies, before a method is chosen,
        // so a fact that is missing blocks rather than quietly narrowing the choice of method.
        let goal_is_destructive = matches!(goal, RecoveryGoal::RestoreDomain);
        if let Some(blocked) = examination.checklist.blocking_error(goal_is_destructive) {
            return Err(blocked);
        }

        let mut available = vec![RestoreMethod::CloneAndCopy];
        if examination.selective_is_possible() {
            available.push(RestoreMethod::SelectiveFileRestore);
        }
        available.push(match examination.root_case {
            Some(_) => RestoreMethod::OfflineRootRecovery,
            None => RestoreMethod::DatasetRollback,
        });
        let Some(method) = choose_method(goal, &available) else {
            return Err(recovery_plan_incomplete(
                "a restore method that achieves the recovery goal",
                &format!(
                    "v0.6 Appendix C.1: nothing this provider offers achieves `{}` for \
                     `{}`. ZFS restores prior state and does not compensate semantics",
                    goal.as_str(),
                    examination.reference
                ),
            ));
        };
        if method.discards_newer_state()
            && let Some(blocked) = examination.checklist.blocking_error(true)
        {
            return Err(blocked);
        }

        let plan_id = source.map_or_else(
            || {
                self.plan.clone().unwrap_or_else(|| {
                    PlanId::of(
                        PROVIDER_ID,
                        &asset.created_at().to_string(),
                        &examination.reference,
                    )
                })
            },
            |plan| plan.id().clone(),
        );
        let objects = objects_to_restore(asset, source);

        let mut fragment = RecoveryPlanFragment::new(PROVIDER_ID, method);
        let mut ordinal = 0;
        let mut unreachable = Vec::new();

        match method {
            RestoreMethod::SelectiveFileRestore => {
                let mountpoint = examination
                    .mount
                    .mountpoint
                    .as_deref()
                    .unwrap_or("")
                    .to_owned();
                for object in &objects {
                    match ZfsProvider::snapshot_path(&mountpoint, &examination.short, object) {
                        Some(source_path) => {
                            fragment = fragment.acting(
                                file_restore_action(
                                    &plan_id,
                                    ordinal,
                                    &source_path,
                                    object,
                                    examination.snapdir(),
                                )
                                .requiring(snapshot_precondition(&examination)),
                            );
                            ordinal += 1;
                        }
                        None => unreachable.push(Arc::clone(object)),
                    }
                }
            }
            RestoreMethod::CloneAndCopy => {
                let clone_name = format!(
                    "{}/ono-restore-{}",
                    examination
                        .dataset_name
                        .split('/')
                        .next()
                        .unwrap_or(&examination.dataset_name),
                    examination.short
                );
                let clone_mount = format!("{CLONE_MOUNT_ROOT}/{}", examination.short);
                fragment = fragment.acting(program_action(
                    &plan_id,
                    ordinal,
                    ActionRole::Prepare,
                    format!(
                        "zfs clone {} {clone_name} — Appendix D.4's CLONE_AND_COPY materialises \
                         the snapshot beside the live dataset rather than over it",
                        examination.reference
                    ),
                    ZFS,
                    vec![
                        Arc::from("clone"),
                        Arc::from("-o"),
                        Arc::from(format!("mountpoint={clone_mount}")),
                        Arc::clone(&examination.reference),
                        Arc::from(clone_name.as_str()),
                    ],
                ));
                ordinal += 1;
                for object in &objects {
                    let relative = examination
                        .mount
                        .mountpoint
                        .as_deref()
                        .and_then(|mountpoint| {
                            is_beneath(object, mountpoint)
                                .then(|| object.get(mountpoint.len()..).unwrap_or("").to_owned())
                        });
                    match relative {
                        Some(relative) => {
                            let source_path =
                                format!("{clone_mount}/{}", relative.trim_start_matches('/'));
                            fragment = fragment.acting(file_restore_action(
                                &plan_id,
                                ordinal,
                                &source_path,
                                object,
                                examination.snapdir(),
                            ));
                            ordinal += 1;
                        }
                        None => unreachable.push(Arc::clone(object)),
                    }
                }
                fragment = fragment.acting(program_action(
                    &plan_id,
                    ordinal,
                    ActionRole::Cleanup,
                    format!("zfs destroy {clone_name} — the clone is temporary (§37)"),
                    ZFS,
                    vec![Arc::from("destroy"), Arc::from(clone_name.as_str())],
                ));
                ordinal += 1;
            }
            RestoreMethod::DatasetRollback | RestoreMethod::OfflineRootRecovery => {
                // §13.6: every destruction is a named action of its own. Ono adds no flag that
                // removes newer history as a side effect of the rollback, so what would be lost
                // is visible object by object in the plan an operator accepts.
                for object in examination
                    .newer_snapshots
                    .iter()
                    .chain(examination.affected_bookmarks.iter())
                {
                    fragment = fragment.acting(program_action(
                        &plan_id,
                        ordinal,
                        ActionRole::Recover,
                        format!(
                            "zfs destroy {object} — §13.6: this stands in the rollback's way, and \
                             destroying it needs explicit acceptance"
                        ),
                        ZFS,
                        vec![Arc::from("destroy"), Arc::clone(object)],
                    ));
                    ordinal += 1;
                }
                fragment = fragment.acting(
                    program_action(
                        &plan_id,
                        ordinal,
                        ActionRole::Recover,
                        format!(
                            "zfs rollback {} — §13.6: the whole dataset returns to this point and \
                             everything written since is discarded",
                            examination.reference
                        ),
                        ZFS,
                        vec![Arc::from("rollback"), Arc::clone(&examination.reference)],
                    )
                    .requiring(snapshot_precondition(&examination)),
                );
                ordinal += 1;
            }
            _ => {}
        }
        let _ = ordinal;

        for object in unreachable {
            fragment = fragment.leaving(UnrecoverableEffect::new(
                Arc::clone(&object),
                EffectDomain::FilesystemPersistent,
                format!(
                    "v0.6 §13.4: `{object}` is not inside the dataset `{}` that this snapshot \
                     holds, so this asset cannot put it back",
                    examination.dataset_name
                ),
            ));
        }
        for clone in &examination.clones {
            fragment = fragment.leaving(UnrecoverableEffect::new(
                Arc::clone(clone),
                EffectDomain::FilesystemPersistent,
                format!(
                    "v0.6 §13.6: `{clone}` is a clone of this snapshot. ZFS refuses the rollback \
                     while it exists, and Ono does not add the flag that would destroy it"
                ),
            ));
        }

        for object in &objects {
            fragment = fragment.verifying(
                VerificationContract::new(
                    &plan_id,
                    VerificationClass::Required,
                    Arc::clone(object),
                    "content equals the state the snapshot holds",
                )
                .about(ono_change_core::EquivalenceDomain::PersistentState),
            );
        }

        fragment = fragment
            .with_newer_state(newer_state_impact(&examination, method))
            .restoring_metadata(Self::metadata_coverage(method));
        if matches!(method, RestoreMethod::OfflineRootRecovery) {
            fragment = fragment.needing_reboot().needing_offline();
        } else if method.discards_newer_state() && examination.mount.mounted == Some(true) {
            // §13.7: Ono MUST NOT promise online rollback merely because a snapshot exists. A
            // mounted dataset has to leave the mount table for the rollback, and saying so before
            // apply is the whole of the requirement.
            fragment = fragment.needing_offline();
        }
        Ok(fragment)
    }

    fn restore(&self, action: &PlanAction, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        self.require_tools()?;
        let Execution::Program { program, argv } = action.execution() else {
            return Err(recovery_apply_failed(
                action.summary(),
                "v0.6 §2.17: this provider carries out a recovery action as a program and an \
                 argument vector, and this action carries neither",
            ));
        };
        if argv.iter().any(|argument| argument.as_ref() == "-R") {
            return Err(recovery_apply_failed(
                action.summary(),
                "v0.6 §13.6: Ono never adds a destructive rollback flag equivalent to removing \
                 newer history, and refuses to run one it did not build",
            ));
        }
        let destroys_history = program.as_ref() == ZFS
            && matches!(
                argv.first().map(Arc::as_ref),
                Some("rollback") | Some("destroy")
            );
        if destroys_history {
            let examination = self.examine(asset)?;
            if let Some(blocked) = examination.checklist.blocking_error(true) {
                return Err(blocked);
            }
            // §24.5's gate comes first: an operator is shown everything the recovery would take
            // away before being told what else it needs. §13.7's requirement is the second
            // refusal, not a way of never reaching the first.
            let destroyed = examination.destroyed();
            if !destroyed.is_empty() && !self.accepted_history_destruction {
                return Err(destructive_history_not_accepted(
                    &destroyed
                        .iter()
                        .map(|object| object.to_string())
                        .collect::<Vec<String>>(),
                ));
            }
            if argv.first().map(Arc::as_ref) == Some("rollback")
                && let Some(case) = examination.root_case
            {
                // §13.7 and §55.3 case 16: the requirement is reported before execution, and the
                // execution then refuses rather than rolling back the running system.
                return Err(match case {
                    RootDatasetCase::RunningRoot => requires_offline(
                        &examination.dataset_name,
                        RestoreMethod::OfflineRootRecovery.as_str(),
                    ),
                    RootDatasetCase::BootEnvironment => requires_reboot(
                        &examination.dataset_name,
                        RestoreMethod::OfflineRootRecovery.as_str(),
                    ),
                });
            }
        }
        let arguments: Vec<&str> = argv.iter().map(Arc::as_ref).collect();
        let output = self.runner.run(program, &arguments)?;
        if output.succeeded() {
            return Ok(());
        }
        let refusal = refusal_text(&output);
        if parse::is_permission_refusal(&refusal) {
            return Err(privilege_required(
                action.summary(),
                "root, or a `zfs allow` delegation that permits this operation",
                true,
            ));
        }
        let refused = parse::rollback_refusal(&refusal);
        if !refused.is_empty() {
            return Err(destructive_history_not_accepted(
                &refused
                    .objects()
                    .iter()
                    .map(|object| object.to_string())
                    .collect::<Vec<String>>(),
            ));
        }
        Err(recovery_apply_failed(action.summary(), &refusal))
    }

    fn cleanup(&self, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        self.require_tools()?;
        let reference = asset.reference().to_owned();
        // §37: exactly this snapshot. Never `-r`, which would take every descendant's snapshot of
        // the same name, and never `-R`, which would take the clones with it.
        let output = self.zfs(&["destroy", &reference])?;
        if output.succeeded() {
            return Ok(());
        }
        let refusal = refusal_text(&output);
        if parse::is_missing_object(&refusal) {
            // §37: the asset is gone, which is what removal was for.
            return Ok(());
        }
        if parse::is_permission_refusal(&refusal) {
            return Err(privilege_required(
                &reference,
                "root, or a `zfs allow` delegation that permits `destroy`",
                true,
            ));
        }
        let dependents = parse::destroy_refusal(&refusal);
        if !dependents.is_empty() {
            return Err(destructive_history_not_accepted(
                &dependents
                    .iter()
                    .map(|object| object.to_string())
                    .collect::<Vec<String>>(),
            ));
        }
        Err(recovery_apply_failed(&reference, &refusal))
    }

    fn estimate_cost(&self, asset: &RecoveryAsset) -> Result<RecoveryCost, ErrorValue> {
        self.require_tools()?;
        let reference = asset.reference().to_owned();
        let dataset = reference.split_once('@').map_or_else(
            || asset.scope().domain().to_owned(),
            |(head, _)| head.to_owned(),
        );
        let properties = self.zfs(&[
            "get",
            "-H",
            "-p",
            "-o",
            "name,property,value",
            "used,written,usedbysnapshots",
            &dataset,
        ])?;
        let listed = self.zfs(&[
            "list",
            "-H",
            "-p",
            "-t",
            "snapshot",
            "-o",
            "name,creation,used,referenced,guid,defer_destroy",
        ])?;
        let rows = if properties.succeeded() {
            parse::properties(ZFS, properties.stdout())?
        } else {
            Vec::new()
        };
        let by_snapshots = parse::property_of(&rows, &dataset, "usedbysnapshots")
            .and_then(|entry| parse::number(&entry.value));
        let snapshot_used = listed
            .succeeded()
            .then(|| crate::layout::snapshots(ZFS, listed.stdout()))
            .transpose()?
            .and_then(|snapshots| {
                snapshots
                    .iter()
                    .find(|snapshot| snapshot.name.as_ref() == reference)
                    .and_then(|snapshot| snapshot.used)
            });
        // §37.5 and §38.2: the figure is labelled estimated and never rendered as zero. ZFS's
        // `used` for a snapshot is the space no other snapshot shares, which is not the space
        // deleting it would free, so the number is an estimate even when ZFS is exact about it.
        Ok(RecoveryCost::unknown().with_space(
            asset
                .cost()
                .initial_bytes()
                .or_else(|| chargeable(snapshot_used)),
            chargeable(snapshot_used).or_else(|| chargeable(by_snapshots)),
            true,
        ))
    }
}

/// Everything the program said, whichever stream it said it on.
fn refusal_text(output: &ToolOutput) -> String {
    let mut text = output.stderr().trim_end().to_owned();
    if !output.stdout().trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(output.stdout().trim_end());
    }
    if text.is_empty() {
        text = format!("the command exited {}", output.status());
    }
    text
}

/// The word a validation detail uses for a check that passed or did not.
const fn yes_no(passed: bool) -> &'static str {
    if passed { "yes" } else { "no" }
}

/// The objects a recovery sets out to put back (Appendix C.2).
///
/// Appendix C.2 is explicit that the goal is not automatically the whole persistence domain: the
/// objects the original plan changed are what the operator wants back. Where no source plan is
/// given, the asset's own scope is the next best statement of what it covers.
fn objects_to_restore(asset: &RecoveryAsset, source: Option<&ChangePlan>) -> Vec<Arc<str>> {
    let mut objects: Vec<Arc<str>> = Vec::new();
    if let Some(plan) = source {
        for action in plan.actions() {
            for effect in action.effects() {
                if effect.domain() == EffectDomain::FilesystemPersistent
                    && let Some(object) = effect.object()
                    && object.starts_with('/')
                    && !objects.iter().any(|seen| seen.as_ref() == object)
                {
                    objects.push(Arc::from(object));
                }
            }
        }
    }
    if objects.is_empty() {
        objects = covered_paths(asset.scope());
    }
    objects
}

/// One `cp` action that puts a file back out of a snapshot or a clone (§13.5, Appendix C.7).
fn file_restore_action(
    plan: &PlanId,
    ordinal: usize,
    source_path: &str,
    object: &str,
    snapdir: &str,
) -> PlanAction {
    program_action(
        plan,
        ordinal,
        ActionRole::Recover,
        format!(
            "restore {object} from {source_path} — §13.5's selective file restore, leaving every \
             other file in the dataset alone. The dataset's `snapdir` is `{snapdir}`, and the \
             snapshot directory is reached by explicit path either way"
        ),
        CP,
        vec![
            Arc::from("--preserve=all"),
            Arc::from("--no-dereference"),
            Arc::from("--no-target-directory"),
            Arc::from("--"),
            Arc::from(source_path),
            Arc::from(object),
        ],
    )
    .on(object)
}

/// One plan action that runs a program with an argument vector and no shell (§2.17, §12.3).
fn program_action(
    plan: &PlanId,
    ordinal: usize,
    role: ActionRole,
    summary: String,
    program: &str,
    argv: Vec<Arc<str>>,
) -> PlanAction {
    PlanAction::new(
        plan,
        ordinal,
        role,
        summary,
        Execution::Program {
            program: Arc::from(program),
            argv,
        },
    )
    .with_idempotency(Idempotency::NonIdempotent)
    .privileged()
}

/// The precondition §7.2 puts on an action that reads from one exact snapshot.
fn snapshot_precondition(examination: &Examination) -> Precondition {
    Precondition::new(
        PreconditionKind::Existence,
        Arc::clone(&examination.reference),
        "guid",
        Value::string(
            examination
                .snapshot
                .as_ref()
                .map_or("", |snapshot| snapshot.guid.as_ref()),
        ),
    )
    .explained(
        "v0.6 §56.1: a snapshot destroyed and recreated under the same name is a different \
         snapshot, and only the GUID says so",
    )
}

/// What the chosen method would do to everything written since the snapshot (Appendix C.3, D.5).
fn newer_state_impact(examination: &Examination, method: RestoreMethod) -> NewerStateImpact {
    let discards = method.discards_newer_state();
    let mut items = Vec::new();
    for snapshot in &examination.newer_snapshots {
        items.push(NewerStateItem::new(
            Arc::clone(snapshot),
            if discards {
                NewerStateClass::DiscardedByMethod
            } else {
                NewerStateClass::PreservedByMethod
            },
            if discards {
                "v0.6 Appendix D.5: this snapshot was taken after the recovery point and a full \
                 rollback cannot pass it"
            } else {
                "v0.6 §59.6: a selective restore leaves later snapshots where they are"
            },
        ));
    }
    for bookmark in &examination.affected_bookmarks {
        items.push(NewerStateItem::new(
            Arc::clone(bookmark),
            if discards {
                NewerStateClass::DiscardedByMethod
            } else {
                NewerStateClass::PreservedByMethod
            },
            "v0.6 §13.6: a bookmark of a newer snapshot is destroyed by a rollback past it",
        ));
    }
    for clone in &examination.clones {
        items.push(NewerStateItem::new(
            Arc::clone(clone),
            if discards {
                NewerStateClass::Conflicting
            } else {
                NewerStateClass::PreservedByMethod
            },
            "v0.6 §13.6: this clone exists because of the snapshot, so rolling the origin back \
             and keeping the clone are the same object pulled two ways",
        ));
    }
    for child in &examination.children {
        // Appendix D.5's "child datasets not covered": a rollback of the parent reaches none of
        // them, which is §13.4 restated from the recovery side.
        items.push(NewerStateItem::new(
            Arc::clone(child),
            NewerStateClass::PreservedByMethod,
            "v0.6 §13.4: a separate dataset is a separate boundary, and this recovery does not \
             reach into it",
        ));
    }
    match examination.written {
        Some(written) if written > 0 && discards => items.push(NewerStateItem::new(
            Arc::clone(&examination.dataset_name),
            NewerStateClass::DiscardedByMethod,
            format!(
                "v0.6 Appendix D.5: `zfs get written` reports {written} bytes written since the \
                 snapshot, and the rollback discards them"
            ),
        )),
        Some(written) => items.push(NewerStateItem::new(
            Arc::clone(&examination.dataset_name),
            NewerStateClass::PreservedByMethod,
            format!(
                "v0.6 Appendix D.5: `zfs get written` reports {written} bytes written since the \
                 snapshot"
            ),
        )),
        None => {}
    }
    if let Some(used) = examination.used_by_snapshots {
        items.push(NewerStateItem::new(
            Arc::clone(&examination.dataset_name),
            NewerStateClass::PreservedByMethod,
            format!(
                "v0.6 §13.2: the dataset's snapshots hold {used} bytes between them, which is the \
                 lifecycle cost of keeping this recovery point"
            ),
        ));
    }

    let mut impact = NewerStateImpact::analysed(items);
    if discards {
        for object in examination.destroyed() {
            impact = impact.destroying(object);
        }
        if let Some(written) = examination.written.filter(|written| *written > 0) {
            impact = impact.discarding(ByteSize::from_bytes(written));
        }
    }
    impact
}
