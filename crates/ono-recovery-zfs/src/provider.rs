//! The ZFS recovery provider (spec v0.6 §13, Appendix D.1-D.5, §56.1).
//!
//! §13 opens by saying why ZFS is a first-party reference provider: its snapshot semantics map
//! naturally onto protected change. The rest of §13 is about the places where that map is
//! misleading, and this module is mostly those places.
//!
//! - §13.4: the dataset is the snapshot boundary. A snapshot of `tank/data` does not protect
//!   `tank/data/customer`, and [`Layout::dataset_of_path`] resolves through mount metadata so a
//!   directory *named* like a dataset never becomes one (Appendix B.8).
//! - §13.3: several datasets protected at one point are named, each of them, in one atomic
//!   `zfs snapshot`, and Appendix D.1 still wants one concrete reference per dataset.
//!   [`RecoveryProvider::plan_protection`] emits one protection action per dataset, so a
//!   multi-dataset creation produces a list of assets rather than a single entry that quietly
//!   stands for several.
//! - §13.6: rollback can require destroying newer snapshots, bookmarks and clones, and Ono MUST
//!   NEVER silently add the flag that does it. No argument vector this provider builds contains
//!   `-r` or `-R`, and [`RecoveryProvider::restore_with`] refuses every spelling of either.
//! - §13.7: rollback may need an unmount, a reboot or a boot-environment switch, and the plan
//!   says so before apply rather than promising online rollback because a snapshot exists.
//! - §56.1 and §56.3: twelve facts are proven before a destructive path is enabled, and a fact
//!   that could not be established blocks rather than being guessed.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::error::{
    asset_create_failed, destructive_history_not_accepted, precondition_failed, privilege_required,
    provider_unavailable, recovery_apply_failed, recovery_plan_incomplete, requires_offline,
    requires_reboot, storage_pressure, target_unresolved, tool_failed,
};
use ono_change_core::{
    ActionRole, ChangePlan, ConsistencyClass, EffectConfidence, EffectDomain, EffectKind,
    Execution, Idempotency, MetadataCoverage, NewerStateClass, NewerStateImpact, NewerStateItem,
    PersistenceDomain, PlanAction, PlanId, Precondition, PreconditionKind, ProtectionAction,
    ProtectionMode, ProviderAvailability, ProviderCapabilities, RecoveryAsset, RecoveryAssetType,
    RecoveryCandidate, RecoveryCapability, RecoveryCost, RecoveryExclusion, RecoveryGoal,
    RecoveryObjective, RecoveryPlanFragment, RecoveryProvider, RecoveryScope, RecoveryValidation,
    RestoreAcceptance, RestoreMethod, RestoreOutcome, ToolOutput, ToolRunner, UnrecoverableEffect,
    VerificationClass, VerificationContract, choose_method,
};
use ono_value::{ByteSize, ErrorValue, Value};

use crate::checklist::{SafetyChecklist, ZfsFact};
use crate::floor::FreeSpaceFloor;
use crate::layout::{
    Dataset, Layout, MountState, MountTable, Pool, Snapshot, is_beneath, is_descendant,
    mount_state, with_status,
};
use crate::naming::{full_name, snapshot_part};
use crate::parse::{self, Grantee};

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

/// The program that lists a snapshot's directory before a selective restore is offered (§13.5).
///
/// `.zfs/snapshot/<name>` is mounted on first access, and where the kernel cannot do that — inside
/// a container, whose mount namespace the module does not mount into — the directory exists and
/// is empty. A listing that shows the snapshot's contents is the evidence a copy can read there.
pub const LS: &str = "/bin/ls";

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

/// Who this process is, for §43.4's privilege probe.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Identity {
    /// Read from `/proc/self/status` and `/etc/passwd` when a probe needs it.
    Proc,
    /// Supplied by the caller — the deterministic suite, or a harness that already knows.
    Recorded { uid: u32, user: Arc<str> },
}

/// The permissions a non-root user needs delegated for a destructive recovery (§43.4).
///
/// `zfs-allow(8)`: `rollback` and `destroy` each "must also have the mount ability". A recovery
/// plan's destructive path is a `destroy` of each newer object and then a `rollback`, so these
/// three are what it needs.
const RESTORE_PERMISSIONS: [&str; 3] = ["rollback", "destroy", "mount"];

/// §53's setting that has to be true before a rollback destroying newer history is planned.
const ALLOW_DESTRUCTIVE_ROLLBACK: &str = "recovery.zfs.allow_destructive_rollback";

/// The ZFS recovery provider of §13.
#[derive(Debug, Clone)]
pub struct ZfsProvider {
    runner: Arc<dyn ToolRunner>,
    mounts: Mounts,
    host: Arc<str>,
    plan: Option<PlanId>,
    now: Option<Timestamp>,
    free_space_floor: FreeSpaceFloor,
    identity: Identity,
    prefer_selective_restore: bool,
    allow_destructive_rollback: bool,
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
            free_space_floor: FreeSpaceFloor::None,
            identity: Identity::Proc,
            // §53's defaults, so a provider nobody configured is the cautious one.
            prefer_selective_restore: true,
            allow_destructive_rollback: false,
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

    /// Sets Appendix D.3's free-space floor as an absolute quantity; zero is no floor.
    ///
    /// Shorthand for [`ZfsProvider::with_minimum_free`] with [`FreeSpaceFloor::Bytes`].
    #[must_use]
    pub fn with_free_space_floor(self, floor: ByteSize) -> Self {
        self.with_minimum_free(if floor == ByteSize::ZERO {
            FreeSpaceFloor::None
        } else {
            FreeSpaceFloor::Bytes(floor)
        })
    }

    /// Sets Appendix D.3's free-space floor as §53's `recovery.min_filesystem_free` states it: a
    /// share of the pool's size, or an absolute quantity.
    ///
    /// The provider has no floor of its own. Appendix D.3 speaks of a *configured* one, and §53's
    /// `"10%"` default belongs to the configuration the shell reads it from:
    ///
    /// ```
    /// # use std::sync::Arc;
    /// # use ono_recovery_zfs::{FreeSpaceFloor, ProcessRunner, ZfsProvider};
    /// # fn main() -> Result<(), ono_value::ErrorValue> {
    /// let provider = ZfsProvider::new(Arc::new(ProcessRunner::default()))
    ///     .with_minimum_free(FreeSpaceFloor::parse("10%")?);
    /// # let _ = provider;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn with_minimum_free(mut self, floor: FreeSpaceFloor) -> Self {
        self.free_space_floor = floor;
        self
    }

    /// Sets §53's `recovery.zfs.prefer_selective_restore` and
    /// `recovery.zfs.allow_destructive_rollback`, whose defaults are `true` and `false`.
    ///
    /// - `prefer_selective_restore = false` puts a dataset rollback ahead of a selective file
    ///   restore wherever the goal permits it. Appendix C.1 lets a lower method stand where the
    ///   upper ones cannot return required metadata — a file copy returns no hard links, file
    ///   capabilities or SELinux labels — so this is that exception, taken by the operator and
    ///   stated on the rollback action. It never moves an offline root recovery forward, and never
    ///   a rollback §56.1 has not fully proven.
    /// - `allow_destructive_rollback = false` keeps every rollback that would destroy a newer
    ///   snapshot, bookmark or clone — or that could not be shown to destroy none — out of the
    ///   plan, and `restore_with` refuses such an act. `--accept-newer-state-loss` does not stand
    ///   in for it (§13.6): the operator sets it first, and accepts the loss after.
    #[must_use]
    pub const fn with_recovery_policy(
        mut self,
        prefer_selective_restore: bool,
        allow_destructive_rollback: bool,
    ) -> Self {
        self.prefer_selective_restore = prefer_selective_restore;
        self.allow_destructive_rollback = allow_destructive_rollback;
        self
    }

    /// States who this process is for §43.4's privilege probe, rather than reading it from procfs.
    ///
    /// `user` is the name `zfs allow` prints for a delegation to `uid`. Production reads both;
    /// the deterministic suite and a harness that already knows who it is say so here.
    #[must_use]
    pub fn running_as(mut self, uid: u32, user: impl Into<Arc<str>>) -> Self {
        self.identity = Identity::Recorded {
            uid,
            user: user.into(),
        };
        self
    }

    /// This process's effective uid and the user name `zfs allow` would print for it.
    fn identity(&self) -> Option<(u32, Arc<str>)> {
        match &self.identity {
            Identity::Recorded { uid, user } => Some((*uid, Arc::clone(user))),
            Identity::Proc => {
                let status = std::fs::read_to_string("/proc/self/status").ok()?;
                let uid: u32 = status
                    .lines()
                    .find_map(|line| line.strip_prefix("Uid:"))?
                    .split_whitespace()
                    .nth(1)?
                    .parse()
                    .ok()?;
                let user = std::fs::read_to_string("/etc/passwd")
                    .ok()
                    .and_then(|passwd| user_name(&passwd, uid))
                    .unwrap_or_else(|| uid.to_string());
                Some((uid, Arc::from(user)))
            }
        }
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

    /// Why the snapshot's own directory cannot be copied out of, or `None` where it can (§13.5).
    ///
    /// A selective restore reads `<mountpoint>/.zfs/snapshot/<name>`, so it is offered only when a
    /// listing of that directory shows the snapshot's contents. An empty listing, a refused one or
    /// no `ls` to ask leaves the fact unestablished (§56.3), and Appendix D.4's CLONE_AND_COPY
    /// materialises the snapshot as a clone instead; the reason travels on the plan.
    fn snapshot_directory_unreadable(&self, examination: &Examination) -> Option<String> {
        let mountpoint = examination.mount.mountpoint.as_deref().unwrap_or("");
        let root = format!(
            "{}/.zfs/snapshot/{}",
            mountpoint.trim_end_matches('/'),
            examination.short
        );
        if !self.runner.is_available(LS) {
            return Some(format!("`{LS}` is not present to read `{root}`"));
        }
        match self.runner.run(LS, &["-A", "--", &root]) {
            Ok(listing) if listing.succeeded() && !listing.stdout().trim().is_empty() => None,
            Ok(listing) if listing.succeeded() => Some(format!(
                "`{root}` lists nothing, so the snapshot is not mounted where a copy would read it"
            )),
            Ok(listing) => Some(format!(
                "`{root}` could not be listed: {}",
                listing.stderr().trim()
            )),
            Err(error) => Some(format!("`{root}` could not be listed: {error}")),
        }
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
                        listed: false,
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

    /// The free-space guard of Appendix D.3, against a share or a quantity (§53).
    ///
    /// A configured floor that cannot be compared — the pool's free space or, for a share, its size
    /// was not read — fails closed: a floor nobody measured the pool against is not met.
    fn space_guard(&self, layout: &Layout, dataset: &str) -> Result<(), ErrorValue> {
        if self.free_space_floor == FreeSpaceFloor::None {
            return Ok(());
        }
        let pool_name = dataset.split('/').next().unwrap_or(dataset);
        let pool = layout.pool(pool_name);
        let size = pool.and_then(|pool| pool.size);
        let free = pool.and_then(|pool| pool.free);
        match (free, self.free_space_floor.minimum_bytes(size)) {
            (Some(free), Some(floor)) if free >= floor => Ok(()),
            (free, _) => Err(storage_pressure(
                pool_name,
                &free.map_or_else(
                    || "an unknown amount".to_owned(),
                    |free| ByteSize::from_bytes(free).to_string(),
                ),
                &self.free_space_floor.describe(size),
            )),
        }
    }

    /// Reads only the pools, for the guards that run before a protection action is planned and
    /// again before it is created: `zpool list` for size, space and state, `zpool status` for the
    /// data errors §13.1's health question also covers.
    fn pool_reading(&self) -> Result<Layout, ErrorValue> {
        self.require_tools()?;
        let listed = self.zpool(&[
            "list",
            "-H",
            "-p",
            "-o",
            "name,size,alloc,free,capacity,fragmentation,health",
        ])?;
        let status = self.zpool(&["status"])?;
        let pools = if listed.succeeded() {
            let pools = crate::layout::pools(ZPOOL, listed.stdout())?;
            if status.succeeded() {
                with_status(pools, status.stdout())
            } else {
                pools
            }
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

    /// §43.4's privilege fact: whether this process may `rollback` and `destroy` on `dataset`.
    ///
    /// Read-only queries succeeding proves only that ZFS lets this process look. Root may do
    /// everything; anyone else needs a `zfs allow` delegation of [`RESTORE_PERMISSIONS`] to the
    /// user or to everyone. Group delegations are not evaluated, and say so.
    fn privilege_evidence(
        &self,
        dataset: &str,
        queries_refused: bool,
    ) -> Result<(bool, String), ErrorValue> {
        if queries_refused {
            return Ok((
                false,
                "ZFS refused a query for want of privilege: the utilities must be run as root"
                    .to_owned(),
            ));
        }
        let Some((uid, user)) = self.identity() else {
            return Ok((
                false,
                "the effective uid of this process could not be read from /proc/self/status, so \
                 whether it may `rollback` and `destroy` is unknown"
                    .to_owned(),
            ));
        };
        if uid == 0 {
            return Ok((
                true,
                "the process runs with effective uid 0, and every ZFS query this plan rests on \
                 ran without a permission refusal"
                    .to_owned(),
            ));
        }
        let listed = self.zfs(&["allow", dataset])?;
        if !listed.succeeded() {
            return Ok((
                false,
                format!(
                    "the process runs as `{user}` (effective uid {uid}), not root, and ZFS \
                     refused to list the delegations on `{dataset}`: {}",
                    refusal_text(&listed)
                ),
            ));
        }
        let uid_text = uid.to_string();
        let granted: Vec<Arc<str>> = parse::delegations(listed.stdout(), dataset)
            .into_iter()
            .filter(|delegation| match &delegation.grantee {
                Grantee::User(name) => name.as_ref() == user.as_ref() || name.as_ref() == uid_text,
                Grantee::Everyone => true,
                Grantee::Group(_) => false,
            })
            .flat_map(|delegation| delegation.permissions)
            .collect();
        let missing: Vec<&str> = RESTORE_PERMISSIONS
            .into_iter()
            .filter(|needed| {
                !granted
                    .iter()
                    .any(|permission| permission.as_ref() == *needed)
            })
            .collect();
        Ok(if missing.is_empty() {
            (
                true,
                format!(
                    "the process runs as `{user}` (effective uid {uid}), and `zfs allow {dataset}` \
                     delegates {} to it",
                    RESTORE_PERMISSIONS.join(", ")
                ),
            )
        } else {
            (
                false,
                format!(
                    "the process runs as `{user}` (effective uid {uid}), not root, and `zfs allow \
                     {dataset}` does not delegate {} to this user or to everyone (group \
                     delegations are not evaluated)",
                    missing.join(", ")
                ),
            )
        })
    }

    /// The bookmarks a rollback to `reference` would take with it (Appendix D.5).
    ///
    /// A bookmark carries the GUID and `createtxg` of the snapshot it was taken from. One whose
    /// GUID belongs to a newer snapshot is in the way and one whose GUID belongs to the target or
    /// an older snapshot is not. One whose snapshot is gone is placed by `createtxg` against the
    /// target's — asked of ZFS only then — and `None` is the answer when that could not be read:
    /// §56.3 refuses rather than guessing which side of the recovery point it lies on.
    fn affected_bookmarks(
        &self,
        layout: &Layout,
        dataset: &str,
        reference: &str,
        snapshot_known: bool,
    ) -> Result<Option<Vec<Arc<str>>>, ErrorValue> {
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
        let mut affected = Vec::new();
        let mut orphans = Vec::new();
        for bookmark in layout.bookmarks_of(dataset) {
            if newer.contains(&bookmark.guid.as_ref()) {
                affected.push(Arc::clone(&bookmark.name));
            } else if !at_or_before.contains(&bookmark.guid.as_ref()) {
                orphans.push(Arc::clone(&bookmark.name));
            }
        }
        if orphans.is_empty() {
            return Ok(Some(affected));
        }
        if !snapshot_known {
            return Ok(None);
        }
        let mut argv: Vec<&str> = vec![
            "get",
            "-H",
            "-p",
            "-o",
            "name,property,value",
            "createtxg",
            reference,
        ];
        argv.extend(orphans.iter().map(AsRef::as_ref));
        let placed = self.zfs(&argv)?;
        if !placed.succeeded() {
            return Ok(None);
        }
        let Ok(rows) = parse::properties(ZFS, placed.stdout()) else {
            return Ok(None);
        };
        let Some(target) = parse::property_of(&rows, reference, "createtxg")
            .and_then(|entry| parse::number(&entry.value))
        else {
            return Ok(None);
        };
        for orphan in orphans {
            match parse::property_of(&rows, &orphan, "createtxg")
                .and_then(|entry| parse::number(&entry.value))
            {
                Some(txg) if txg > target => affected.push(orphan),
                Some(_) => {}
                None => return Ok(None),
            }
        }
        Ok(Some(affected))
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
    /// `acceptance` is what the operator accepted for this run (§24.5). The last fact — explicit
    /// acceptance for history destruction — is established by it and by nothing else, unless a
    /// rollback would neither destroy an object nor discard a byte: an enumeration of what would
    /// be lost is what an acceptance covers, and is not the acceptance.
    ///
    /// # Errors
    ///
    /// A structured error when a program could not be run at all. A program that ran and refused
    /// is a fact this provider failed to establish, which is a checklist entry rather than an
    /// error: §56.3's block is raised by the caller that wanted a destructive path.
    pub fn safety_checklist(
        &self,
        asset: &RecoveryAsset,
        acceptance: &RestoreAcceptance,
    ) -> Result<SafetyChecklist, ErrorValue> {
        Ok(self.examine(asset, acceptance)?.checklist)
    }

    /// Reads everything a recovery plan over `asset` needs, and scores §56.1's checklist.
    fn examine(
        &self,
        asset: &RecoveryAsset,
        acceptance: &RestoreAcceptance,
    ) -> Result<Examination, ErrorValue> {
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
        // Appendix D.5: what a rollback discards is everything written since the recovery point.
        // Plain `written` counts only since the dataset's newest snapshot, which is that point
        // only when nothing newer exists; past newer snapshots the figure is `written@<snapshot>`.
        let written_property = match reference.split_once('@') {
            Some((_, short)) if !layout.newer_snapshots(&reference).is_empty() => {
                format!("written@{short}")
            }
            _ => "written".to_owned(),
        };
        let written = self.zfs(&[
            "get",
            "-H",
            "-p",
            "-o",
            "name,property,value",
            &written_property,
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

        let refused = Self::refused_for_privilege(&[&clones, &written, &space, &placement]);
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
            parse::number(&parse::property_of(&listed, &dataset_name, &written_property)?.value)
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
        // §13.6: `rollback -R` destroys the clones of the snapshots it destroys — the newer ones.
        // A clone of the target itself is untouched by a rollback to it.
        let mut newer_clones: Vec<(Arc<str>, Arc<str>)> = Vec::new();
        for newer in layout.newer_snapshots(&reference) {
            for clone in layout.clones_of(&newer.name) {
                if !newer_clones.iter().any(|(seen, _)| *seen == clone.name) {
                    newer_clones.push((Arc::clone(&clone.name), Arc::clone(&newer.name)));
                }
            }
        }
        let mut own_clones: Vec<Arc<str>> = clone_names.clone().unwrap_or_default();
        for clone in layout.clones_of(&reference) {
            if !own_clones.contains(&clone.name) {
                own_clones.push(Arc::clone(&clone.name));
            }
        }
        let affected_bookmarks =
            self.affected_bookmarks(&layout, &dataset_name, &reference, snapshot.is_some())?;
        let children: Vec<Arc<str>> = layout
            .descendants_of(&dataset_name)
            .into_iter()
            .map(|child| Arc::clone(&child.name))
            .collect();
        let privilege = self.privilege_evidence(&dataset_name, refused)?;

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
                    .filter(|found| status.filesystems && found.listed)
                    .map(|found| {
                        format!(
                            "`zfs list -t filesystem` reports the dataset `{}` at `{}`",
                            found.name, found.mountpoint
                        )
                    }),
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
                    format!(
                        "`zfs get {written_property}` reports {bytes} bytes written since \
                         `{reference}`"
                    )
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

        checklist = if status.origins && clone_names.is_some() {
            checklist.established(
                ZfsFact::AffectedClones,
                if newer_clones.is_empty() {
                    format!(
                        "`zfs list -o name,origin` shows no clone of the {} newer snapshot(s), \
                         which are what `-R` would take; {} clone(s) of `{reference}` itself are \
                         untouched by a rollback to it",
                        newer_snapshots.len(),
                        own_clones.len()
                    )
                } else {
                    format!(
                        "the clone(s) {} depend on newer snapshot(s) a rollback must destroy",
                        newer_clones
                            .iter()
                            .map(|(clone, origin)| format!("`{clone}` (of `{origin}`)"))
                            .collect::<Vec<String>>()
                            .join(", ")
                    )
                },
            )
        } else {
            checklist.missing(
                ZfsFact::AffectedClones,
                "`zfs get clones` or `zfs list -o name,origin` did not answer, so the clones a \
                 destructive flag would take are unknown",
            )
        };

        checklist = if status.order
            && status.bookmarks
            && layout.has_creation_order()
            && snapshot.is_some()
            && affected_bookmarks.is_some()
        {
            checklist.established(
                ZfsFact::NewerSnapshotsAndBookmarks,
                format!(
                    "{} newer snapshot(s) and {} affected bookmark(s) were enumerated",
                    newer_snapshots.len(),
                    affected_bookmarks.as_ref().map_or(0, Vec::len)
                ),
            )
        } else {
            checklist.missing(
                ZfsFact::NewerSnapshotsAndBookmarks,
                "the snapshot and bookmark listings did not together establish what rollback \
                 would destroy, or a bookmark whose snapshot is gone could not be placed by its \
                 createtxg",
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

        let (privileged, privilege_detail) = privilege;
        checklist = if privileged {
            checklist.established(ZfsFact::SufficientPrivilege, privilege_detail)
        } else {
            checklist.missing(ZfsFact::SufficientPrivilege, privilege_detail)
        };

        let enumeration_complete = checklist.is_established(ZfsFact::LatestRelevantSnapshot)
            && checklist.is_established(ZfsFact::NewerSnapshotsAndBookmarks)
            && checklist.is_established(ZfsFact::AffectedClones);
        let newer_clone_names: Vec<Arc<str>> = newer_clones
            .iter()
            .map(|(clone, _)| Arc::clone(clone))
            .collect();
        let destroyed = destroyed_objects(
            &newer_snapshots,
            affected_bookmarks.as_deref().unwrap_or(&[]),
            &newer_clone_names,
        );
        let discarded = written_bytes
            .filter(|bytes| *bytes > 0)
            .map_or_else(String::new, |bytes| {
                format!(" and {bytes} bytes written since the snapshot")
            });
        checklist = if !enumeration_complete {
            checklist.missing(
                ZfsFact::HistoryDestructionAccepted,
                "what acceptance would cover could not be enumerated, so it cannot be explicit",
            )
        } else if destroyed.is_empty() && written_bytes == Some(0) {
            checklist.established(
                ZfsFact::HistoryDestructionAccepted,
                "a rollback to this snapshot destroys no newer snapshot, bookmark or clone and \
                 `zfs get written` reports nothing written since, so there is nothing to accept",
            )
        } else if acceptance.accepts_newer_state_loss() {
            checklist.established(
                ZfsFact::HistoryDestructionAccepted,
                format!(
                    "the operator accepted losing {} object(s){discarded} \
                     (`--accept-newer-state-loss`)",
                    destroyed.len()
                ),
            )
        } else {
            checklist.missing(
                ZfsFact::HistoryDestructionAccepted,
                format!(
                    "{} object(s) would be destroyed{discarded}, and losing them has not been \
                     accepted (`--accept-newer-state-loss`)",
                    destroyed.len()
                ),
            )
        };

        Ok(Examination {
            reference: Arc::from(reference.as_str()),
            dataset_name: Arc::from(dataset_name.as_str()),
            dataset,
            snapshot,
            short,
            newer_snapshots,
            affected_bookmarks: affected_bookmarks.unwrap_or_default(),
            newer_clones,
            own_clones,
            children,
            mount,
            root_case,
            written: written_bytes,
            used_by_snapshots,
            checklist,
            enumeration_complete,
            layout,
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
    /// Clones of newer snapshots, each with the snapshot it depends on (§13.6).
    newer_clones: Vec<(Arc<str>, Arc<str>)>,
    /// Clones of the recovery point itself, which no method here touches.
    own_clones: Vec<Arc<str>>,
    children: Vec<Arc<str>>,
    mount: MountState,
    root_case: Option<RootDatasetCase>,
    written: Option<u128>,
    used_by_snapshots: Option<u128>,
    checklist: SafetyChecklist,
    /// Whether what an acceptance would cover was enumerated, so it can be deferred to apply.
    enumeration_complete: bool,
    layout: Layout,
}

impl Examination {
    /// Everything a full rollback would destroy, in the order §13.6 enumerates them.
    fn destroyed(&self) -> Vec<Arc<str>> {
        let clones: Vec<Arc<str>> = self
            .newer_clones
            .iter()
            .map(|(clone, _)| Arc::clone(clone))
            .collect();
        destroyed_objects(&self.newer_snapshots, &self.affected_bookmarks, &clones)
    }

    /// The newer snapshots and bookmarks a rollback plan destroys by name, one action each.
    fn history(&self) -> Vec<Arc<str>> {
        destroyed_objects(&self.newer_snapshots, &self.affected_bookmarks, &[])
    }

    /// The GUID ZFS reports now for a snapshot or bookmark of this pool.
    fn guid_of(&self, object: &str) -> Option<Arc<str>> {
        self.layout
            .snapshot(object)
            .map(|snapshot| Arc::clone(&snapshot.guid))
            .or_else(|| {
                self.layout
                    .bookmarks()
                    .iter()
                    .find(|bookmark| bookmark.name.as_ref() == object)
                    .map(|bookmark| Arc::clone(&bookmark.guid))
            })
    }

    /// The temporary clone Appendix D.4's CLONE_AND_COPY materialises, and where it is mounted.
    fn temporary_clone(&self) -> (String, String) {
        let pool = self
            .dataset_name
            .split('/')
            .next()
            .unwrap_or(&self.dataset_name);
        (
            format!("{pool}/ono-restore-{}", self.short),
            format!("{CLONE_MOUNT_ROOT}/{}", self.short),
        )
    }

    /// Where `object` lies inside this dataset, as a path relative to its mountpoint — or why
    /// this snapshot does not hold it (§13.4).
    ///
    /// Beneath the mountpoint is necessary and not sufficient: a child dataset, or any other
    /// filesystem, mounted inside it is a boundary of its own, and what lives there is not in the
    /// parent's snapshot however the path reads.
    fn placement_of(&self, object: &str) -> Result<String, String> {
        let outside = || {
            format!(
                "v0.6 §13.4: `{object}` is not inside the dataset `{}` that this snapshot holds, \
                 so this asset cannot put it back",
                self.dataset_name
            )
        };
        if !object.starts_with('/') || object.split('/').any(|part| part == "." || part == "..") {
            return Err(outside());
        }
        let Some(mountpoint) = self
            .mount
            .mountpoint
            .as_deref()
            .filter(|mountpoint| mountpoint.starts_with('/'))
        else {
            return Err(outside());
        };
        if !is_beneath(object, mountpoint) {
            return Err(outside());
        }
        if let Some(holder) = self.foreign_holder(object, mountpoint) {
            return Err(format!(
                "v0.6 §13.4: `{object}` lives in `{holder}`, which is mounted inside `{}` and is a \
                 boundary of its own, so the snapshot `{}` does not hold it",
                self.dataset_name, self.reference
            ));
        }
        Ok(object
            .get(mountpoint.len()..)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_owned())
    }

    /// The filesystem other than this dataset that holds `path`, where one is mounted inside it.
    fn foreign_holder(&self, path: &str, own: &str) -> Option<Arc<str>> {
        let mounted = self
            .layout
            .mounts()
            .mounts()
            .iter()
            .filter(|mount| {
                mount.mount_point.as_ref() != own
                    && is_beneath(&mount.mount_point, own)
                    && is_beneath(path, &mount.mount_point)
                    && !(mount.kind() == ono_change_core::FilesystemKind::Zfs
                        && mount.source.as_ref() == self.dataset_name.as_ref())
            })
            .map(|mount| (mount.mount_point.len(), Arc::clone(&mount.source)));
        let placed = self
            .layout
            .descendants_of(&self.dataset_name)
            .into_iter()
            .filter(|child| {
                child.mounted
                    && child.has_placed_mountpoint()
                    && child.mountpoint.as_ref() != own
                    && is_beneath(path, &child.mountpoint)
            })
            .map(|child| (child.mountpoint.len(), Arc::clone(&child.name)));
        mounted
            .chain(placed)
            .max_by_key(|(length, _)| *length)
            .map(|(_, holder)| holder)
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

/// Why a pool is not fit for §13.1's "proposed protection operation", where it is not.
///
/// `ONLINE` with `No known data errors` is fit. Anything else — DEGRADED, SUSPENDED, FAULTED, a
/// pool reporting data errors, a pool whose `errors:` line or listing was not read — is a reason,
/// and §56.3 makes an unread one a reason too.
fn pool_unfitness(pool: Option<&Pool>, name: &str) -> Option<String> {
    let Some(pool) = pool else {
        return Some(format!(
            "the health of pool `{name}` could not be read: `zpool list` did not report it"
        ));
    };
    if pool.health.as_ref() != "ONLINE" {
        return Some(format!("the pool `{name}` is {}, not healthy", pool.health));
    }
    match pool.errors.as_deref() {
        None => Some(format!(
            "the health of pool `{name}` could not be read: `zpool status` gave no `errors:` line \
             for it, so whether it holds data errors is unknown"
        )),
        Some(_) if pool.is_healthy() => None,
        Some(errors) => Some(format!("the pool `{name}` reports `{errors}`, not healthy")),
    }
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
        // §13.4: the datasets an operator may take for coverage — an ancestor by name, or the
        // dataset mounted above this one, as in §13.4's own `/` over `/data`. Mount points come
        // from the mount table, never from a path's shape (Appendix B.8).
        let placed = |name: &str| {
            layout
                .mounts()
                .of_dataset(name)
                .map(|mount| Arc::clone(&mount.mount_point))
        };
        let own = placed(&dataset.name);
        let enclosing: Vec<Arc<str>> = layout
            .datasets()
            .iter()
            .filter(|other| other.name != dataset.name)
            .filter(|other| {
                crate::layout::is_descendant(&dataset.name, &other.name)
                    || matches!(
                        (own.as_deref(), placed(&other.name)),
                        (Some(inner), Some(outer)) if crate::layout::is_beneath(inner, &outer)
                    )
            })
            .map(|other| Arc::clone(&other.name))
            .collect();
        let pool_note = match pool_unfitness(layout.pool(dataset.pool()), dataset.pool()) {
            None => layout
                .pool(dataset.pool())
                .map_or_else(String::new, |pool| {
                    format!(
                        "the pool `{}` is {} with {} fragmentation",
                        pool.name,
                        pool.health,
                        pool.fragmentation
                            .map_or_else(|| "unknown".to_owned(), |value| format!("{value}%"))
                    )
                }),
            // §13.1 and §56.3: the reason protection is blocked travels with the candidate, so a
            // plan shows why no snapshot will be made rather than failing without one.
            Some(reason) => format!(
                "v0.6 §13.1 and §56.3: {reason}, so no snapshot is created on it until it is \
                 healthy"
            ),
        };

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
        exact = exact.needing_to_create(pool_note.clone());
        for object in &enclosing {
            exact = exact.outside_of(Arc::clone(object));
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
                    "snapshot {}@{part} and {} descendant dataset(s) inside the tree, named in one \
                     atomic creation",
                    dataset.name,
                    in_tree.len()
                ),
            )
            .at_consistency(ConsistencyClass::FilesystemConsistent)
            .restored_by(RestoreMethod::SelectiveFileRestore)
            .costing(RecoveryCost::unknown())
            .needing_to_create(
                "v0.6 §13.3: one atomic `zfs snapshot` naming each dataset, recorded as one asset \
                 per dataset",
            )
            .needing_to_create(pool_note.clone())
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
            for object in &enclosing {
                recursive = recursive.outside_of(Arc::clone(object));
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
                let pool = dataset.split('/').next().unwrap_or(dataset);
                if let Some(reason) = pool_unfitness(pools.pool(pool), pool)
                    && !mode.refuses_shortfall()
                {
                    // §13.1: a snapshot is not made on a pool that is not healthy. `require`
                    // still plans, and `create` refuses, exactly as Appendix D.3 does for space.
                    return Err(asset_create_failed(PROVIDER_ID, dataset, &reason));
                }
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
                    let names: Vec<String> = datasets
                        .iter()
                        .map(|named| full_name(named, &part).to_string())
                        .collect();
                    format!(
                        "zfs snapshot {} — one atomic creation over {} datasets, recorded \
                         individually (§13.3, Appendix D.1)",
                        names.join(" "),
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
        // §13.1 and Appendix D.3: `require` plans through an unhealthy pool and storage pressure,
        // and fails here instead.
        let pools = self.pool_reading()?;
        let pool = dataset.split('/').next().unwrap_or(&dataset);
        if let Some(reason) = pool_unfitness(pools.pool(pool), pool) {
            return Err(asset_create_failed(PROVIDER_ID, &dataset, &reason));
        }
        self.space_guard(&pools, &dataset)?;

        // §13.3: several datasets at one point are named, each of them, in one `zfs snapshot`,
        // which ZFS creates atomically. `-r` would also snapshot every child outside the tree
        // and every zvol beneath, which no asset records and no cleanup would ever remove.
        let datasets = covered_datasets(action.candidate());
        let together =
            datasets.len() > 1 && top_dataset(&datasets).as_deref() == Some(dataset.as_str());
        let names: Vec<Arc<str>> = match generated {
            Some(part) if together => datasets
                .iter()
                .map(|named| full_name(named, part))
                .collect(),
            _ => vec![Arc::from(reference.as_str())],
        };
        let mut argv: Vec<&str> = vec!["snapshot"];
        argv.extend(names.iter().map(AsRef::as_ref));
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
        let examination = self.examine(asset, &RestoreAcceptance::none())?;
        // §24.5: the operator accepts at apply, after seeing this plan, so acceptance is the one
        // fact a plan is built without — provided what it would cover was enumerated.
        // `restore_with` holds the act itself to all twelve.
        let deferred: &[ZfsFact] = if examination.enumeration_complete {
            &[ZfsFact::HistoryDestructionAccepted]
        } else {
            &[]
        };
        // §56.3: the goal decides whether the whole checklist applies, before a method is chosen,
        // so a fact that is missing blocks rather than quietly narrowing the choice of method.
        let goal_is_destructive = matches!(goal, RecoveryGoal::RestoreDomain);
        if let Some(blocked) = examination
            .checklist
            .blocking_error_deferring(goal_is_destructive, deferred)
        {
            return Err(blocked);
        }

        let rollback = match examination.root_case {
            Some(_) => RestoreMethod::OfflineRootRecovery,
            None => RestoreMethod::DatasetRollback,
        };
        let destroyed = examination.destroyed();
        // §53 and §13.6: while destructive rollback is not allowed, a rollback that would destroy
        // history — or that could not be shown to destroy none — is not offered at all.
        let rollback_forbidden = !self.allow_destructive_rollback
            && (!examination.enumeration_complete || !destroyed.is_empty());
        let mut available = vec![RestoreMethod::CloneAndCopy];
        let set_aside = if examination.selective_is_possible() {
            let unreadable = self.snapshot_directory_unreadable(&examination);
            if unreadable.is_none() {
                available.push(RestoreMethod::SelectiveFileRestore);
            }
            unreadable
        } else {
            None
        };
        if !rollback_forbidden {
            available.push(rollback);
        }
        let ordinary = choose_method(goal, &available);
        // §53's `prefer_selective_restore = false`, read through Appendix C.1's metadata
        // exception: a dataset rollback, fully proven, ahead of a file restore.
        let rollback_preferred = !self.prefer_selective_restore
            && !rollback_forbidden
            && rollback == RestoreMethod::DatasetRollback
            && choose_method(goal, &[rollback]).is_some()
            && examination
                .checklist
                .blocking_error_deferring(true, deferred)
                .is_none();
        let preference = if rollback_preferred && ordinary != Some(rollback) {
            format!(
                ". Chosen ahead of {} because `recovery.zfs.prefer_selective_restore` is false: \
                 Appendix C.1 lets a lower method stand where the upper ones cannot return \
                 required metadata, and a file copy returns no hard links, file capabilities or \
                 SELinux labels",
                ordinary.map_or("a file restore", RestoreMethod::as_str)
            )
        } else {
            String::new()
        };
        let chosen = if rollback_preferred {
            Some(rollback)
        } else {
            ordinary
        };
        let Some(method) = chosen else {
            if rollback_forbidden && choose_method(goal, &[rollback]).is_some() {
                return Err(destructive_rollback_forbidden(&examination, &destroyed));
            }
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
            && let Some(blocked) = examination
                .checklist
                .blocking_error_deferring(true, deferred)
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
        // §13.4: an object beneath the mountpoint may still live in a child dataset, or on some
        // other filesystem mounted inside this one, and neither is in this snapshot.
        let mut restorable: Vec<(Arc<str>, String)> = Vec::new();
        let mut excluded: Vec<(Arc<str>, String)> = Vec::new();
        for object in &objects {
            match examination.placement_of(object) {
                Ok(relative) => restorable.push((Arc::clone(object), relative)),
                Err(reason) => excluded.push((Arc::clone(object), reason)),
            }
        }

        let mut fragment = RecoveryPlanFragment::new(PROVIDER_ID, method);
        let mut ordinal = 0;

        match method {
            RestoreMethod::SelectiveFileRestore => {
                let mountpoint = examination.mount.mountpoint.as_deref().unwrap_or("");
                for (object, _) in &restorable {
                    match ZfsProvider::snapshot_path(mountpoint, &examination.short, object) {
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
                        None => excluded.push((
                            Arc::clone(object),
                            format!(
                                "v0.6 §13.5: `{object}` has no path inside the snapshot directory \
                                 of `{}`",
                                examination.dataset_name
                            ),
                        )),
                    }
                }
            }
            RestoreMethod::CloneAndCopy => {
                let (clone_name, clone_mount) = examination.temporary_clone();
                fragment = fragment.acting(
                    program_action(
                        &plan_id,
                        ordinal,
                        ActionRole::Prepare,
                        format!(
                            "zfs clone {} {clone_name} — Appendix D.4's CLONE_AND_COPY \
                             materialises the snapshot beside the live dataset rather than over \
                             it{}",
                            examination.reference,
                            set_aside.as_deref().map_or_else(String::new, |reason| format!(
                                ". §13.5's selective restore was set aside: {reason}"
                            ))
                        ),
                        ZFS,
                        vec![
                            Arc::from("clone"),
                            Arc::from("-o"),
                            Arc::from(format!("mountpoint={clone_mount}")),
                            Arc::clone(&examination.reference),
                            Arc::from(clone_name.as_str()),
                        ],
                    )
                    .requiring(snapshot_precondition(&examination))
                    .declaring(
                        EffectDomain::FilesystemPersistent,
                        EffectKind::Create,
                        EffectConfidence::Guaranteed,
                        clone_name.as_str(),
                        "a temporary clone of the snapshot is created beside the live dataset (Appendix D.4)",
                    ),
                );
                ordinal += 1;
                for (object, relative) in &restorable {
                    let source_path = format!("{clone_mount}/{relative}");
                    fragment = fragment.acting(file_restore_action(
                        &plan_id,
                        ordinal,
                        &source_path,
                        object,
                        examination.snapdir(),
                    ));
                    ordinal += 1;
                }
                fragment = fragment.acting(
                    program_action(
                        &plan_id,
                        ordinal,
                        ActionRole::Cleanup,
                        format!("zfs destroy {clone_name} — the clone is temporary (§37)"),
                        ZFS,
                        vec![Arc::from("destroy"), Arc::from(clone_name.as_str())],
                    )
                    .declaring(
                        EffectDomain::FilesystemPersistent,
                        EffectKind::Remove,
                        EffectConfidence::Guaranteed,
                        clone_name.as_str(),
                        "the temporary clone is destroyed (§37)",
                    ),
                );
                ordinal += 1;
            }
            RestoreMethod::DatasetRollback | RestoreMethod::OfflineRootRecovery => {
                // §13.6: every destruction is a named action of its own. Ono adds no flag that
                // removes newer history as a side effect of the rollback, so what would be lost
                // is visible object by object in the plan an operator accepts — and each action
                // carries the GUID of the object the operator saw, so a name that came back as
                // something else is not destroyed on the strength of that acceptance.
                for object in examination.history() {
                    let guid = examination.guid_of(&object);
                    fragment = fragment.acting(
                        program_action(
                            &plan_id,
                            ordinal,
                            ActionRole::Recover,
                            format!(
                                "zfs destroy {object} — §13.6: this stands in the rollback's way, \
                                 and destroying it needs explicit acceptance"
                            ),
                            ZFS,
                            vec![Arc::from("destroy"), Arc::clone(&object)],
                        )
                        .requiring(guid_precondition(&object, guid.as_deref()))
                        .declaring(
                            EffectDomain::FilesystemPersistent,
                            EffectKind::Remove,
                            EffectConfidence::Guaranteed,
                            Arc::clone(&object),
                            "the snapshot or bookmark is destroyed; §13.6's acceptance is what permits it",
                        ),
                    );
                    ordinal += 1;
                }
                fragment = fragment.acting(
                    program_action(
                        &plan_id,
                        ordinal,
                        ActionRole::Recover,
                        format!(
                            "zfs rollback {} — §13.6: the whole dataset returns to this point and \
                             everything written since is discarded{preference}",
                            examination.reference
                        ),
                        ZFS,
                        vec![Arc::from("rollback"), Arc::clone(&examination.reference)],
                    )
                    .requiring(snapshot_precondition(&examination))
                    .declaring(
                        EffectDomain::FilesystemPersistent,
                        EffectKind::Replace,
                        EffectConfidence::Guaranteed,
                        Arc::clone(&examination.dataset_name),
                        "the dataset returns to the snapshot and everything written since is discarded (§13.6)",
                    ),
                );
                ordinal += 1;
            }
            _ => {}
        }
        let _ = ordinal;

        for (object, reason) in excluded {
            fragment = fragment.leaving(UnrecoverableEffect::new(
                object,
                EffectDomain::FilesystemPersistent,
                reason,
            ));
        }
        if method.discards_newer_state() {
            for (clone, origin) in &examination.newer_clones {
                fragment = fragment.leaving(UnrecoverableEffect::new(
                    Arc::clone(clone),
                    EffectDomain::FilesystemPersistent,
                    format!(
                        "v0.6 §13.6: `{clone}` is a clone of the newer snapshot `{origin}`, which \
                         this rollback has to destroy. ZFS refuses while the clone exists, and Ono \
                         does not add the flag that would destroy it"
                    ),
                ));
            }
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
        self.carry_out(action, asset, &RestoreAcceptance::none())
    }

    /// Carries out one action of a plan this provider built, re-proving what it rests on.
    ///
    /// Every action of a ZFS recovery plan arrives here, whatever built it. Each is recognised
    /// against a fresh reading as exactly one of the shapes [`RecoveryProvider::plan_recovery`]
    /// emits — anything else is refused before it runs — and each is held, at the moment of the
    /// act, to the §56.1 facts it depends on:
    ///
    /// - no `-r` or `-R` in any spelling, whatever was accepted (§13.6);
    /// - every act needs the snapshot to exist, be the one recorded, and be readable (§56.3);
    /// - destroying a newer snapshot or bookmark, and the rollback itself, need all twelve facts,
    ///   the operator's `acceptance`, no clone of a newer snapshot in the way, and a dataset that
    ///   is not a running root or boot environment (§13.6, §13.7, §24.5);
    /// - a destroy names an object the reading enumerates, with the GUID the plan recorded.
    fn restore_with(
        &self,
        action: &PlanAction,
        asset: &RecoveryAsset,
        acceptance: &RestoreAcceptance,
    ) -> Result<RestoreOutcome, ErrorValue> {
        self.carry_out(action, asset, acceptance)
            .map(|()| RestoreOutcome::default())
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
    .declaring(
        EffectDomain::FilesystemPersistent,
        EffectKind::Replace,
        EffectConfidence::Guaranteed,
        object,
        "the live file is replaced by the snapshot's copy of it (§13.5)",
    )
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
    guid_precondition(
        &examination.reference,
        examination
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.guid.as_ref()),
    )
}

/// The precondition that ties an action to the snapshot or bookmark `object` as it was read.
fn guid_precondition(object: &str, guid: Option<&str>) -> Precondition {
    Precondition::new(
        PreconditionKind::Existence,
        object,
        "guid",
        Value::string(guid.unwrap_or("")),
    )
    .explained(
        "v0.6 §56.1: a snapshot or bookmark destroyed and recreated under the same name is a \
         different object, and only the GUID says so",
    )
}

/// Refuses when a GUID an action recorded is not the GUID ZFS reports now (§56.1).
fn check_guid_preconditions(
    action: &PlanAction,
    examination: &Examination,
) -> Result<(), ErrorValue> {
    for precondition in action
        .preconditions()
        .iter()
        .filter(|precondition| precondition.field() == "guid")
    {
        let now = examination.guid_of(precondition.subject());
        if now
            .as_deref()
            .is_some_and(|guid| precondition.expected() == &Value::string(guid))
        {
            continue;
        }
        return Err(precondition_failed(
            action.id(),
            precondition.subject(),
            &format!(
                "v0.6 §56.1: the plan recorded GUID {} for `{}`, and ZFS now reports {}, so this \
                 is not the object the plan was built over.",
                precondition.expected(),
                precondition.subject(),
                now.map_or_else(|| "no such object".to_owned(), |guid| guid.to_string()),
            ),
        ));
    }
    Ok(())
}

/// What one recovery action is, once recognised as a shape this provider's plans emit.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Act {
    /// `cp` out of the snapshot directory or the temporary clone (§13.5, Appendix D.4).
    Copy,
    /// `zfs clone` of the recovery point as the temporary clone.
    Clone,
    /// `zfs destroy` of a newer snapshot or bookmark the reading enumerates (§13.6).
    DestroyHistory(Arc<str>),
    /// `zfs destroy` of the temporary clone, and whether it is still there.
    DestroyTemporaryClone { present: bool },
    /// `zfs rollback` to the recovery point.
    Rollback,
}

/// Recognises `program` and `words` as one of the shapes `plan_recovery` emits, against a fresh
/// reading, or says why it is not one.
fn recognise(program: &str, words: &[&str], examination: &Examination) -> Result<Act, String> {
    let (clone_name, clone_mount) = examination.temporary_clone();
    let reference = examination.reference.as_ref();
    if program == CP {
        let [
            "--preserve=all",
            "--no-dereference",
            "--no-target-directory",
            "--",
            source,
            target,
        ] = words
        else {
            return Err(
                "a restore copies with `--preserve=all --no-dereference --no-target-directory`, \
                 a source and a target, and this copy is not that"
                    .to_owned(),
            );
        };
        let relative = examination.placement_of(target)?;
        let mountpoint = examination.mount.mountpoint.as_deref().unwrap_or("");
        if ZfsProvider::snapshot_path(mountpoint, &examination.short, target).as_deref()
            == Some(*source)
        {
            return Ok(Act::Copy);
        }
        if *source == format!("{clone_mount}/{relative}") {
            return match examination.layout.dataset(&clone_name) {
                Some(clone)
                    if clone.origin.as_deref() == Some(reference)
                        && clone.mountpoint.as_ref() == clone_mount =>
                {
                    Ok(Act::Copy)
                }
                _ => Err(format!(
                    "`{source}` is read through the temporary clone `{clone_name}`, and ZFS does \
                     not report that clone of `{reference}` mounted at `{clone_mount}`"
                )),
            };
        }
        return Err(format!(
            "`{source}` is neither `{target}` inside the snapshot `{reference}` nor inside its \
             temporary clone, so this copy does not restore from this asset"
        ));
    }
    if program != ZFS {
        return Err(format!(
            "`{program}` is not a program a ZFS recovery plan runs"
        ));
    }
    let mountpoint_option = format!("mountpoint={clone_mount}");
    match words {
        ["rollback", target] if *target == reference => Ok(Act::Rollback),
        ["clone", "-o", option, origin, name]
            if *option == mountpoint_option && *origin == reference && *name == clone_name =>
        {
            Ok(Act::Clone)
        }
        ["destroy", object]
            if examination
                .history()
                .iter()
                .any(|named| named.as_ref() == *object) =>
        {
            Ok(Act::DestroyHistory(Arc::from(*object)))
        }
        ["destroy", object] if *object == clone_name => match examination.layout.dataset(object) {
            None => Ok(Act::DestroyTemporaryClone { present: false }),
            Some(clone) if clone.origin.as_deref() == Some(reference) => {
                Ok(Act::DestroyTemporaryClone { present: true })
            }
            Some(_) => Err(format!(
                "`{object}` is not a clone of `{reference}`, so it is not this recovery's \
                     temporary clone and is not destroyed"
            )),
        },
        ["destroy", object] => Err(format!(
            "`{object}` is not a newer snapshot or bookmark this recovery enumerated, nor its \
             temporary clone, so no acceptance covers destroying it"
        )),
        _ => Err(
            "this is not an action a ZFS recovery plan emits, and the provider runs only those"
                .to_owned(),
        ),
    }
}

/// The first argument carrying `-r` or `-R`, in any spelling (§13.6).
///
/// A short-option cluster is read letter by letter, so `-rR`, `-Rf` and `-fr` are all caught;
/// `--recursive` is the long spelling. Operands after `--` are not options.
fn recursive_flag<'a>(words: &[&'a str]) -> Option<&'a str> {
    words
        .iter()
        .copied()
        .take_while(|word| *word != "--")
        .find(|word| {
            *word == "--recursive"
                || (word.starts_with('-')
                    && !word.starts_with("--")
                    && word
                        .chars()
                        .skip(1)
                        .any(|letter| letter == 'r' || letter == 'R'))
        })
}

/// Refuses a destructive act §56.1, §24.5, §13.6 and §13.7 do not permit right now.
///
/// §24.5's gate comes first: an operator is shown everything the recovery would take away before
/// being told what else it needs. §13.7's requirement is refused before anything is destroyed on
/// behalf of a rollback that cannot then be carried out.
fn guard_destruction(examination: &Examination, allowed: bool) -> Result<(), ErrorValue> {
    if let Some(blocked) = examination
        .checklist
        .blocking_error_deferring(true, &[ZfsFact::HistoryDestructionAccepted])
    {
        return Err(blocked);
    }
    // §53 before §24.5: the operator allows destructive rollback first, and accepts the loss
    // after. An acceptance given while the setting is false does not stand in for it.
    let destroyed = examination.destroyed();
    if !allowed && (!destroyed.is_empty() || !examination.enumeration_complete) {
        return Err(recovery_apply_failed(
            &format!("zfs rollback {}", examination.reference),
            &format!(
                "v0.6 §13.6 and §53: `{ALLOW_DESTRUCTIVE_ROLLBACK}` is false, and this rollback \
                 destroys newer history. Nothing was destroyed"
            ),
        )
        .with_metadata("setting", Value::string(ALLOW_DESTRUCTIVE_ROLLBACK))
        .with_metadata(
            "destroyed",
            Value::list(destroyed.iter().map(|object| Value::string(object))),
        ));
    }
    if !examination
        .checklist
        .is_established(ZfsFact::HistoryDestructionAccepted)
    {
        let mut destroyed: Vec<String> = examination
            .destroyed()
            .iter()
            .map(|object| object.to_string())
            .collect();
        if destroyed.is_empty() {
            destroyed.push(examination.dataset_name.to_string());
        }
        return Err(destructive_history_not_accepted(&destroyed));
    }
    if !examination.newer_clones.is_empty() {
        let clones: Vec<&str> = examination
            .newer_clones
            .iter()
            .map(|(clone, _)| clone.as_ref())
            .collect();
        return Err(recovery_apply_failed(
            &format!("zfs rollback {}", examination.reference),
            &format!(
                "v0.6 §13.6: the clone(s) {} depend on newer snapshots this rollback must destroy. \
                 ZFS refuses while they exist, and Ono never adds `-R`; promote or remove them \
                 first. Nothing was destroyed",
                clones.join(", ")
            ),
        )
        .with_metadata(
            "clones",
            Value::list(clones.iter().map(|clone| Value::string(clone))),
        ));
    }
    if let Some(case) = examination.root_case {
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
    Ok(())
}

/// The refusal for a goal only a rollback achieves, while §53 does not allow the destruction.
fn destructive_rollback_forbidden(examination: &Examination, destroyed: &[Arc<str>]) -> ErrorValue {
    let what = if destroyed.is_empty() {
        "and what it would destroy could not be enumerated".to_owned()
    } else {
        format!(
            "and it would destroy {}",
            destroyed
                .iter()
                .map(|object| format!("`{object}`"))
                .collect::<Vec<String>>()
                .join(", ")
        )
    };
    recovery_plan_incomplete(
        "a restore method that achieves the recovery goal",
        &format!(
            "v0.6 §13.6 and §53: only a rollback to `{}` achieves this goal, {what}, and \
             `{ALLOW_DESTRUCTIVE_ROLLBACK}` is false. Set it to true to have the rollback planned; \
             `--accept-newer-state-loss` then accepts the loss at apply (§24.5), and does not \
             stand in for the setting",
            examination.reference
        ),
    )
    .with_metadata("setting", Value::string(ALLOW_DESTRUCTIVE_ROLLBACK))
    .with_metadata(
        "destroyed",
        Value::list(destroyed.iter().map(|object| Value::string(object))),
    )
}

/// The name `/etc/passwd` gives `uid`, which is how `zfs allow` prints a delegation to it.
fn user_name(passwd: &str, uid: u32) -> Option<String> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let _password = fields.next()?;
        let id = fields.next()?.parse::<u32>().ok()?;
        (id == uid).then(|| name.to_owned())
    })
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
    for (clone, origin) in &examination.newer_clones {
        items.push(NewerStateItem::new(
            Arc::clone(clone),
            if discards {
                NewerStateClass::Conflicting
            } else {
                NewerStateClass::PreservedByMethod
            },
            format!(
                "v0.6 §13.6: this clone depends on the newer snapshot `{origin}`, so destroying \
                 that snapshot for a rollback and keeping the clone are the same object pulled \
                 two ways"
            ),
        ));
    }
    for clone in &examination.own_clones {
        items.push(NewerStateItem::new(
            Arc::clone(clone),
            NewerStateClass::PreservedByMethod,
            "v0.6 §13.6: a clone of the recovery point itself; neither a rollback to it nor a \
             file restore touches it",
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

impl ZfsProvider {
    /// Everything [`RecoveryProvider::restore_with`] does, answering only whether it was done.
    fn carry_out(
        &self,
        action: &PlanAction,
        asset: &RecoveryAsset,
        acceptance: &RestoreAcceptance,
    ) -> Result<(), ErrorValue> {
        self.require_tools()?;
        let Execution::Program { program, argv } = action.execution() else {
            return Err(recovery_apply_failed(
                action.summary(),
                "v0.6 §2.17: this provider carries out a recovery action as a program and an \
                 argument vector, and this action carries neither",
            ));
        };
        let words: Vec<&str> = argv.iter().map(Arc::as_ref).collect();
        if let Some(flag) = recursive_flag(&words) {
            return Err(recovery_apply_failed(
                action.summary(),
                &format!(
                    "v0.6 §13.6: `{flag}` carries a recursive flag. Ono never adds a destructive \
                     flag equivalent to removing newer history, in any spelling, and refuses to \
                     run one it did not build"
                ),
            ));
        }
        let examination = self.examine(asset, acceptance)?;
        if let Some(blocked) = examination.checklist.blocking_error(false) {
            return Err(blocked);
        }
        let act = recognise(program, &words, &examination)
            .map_err(|detail| recovery_apply_failed(action.summary(), &detail))?;
        match &act {
            Act::DestroyHistory(object) => {
                guard_destruction(&examination, self.allow_destructive_rollback)?;
                if !action.preconditions().iter().any(|precondition| {
                    precondition.subject() == object.as_ref() && precondition.field() == "guid"
                }) {
                    return Err(recovery_apply_failed(
                        action.summary(),
                        &format!(
                            "v0.6 §56.1: the action carries no GUID for `{object}`, so what the \
                             operator accepted cannot be tied to the object that is there now"
                        ),
                    ));
                }
            }
            Act::Rollback => guard_destruction(&examination, self.allow_destructive_rollback)?,
            Act::DestroyTemporaryClone { present: false } => {
                // §37: the temporary clone is already gone, which is what destroying it was for.
                return Ok(());
            }
            Act::Copy | Act::Clone | Act::DestroyTemporaryClone { .. } => {}
        }
        check_guid_preconditions(action, &examination)?;

        let output = self.runner.run(program, &words)?;
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
        let named = match act {
            Act::DestroyHistory(_) | Act::DestroyTemporaryClone { .. } => {
                parse::destroy_refusal(&refusal)
            }
            _ => parse::rollback_refusal(&refusal).objects(),
        };
        if !named.is_empty() {
            return Err(destructive_history_not_accepted(
                &named
                    .iter()
                    .map(|object| object.to_string())
                    .collect::<Vec<String>>(),
            ));
        }
        Err(recovery_apply_failed(action.summary(), &refusal))
    }
}
