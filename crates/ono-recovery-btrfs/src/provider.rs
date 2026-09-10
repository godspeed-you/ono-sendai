//! The provider itself: identity, protection, recovery, cleanup and cost (§14, Appendix D.6–D.9).
//!
//! Everything the provider says about a filesystem comes from a `btrfs` invocation or from the
//! mount table, and everything it refuses to say comes from one of those failing. The shape of
//! each method follows from that: there are no defaults for facts, so a query that did not answer
//! leaves a [`SafetyFact`] outstanding and §56.3 blocks.
//!
//! # Where the calls go
//!
//! Each method makes a fixed sequence of `btrfs` calls, in a fixed order, so a recorded script
//! replays deterministically and a reader can see what a method costs:
//!
//! | method | calls |
//! |---|---|
//! | `mounts` | none for an identified table; `filesystem show` otherwise |
//! | `availability` | `--version` |
//! | `resolve_domain` | `filesystem show`, `subvolume show`, `subvolume list` |
//! | `discover` | `subvolume list`, `filesystem usage` — after `resolve_domain`'s three for a domain named by its `subvol=` option |
//! | `plan_protection` | none — §2.1 keeps planning side-effect free |
//! | `create` | `subvolume snapshot -r`, `subvolume show`, `property get … ro` |
//! | `validate` | `subvolume show` (snapshot), `property get … ro`, `subvolume show` (source) |
//! | `plan_recovery` | `subvolume show` (snapshot), `property get … ro`, `filesystem show`, `subvolume show` (source), `subvolume list`, `subvolume get-default` |
//! | `restore_with` | derive: `subvolume snapshot`, `subvolume show`; replace or rename: `subvolume show` ×3; set-default: `subvolume show` ×3, `subvolume set-default`; restore a file: none |
//! | `cleanup` | `subvolume delete` |
//! | `estimate_cost` | `filesystem usage` |
//!
//! # What it will not say
//!
//! §14.4 forbids presenting Btrfs as having a generic in-place rollback primitive, so no method
//! here produces [`RestoreMethod::DatasetRollback`] and no sentence it composes uses that word for
//! what Btrfs would do. The four methods it does produce — selective file restore, clone and copy,
//! subvolume replacement and offline root recovery — are §14.4's own list, and every fragment
//! declares which one it is.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use ono_change_core::{
    ActionRole, ChangePlan, ConsistencyClass, EffectConfidence, EffectDomain, EffectKind,
    EquivalenceDomain, Execution, NewerStateClass, NewerStateImpact, NewerStateItem,
    PersistenceDomain, PlanAction, PlanId, ProtectionAction, ProtectionMode, ProviderAvailability,
    ProviderCapabilities, RecoveryAsset, RecoveryAssetType, RecoveryCandidate, RecoveryCapability,
    RecoveryCost, RecoveryExclusion, RecoveryGoal, RecoveryObjective, RecoveryPlanFragment,
    RecoveryProvider, RecoveryScope, RecoveryValidation, ResolvedMount, RestoreAcceptance,
    RestoreMethod, RestoreOutcome, ToolOutput, ToolRunner, UnrecoverableEffect, VerificationClass,
    VerificationContract, error as core_error,
};
use ono_value::{ErrorValue, Value};

use crate::assets::{
    ProtectionShortfall, RecoveryAssetSet, SEQUENTIAL_CREATION, SEQUENTIAL_CREATION_REASON,
};
use crate::boot::{BootSelection, KERNEL_CMDLINE, boot_selection};
use crate::boundary::{RequiredProtection, SubvolumeBoundary, SubvolumeLayout};
use crate::config::{BtrfsConfig, RootRecovery, sanitised_name, snapshot_name};
use crate::error::{
    PROVIDER_ID, command_failed, fact_not_established, no_btrfs_mount, recursive_snapshot_location,
    snapshot_failed,
};
use crate::files::{FileStore, SystemFiles};
use crate::mount::{BtrfsMount, BtrfsMounts};
use crate::newer::{classify_object, classify_subvolume};
use crate::parse::{
    BtrfsVersion, DefaultSubvolume, FilesystemInfo, FilesystemUsage, ShowOutcome, SubvolumeShow,
    parse_filesystem_list, parse_filesystem_show, parse_filesystem_usage, parse_get_default,
    parse_read_only_property, parse_subvolume_list, parse_version, read_subvolume_show,
    spoken_text,
};
use crate::safety::{SafetyChecklist, SafetyFact};
use crate::subvolume::SubvolumeRef;

/// The program every Btrfs operation runs through (§12.3).
pub const BTRFS: &str = "btrfs";

/// The `btrfs-progs` series this provider has been validated against (Appendix G.4).
///
/// Appendix G.4 requires a provider to degrade rather than execute semantics it has not tested,
/// so the list holds the versions the recorded fixtures were taken from and the ones the gated
/// real-filesystem suite has run against — nothing else. A version outside it makes the provider
/// [`ProviderAvailability::Unsupported`], which is a refusal a person can read and act on.
pub const VALIDATED_VERSIONS: &[&str] = &["6.16", "6.17"];

/// The persistence-object kind a Btrfs recovery scope carries (§11.2).
pub const SCOPE_KIND: &str = "btrfs-subvolume";

/// The argument naming the operation a recovery action performs.
pub const ARG_OPERATION: &str = "operation";

/// The argument naming what a recovery action reads from.
pub const ARG_SOURCE: &str = "source";

/// The argument naming what a recovery action writes to.
pub const ARG_DESTINATION: &str = "destination";

/// The argument naming the subvolume a recovery action acts on.
pub const ARG_SUBVOLUME: &str = "subvolume";

/// The argument naming the subvolume id a recovery action acts on.
pub const ARG_SUBVOLUME_ID: &str = "subvolume-id";

/// The argument naming the mount a recovery action acts through.
pub const ARG_MOUNT: &str = "mount";

/// Copy one object out of the read-only snapshot into the live subvolume (§13.5).
pub const OP_RESTORE_FILE: &str = "restore-file";

/// Create a writable subvolume from the read-only snapshot (§14.5, Appendix D.9).
pub const OP_DERIVE_WRITABLE: &str = "derive-writable-subvolume";

/// Point the next boot at a subvolume (Appendix D.9).
pub const OP_SET_DEFAULT: &str = "set-default-subvolume";

/// Put a derived subvolume in place of the live one, with the live one unmounted (§14.4).
pub const OP_REPLACE_SUBVOLUME: &str = "replace-subvolume";

/// Give a derived subvolume the root's name, for a boot that selects the root by name (§14.6).
///
/// The same two renames as [`OP_REPLACE_SUBVOLUME`], made while the root is running: the running
/// system keeps the subvolume it booted, whatever it is now called, until the reboot.
pub const OP_SWAP_FOR_NEXT_BOOT: &str = "rename-subvolume-for-next-boot";

/// The argument naming where the displaced subvolume is moved aside to (§14.4).
pub const ARG_ASIDE: &str = "aside";

/// The argument naming where the subvolume being recovered is visible now.
pub const ARG_LIVE: &str = "live";

/// The suffix a derived writable subvolume's name carries (§14.5).
pub const DERIVED_SUFFIX: &str = "-rw";

/// The suffix the subvolume being replaced is moved aside under, before the plan id (§14.4).
///
/// `@var` becomes `@var.ono-superseded-<plan>`. The live subvolume is renamed rather than deleted, so a replacement that turns out to have
/// been the wrong idea still has the state it displaced. §2.15 and Appendix F both prefer keeping
/// the evidence over a tidy tree.
pub const SUPERSEDED_SUFFIX: &str = ".ono-superseded";

/// The first-party Btrfs recovery provider (§14).
#[derive(Debug, Clone)]
pub struct BtrfsProvider {
    runner: Arc<dyn ToolRunner>,
    files: Arc<dyn FileStore>,
    mounts: Option<BtrfsMounts>,
    config: BtrfsConfig,
    host: Arc<str>,
    program: Arc<str>,
    plan: Option<PlanId>,
    instant: Option<Timestamp>,
    derived: Arc<Mutex<Vec<RecoveryAsset>>>,
}

/// Where a subvolume is visible now, and whether a mount names it or only leads to it.
#[derive(Debug, Clone)]
struct LiveSubvolume {
    path: PathBuf,
    mount: BtrfsMount,
    /// The mount's own `subvolid=` is this subvolume's id. When it is not — a nested subvolume
    /// reached through its parent's mount — the path is confirmed with `subvolume show` before
    /// anything acts on it (Appendix B.9).
    named_by_mount: bool,
}

/// The three paths of a subvolume swap, all visible through one mount (§14.4).
#[derive(Debug, Clone)]
struct SwapPaths {
    mount_point: String,
    derived: PathBuf,
    live: PathBuf,
    aside: PathBuf,
}

/// What a whole-subvolume step read about the three subvolumes it involves (§56.2).
#[derive(Debug)]
struct Lineage {
    live: Box<SubvolumeShow>,
    snapshot: Box<SubvolumeShow>,
    derived: Box<SubvolumeShow>,
}

/// How a next-boot recovery of the root steers the boot (§14.6, Appendix D.9).
#[derive(Debug, Clone)]
struct Steering {
    set_default: bool,
    rename: bool,
    evidence: String,
}

impl BtrfsProvider {
    /// A provider that runs `btrfs` through `runner` (§12.3).
    ///
    /// The mount table is read from `/proc/self/mountinfo` on first use unless
    /// [`BtrfsProvider::with_mounts`] supplied one, and file content is read and copied through
    /// the real filesystem unless [`BtrfsProvider::with_files`] supplied a store.
    #[must_use]
    pub fn new(runner: Arc<dyn ToolRunner>) -> Self {
        Self {
            runner,
            files: Arc::new(SystemFiles),
            mounts: None,
            config: BtrfsConfig::default(),
            host: Arc::from("localhost"),
            program: Arc::from(BTRFS),
            plan: None,
            instant: None,
            derived: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The writable subvolumes `restore_with` derived from recovery points, as assets (§14.5).
    ///
    /// §14.5 requires a writable subvolume derived from a retained snapshot to be tracked
    /// separately. [`RecoveryProvider::restore_with`] returns nothing, so the asset each
    /// derivation creates — depending on the recovery point it came from — is kept here for the
    /// caller that persists assets to collect.
    #[must_use]
    pub fn derived_assets(&self) -> Vec<RecoveryAsset> {
        self.derived
            .lock()
            .map(|derived| derived.clone())
            .unwrap_or_default()
    }

    /// Resolves paths against `mounts` rather than against `/proc/self/mountinfo`.
    #[must_use]
    pub fn with_mounts(mut self, mounts: BtrfsMounts) -> Self {
        self.mounts = Some(mounts);
        self
    }

    /// Reads and copies file content through `files` (§13.5).
    #[must_use]
    pub fn with_files(mut self, files: Arc<dyn FileStore>) -> Self {
        self.files = files;
        self
    }

    /// Applies the `[recovery.btrfs]` settings of §53.
    #[must_use]
    pub fn with_config(mut self, config: BtrfsConfig) -> Self {
        self.config = config;
        self
    }

    /// Names the host the scopes belong to (§11.2).
    #[must_use]
    pub fn on_host(mut self, host: impl Into<Arc<str>>) -> Self {
        self.host = host.into();
        self
    }

    /// Runs a program other than plain `btrfs` — an absolute path, or a test double.
    #[must_use]
    pub fn with_program(mut self, program: impl Into<Arc<str>>) -> Self {
        self.program = program.into();
        self
    }

    /// Attributes the assets this provider proposes to `plan` (§11.1's `source_plan`).
    ///
    /// The plan id is also what makes a snapshot name predictable and attributable: the fixtures'
    /// `ono-a82f-var` is plan `a82f`'s snapshot of `@var` (§37, §43.6, Appendix D.8).
    #[must_use]
    pub fn for_plan(mut self, plan: PlanId) -> Self {
        self.plan = Some(plan);
        self
    }

    /// Sets the instant the provider stamps proposals and validations with.
    ///
    /// A provider given an instant is a deterministic function of its inputs, which is what a test
    /// needs; one given none reads the clock whenever it proposes or validates, which is what a
    /// long-lived session needs. The instant a snapshot *exists* at is never this one: that is
    /// read back from the filesystem's own record of when it was created (Appendix D.7).
    #[must_use]
    pub const fn at_instant(mut self, instant: Timestamp) -> Self {
        self.instant = Some(instant);
        self
    }

    /// The instant the provider stamps its work with: the one it was given, or the clock now.
    fn now(&self) -> Timestamp {
        self.instant.unwrap_or_else(Timestamp::now)
    }

    /// The settings in force (§53).
    #[must_use]
    pub const fn config(&self) -> &BtrfsConfig {
        &self.config
    }

    /// The mount table resolutions are performed against, each mount tied to its filesystem UUID
    /// (Appendix B.1, §56.2).
    ///
    /// A table supplied with [`BtrfsProvider::with_mounts`] that is already identified is used
    /// as it is. Otherwise — `/proc/self/mountinfo`, or a supplied table without UUIDs — the
    /// provider asks `btrfs filesystem show` which filesystem each device belongs to, because
    /// subvolume ids start at 256 on every filesystem and an id means nothing without its UUID.
    ///
    /// # Errors
    ///
    /// A structured error when no table was supplied and `/proc/self/mountinfo` cannot be read, or
    /// when the filesystems could not be listed.
    pub fn mounts(&self) -> Result<Cow<'_, BtrfsMounts>, ErrorValue> {
        let table = match &self.mounts {
            Some(mounts) if mounts.is_identified() => return Ok(Cow::Borrowed(mounts)),
            Some(mounts) => mounts.clone(),
            None => BtrfsMounts::from_proc()?,
        };
        if table.is_identified() {
            return Ok(Cow::Owned(table));
        }
        let output = self.btrfs(&["filesystem", "show"])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs filesystem show",
                spoken_text(&output).trim(),
            ));
        }
        let filesystems = parse_filesystem_list(spoken_text(&output))?;
        Ok(Cow::Owned(table.identified_by(&filesystems)))
    }

    /// Runs one `btrfs` subcommand (§12.3).
    ///
    /// # Errors
    ///
    /// A structured error when the program could not be run at all. A program that ran and failed
    /// comes back as a [`ToolOutput`], because what it printed is usually the answer.
    pub fn btrfs(&self, argv: &[&str]) -> Result<ToolOutput, ErrorValue> {
        self.runner.run(&self.program, argv)
    }

    /// The version of `btrfs-progs` in use (Appendix G.4).
    ///
    /// # Errors
    ///
    /// A structured error when `btrfs --version` could not be run or could not be read.
    pub fn version(&self) -> Result<BtrfsVersion, ErrorValue> {
        let output = self.btrfs(&["--version"])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs --version",
                spoken_text(&output).trim(),
            ));
        }
        parse_version(spoken_text(&output))
    }

    /// Every subvolume of the filesystem `mount` belongs to, as boundaries (§14.3).
    ///
    /// This is the query §14.3 turns on. Each entry is a place a snapshot stops, and the flags
    /// asked for — `-a -p -u -q -R` — are what make the answer complete: every subvolume rather
    /// than only those below the mount, with the parent id, the uuid and the lineage that ties a
    /// snapshot to what it was taken from.
    ///
    /// # Errors
    ///
    /// A structured error when the listing failed — including the unprivileged case, where
    /// `btrfs` answers `ERROR: can't perform the search: Operation not permitted`. §56.3 makes
    /// that a refusal: a boundary list nobody could read is not an empty boundary list.
    pub fn boundaries(&self, mount: &Path) -> Result<Vec<SubvolumeBoundary>, ErrorValue> {
        let mount_text = mount.to_string_lossy().into_owned();
        let output = self.btrfs(&[
            "subvolume",
            "list",
            "-a",
            "-p",
            "-u",
            "-q",
            "-R",
            &mount_text,
        ])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs subvolume list",
                spoken_text(&output).trim(),
            ));
        }
        Ok(parse_subvolume_list(spoken_text(&output))?
            .iter()
            .map(SubvolumeBoundary::from_entry)
            .collect())
    }

    /// The subvolumes and the mounts of the same filesystem that reach them (§14.3, Appendix B.9).
    ///
    /// # Errors
    ///
    /// A structured error when either half could not be established, or when no Btrfs mount
    /// serves `mount`.
    pub fn layout(&self, mount: &Path) -> Result<SubvolumeLayout, ErrorValue> {
        let mounts = self.mounts()?;
        let serving = mounts
            .covering(mount)
            .ok_or_else(|| no_btrfs_mount(&mount.to_string_lossy()))?;
        // §56.2: the listing is one filesystem's, and so are the mounts it is laid out against.
        let own = mounts.same_filesystem_as(serving);
        Ok(SubvolumeLayout::new(self.boundaries(mount)?, own))
    }

    /// The subvolumes a plan changing `paths` must snapshot, each separately (§14.3, §59.3).
    ///
    /// # Errors
    ///
    /// A structured error when the boundaries could not be listed.
    pub fn required_protection(
        &self,
        mount: &Path,
        paths: &[&Path],
    ) -> Result<RequiredProtection, ErrorValue> {
        Ok(self.layout(mount)?.required_for(paths))
    }

    /// What `btrfs filesystem show` says about the filesystem at `mount` (Appendix D.6).
    ///
    /// # Errors
    ///
    /// A structured error when the command failed or its output carried no filesystem UUID.
    pub fn filesystem_info(&self, mount: &Path) -> Result<FilesystemInfo, ErrorValue> {
        let mount_text = mount.to_string_lossy().into_owned();
        let output = self.btrfs(&["filesystem", "show", &mount_text])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs filesystem show",
                spoken_text(&output).trim(),
            ));
        }
        parse_filesystem_show(spoken_text(&output))
    }

    /// What `btrfs filesystem usage` says about space at `mount` (§38).
    ///
    /// # Errors
    ///
    /// A structured error when the command failed or its output could not be read.
    pub fn filesystem_usage(&self, mount: &Path) -> Result<FilesystemUsage, ErrorValue> {
        let mount_text = mount.to_string_lossy().into_owned();
        let output = self.btrfs(&["filesystem", "usage", &mount_text])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs filesystem usage",
                spoken_text(&output).trim(),
            ));
        }
        parse_filesystem_usage(spoken_text(&output))
    }

    /// What `btrfs subvolume show` says about `path` (§14.1, Appendix B.9).
    ///
    /// # Errors
    ///
    /// A structured error when the command could not be run or printed unreadable output. A path
    /// that is a plain directory, or is not there at all, is an outcome rather than an error.
    pub fn show(&self, path: &Path) -> Result<ShowOutcome, ErrorValue> {
        let path_text = path.to_string_lossy().into_owned();
        let output = self.btrfs(&["subvolume", "show", &path_text])?;
        read_subvolume_show(&output)
    }

    /// Whether the subvolume at `path` carries the read-only flag (§14.5).
    ///
    /// # Errors
    ///
    /// A structured error when the property could not be read. §14.5 asks for the flag to be
    /// verified rather than assumed, and an unread flag is not `true`.
    pub fn is_read_only(&self, path: &Path) -> Result<bool, ErrorValue> {
        let path_text = path.to_string_lossy().into_owned();
        let output = self.btrfs(&["property", "get", &path_text, "ro"])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs property get",
                spoken_text(&output).trim(),
            ));
        }
        parse_read_only_property(spoken_text(&output))
    }

    /// Which subvolume the filesystem at `mount` mounts by default (§56.2, Appendix D.9).
    ///
    /// # Errors
    ///
    /// A structured error when the default could not be read, which leaves §56.2's
    /// default-subvolume and boot impact fact outstanding.
    pub fn default_subvolume(&self, mount: &Path) -> Result<DefaultSubvolume, ErrorValue> {
        let mount_text = mount.to_string_lossy().into_owned();
        let output = self.btrfs(&["subvolume", "get-default", &mount_text])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs subvolume get-default",
                spoken_text(&output).trim(),
            ));
        }
        parse_get_default(spoken_text(&output))
    }

    /// Creates a writable subvolume from a read-only snapshot, tracked as its own asset (§14.5).
    ///
    /// §14.5 is explicit: *"If recovery requires a writable clone/subvolume derived from them,
    /// that derived object MUST be tracked separately."* So this returns a second
    /// [`RecoveryAsset`] that depends on the first rather than changing it, and the read-only
    /// snapshot it came from stays read-only and stays retained.
    ///
    /// # Errors
    ///
    /// A structured error when the derived subvolume could not be created or read back.
    pub fn derive_writable(
        &self,
        asset: &RecoveryAsset,
        destination: &Path,
    ) -> Result<RecoveryAsset, ErrorValue> {
        let Some(source) = SubvolumeRef::parse(asset.scope().domain()) else {
            return Err(snapshot_failed(
                asset.scope().domain(),
                "the recovery point's scope names no filesystem and subvolume, so a subvolume \
                 derived from it could not be tied to one (§56.2)",
            ));
        };
        let destination_text = destination.to_string_lossy().into_owned();
        let output = self.btrfs(&[
            "subvolume",
            "snapshot",
            asset.reference(),
            &destination_text,
        ])?;
        if !output.succeeded() {
            return Err(snapshot_failed(
                asset.scope().domain(),
                spoken_text(&output).trim(),
            ));
        }
        let derived = match self.show(destination)? {
            ShowOutcome::Subvolume(show) => show,
            other => {
                return Err(snapshot_failed(
                    asset.scope().domain(),
                    &format!(
                        "the derived subvolume at {destination_text} could not be read back: \
                         {other:?}"
                    ),
                ));
            }
        };
        let scope = RecoveryScope::new(
            SCOPE_KIND,
            SubvolumeRef::new(source.filesystem(), derived.id(), derived.tree_path()).reference(),
            Arc::clone(&self.host),
        )
        .covering(destination_text.clone());
        let mut writable = RecoveryAsset::proposed(
            PROVIDER_ID,
            RecoveryAssetType::BtrfsSnapshot,
            destination_text,
            scope,
            derived.created_at().unwrap_or_else(|| self.now()),
        );
        if let Some(plan) = asset.source_plan() {
            writable = writable.for_plan(plan.clone());
        }
        Ok(writable
            .depending_on(asset.id().clone())
            .at_consistency(asset.consistency())
            .restored_by(RestoreMethod::CloneAndCopy)
            .costing(snapshot_cost())
            .creating()
            .excluding(RecoveryExclusion::new(
                format!("the read-only snapshot {} it came from", asset.reference()),
                "§14.5: a writable subvolume derived from a retained snapshot is tracked as its \
                 own asset, because what it holds stops being the captured state the moment \
                 anything writes to it",
            )))
    }

    /// Creates every snapshot a plan's protection needs, in order (Appendix D.7, F.1).
    ///
    /// # Errors
    ///
    /// A boxed [`ProtectionShortfall`] when one of them fails. It carries the snapshots that were
    /// already created, because Appendix F.1 keeps them until a cleanup decision is made — and
    /// §2.3 forbids mutating any plan target either way.
    pub fn create_set(
        &self,
        actions: &[ProtectionAction],
    ) -> Result<RecoveryAssetSet, Box<ProtectionShortfall>> {
        let mut created = Vec::new();
        for action in actions {
            match self.create(action) {
                Ok(asset) => created.push(asset),
                Err(error) => {
                    return Err(Box::new(ProtectionShortfall::new(
                        created,
                        action.candidate().scope().domain(),
                        error,
                    )));
                }
            }
        }
        Ok(RecoveryAssetSet::sequential(created))
    }

    /// Turns candidates into protection actions for `plan`, stamped at `at` (§12.1, Appendix D.6).
    ///
    /// # Errors
    ///
    /// A structured error when Appendix D.8's location rule is violated, when the recovery
    /// namespace is not reachable, or when a candidate's scope does not name a subvolume.
    pub fn plan_protection_for(
        &self,
        candidates: &[RecoveryCandidate],
        mode: ProtectionMode,
        plan: Option<&PlanId>,
        at: Timestamp,
    ) -> Result<Vec<ProtectionAction>, ErrorValue> {
        if !mode.creates_assets() {
            return Ok(Vec::new());
        }
        let mounts = self.mounts()?;
        let mut actions = Vec::new();
        for candidate in candidates {
            let Some(reference) = SubvolumeRef::parse(candidate.scope().domain()) else {
                return Err(fact_not_established(
                    SafetyFact::FilesystemAndSubvolumeId,
                    &format!(
                        "the candidate scope `{}` does not name a Btrfs subvolume, so nothing \
                         could be snapshotted from it",
                        candidate.scope().domain()
                    ),
                ));
            };
            if self.config.is_nested_in(reference.tree_path()) {
                return Err(recursive_snapshot_location(
                    self.config.snapshot_location(),
                    reference.tree_path(),
                ));
            }
            let destination = self.snapshot_destination(&mounts, plan, &reference, at)?;
            let destination_text = destination.to_string_lossy().into_owned();
            let mut asset = RecoveryAsset::proposed(
                PROVIDER_ID,
                RecoveryAssetType::BtrfsSnapshot,
                destination_text.clone(),
                candidate.scope().clone(),
                at,
            );
            if let Some(plan) = plan {
                asset = asset.for_plan(plan.clone());
            }
            for exclusion in candidate.exclusions() {
                asset = asset.excluding(exclusion.clone());
            }
            let asset = asset
                .at_consistency(candidate.consistency())
                .restored_by(candidate.restore_method())
                .costing(candidate.cost().clone());
            let summary = format!(
                "snapshot {} to {destination_text} ({})",
                reference.describe(),
                if self.config.prefers_read_only_snapshots() {
                    "read-only"
                } else {
                    "writable"
                }
            );
            actions.push(ProtectionAction::new(
                PROVIDER_ID,
                summary,
                candidate.clone(),
                asset,
            ));
        }
        Ok(actions)
    }

    /// Reads §56.2's ten facts, and blocks where one is missing (§56.3).
    ///
    /// The checklist comes back beside the fragment so a caller can show what was proven before
    /// executing anything (§14.6's "It MUST be shown before execution").
    ///
    /// # Errors
    ///
    /// [`ono_change_core::error::recovery_plan_incomplete`], naming the first fact that could not
    /// be established.
    pub fn plan_recovery_with_checklist(
        &self,
        asset: &RecoveryAsset,
        source: Option<&ChangePlan>,
        goal: RecoveryGoal,
    ) -> Result<(RecoveryPlanFragment, SafetyChecklist), ErrorValue> {
        let mut checklist = SafetyChecklist::outstanding();
        let mounts = self.mounts()?;
        let snapshot_path = PathBuf::from(asset.reference());

        let Some(reference) = SubvolumeRef::parse(asset.scope().domain()) else {
            checklist.block(
                SafetyFact::FilesystemAndSubvolumeId,
                format!(
                    "the asset's scope `{}` does not name a filesystem and a subvolume id",
                    asset.scope().domain()
                ),
            );
            return Err(refusal(&checklist));
        };
        let Some(LiveSubvolume {
            path: live_path,
            mount: live_mount,
            ..
        }) = self.live_path(&mounts, &reference)
        else {
            checklist.block(
                SafetyFact::MountAndRebootRequirement,
                format!(
                    "{} is not mounted anywhere this process can see, so whether recovery needs a \
                     remount, an offline window or a reboot could not be established",
                    reference.describe()
                ),
            );
            return Err(refusal(&checklist));
        };
        let query_mount = PathBuf::from(live_mount.mount_point());

        // §56.2's second and ninth facts: the snapshot exists, and it is still read-only.
        let snapshot = match self.show(&snapshot_path)? {
            ShowOutcome::Subvolume(show) => {
                checklist.establish(
                    SafetyFact::SnapshotExistsAndValid,
                    format!(
                        "the recovery point at {} is subvolume {} with uuid {}, created {}",
                        asset.reference(),
                        show.id(),
                        show.uuid(),
                        show.created_at().map_or_else(
                            || "at an unrecorded time".to_owned(),
                            |at| at.to_string()
                        )
                    ),
                );
                show
            }
            outcome => {
                checklist.block(
                    SafetyFact::SnapshotExistsAndValid,
                    format!(
                        "`btrfs subvolume show {}` reports no subvolume ({}), so the recovery \
                         point could not be confirmed to exist",
                        asset.reference(),
                        describe_outcome(&outcome)
                    ),
                );
                return Err(refusal(&checklist));
            }
        };
        self.check_read_only(&mut checklist, &snapshot_path);

        self.plan_recovery_from(
            checklist,
            asset,
            source,
            goal,
            &reference,
            &snapshot,
            &live_path,
            &live_mount,
            &query_mount,
            &mounts,
        )
    }

    /// Which of §14.4's methods a recovery would be (§14.4, §14.6, Appendix C.1).
    ///
    /// Appendix C.1's least-destructive principle decides the first branch: where the goal is to
    /// put named objects back and they can be read out of the snapshot, that is what happens,
    /// whatever the root policy says, because it touches nothing else and needs no reboot. The
    /// root policy of §53 governs the case where a whole subvolume has to come back, and it
    /// selects between §14.6's three: an online selective restore, an offline subvolume
    /// replacement, or a next-boot switch.
    ///
    /// `None` means no method this provider has can achieve the goal, which §56.3 makes a block
    /// rather than a fall back to the biggest hammer.
    #[must_use]
    pub fn method_for(
        &self,
        goal: RecoveryGoal,
        is_root: bool,
        restore_set: &[PathBuf],
    ) -> Option<RestoreMethod> {
        let selective = !restore_set.is_empty();
        match goal {
            RecoveryGoal::RestoreChangedObjects | RecoveryGoal::RestoreServiceHealth => {
                selective.then_some(RestoreMethod::SelectiveFileRestore)
            }
            RecoveryGoal::CompensateSemantics => None,
            RecoveryGoal::RestoreDomain if is_root => match self.config.root_recovery() {
                RootRecovery::NextBoot => Some(RestoreMethod::OfflineRootRecovery),
                RootRecovery::OfflineSubvolumeReplacement => {
                    Some(RestoreMethod::SubvolumeReplacement)
                }
                RootRecovery::OnlineSelectiveRestore => {
                    selective.then_some(RestoreMethod::CloneAndCopy)
                }
            },
            RecoveryGoal::RestoreDomain if self.config.permits_offline_replacement() => {
                Some(RestoreMethod::SubvolumeReplacement)
            }
            RecoveryGoal::RestoreDomain => selective.then_some(RestoreMethod::CloneAndCopy),
        }
    }

    /// Which of §14.6's three root workflows `method` is, where the subvolume is the root.
    #[must_use]
    pub const fn root_workflow(method: RestoreMethod) -> Option<RootRecovery> {
        match method {
            RestoreMethod::SelectiveFileRestore | RestoreMethod::CloneAndCopy => {
                Some(RootRecovery::OnlineSelectiveRestore)
            }
            RestoreMethod::SubvolumeReplacement => Some(RootRecovery::OfflineSubvolumeReplacement),
            RestoreMethod::OfflineRootRecovery => Some(RootRecovery::NextBoot),
            _ => None,
        }
    }

    /// Where the snapshot of `reference` goes (Appendix D.8).
    ///
    /// The name carries the plan it was taken for, and — for a provider no single plan asked, as
    /// a session's is — the instant it was proposed at, so two recovery points of one subvolume are
    /// two snapshots rather than a second one refused because the first is in its place (§37).
    fn snapshot_destination(
        &self,
        mounts: &BtrfsMounts,
        plan: Option<&PlanId>,
        reference: &SubvolumeRef,
        at: Timestamp,
    ) -> Result<PathBuf, ErrorValue> {
        let owner = plan.map_or_else(
            || format!("manual-{}", at.strftime("%Y%m%dT%H%M%SZ")),
            |plan| plan.short().to_owned(),
        );
        let name = snapshot_name(&owner, reference.tree_path());
        Ok(self
            .recovery_namespace(mounts, reference.filesystem(), reference.tree_path())?
            .join(name))
    }

    /// Where the recovery namespace of filesystem `filesystem` is visible (Appendix D.8).
    fn recovery_namespace(
        &self,
        mounts: &BtrfsMounts,
        filesystem: &str,
        subject: &str,
    ) -> Result<PathBuf, ErrorValue> {
        let location = self.config.snapshot_location();
        mounts
            .of_filesystem(filesystem)
            .into_iter()
            .filter(|mount| !mount.is_read_only())
            .find_map(|mount| mount.visible_path(location))
            .ok_or_else(|| {
                core_error::asset_create_failed(
                    PROVIDER_ID,
                    subject,
                    &format!(
                        "the recovery namespace {location} is not reachable through any writable \
                         mount of Btrfs filesystem {filesystem}, so a snapshot could not be placed \
                         on the same filesystem as its source (Appendix D.8)"
                    ),
                )
            })
    }

    /// Where a subvolume of the scope's own filesystem is visible now (§14.6, §56.2).
    ///
    /// A mount whose `subvolid=` is the subvolume's id is the answer. Otherwise the subvolume is
    /// reached through the most specific mount of the same filesystem it is visible beneath — a
    /// nested subvolume inside its mounted parent, or any subvolume under the top level — and
    /// the caller confirms the path with `subvolume show`, because a tree path is not an
    /// identity. A mount of another filesystem is never considered, whatever ids it shows.
    fn live_path(&self, mounts: &BtrfsMounts, reference: &SubvolumeRef) -> Option<LiveSubvolume> {
        let own = mounts.of_filesystem(reference.filesystem());
        let wanted = reference.tree_path().trim_matches('/');
        if let Some((mount, path)) = own
            .iter()
            .filter(|mount| mount.subvolume_id() == Some(reference.id()))
            .find_map(|mount| mount.visible_path(wanted).map(|path| (*mount, path)))
        {
            return Some(LiveSubvolume {
                path,
                mount: mount.clone(),
                named_by_mount: true,
            });
        }
        own.iter()
            // A mount of this tree path under another id shows a different subvolume now.
            .filter(|mount| mount.tree_path() != wanted)
            .filter_map(|mount| mount.visible_path(wanted).map(|path| (*mount, path)))
            .max_by(|(left, _), (right, _)| {
                left.tree_path()
                    .len()
                    .cmp(&right.tree_path().len())
                    .then_with(|| right.mount_point().cmp(left.mount_point()))
            })
            .map(|(mount, path)| LiveSubvolume {
                path,
                mount: mount.clone(),
                named_by_mount: false,
            })
    }

    /// The paths a swap of `reference` with a derived subvolume renames, through one mount.
    ///
    /// rename(2) between two mounts fails with EXDEV even when both show one filesystem, so the
    /// live subvolume, the derived one and the name the live one is moved aside to must all be
    /// visible through the same writable mount — which is not the subvolume's own mount, because
    /// a mountpoint cannot be renamed. The filesystem's top level is the one that always
    /// qualifies, and it is preferred.
    fn swap_paths(
        &self,
        mounts: &BtrfsMounts,
        reference: &SubvolumeRef,
        derived_tree: &str,
        plan: &PlanId,
    ) -> Option<SwapPaths> {
        let target = reference.tree_path().trim_matches('/');
        if target.is_empty() {
            return None;
        }
        let aside_tree = format!(
            "{target}{SUPERSEDED_SUFFIX}-{}",
            sanitised_name(plan.short())
        );
        mounts
            .of_filesystem(reference.filesystem())
            .into_iter()
            .filter(|mount| !mount.is_read_only() && mount.tree_path() != target)
            .filter_map(|mount| {
                Some((
                    mount.tree_path().len(),
                    SwapPaths {
                        mount_point: mount.mount_point().to_owned(),
                        derived: mount.visible_path(derived_tree)?,
                        live: mount.visible_path(target)?,
                        aside: mount.visible_path(&aside_tree)?,
                    },
                ))
            })
            .min_by_key(|(depth, _)| *depth)
            .map(|(_, paths)| paths)
    }

    /// How the next boot can be steered to a recovered root, or why it cannot (§14.6).
    fn next_boot_steering(
        &self,
        default: &DefaultSubvolume,
        reference: &SubvolumeRef,
        live_path: &Path,
        mounts: &BtrfsMounts,
    ) -> Result<Steering, String> {
        let cmdline = match self.files.read(Path::new(KERNEL_CMDLINE)) {
            Ok(bytes) => bytes.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
            Err(error) => {
                return Err(format!(
                    "the kernel command line at {KERNEL_CMDLINE} could not be read ({}), so how \
                     the next boot selects its root could not be established",
                    diagnosis(&error)
                ));
            }
        };
        let fstab = self
            .files
            .read(&live_path.join("etc/fstab"))
            .ok()
            .flatten()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let own = mounts.of_filesystem(reference.filesystem());
        let devices: Vec<&str> = own.iter().map(|mount| mount.source()).collect();
        let tree = reference.tree_path().trim_matches('/');
        match boot_selection(
            cmdline.as_deref(),
            fstab.as_deref(),
            reference.filesystem(),
            &devices,
        ) {
            BootSelection::Unobservable { reason } => Err(format!(
                "{reason}, so how the next boot selects its root could not be established and \
                 neither a new default subvolume nor a rename can be chosen (§14.6)"
            )),
            BootSelection::ById { id, evidence } => Err(format!(
                "{evidence}: the next boot mounts subvolume {id} whatever the default subvolume \
                 is and whatever it is called, so neither `btrfs subvolume set-default` nor a \
                 rename changes what boots. Only the boot entry itself could, and this provider \
                 does not edit boot entries"
            )),
            BootSelection::ByName {
                tree_path,
                evidence,
            } => {
                if tree_path.as_ref() == tree {
                    Ok(Steering {
                        set_default: false,
                        rename: true,
                        evidence: format!(
                            "{evidence}, so this recovery leaves the default subvolume alone and \
                             renames: the subvolume derived from the recovery point takes the \
                             name `{tree}`, which changes what boots"
                        ),
                    })
                } else {
                    Err(format!(
                        "{evidence}, and `{tree_path}` is not `{tree}`, the subvolume being \
                         recovered, so recovering it would not change what boots"
                    ))
                }
            }
            BootSelection::ByDefault {
                also_named,
                evidence,
            } => {
                if default.id() != reference.id() {
                    return Err(format!(
                        "{evidence}, which is subvolume {} and not subvolume {} being recovered; \
                         pointing the default at the recovered subvolume would change what boots \
                         on the strength of a layout nobody has established",
                        default.id(),
                        reference.id()
                    ));
                }
                match also_named.as_deref() {
                    Some(named) if named != tree => Err(format!(
                        "{evidence}, and the root's /etc/fstab names `{named}` for `/`, which is \
                         not `{tree}`; the two disagree about what this root is"
                    )),
                    Some(_) => Ok(Steering {
                        set_default: true,
                        rename: true,
                        evidence: format!(
                            "{evidence}, and the root's /etc/fstab names `{tree}` for `/`, so \
                             this recovery changes the default subvolume to the derived one and \
                             gives it the name `{tree}` as well; it changes what boots"
                        ),
                    }),
                    None => Ok(Steering {
                        set_default: true,
                        rename: false,
                        evidence: format!(
                            "{evidence}, so this recovery changes the default subvolume to the \
                             derived one, which changes what boots"
                        ),
                    }),
                }
            }
        }
    }

    /// The mount serving `path`, when it is a mount of the recovery point's own filesystem.
    fn mount_on_filesystem(
        action: &PlanAction,
        mounts: &BtrfsMounts,
        reference: &SubvolumeRef,
        path: &Path,
    ) -> Result<BtrfsMount, ErrorValue> {
        match mounts.covering(path) {
            Some(mount) if mount.filesystem_uuid() == Some(reference.filesystem()) => {
                Ok(mount.clone())
            }
            other => Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!(
                    "{} is on {}, and the recovery point belongs to Btrfs filesystem {}. §56.2 \
                     ties every step of a recovery to the exact filesystem, so nothing was changed",
                    path.display(),
                    other.map_or_else(
                        || "no Btrfs mount this provider can see".to_owned(),
                        |mount| format!(
                            "filesystem {}",
                            mount.filesystem_uuid().unwrap_or("of unestablished UUID")
                        )
                    ),
                    reference.filesystem()
                ),
            )),
        }
    }

    /// The candidate a snapshot of `boundary` would be (§11.1, §14.3, §14.7).
    fn candidate_for(
        &self,
        layout: &SubvolumeLayout,
        filesystem: &str,
        boundary: &SubvolumeBoundary,
        covers: &[&str],
        objective: RecoveryObjective,
        usage: FilesystemUsage,
    ) -> RecoveryCandidate {
        let reference = SubvolumeRef::new(filesystem, boundary.id(), boundary.tree_path());
        let mut scope =
            RecoveryScope::new(SCOPE_KIND, reference.reference(), Arc::clone(&self.host));
        for object in covers {
            scope = scope.covering(*object);
        }
        let nested = layout.nested_within(boundary);
        let is_root = layout
            .mount_for(boundary)
            .is_some_and(|mount| layout.mounts().is_root_subvolume(mount));
        let detail = format!(
            "a {} Btrfs snapshot of {}. It shares extents with the live subvolume and lives \
             on the same filesystem and the same devices, so it is a local recovery point and \
             shares the storage failure domain of what it protects — it is not a backup \
             (§14.7){}{}",
            if self.config.prefers_read_only_snapshots() {
                "read-only"
            } else {
                "writable"
            },
            reference.describe(),
            if self.config.prefers_read_only_snapshots() {
                ""
            } else {
                ". Being writable, it stops being provably the captured state the moment anything \
                 writes to it (§14.5), because `prefer_read_only_snapshots` is off"
            },
            if nested.is_empty() {
                String::new()
            } else {
                format!(
                    ". It does not contain the {} nested inside it, each of which needs its own \
                     snapshot (§14.3)",
                    nested
                        .iter()
                        .map(|nested| format!("{} ({})", nested.tree_path(), nested.id()))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        );
        let mut cost = snapshot_cost();
        if is_root && self.config.root_recovery().requires_reboot() {
            cost = cost.needing_reboot();
        }
        if is_root && self.config.root_recovery().requires_offline() {
            cost = cost.needing_offline();
        }
        let mut candidate = RecoveryCandidate::new(
            PROVIDER_ID,
            scope,
            EffectDomain::FilesystemPersistent,
            objective,
            detail,
        )
        .at_consistency(ConsistencyClass::FilesystemConsistent)
        .restored_by(RestoreMethod::SelectiveFileRestore)
        .costing(cost)
        .excluding(RecoveryExclusion::new(
            "recovery from the loss of the filesystem or its devices",
            "§14.7: the snapshot shares the same filesystem and storage failure domain as what it \
             protects. Losing the devices loses both, and only a separate backup provider covers \
             that",
        ));
        for nested in nested {
            candidate = candidate.excluding(nested_exclusion(nested));
        }
        candidate = candidate
            .needing_to_create(
                "the privilege to create a subvolume snapshot; an unprivileged caller is refused \
                 with `Operation not permitted` (§43.4)",
            )
            .needing_to_restore(restore_requirement(is_root, self.config.root_recovery()));
        if let Some(free) = usage.free_estimated() {
            candidate = candidate.needing_to_create(format!(
                "room on the filesystem, which reports {free} free — Btrfs's own estimate rather \
                 than an exact figure (§37.5)"
            ));
        }
        candidate
    }

    /// §56.2's ninth fact (§14.5).
    fn check_read_only(&self, checklist: &mut SafetyChecklist, snapshot_path: &Path) {
        match self.is_read_only(snapshot_path) {
            Ok(true) => checklist.establish(
                SafetyFact::ReadOnlySnapshotTreatment,
                format!(
                    "`btrfs property get {} ro` answers ro=true, so the retained snapshot is \
                     unchanged since it was taken; anything writable derived from it is tracked \
                     as its own asset (§14.5)",
                    snapshot_path.display()
                ),
            ),
            Ok(false) => checklist.block(
                SafetyFact::ReadOnlySnapshotTreatment,
                format!(
                    "the snapshot at {} is writable{}, so what it holds is no longer provably the \
                     state that was captured (§14.5)",
                    snapshot_path.display(),
                    if self.config.prefers_read_only_snapshots() {
                        ""
                    } else {
                        " — `prefer_read_only_snapshots` is off, which is how it was taken"
                    }
                ),
            ),
            Err(error) => checklist.block(
                SafetyFact::ReadOnlySnapshotTreatment,
                format!(
                    "the read-only flag of {} could not be read: {}",
                    snapshot_path.display(),
                    diagnosis(&error)
                ),
            ),
        }
    }

    /// The rest of §56.2's checklist, and the fragment it permits (§14.4, §14.6).
    #[allow(clippy::too_many_arguments)]
    fn plan_recovery_from(
        &self,
        mut checklist: SafetyChecklist,
        asset: &RecoveryAsset,
        source: Option<&ChangePlan>,
        goal: RecoveryGoal,
        reference: &SubvolumeRef,
        snapshot: &SubvolumeShow,
        live_path: &Path,
        live_mount: &BtrfsMount,
        query_mount: &Path,
        mounts: &BtrfsMounts,
    ) -> Result<(RecoveryPlanFragment, SafetyChecklist), ErrorValue> {
        // §56.2's first fact: the exact filesystem, and the subvolume id from real metadata.
        let live_show = self.check_identity(&mut checklist, reference, live_path, query_mount)?;

        // §56.2's third and tenth facts: the nested boundaries, and no assumption about them.
        let own = mounts.same_filesystem_as(live_mount);
        let layout = self.check_boundaries(&mut checklist, asset, reference, query_mount, &own);

        // §56.2's fourth fact: whether a selected restore is possible.
        let restore_set = self.restore_set(asset, live_path, layout.as_ref(), &mut checklist);

        // §56.2's fifth fact: whether replacement is required, and whether the snapshot could do
        // it — which is a question about lineage, not about the operator's preference.
        let is_root = mounts.is_root_subvolume(live_mount);
        let method = self.method_for(goal, is_root, &restore_set);
        self.check_replacement(
            &mut checklist,
            asset,
            reference,
            snapshot,
            live_show.as_deref(),
            method,
            goal,
        );
        let plan_id = source.map_or_else(
            || PlanId::of(PROVIDER_ID, asset.reference(), goal.as_str()),
            |plan| plan.id().clone(),
        );

        // §56.2's sixth fact: the default subvolume and the boot impact — and, for a next-boot
        // recovery of the root, which of the two steering methods the boot entry honours.
        let mut steering = None;
        match self.default_subvolume(query_mount) {
            Ok(default) => {
                let what = default_description(&default, reference);
                if is_root && method == Some(RestoreMethod::OfflineRootRecovery) {
                    match self.next_boot_steering(&default, reference, live_path, mounts) {
                        Ok(found) => {
                            checklist.establish(
                                SafetyFact::DefaultSubvolumeAndBootImpact,
                                format!("{what}. {}", found.evidence),
                            );
                            steering = Some(found);
                        }
                        Err(reason) => checklist.block(
                            SafetyFact::DefaultSubvolumeAndBootImpact,
                            format!("{what}. {reason}"),
                        ),
                    }
                } else {
                    checklist.establish(
                        SafetyFact::DefaultSubvolumeAndBootImpact,
                        format!("{what}. {}", boot_impact(method, reference.tree_path())),
                    );
                }
            }
            Err(error) => checklist.block(
                SafetyFact::DefaultSubvolumeAndBootImpact,
                format!(
                    "the default subvolume of {} could not be read, so the boot impact of this \
                     recovery is unknown: {}",
                    query_mount.display(),
                    diagnosis(&error)
                ),
            ),
        }

        // Where the derived subvolume goes, and whether one mount can swap it in (§14.4, §14.5).
        let derived_name = format!(
            "{}{DERIVED_SUFFIX}",
            PathBuf::from(asset.reference()).file_name().map_or_else(
                || "ono-derived".to_owned(),
                |name| name.to_string_lossy().into_owned()
            )
        );
        let derived_tree = format!(
            "{}/{derived_name}",
            self.config.snapshot_location().trim_matches('/')
        );
        let needs_swap = method == Some(RestoreMethod::SubvolumeReplacement)
            || steering.as_ref().is_some_and(|found| found.rename);
        let swap = if needs_swap {
            self.swap_paths(mounts, reference, &derived_tree, &plan_id)
        } else {
            None
        };
        let derived = swap.as_ref().map_or_else(
            || {
                self.recovery_namespace(mounts, reference.filesystem(), reference.tree_path())
                    .map_or_else(
                        |_| PathBuf::from(format!("{}{DERIVED_SUFFIX}", asset.reference())),
                        |namespace| namespace.join(&derived_name),
                    )
            },
            |paths| paths.derived.clone(),
        );

        // §56.2's seventh fact: the mount and reboot requirement.
        if needs_swap && swap.is_none() {
            checklist.block(
                SafetyFact::MountAndRebootRequirement,
                format!(
                    "no writable mount of Btrfs filesystem {} shows both {} and {derived_tree}. \
                     The recovery renames one into the other's place, and rename(2) between two \
                     mounts fails with EXDEV even on one filesystem; mounting the filesystem's top \
                     level (subvolid=5) makes both visible through one mount",
                    reference.filesystem(),
                    reference.tree_path()
                ),
            );
        } else {
            checklist.establish(
                SafetyFact::MountAndRebootRequirement,
                mount_evidence(
                    live_mount,
                    live_path,
                    is_root,
                    method,
                    self.config.root_recovery(),
                ),
            );
        }

        // §56.2's eighth fact: the later state this method would discard.
        let newer = self.newer_state(
            &mut checklist,
            method,
            &restore_set,
            reference,
            asset.reference(),
            live_path,
            snapshot,
            live_show.as_deref(),
        );

        let Some(method) = method else {
            return Err(refusal(&checklist));
        };
        if !checklist.is_complete() {
            return Err(refusal(&checklist));
        }

        let fragment = self.fragment(
            &plan_id,
            asset,
            method,
            is_root,
            &restore_set,
            reference,
            live_path,
            live_mount,
            layout.as_ref(),
            &derived,
            swap.as_ref(),
            steering.as_ref(),
            newer,
        );
        Ok((fragment, checklist))
    }

    /// §56.2's first fact (§14.1, Appendix B.9).
    fn check_identity(
        &self,
        checklist: &mut SafetyChecklist,
        reference: &SubvolumeRef,
        live_path: &Path,
        query_mount: &Path,
    ) -> Result<Option<Box<SubvolumeShow>>, ErrorValue> {
        let info = match self.filesystem_info(query_mount) {
            Ok(info) => info,
            Err(error) => {
                checklist.block(
                    SafetyFact::FilesystemAndSubvolumeId,
                    format!(
                        "the filesystem at {} could not be identified: {}",
                        query_mount.display(),
                        diagnosis(&error)
                    ),
                );
                return Ok(None);
            }
        };
        if info.uuid() != reference.filesystem() {
            checklist.block(
                SafetyFact::FilesystemAndSubvolumeId,
                format!(
                    "the filesystem at {} is {} and the asset names {}, so the subvolume id \
                     belongs to a different filesystem",
                    query_mount.display(),
                    info.uuid(),
                    reference.filesystem()
                ),
            );
            return Ok(None);
        }
        match self.show(live_path)? {
            ShowOutcome::Subvolume(show) if show.id() == reference.id() => {
                checklist.establish(
                    SafetyFact::FilesystemAndSubvolumeId,
                    format!(
                        "filesystem {} carries subvolume {} at {}, both read from `btrfs \
                         filesystem show` and `btrfs subvolume show` rather than from the shape \
                         of the path (Appendix B.9)",
                        info.uuid(),
                        show.id(),
                        live_path.display()
                    ),
                );
                Ok(Some(show))
            }
            outcome => {
                checklist.block(
                    SafetyFact::FilesystemAndSubvolumeId,
                    format!(
                        "{} does not report subvolume {}: {}",
                        live_path.display(),
                        reference.id(),
                        describe_outcome(&outcome)
                    ),
                );
                Ok(None)
            }
        }
    }

    /// §56.2's third and tenth facts (§14.3).
    fn check_boundaries(
        &self,
        checklist: &mut SafetyChecklist,
        asset: &RecoveryAsset,
        reference: &SubvolumeRef,
        query_mount: &Path,
        mounts: &BtrfsMounts,
    ) -> Option<SubvolumeLayout> {
        let boundaries = match self.boundaries(query_mount) {
            Ok(boundaries) => boundaries,
            Err(error) => {
                checklist.block(
                    SafetyFact::NestedBoundaries,
                    format!(
                        "the subvolumes of {} could not be listed, so where a snapshot of {} stops \
                         is unknown: {}",
                        query_mount.display(),
                        reference.tree_path(),
                        diagnosis(&error)
                    ),
                );
                return None;
            }
        };
        let layout = SubvolumeLayout::new(boundaries, mounts.clone());
        let Some(boundary) = layout.by_id(reference.id()) else {
            checklist.block(
                SafetyFact::NestedBoundaries,
                format!(
                    "subvolume {} does not appear in `btrfs subvolume list` for {}, so the \
                     boundaries around it could not be established",
                    reference.id(),
                    query_mount.display()
                ),
            );
            return None;
        };
        let nested = layout.nested_within(boundary);
        checklist.establish(
            SafetyFact::NestedBoundaries,
            if nested.is_empty() {
                format!(
                    "no subvolume is nested inside {}, so a snapshot of it stops at the subvolume \
                     itself",
                    reference.tree_path()
                )
            } else {
                format!(
                    "{} nested inside {}, and a snapshot of the parent holds an empty directory \
                     where each one is mounted (§14.3)",
                    nested
                        .iter()
                        .map(|nested| format!("{} {}", nested.id(), nested.tree_path()))
                        .collect::<Vec<_>>()
                        .join(", "),
                    reference.tree_path()
                )
            },
        );
        let unstated: Vec<&str> = nested
            .iter()
            .filter(|nested| {
                !asset
                    .exclusions()
                    .iter()
                    .any(|exclusion| names_subvolume(exclusion.subject(), nested.tree_path()))
            })
            .map(|nested| nested.tree_path())
            .collect();
        if unstated.is_empty() {
            checklist.establish(
                SafetyFact::NoRecursiveCoverageAssumption,
                format!(
                    "every subvolume nested inside {} is recorded on the asset as something it \
                     does not hold, so nothing treats this snapshot as recursive (§14.3)",
                    reference.tree_path()
                ),
            );
        } else {
            checklist.block(
                SafetyFact::NoRecursiveCoverageAssumption,
                format!(
                    "the asset says nothing about {}, which is nested inside {} and is therefore \
                     an empty directory inside the snapshot; treating the snapshot as covering it \
                     is the recursive assumption §14.3 forbids",
                    unstated.join(", "),
                    reference.tree_path()
                ),
            );
        }
        Some(layout)
    }

    /// §56.2's fifth fact (§14.4).
    #[allow(clippy::too_many_arguments)]
    fn check_replacement(
        &self,
        checklist: &mut SafetyChecklist,
        asset: &RecoveryAsset,
        reference: &SubvolumeRef,
        snapshot: &SubvolumeShow,
        live: Option<&SubvolumeShow>,
        method: Option<RestoreMethod>,
        goal: RecoveryGoal,
    ) {
        let Some(method) = method else {
            checklist.block(
                SafetyFact::SubvolumeReplacementRequired,
                format!(
                    "no method this provider offers achieves the goal `{}` for {} under the \
                     configured root policy `{}`, so whether the subvolume would have to be \
                     replaced could not be answered",
                    goal.as_str(),
                    reference.tree_path(),
                    self.config.root_recovery().token()
                ),
            );
            return;
        };
        match (snapshot.parent_uuid(), live) {
            (Some(parent), Some(live)) if parent == live.uuid() => checklist.establish(
                SafetyFact::SubvolumeReplacementRequired,
                format!(
                    "the recovery point's parent uuid {parent} is subvolume {}'s own uuid, so it \
                     is a snapshot of it and could stand in its place; {}",
                    reference.id(),
                    replacement_sentence(method)
                ),
            ),
            (Some(parent), _) => checklist.block(
                SafetyFact::SubvolumeReplacementRequired,
                format!(
                    "the recovery point's parent uuid is {parent} and subvolume {}'s own uuid \
                     could not be matched against it, so what putting it in place would produce \
                     is unknown",
                    reference.id()
                ),
            ),
            (None, _) => checklist.block(
                SafetyFact::SubvolumeReplacementRequired,
                format!(
                    "the object at {} carries no parent uuid, so it is not a snapshot of \
                     subvolume {} and could not stand in its place",
                    asset.reference(),
                    reference.id()
                ),
            ),
        }
    }

    /// §56.2's fourth fact: the objects a selective restore would put back (Appendix C.1).
    fn restore_set(
        &self,
        asset: &RecoveryAsset,
        live_path: &Path,
        layout: Option<&SubvolumeLayout>,
        checklist: &mut SafetyChecklist,
    ) -> Vec<PathBuf> {
        let covered: Vec<PathBuf> = asset
            .scope()
            .covers()
            .iter()
            .map(|object| PathBuf::from(object.as_ref()))
            .collect();
        if covered.is_empty() {
            checklist.block(
                SafetyFact::SelectiveRestorePossible,
                "the asset names no object it covers, so whether the wanted state could be put \
                 back one object at a time could not be established"
                    .to_owned(),
            );
            return covered;
        }
        let mut beyond: Vec<String> = Vec::new();
        for object in &covered {
            if !object.starts_with(live_path) {
                beyond.push(format!(
                    "{} is outside {}, which is what this snapshot holds",
                    object.display(),
                    live_path.display()
                ));
                continue;
            }
            if let Some(layout) = layout
                && let Some(boundary) = layout.containing(object)
                && layout
                    .visible_path(boundary)
                    .is_some_and(|path| path != live_path)
            {
                beyond.push(format!(
                    "{} lies in the nested subvolume {} {}, which a snapshot of the parent does \
                     not contain — inside the snapshot that path is an empty directory (§14.3)",
                    object.display(),
                    boundary.id(),
                    boundary.tree_path()
                ));
            }
        }
        if beyond.is_empty() {
            checklist.establish(
                SafetyFact::SelectiveRestorePossible,
                format!(
                    "all {} covered object(s) lie inside the snapshotted subvolume and can be read \
                     out of the snapshot one at a time",
                    covered.len()
                ),
            );
        } else {
            checklist.block(SafetyFact::SelectiveRestorePossible, beyond.join("; "));
        }
        covered
    }

    /// §56.2's eighth fact (Appendix C.3, C.4).
    #[allow(clippy::too_many_arguments)]
    fn newer_state(
        &self,
        checklist: &mut SafetyChecklist,
        method: Option<RestoreMethod>,
        restore_set: &[PathBuf],
        reference: &SubvolumeRef,
        snapshot_root: &str,
        live_path: &Path,
        snapshot: &SubvolumeShow,
        live: Option<&SubvolumeShow>,
    ) -> NewerStateImpact {
        let Some(method) = method else {
            checklist.block(
                SafetyFact::LaterStateDiscarded,
                "no recovery method was selected, so what it would discard could not be computed"
                    .to_owned(),
            );
            return NewerStateImpact::unanalysed();
        };
        let mut items: Vec<NewerStateItem> = Vec::new();
        if method.discards_newer_state() {
            if let Some(item) = classify_subvolume(
                reference.tree_path(),
                live.and_then(SubvolumeShow::generation),
                snapshot.generation(),
            ) {
                items.push(item);
            }
        } else {
            for object in restore_set {
                let Ok(relative) = object.strip_prefix(live_path) else {
                    continue;
                };
                let inside = PathBuf::from(snapshot_root).join(relative);
                let before = match self.files.read(&inside) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        checklist.block(
                            SafetyFact::LaterStateDiscarded,
                            format!(
                                "the snapshot's copy of {} could not be read: {}",
                                object.display(),
                                diagnosis(&error)
                            ),
                        );
                        return NewerStateImpact::unanalysed();
                    }
                };
                let after = match self.files.read(object) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        checklist.block(
                            SafetyFact::LaterStateDiscarded,
                            format!(
                                "the live {} could not be read: {}",
                                object.display(),
                                diagnosis(&error)
                            ),
                        );
                        return NewerStateImpact::unanalysed();
                    }
                };
                items.push(classify_object(
                    object,
                    true,
                    before.as_deref(),
                    after.as_deref(),
                ));
            }
        }
        if let Some(unknown) = items
            .iter()
            .find(|item| item.class() == NewerStateClass::Unknown)
        {
            checklist.block(
                SafetyFact::LaterStateDiscarded,
                format!("{}: {}", unknown.object(), unknown.detail()),
            );
            return NewerStateImpact::unanalysed();
        }
        let losses = items.iter().filter(|item| item.class().is_loss()).count();
        checklist.establish(
            SafetyFact::LaterStateDiscarded,
            if losses == 0 {
                format!(
                    "nothing written since the recovery point would be taken away by a {}",
                    method.as_str()
                )
            } else {
                format!(
                    "{losses} object(s) have changed since the recovery point, and a {} would \
                     discard those changes (Appendix C.4)",
                    method.as_str()
                )
            },
        );
        NewerStateImpact::analysed(items)
    }

    /// What a restore by `method` puts back (Appendix C.7).
    ///
    /// A copy out of the snapshot returns what [`crate::files::METADATA_COVERAGE`] names. A
    /// method that makes the snapshot the live subvolume returns the recorded tree itself, so
    /// every piece of metadata comes back with it. The two methods this provider never offers
    /// claim nothing, because silence is not a claim.
    const fn metadata_coverage(method: RestoreMethod) -> ono_change_core::MetadataCoverage {
        match method {
            RestoreMethod::SelectiveFileRestore | RestoreMethod::CloneAndCopy => {
                crate::files::METADATA_COVERAGE
            }
            RestoreMethod::SubvolumeReplacement
            | RestoreMethod::DatasetRollback
            | RestoreMethod::OfflineRootRecovery => ono_change_core::MetadataCoverage {
                content: true,
                mode: true,
                owner: true,
                acl: true,
                xattrs: true,
                capabilities: true,
                selinux: true,
                hardlinks: true,
            },
            RestoreMethod::ProviderNativeRestore | RestoreMethod::Compensation => {
                ono_change_core::MetadataCoverage::none()
            }
        }
    }

    /// Builds the fragment for `method` (§12.1, §14.4, §14.6).
    ///
    /// Every action carries everything `restore_with` needs to perform it and to re-check it at
    /// the moment it runs: the paths, the subvolume's tree path and id, and — for a swap — the
    /// name the live subvolume is moved aside to. A root recovery's actions each name the §14.6
    /// workflow they belong to, so the plan shows it before anything executes.
    #[allow(clippy::too_many_arguments)]
    fn fragment(
        &self,
        plan_id: &PlanId,
        asset: &RecoveryAsset,
        method: RestoreMethod,
        is_root: bool,
        restore_set: &[PathBuf],
        reference: &SubvolumeRef,
        live_path: &Path,
        live_mount: &BtrfsMount,
        layout: Option<&SubvolumeLayout>,
        derived: &Path,
        swap: Option<&SwapPaths>,
        steering: Option<&Steering>,
        newer: NewerStateImpact,
    ) -> RecoveryPlanFragment {
        let mut fragment = RecoveryPlanFragment::new(PROVIDER_ID, method)
            .with_newer_state(newer)
            .restoring_metadata(Self::metadata_coverage(method));
        let workflow = if is_root {
            Self::root_workflow(method)
                .map(|workflow| format!("§14.6 {}: ", workflow.workflow()))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let identity = || {
            vec![
                (ARG_SUBVOLUME, Value::string(reference.tree_path())),
                (ARG_SUBVOLUME_ID, Value::string(&reference.id().to_string())),
            ]
        };
        let mut ordinal = 0usize;
        if matches!(
            method,
            RestoreMethod::CloneAndCopy
                | RestoreMethod::SubvolumeReplacement
                | RestoreMethod::OfflineRootRecovery
        ) {
            fragment = fragment.acting(
                PlanAction::new(
                    plan_id,
                    ordinal,
                    ActionRole::Prepare,
                    format!(
                        "{workflow}create the writable subvolume {} from the read-only recovery \
                         point {}, tracked as its own recovery asset (§14.5)",
                        derived.display(),
                        asset.reference()
                    ),
                    recovery_operation(
                        RecoveryCapability::Prepare,
                        OP_DERIVE_WRITABLE,
                        vec![
                            (ARG_SOURCE, Value::string(asset.reference())),
                            (ARG_DESTINATION, Value::string(&derived.to_string_lossy())),
                        ],
                    ),
                )
                .on(asset.reference().to_owned())
                .privileged(),
            );
            ordinal += 1;
        }
        match method {
            RestoreMethod::SelectiveFileRestore | RestoreMethod::CloneAndCopy => {
                let root = if method == RestoreMethod::SelectiveFileRestore {
                    PathBuf::from(asset.reference())
                } else {
                    derived.to_path_buf()
                };
                for object in restore_set {
                    let Ok(relative) = object.strip_prefix(live_path) else {
                        continue;
                    };
                    let from = root.join(relative);
                    fragment = fragment.acting(
                        PlanAction::new(
                            plan_id,
                            ordinal,
                            ActionRole::Recover,
                            format!(
                                "{workflow}restore {} from {}",
                                object.display(),
                                from.display()
                            ),
                            recovery_operation(
                                RecoveryCapability::Restore,
                                OP_RESTORE_FILE,
                                vec![
                                    (ARG_SOURCE, Value::string(&from.to_string_lossy())),
                                    (ARG_DESTINATION, Value::string(&object.to_string_lossy())),
                                ],
                            ),
                        )
                        .on(object.to_string_lossy().into_owned())
                        .declaring(
                            EffectDomain::FilesystemPersistent,
                            EffectKind::Replace,
                            EffectConfidence::Guaranteed,
                            object.to_string_lossy().into_owned(),
                            "the live file is replaced by the snapshot's copy of it (§14.4)",
                        )
                        .privileged(),
                    );
                    ordinal += 1;
                }
            }
            RestoreMethod::SubvolumeReplacement => {
                if let Some(swap) = swap {
                    let label = if is_root {
                        workflow.clone()
                    } else {
                        "§14.4 subvolume replacement: ".to_owned()
                    };
                    fragment = fragment
                        .acting(
                            PlanAction::new(
                                plan_id,
                                ordinal,
                                ActionRole::Recover,
                                format!(
                                    "{label}with {} unmounted, rename {} to {} and {} to {} \
                                     through the mount at {}{}",
                                    reference.tree_path(),
                                    swap.live.display(),
                                    swap.aside.display(),
                                    swap.derived.display(),
                                    swap.live.display(),
                                    swap.mount_point,
                                    moved_aside_note(layout, reference)
                                ),
                                recovery_operation(
                                    RecoveryCapability::Restore,
                                    OP_REPLACE_SUBVOLUME,
                                    swap_arguments(swap, identity()),
                                ),
                            )
                            .on(swap.live.to_string_lossy().into_owned())
                            .declaring(
                                EffectDomain::FilesystemPersistent,
                                EffectKind::Replace,
                                EffectConfidence::Guaranteed,
                                swap.live.to_string_lossy().into_owned(),
                                "the live subvolume is set aside and the snapshot's writable copy takes its place (§14.4)",
                            )
                            .privileged(),
                        )
                        .needing_offline();
                }
            }
            RestoreMethod::OfflineRootRecovery => {
                if let Some(steering) = steering {
                    if steering.set_default {
                        let mut arguments = vec![
                            (ARG_SOURCE, Value::string(&derived.to_string_lossy())),
                            (ARG_MOUNT, Value::string(live_mount.mount_point())),
                            (ARG_LIVE, Value::string(&live_path.to_string_lossy())),
                        ];
                        arguments.extend(identity());
                        fragment = fragment.acting(
                            PlanAction::new(
                                plan_id,
                                ordinal,
                                ActionRole::Recover,
                                format!(
                                    "{workflow}point the next boot at {} by making it the \
                                     default subvolume of Btrfs filesystem {} (Appendix D.9)",
                                    derived.display(),
                                    reference.filesystem()
                                ),
                                recovery_operation(
                                    RecoveryCapability::Restore,
                                    OP_SET_DEFAULT,
                                    arguments,
                                ),
                            )
                            .on(live_mount.mount_point().to_owned())
                            .declaring(
                                EffectDomain::FilesystemPersistent,
                                EffectKind::Modify,
                                EffectConfidence::Guaranteed,
                                live_mount.mount_point().to_owned(),
                                "the filesystem's default subvolume changes, so the next boot mounts the recovered one (Appendix D.9)",
                            )
                            .privileged(),
                        );
                        ordinal += 1;
                    }
                    if steering.rename
                        && let Some(swap) = swap
                    {
                        fragment = fragment.acting(
                            PlanAction::new(
                                plan_id,
                                ordinal,
                                ActionRole::Recover,
                                format!(
                                    "{workflow}for the next boot, rename {} to {} and {} to {} \
                                     through the mount at {}; the running system keeps the \
                                     subvolume it booted until the reboot (Appendix D.9)",
                                    swap.live.display(),
                                    swap.aside.display(),
                                    swap.derived.display(),
                                    swap.live.display(),
                                    swap.mount_point
                                ),
                                recovery_operation(
                                    RecoveryCapability::Restore,
                                    OP_SWAP_FOR_NEXT_BOOT,
                                    swap_arguments(swap, identity()),
                                ),
                            )
                            .on(swap.live.to_string_lossy().into_owned())
                            .declaring(
                                EffectDomain::FilesystemPersistent,
                                EffectKind::Replace,
                                EffectConfidence::Guaranteed,
                                swap.live.to_string_lossy().into_owned(),
                                "at the next boot the recovered subvolume takes the live one's name (Appendix D.9)",
                            )
                            .privileged(),
                        );
                    }
                }
                fragment = fragment.needing_reboot();
            }
            _ => {}
        }
        if let Some(layout) = layout
            && let Some(boundary) = layout.by_id(reference.id())
        {
            for nested in layout.nested_within(boundary) {
                fragment = fragment.leaving(
                    UnrecoverableEffect::new(
                        nested.tree_path().to_owned(),
                        EffectDomain::FilesystemPersistent,
                        "§14.3: this subvolume is nested inside the one being recovered, and the \
                         recovery point holds an empty directory where it is mounted. Recovering \
                         it needs its own recovery asset",
                    )
                    .compensated_by(format!(
                        "recover subvolume {} from an asset of its own",
                        nested.tree_path()
                    )),
                );
            }
        }
        fragment = fragment.verifying(
            VerificationContract::new(
                plan_id,
                VerificationClass::Required,
                reference.tree_path().to_owned(),
                format!(
                    "the recovered state of {} matches the recovery point {}",
                    reference.tree_path(),
                    asset.reference()
                ),
            )
            .about(EquivalenceDomain::PersistentState),
        );
        if is_root && method == RestoreMethod::OfflineRootRecovery {
            fragment = fragment.verifying(
                VerificationContract::new(
                    plan_id,
                    VerificationClass::Required,
                    live_mount.mount_point().to_owned(),
                    "after the reboot, the running root is the subvolume the recovery selected",
                )
                .about(EquivalenceDomain::PersistentState),
            );
        }
        fragment
    }

    /// Derives the writable subvolume a plan names, and records it as an asset (§14.5).
    fn restore_derive(
        &self,
        action: &PlanAction,
        asset: &RecoveryAsset,
        reference: &SubvolumeRef,
        source: &str,
        destination: &str,
    ) -> Result<(), ErrorValue> {
        if source != asset.reference() || destination.is_empty() {
            return Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!(
                    "the action derives {destination} from {source}, and the recovery point it was \
                     handed is {}; a writable subvolume is only derived from the recovery point \
                     the plan names. Nothing was changed",
                    asset.reference()
                ),
            ));
        }
        let mounts = self.mounts()?;
        Self::mount_on_filesystem(action, &mounts, reference, Path::new(destination))?;
        if self.files.exists(Path::new(destination))? {
            return Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!(
                    "{destination} already exists, and `btrfs subvolume snapshot` into an existing \
                     directory creates the subvolume inside it rather than at it. Nothing was \
                     changed"
                ),
            ));
        }
        let derived = self.derive_writable(asset, Path::new(destination))?;
        self.derived
            .lock()
            .map_err(|_| {
                core_error::recovery_apply_failed(
                    action.summary(),
                    &format!(
                        "the writable subvolume {destination} was created and could not be \
                         recorded as an asset; it is on the filesystem and must be tracked by hand \
                         (§14.5)"
                    ),
                )
            })?
            .push(derived);
        Ok(())
    }

    /// Copies one object back, onto the recovery point's own filesystem (§13.5, §15.4).
    fn restore_file(
        &self,
        action: &PlanAction,
        reference: &SubvolumeRef,
        source: &str,
        destination: &str,
    ) -> Result<(), ErrorValue> {
        let mounts = self.mounts()?;
        Self::mount_on_filesystem(action, &mounts, reference, Path::new(source))?;
        Self::mount_on_filesystem(action, &mounts, reference, Path::new(destination))?;
        self.files.copy(Path::new(source), Path::new(destination))
    }

    /// Puts the derived subvolume in the live one's place by two renames (§14.4, §14.6).
    ///
    /// What is renamed aside is confirmed to be the subvolume the recovery point was taken of,
    /// and what takes its place is confirmed to be derived from that recovery point, at the
    /// moment of the act rather than at planning (§56.2, §43.5).
    #[allow(clippy::too_many_arguments)]
    fn restore_swap(
        &self,
        action: &PlanAction,
        asset: &RecoveryAsset,
        reference: &SubvolumeRef,
        arguments: &[(Arc<str>, Value)],
        acceptance: &RestoreAcceptance,
        requires_unmounted: bool,
    ) -> Result<(), ErrorValue> {
        let source = argument(arguments, ARG_SOURCE).unwrap_or_default();
        let destination = argument(arguments, ARG_DESTINATION).unwrap_or_default();
        let aside = argument(arguments, ARG_ASIDE).unwrap_or_default();
        Self::same_subvolume(action, reference, arguments)?;
        if source.is_empty() || destination.is_empty() || aside.is_empty() {
            return Err(core_error::recovery_apply_failed(
                action.summary(),
                "the action does not name the derived subvolume, the live one and where the live \
                 one is moved aside to, so the swap could not be carried out. Nothing was changed",
            ));
        }
        let mounts = self.mounts()?;
        if requires_unmounted
            && mounts
                .of_filesystem(reference.filesystem())
                .iter()
                .any(|mount| {
                    mount.subvolume_id() == Some(reference.id())
                        || (mount.subvolume_id().is_none()
                            && mount.tree_path() == reference.tree_path().trim_matches('/'))
                })
        {
            return Err(core_error::requires_offline(
                &reference.describe(),
                RestoreMethod::SubvolumeReplacement.as_str(),
            ));
        }
        let through =
            Self::mount_on_filesystem(action, &mounts, reference, Path::new(&destination))?;
        for other in [&source, &aside] {
            let mount = Self::mount_on_filesystem(action, &mounts, reference, Path::new(other))?;
            if mount.mount_point() != through.mount_point() {
                return Err(core_error::recovery_apply_failed(
                    action.summary(),
                    &format!(
                        "{other} is reached through {} and {destination} through {}; rename(2) \
                         between two mounts fails with EXDEV. Nothing was changed",
                        mount.mount_point(),
                        through.mount_point()
                    ),
                ));
            }
        }
        let lineage = self.confirm_lineage(action, asset, reference, &destination, &source)?;
        Self::accept_displacement(
            action,
            reference,
            &lineage.live,
            &lineage.snapshot,
            acceptance,
        )?;
        if self.files.exists(Path::new(&aside))? {
            return Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!(
                    "{aside} already exists, and moving {destination} there would replace it. \
                     Nothing was changed"
                ),
            ));
        }
        self.files
            .rename(Path::new(&destination), Path::new(&aside))?;
        if let Err(error) = self
            .files
            .rename(Path::new(&source), Path::new(&destination))
        {
            let restored = self
                .files
                .rename(Path::new(&aside), Path::new(&destination));
            return Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!(
                    "{} was moved aside to {aside}, and putting {source} in its place failed: {}. \
                     {}",
                    destination,
                    error.message(),
                    match restored {
                        Ok(()) => format!("{aside} was moved back to {destination}"),
                        Err(back) => format!(
                            "moving it back failed too ({}), so the live subvolume is at {aside}",
                            back.message()
                        ),
                    }
                ),
            ));
        }
        Ok(())
    }

    /// Points the default subvolume at the derived one (§14.6, Appendix D.9).
    fn restore_set_default(
        &self,
        action: &PlanAction,
        asset: &RecoveryAsset,
        reference: &SubvolumeRef,
        arguments: &[(Arc<str>, Value)],
        acceptance: &RestoreAcceptance,
    ) -> Result<(), ErrorValue> {
        let source = argument(arguments, ARG_SOURCE).unwrap_or_default();
        let mount = argument(arguments, ARG_MOUNT).unwrap_or_default();
        let live_path = argument(arguments, ARG_LIVE).unwrap_or_default();
        Self::same_subvolume(action, reference, arguments)?;
        let mounts = self.mounts()?;
        for path in [&source, &mount, &live_path] {
            Self::mount_on_filesystem(action, &mounts, reference, Path::new(path))?;
        }
        let lineage = self.confirm_lineage(action, asset, reference, &live_path, &source)?;
        Self::accept_displacement(
            action,
            reference,
            &lineage.live,
            &lineage.snapshot,
            acceptance,
        )?;
        // The id `subvolume show` read from the derived subvolume a moment ago, so the default is
        // pointed at the subvolume whose lineage was just confirmed.
        let derived = lineage.derived.id().to_string();
        let output = self.btrfs(&["subvolume", "set-default", &derived, &mount])?;
        if output.succeeded() {
            Ok(())
        } else {
            Err(core_error::recovery_apply_failed(
                action.summary(),
                spoken_text(&output).trim(),
            ))
        }
    }

    /// Refuses an action whose subvolume is not the one the asset was taken of.
    fn same_subvolume(
        action: &PlanAction,
        reference: &SubvolumeRef,
        arguments: &[(Arc<str>, Value)],
    ) -> Result<(), ErrorValue> {
        let tree = argument(arguments, ARG_SUBVOLUME).unwrap_or_default();
        let id = argument(arguments, ARG_SUBVOLUME_ID).unwrap_or_default();
        if tree.trim_matches('/') == reference.tree_path().trim_matches('/')
            && id == reference.id().to_string()
        {
            Ok(())
        } else {
            Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!(
                    "the action names subvolume {id} `{tree}`, and the recovery point it was handed \
                     is {}. Nothing was changed",
                    reference.describe()
                ),
            ))
        }
    }

    /// Confirms what a whole-subvolume step displaces and what it puts in place (§56.2).
    ///
    /// The live path must be the subvolume the recovery point was taken of, by id; the recovery
    /// point must still be there; and the derived subvolume must be a snapshot of it, by parent
    /// UUID. The live and recovery-point metadata come back for the newer-state check.
    fn confirm_lineage(
        &self,
        action: &PlanAction,
        asset: &RecoveryAsset,
        reference: &SubvolumeRef,
        live_path: &str,
        derived_path: &str,
    ) -> Result<Lineage, ErrorValue> {
        let refuse = |detail: String| core_error::recovery_apply_failed(action.summary(), &detail);
        let live = match self.show(Path::new(live_path))? {
            ShowOutcome::Subvolume(show) if show.id() == reference.id() => show,
            outcome => {
                return Err(refuse(format!(
                    "{live_path} is not subvolume {} any more: {}. Nothing was changed",
                    reference.id(),
                    describe_outcome(&outcome)
                )));
            }
        };
        let snapshot = match self.show(Path::new(asset.reference()))? {
            ShowOutcome::Subvolume(show) => show,
            outcome => {
                return Err(refuse(format!(
                    "the recovery point {} is not there: {}. Nothing was changed",
                    asset.reference(),
                    describe_outcome(&outcome)
                )));
            }
        };
        let derived = match self.show(Path::new(derived_path))? {
            ShowOutcome::Subvolume(show) if show.parent_uuid() == Some(snapshot.uuid()) => show,
            ShowOutcome::Subvolume(show) => {
                return Err(refuse(format!(
                    "{derived_path} is subvolume {} with parent uuid {}, and the recovery point's \
                     uuid is {}; it is not derived from this recovery point. Nothing was changed",
                    show.id(),
                    show.parent_uuid().unwrap_or("-"),
                    snapshot.uuid()
                )));
            }
            outcome => {
                return Err(refuse(format!(
                    "the derived subvolume {derived_path} is not there: {}. Nothing was changed",
                    describe_outcome(&outcome)
                )));
            }
        };
        Ok(Lineage {
            live,
            snapshot,
            derived,
        })
    }

    /// Refuses to displace later writes the operator did not accept losing (§24.5, Appendix C.3).
    fn accept_displacement(
        action: &PlanAction,
        reference: &SubvolumeRef,
        live: &SubvolumeShow,
        snapshot: &SubvolumeShow,
        acceptance: &RestoreAcceptance,
    ) -> Result<(), ErrorValue> {
        let Some(item) = classify_subvolume(
            reference.tree_path(),
            live.generation(),
            snapshot.generation(),
        ) else {
            return Ok(());
        };
        if item.class() == NewerStateClass::Unknown {
            return Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!("{}. Nothing was changed", item.detail()),
            ));
        }
        if acceptance.accepts_newer_state_loss() {
            Ok(())
        } else {
            Err(core_error::newer_state_conflict(&[format!(
                "{}: {}",
                item.object(),
                item.detail()
            )]))
        }
    }
}

impl RecoveryProvider for BtrfsProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        let capabilities = RecoveryCapability::REQUIRED.iter().fold(
            ProviderCapabilities::new(PROVIDER_ID),
            |carry, capability| carry.recovering(*capability),
        );
        VALIDATED_VERSIONS
            .iter()
            .fold(capabilities, |carry, version| {
                carry.tested_against("btrfs-progs", *version)
            })
    }

    fn availability(&self) -> ProviderAvailability {
        if !self.runner.is_available(&self.program) {
            return ProviderAvailability::Unavailable {
                reason: Arc::from(format!(
                    "`{}` is not installed, so no Btrfs subvolume can be inspected or snapshotted",
                    self.program
                )),
            };
        }
        match self.version() {
            Ok(version) if VALIDATED_VERSIONS.contains(&version.series().as_str()) => {
                ProviderAvailability::Available {
                    version: Arc::from(version.raw()),
                }
            }
            Ok(version) => ProviderAvailability::Unsupported {
                version: Arc::from(version.raw()),
                reason: Arc::from(format!(
                    "this provider has been validated against btrfs-progs {} and not against {}. \
                     Appendix G.4: it degrades rather than executing semantics it has not tested",
                    VALIDATED_VERSIONS.join(", "),
                    version.series()
                )),
            },
            Err(error) => ProviderAvailability::Unavailable {
                reason: Arc::from(format!(
                    "the version of `{}` could not be established: {}",
                    self.program,
                    error.message()
                )),
            },
        }
    }

    fn resolve_domain(&self, path: &str) -> Result<Option<PersistenceDomain>, ErrorValue> {
        let target = PathBuf::from(path);
        let mounts = self.mounts()?;
        let Some(mount) = mounts.covering(&target) else {
            return Ok(None);
        };
        let mount_point = PathBuf::from(mount.mount_point());
        let info = self.filesystem_info(&mount_point)?;
        if let Some(known) = mount.filesystem_uuid()
            && known != info.uuid()
        {
            return Err(fact_not_established(
                SafetyFact::FilesystemAndSubvolumeId,
                &format!(
                    "the mount at {} was identified as Btrfs filesystem {known}, and `btrfs \
                     filesystem show {}` now answers {}",
                    mount.mount_point(),
                    mount.mount_point(),
                    info.uuid()
                ),
            ));
        }
        let outcome = self.show(&target)?;
        let layout = SubvolumeLayout::new(
            self.boundaries(&mount_point)?,
            mounts.same_filesystem_as(mount),
        );
        let (boundary, evidence) = match &outcome {
            ShowOutcome::Subvolume(show) => match layout.by_id(show.id()) {
                Some(boundary) => (
                    boundary,
                    format!(
                        "`btrfs subvolume show` reports the path as subvolume {} itself",
                        show.id()
                    ),
                ),
                None => {
                    return Err(fact_not_established(
                        SafetyFact::FilesystemAndSubvolumeId,
                        &format!(
                            "`btrfs subvolume show {path}` reports subvolume {}, which does not \
                             appear in `btrfs subvolume list` for the same filesystem",
                            show.id()
                        ),
                    ));
                }
            },
            ShowOutcome::PlainDirectory => match layout.containing(&target) {
                Some(boundary) => (
                    boundary,
                    "`btrfs subvolume show` answers `Not a Btrfs subvolume`, so the path is an \
                     ordinary directory and its state lives in the subvolume containing it — a \
                     name that resembles a subvolume is not evidence of one (Appendix B.9)"
                        .to_owned(),
                ),
                None => return Err(no_btrfs_mount(path)),
            },
            ShowOutcome::Missing => match layout.containing(&target) {
                Some(boundary) => (
                    boundary,
                    "the path does not exist yet, so its state will land in the subvolume that \
                     contains it"
                        .to_owned(),
                ),
                None => return Err(no_btrfs_mount(path)),
            },
            ShowOutcome::Refused(reason) => {
                return Err(command_failed("btrfs subvolume show", reason));
            }
        };
        let reference = SubvolumeRef::new(info.uuid(), boundary.id(), boundary.tree_path());
        let nested = layout.nested_within(boundary).len();
        let detail = format!(
            "{}; {evidence}{}",
            reference.describe(),
            if nested == 0 {
                String::new()
            } else {
                format!(
                    ". {nested} subvolume(s) are nested inside it, and a snapshot of it holds \
                     none of them (§14.3)"
                )
            }
        );
        Ok(Some(
            PersistenceDomain::resolved(
                path,
                resolved_mount(mount),
                SCOPE_KIND,
                reference.reference(),
                detail,
            )
            .with_boundary(boundary.tree_path()),
        ))
    }

    // Two spellings of a Btrfs domain reach this method. `resolve_domain`'s own names the
    // subvolume by filesystem UUID, id and tree path. The shell's generic resolver
    // (`ono_change_protection::domain`) names the mount's `subvol=` option — `/@var` — which is
    // neither tied to a filesystem nor, for a path inside a nested subvolume, the subvolume that
    // holds the path. That spelling is resolved again here from the path, and the superblock the
    // two resolutions went through must agree. Any other object is refused with the text quoted:
    // an empty list would read as "nothing to protect here" (§55.6 case 29).
    fn discover(
        &self,
        domain: &PersistenceDomain,
        objective: RecoveryObjective,
    ) -> Result<Vec<RecoveryCandidate>, ErrorValue> {
        if domain.object_kind() != SCOPE_KIND || !domain.is_protectable() {
            return Ok(Vec::new());
        }
        let object = domain.object().unwrap_or_default();
        let (reference, mount_point) = if let Some(reference) = SubvolumeRef::parse(object) {
            (reference, PathBuf::from(domain.mount().mount_point()))
        } else if object.starts_with('/') {
            let resolved = self
                .resolve_domain(domain.path())?
                .ok_or_else(|| no_btrfs_mount(domain.path()))?;
            if resolved.mount().mount_id() != domain.mount().mount_id() {
                return Err(fact_not_established(
                    SafetyFact::FilesystemAndSubvolumeId,
                    &format!(
                        "the domain for {} was resolved through the mount at {} on superblock {}, \
                         and this provider finds the path on the mount at {} on superblock {}; \
                         the two resolutions disagree about which filesystem holds it",
                        domain.path(),
                        domain.mount().mount_point(),
                        domain.mount().mount_id(),
                        resolved.mount().mount_point(),
                        resolved.mount().mount_id()
                    ),
                ));
            }
            let reference = resolved
                .object()
                .and_then(SubvolumeRef::parse)
                .ok_or_else(|| no_btrfs_mount(domain.path()))?;
            (reference, PathBuf::from(resolved.mount().mount_point()))
        } else {
            return Err(fact_not_established(
                SafetyFact::FilesystemAndSubvolumeId,
                &format!(
                    "the Btrfs domain for {} names the object `{object}`, which is neither a \
                     subvolume reference `<filesystem-uuid>:<id>:<tree-path>` nor a `subvol=` \
                     path, so no subvolume could be discovered from it",
                    domain.path()
                ),
            ));
        };
        let mounts = self.mounts()?;
        let serving = mounts
            .covering(&mount_point)
            .and_then(BtrfsMount::filesystem_uuid);
        if serving != Some(reference.filesystem()) {
            return Err(fact_not_established(
                SafetyFact::FilesystemAndSubvolumeId,
                &format!(
                    "the domain names Btrfs filesystem {} and the mount at {} is {}",
                    reference.filesystem(),
                    mount_point.display(),
                    serving.map_or_else(
                        || "not one whose filesystem UUID is established".to_owned(),
                        |uuid| format!("filesystem {uuid}")
                    )
                ),
            ));
        }
        let layout = self.layout(&mount_point)?;
        let usage = self.filesystem_usage(&mount_point)?;
        let Some(boundary) = layout.by_id(reference.id()) else {
            return Err(fact_not_established(
                SafetyFact::FilesystemAndSubvolumeId,
                &format!(
                    "subvolume {} does not appear in `btrfs subvolume list` for {}, so there is \
                     no metadata to protect it from",
                    reference.id(),
                    mount_point.display()
                ),
            ));
        };
        Ok(vec![self.candidate_for(
            &layout,
            reference.filesystem(),
            boundary,
            &[domain.path()],
            objective,
            usage,
        )])
    }

    fn plan_protection(
        &self,
        candidates: &[RecoveryCandidate],
        mode: ProtectionMode,
    ) -> Result<Vec<ProtectionAction>, ErrorValue> {
        self.plan_protection_for(candidates, mode, self.plan.as_ref(), self.now())
    }

    fn create(&self, action: &ProtectionAction) -> Result<RecoveryAsset, ErrorValue> {
        let proposed = action.proposed_asset();
        let Some(reference) = SubvolumeRef::parse(proposed.scope().domain()) else {
            return Err(snapshot_failed(
                proposed.scope().domain(),
                "the action's scope does not name a Btrfs subvolume",
            ));
        };
        let mounts = self.mounts()?;
        let Some(live) = self.live_path(&mounts, &reference) else {
            return Err(snapshot_failed(
                proposed.scope().domain(),
                &format!(
                    "{} is not mounted anywhere this process can see, so there is nothing to \
                     snapshot from. Nothing was changed",
                    reference.describe()
                ),
            ));
        };
        if !live.named_by_mount {
            // Reached through a parent's mount, so the path is confirmed to be the subvolume
            // rather than taken from its shape (Appendix B.9).
            let confirmed = match self.show(&live.path) {
                Ok(ShowOutcome::Subvolume(show)) if show.id() == reference.id() => Ok(()),
                Ok(outcome) => Err(describe_outcome(&outcome)),
                Err(error) => Err(diagnosis(&error)),
            };
            if let Err(detail) = confirmed {
                return Err(snapshot_failed(
                    proposed.scope().domain(),
                    &format!(
                        "{} is reached through the mount at {} and could not be confirmed to be \
                         subvolume {}: {detail}. Nothing was changed",
                        live.path.display(),
                        live.mount.mount_point(),
                        reference.id()
                    ),
                ));
            }
        }
        let destination = proposed.reference().to_owned();
        if mounts
            .covering(Path::new(&destination))
            .and_then(BtrfsMount::filesystem_uuid)
            != Some(reference.filesystem())
        {
            return Err(snapshot_failed(
                proposed.scope().domain(),
                &format!(
                    "{destination} is not on Btrfs filesystem {}, and a snapshot can only be \
                     created on the filesystem of its source (Appendix D.8). Nothing was changed",
                    reference.filesystem()
                ),
            ));
        }
        // Appendix D.8: the recovery namespace is a subvolume at the top level of the source's
        // filesystem, and a filesystem Ono has never protected has none. It is made here, as part
        // of the protection that needs it, rather than failing the first snapshot taken on it.
        if let Some(namespace) = Path::new(&destination).parent()
            && std::fs::symlink_metadata(namespace).is_err()
        {
            let namespace_text = namespace.to_string_lossy().into_owned();
            let made = self.btrfs(&["subvolume", "create", &namespace_text])?;
            if !made.succeeded() {
                return Err(snapshot_failed(
                    proposed.scope().domain(),
                    &format!(
                        "the recovery namespace {namespace_text} does not exist and could not be \
                         created: {}. Nothing was changed",
                        spoken_text(&made).trim()
                    ),
                ));
            }
        }
        let source_text = live.path.to_string_lossy().into_owned();
        let output = if self.config.prefers_read_only_snapshots() {
            self.btrfs(&["subvolume", "snapshot", "-r", &source_text, &destination])?
        } else {
            self.btrfs(&["subvolume", "snapshot", &source_text, &destination])?
        };
        if !output.succeeded() {
            return Err(snapshot_failed(
                proposed.scope().domain(),
                spoken_text(&output).trim(),
            ));
        }
        let created = match self.show(Path::new(&destination))? {
            ShowOutcome::Subvolume(show) => show,
            outcome => {
                return Err(snapshot_failed(
                    proposed.scope().domain(),
                    &format!(
                        "`btrfs subvolume snapshot` reported success and {destination} is not a \
                         subvolume: {}",
                        describe_outcome(&outcome)
                    ),
                ));
            }
        };
        // §14.5: the read-only flag is verified rather than assumed. A snapshot that was asked to
        // be read-only and is not would be retained as a recovery point whose contents anything
        // could change, which is the one thing a recovery point may not be.
        let read_only = self.is_read_only(Path::new(&destination))?;
        if self.config.prefers_read_only_snapshots() && !read_only {
            return Err(snapshot_failed(
                proposed.scope().domain(),
                &format!(
                    "the snapshot at {destination} was created and `btrfs property get \
                     {destination} ro` answers ro=false, so it is not the read-only recovery \
                     point §14.5 asks for"
                ),
            ));
        }
        let mut asset = RecoveryAsset::proposed(
            PROVIDER_ID,
            RecoveryAssetType::BtrfsSnapshot,
            destination,
            proposed.scope().clone(),
            created.created_at().unwrap_or_else(|| self.now()),
        );
        if let Some(plan) = proposed.source_plan() {
            asset = asset.for_plan(plan.clone());
        }
        for exclusion in proposed.exclusions() {
            asset = asset.excluding(exclusion.clone());
        }
        Ok(asset
            .excluding(RecoveryExclusion::new(
                SEQUENTIAL_CREATION,
                SEQUENTIAL_CREATION_REASON,
            ))
            .at_consistency(ConsistencyClass::FilesystemConsistent)
            .restored_by(RestoreMethod::SelectiveFileRestore)
            .costing(proposed.cost().clone())
            .retained_for(proposed.retention())
            .capturing(format!("btrfs-subvolume-uuid:{}", created.uuid()))
            .creating())
    }

    // §11.4's five checks, in the order they can be answered. Two of them are worth stating
    // outright. Identity rests on lineage *and* on the read-only flag: a writable snapshot may
    // still name the subvolume it came from and no longer hold what it captured, so under §14.5's
    // default it is not the identity the plan recorded. Permissions is `true` on the strength of
    // the queries having answered at all — reading a subvolume's metadata is itself privileged,
    // and an unprivileged caller reaches the `Refused` arm above, which is an error rather than a
    // validation that failed (§56.3).
    fn validate(&self, asset: &RecoveryAsset) -> Result<RecoveryValidation, ErrorValue> {
        let snapshot_path = PathBuf::from(asset.reference());
        let detail_prefix = format!("recovery point {}", asset.reference());
        let outcome = self.show(&snapshot_path)?;
        let snapshot = match &outcome {
            ShowOutcome::Subvolume(show) => show,
            ShowOutcome::Refused(reason) => {
                return Err(command_failed("btrfs subvolume show", reason));
            }
            other => {
                return Ok(RecoveryValidation::none(
                    self.now(),
                    format!(
                        "{detail_prefix} is not there: {} (§11.4)",
                        describe_outcome(other)
                    ),
                ));
            }
        };
        let read_only = self.is_read_only(&snapshot_path)?;
        let mounts = self.mounts()?;
        let reference = SubvolumeRef::parse(asset.scope().domain());
        let live = match &reference {
            Some(reference) => match self.live_path(&mounts, reference) {
                Some(live) => match self.show(&live.path)? {
                    ShowOutcome::Subvolume(show) => Some((show, live.mount)),
                    _ => None,
                },
                None => None,
            },
            None => None,
        };
        let identity = snapshot.parent_uuid().is_some_and(|parent| {
            live.as_ref()
                .is_some_and(|(source, _)| parent == source.uuid())
        }) && (read_only || !self.config.prefers_read_only_snapshots());
        let scope_matches = reference
            .as_ref()
            .zip(live.as_ref())
            .is_some_and(|(reference, (source, _))| source.id() == reference.id());
        let restore_available = live
            .as_ref()
            .is_some_and(|(_, mount)| !mount.is_read_only());
        Ok(RecoveryValidation::complete(
            self.now(),
            format!(
                "{detail_prefix} is subvolume {} (ro={read_only}), taken from {}",
                snapshot.id(),
                snapshot
                    .parent_uuid()
                    .unwrap_or("no subvolume it could name"),
            ),
        )
        .existing(true)
        .identity(identity)
        .scope(scope_matches)
        .restore(restore_available)
        .permissions(true))
    }

    fn plan_recovery(
        &self,
        asset: &RecoveryAsset,
        source: Option<&ChangePlan>,
        goal: RecoveryGoal,
    ) -> Result<RecoveryPlanFragment, ErrorValue> {
        self.plan_recovery_with_checklist(asset, source, goal)
            .map(|(fragment, _)| fragment)
    }

    fn restore(&self, action: &PlanAction, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        self.carry_out(action, asset, &RestoreAcceptance::none())
    }

    // Every action `plan_recovery` emits is performed here, including the derivation of the
    // writable subvolume, and each whole-subvolume step re-reads what it displaces at the moment
    // it runs. Displacing a subvolume written since the recovery point needs the operator's
    // `--accept-newer-state-loss`; a restore of named objects is gated by the plan's own
    // newer-state analysis and is not refused here.
    fn restore_with(
        &self,
        action: &PlanAction,
        asset: &RecoveryAsset,
        acceptance: &RestoreAcceptance,
    ) -> Result<RestoreOutcome, ErrorValue> {
        // §14.5: a derived subvolume is an asset of its own, and only the store can give it a
        // lifecycle, so the ones this action derived travel back in the outcome.
        let before = self.derived_assets().len();
        self.carry_out(action, asset, acceptance)?;
        Ok(self
            .derived_assets()
            .into_iter()
            .skip(before)
            .fold(RestoreOutcome::default(), RestoreOutcome::creating))
    }

    fn cleanup(&self, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        let reference = PathBuf::from(asset.reference());
        let Some(owner) = SubvolumeRef::parse(asset.scope().domain()) else {
            return Err(core_error::asset_invalid(
                asset.id(),
                &[
                    "the asset's scope names no filesystem and subvolume, so which recovery \
                   namespace it belongs to could not be established (§56.2)",
                ],
            ));
        };
        let mounts = self.mounts()?;
        let namespace =
            self.recovery_namespace(&mounts, owner.filesystem(), asset.scope().domain())?;
        if reference.parent() != Some(namespace.as_path()) {
            return Err(core_error::asset_invalid(
                asset.id(),
                &[
                    "the asset is not inside this provider's recovery namespace, and §37 removes \
                     the objects this provider created rather than any subvolume it is pointed at",
                ],
            ));
        }
        // Exactly one path, and no recursive flag: `btrfs subvolume delete` takes what it is
        // given, and a recursive removal here would take the nested subvolumes §14.3 keeps
        // separate with it.
        let output = self.btrfs(&["subvolume", "delete", asset.reference()])?;
        if output.succeeded() {
            Ok(())
        } else {
            Err(command_failed(
                "btrfs subvolume delete",
                spoken_text(&output).trim(),
            ))
        }
    }

    fn estimate_cost(&self, asset: &RecoveryAsset) -> Result<RecoveryCost, ErrorValue> {
        let mounts = self.mounts()?;
        let reference = SubvolumeRef::parse(asset.scope().domain());
        let query_mount = reference
            .as_ref()
            .and_then(|reference| self.live_path(&mounts, reference))
            .map(|live| PathBuf::from(live.mount.mount_point()))
            .or_else(|| {
                mounts
                    .covering(Path::new(asset.reference()))
                    .map(|mount| PathBuf::from(mount.mount_point()))
            })
            .ok_or_else(|| no_btrfs_mount(asset.reference()))?;
        // The figure is read even though neither space field can be filled from it: a filesystem
        // that no longer answers is a cost question that has changed its answer, and §37.5 would
        // rather refuse than report yesterday's number.
        let _usage = self.filesystem_usage(&query_mount)?;
        let mut cost = snapshot_cost();
        let is_root = reference
            .as_ref()
            .and_then(|reference| self.live_path(&mounts, reference))
            .is_some_and(|live| mounts.is_root_subvolume(&live.mount));
        if is_root && self.config.root_recovery().requires_reboot() {
            cost = cost.needing_reboot();
        }
        if is_root && self.config.root_recovery().requires_offline() {
            cost = cost.needing_offline();
        }
        Ok(cost)
    }
}

/// The cost of a Btrfs snapshot: never zero, never exact (§37.5, §38.2).
///
/// A snapshot shares every extent with its source at creation, and what deleting it would free
/// depends on what has been rewritten since. Without quota groups the filesystem cannot answer
/// that at all — the recorded `quota-disabled.txt` is what it says when asked — so both figures
/// are null and the estimate flag is set. §38.2 forbids the alternative: showing a
/// copy-on-write snapshot as free.
#[must_use]
pub fn snapshot_cost() -> RecoveryCost {
    RecoveryCost::unknown().with_space(None, None, true)
}

/// The exclusion a nested subvolume produces (§14.3, §55.4 case 18).
#[must_use]
pub fn nested_exclusion(boundary: &SubvolumeBoundary) -> RecoveryExclusion {
    RecoveryExclusion::new(
        format!(
            "the nested subvolume {} ({})",
            boundary.tree_path(),
            boundary.id()
        ),
        "§14.3: a snapshot of a parent subvolume does not contain the live contents of a nested \
         subvolume. Inside the snapshot this path is an empty directory — which is what the \
         recorded `nested-live.txt` and `nested-in-snapshot.txt` pair shows — so protecting it \
         needs a snapshot of its own",
    )
}

/// What a tool refusal said, message and remediation together.
///
/// The code is on the error and the sentence a person acts on is in its help, so a checklist
/// reason that carried only the message would drop `Operation not permitted` — the one word that
/// tells an operator the recovery is blocked on privilege rather than on the filesystem.
fn diagnosis(error: &ErrorValue) -> String {
    match error.help() {
        Some(help) => format!("{} ({help})", error.message()),
        None => error.message().to_owned(),
    }
}

/// §56.3's refusal, from a checklist with something outstanding.
fn refusal(checklist: &SafetyChecklist) -> ErrorValue {
    checklist.refusal().unwrap_or_else(|| {
        fact_not_established(
            SafetyFact::SelectiveRestorePossible,
            "the recovery could not be planned",
        )
    })
}

/// A recovery operation, as the structured execution §2.17 requires.
fn recovery_operation(
    capability: RecoveryCapability,
    operation: &str,
    arguments: Vec<(&str, Value)>,
) -> Execution {
    let mut typed: Vec<(Arc<str>, Value)> =
        vec![(Arc::from(ARG_OPERATION), Value::string(operation))];
    for (name, value) in arguments {
        typed.push((Arc::from(name), value));
    }
    Execution::RecoveryOperation {
        provider: Arc::from(PROVIDER_ID),
        capability: Arc::from(capability.as_str()),
        arguments: typed,
    }
}

/// The text of one argument of a recovery operation.
fn argument(arguments: &[(Arc<str>, Value)], name: &str) -> Option<String> {
    arguments
        .iter()
        .find(|(key, _)| key.as_ref() == name)
        .and_then(|(_, value)| match value {
            Value::String(text) => Some(text.to_string()),
            _ => None,
        })
}

/// A [`ShowOutcome`] in a sentence, for a refusal a person reads.
fn describe_outcome(outcome: &ShowOutcome) -> String {
    match outcome {
        ShowOutcome::Subvolume(show) => format!("it is subvolume {}", show.id()),
        ShowOutcome::PlainDirectory => {
            "`btrfs` answers `Not a Btrfs subvolume`, so it is an ordinary directory".to_owned()
        }
        ShowOutcome::Missing => "the path does not exist".to_owned(),
        ShowOutcome::Refused(reason) => format!("`btrfs` refused: {reason}"),
    }
}

/// The mount as `ono-change-core` models it (Appendix B.1).
fn resolved_mount(mount: &BtrfsMount) -> ResolvedMount {
    let mut options: Vec<Arc<str>> =
        vec![Arc::from(if mount.is_read_only() { "ro" } else { "rw" })];
    if let Some(id) = mount.subvolume_id() {
        options.push(Arc::from(format!("subvolid={id}")));
    }
    if let Some(subvolume) = mount.subvolume() {
        options.push(Arc::from(format!("subvol={subvolume}")));
    }
    ResolvedMount::new(
        mount.device().to_owned(),
        mount.mount_point().to_owned(),
        "btrfs",
        mount.source().to_owned(),
        mount.root().to_owned(),
    )
    .with_options(options)
}

/// What a method means for the question "must the subvolume be replaced?" (§56.2).
fn replacement_sentence(method: RestoreMethod) -> String {
    match method {
        RestoreMethod::SelectiveFileRestore => "replacing the subvolume is not required, because \
             the wanted objects can be read out of it one at a time"
            .to_owned(),
        RestoreMethod::CloneAndCopy => "replacing the subvolume is not required, because the \
             recovery point is materialised as a separate writable subvolume and the wanted state \
             copied out of it"
            .to_owned(),
        RestoreMethod::SubvolumeReplacement => "replacing the subvolume is required, and it needs \
             the filesystem offline (§14.4)"
            .to_owned(),
        RestoreMethod::OfflineRootRecovery => "replacing the running root subvolume is required, \
             and it takes effect on the next boot (§14.6, Appendix D.9)"
            .to_owned(),
        other => format!("the method is {}", other.as_str()),
    }
}

/// What the filesystem's default subvolume is, relative to the one being recovered (§56.2).
fn default_description(default: &DefaultSubvolume, reference: &SubvolumeRef) -> String {
    if default.is_filesystem_tree() {
        format!(
            "the filesystem's default subvolume is its top level (id {})",
            default.id()
        )
    } else if default.id() == reference.id() {
        format!(
            "the filesystem's default subvolume is {}, the one being recovered",
            default.id()
        )
    } else {
        format!(
            "the filesystem's default subvolume is {}{}, which is not the one being recovered",
            default.id(),
            default
                .tree_path()
                .map_or_else(String::new, |path| format!(" ({path})"))
        )
    }
}

/// What a recovery that is not a next-boot recovery of the root does to what boots (§56.2).
fn boot_impact(method: Option<RestoreMethod>, tree_path: &str) -> String {
    if method == Some(RestoreMethod::SubvolumeReplacement) {
        format!(
            "This recovery leaves the default subvolume alone and gives the recovered subvolume \
             the name `{tree_path}`: a boot entry that selects `{tree_path}` by name boots it, and \
             one that selects by id or by default does not"
        )
    } else {
        "This recovery does not change the default subvolume or any subvolume's name, so it does \
         not change what boots"
            .to_owned()
    }
}

/// The arguments of a swap: the three paths, and the subvolume they are about.
fn swap_arguments(
    swap: &SwapPaths,
    identity: Vec<(&'static str, Value)>,
) -> Vec<(&'static str, Value)> {
    let mut arguments = vec![
        (ARG_SOURCE, Value::string(&swap.derived.to_string_lossy())),
        (ARG_DESTINATION, Value::string(&swap.live.to_string_lossy())),
        (ARG_ASIDE, Value::string(&swap.aside.to_string_lossy())),
    ];
    arguments.extend(identity);
    arguments
}

/// What happens to subvolumes nested in the tree of one being replaced (§14.3).
///
/// They are children of the subvolume in the filesystem tree, so they move aside with it, and the
/// derived subvolume holds an empty directory where each one was.
fn moved_aside_note(layout: Option<&SubvolumeLayout>, reference: &SubvolumeRef) -> String {
    let Some(layout) = layout else {
        return String::new();
    };
    let Some(boundary) = layout.by_id(reference.id()) else {
        return String::new();
    };
    let children: Vec<String> = layout
        .boundaries()
        .iter()
        .filter(|candidate| candidate.is_nested_in(boundary))
        .map(|nested| format!("{} ({})", nested.tree_path(), nested.id()))
        .collect();
    if children.is_empty() {
        String::new()
    } else {
        format!(
            ". The nested {} move aside with it, and the derived subvolume holds an empty \
             directory where each one was (§14.3)",
            children.join(", ")
        )
    }
}

/// Whether an exclusion's subject names `tree_path` as a whole path rather than as part of one.
///
/// `the nested subvolume @var/lib-app (260)` names `@var/lib-app` and does not name `@var`.
fn names_subvolume(subject: &str, tree_path: &str) -> bool {
    let wanted = tree_path.trim_matches('/');
    if wanted.is_empty() {
        return false;
    }
    subject.match_indices(wanted).any(|(at, _)| {
        let before = subject.get(..at).and_then(|text| text.chars().next_back());
        let after = subject
            .get(at + wanted.len()..)
            .and_then(|text| text.chars().next());
        before.is_none_or(|character| character.is_whitespace() || character == '(')
            && after.is_none_or(|character| {
                character.is_whitespace() || matches!(character, ')' | ',' | ';' | ':')
            })
    })
}

/// §56.2's mount and reboot fact, as evidence (§14.6).
fn mount_evidence(
    mount: &BtrfsMount,
    live_path: &Path,
    is_root: bool,
    method: Option<RestoreMethod>,
    policy: RootRecovery,
) -> String {
    let where_it_is = format!(
        "{} is mounted at {} ({}), and the mount is {}",
        live_path.display(),
        mount.mount_point(),
        mount.subvolume().map_or_else(
            || "no subvol= option".to_owned(),
            |subvol| format!("subvol={subvol}")
        ),
        if mount.is_read_only() {
            "read-only, so nothing could be written back into it (Appendix G.2)"
        } else {
            "writable"
        }
    );
    let requirement = match method {
        Some(RestoreMethod::SelectiveFileRestore) => {
            "recovery writes into the mounted subvolume and needs neither an unmount nor a reboot"
                .to_owned()
        }
        Some(RestoreMethod::CloneAndCopy) => "recovery materialises the recovery point beside the \
             live subvolume and copies out of it, needing neither an unmount nor a reboot"
            .to_owned(),
        Some(RestoreMethod::SubvolumeReplacement) => format!(
            "recovery replaces the subvolume, which requires it to be unmounted first{}",
            if is_root {
                " — and for the root subvolume that means an offline window under the \
                 `offline-replacement` policy"
            } else {
                ""
            }
        ),
        Some(RestoreMethod::OfflineRootRecovery) => format!(
            "recovery selects the subvolume the machine boots into, so it takes effect on the \
             next reboot under the `{}` policy (§55.4 case 22)",
            policy.token()
        ),
        Some(other) => format!("the method is {}", other.as_str()),
        None => "no method was selected".to_owned(),
    };
    format!("{where_it_is}. {requirement}")
}

/// What restoring from a candidate would need (§43.4, §14.6).
fn restore_requirement(is_root: bool, policy: RootRecovery) -> String {
    if is_root {
        format!(
            "the live subvolume mounted read-write for a selective restore; a whole-subvolume \
             recovery of the root follows the `{}` policy of §14.6, which {}",
            policy.token(),
            if policy.requires_reboot() {
                "takes effect on the next boot"
            } else if policy.requires_offline() {
                "needs the filesystem offline"
            } else {
                "stays online and restores the named objects"
            }
        )
    } else {
        "the live subvolume mounted read-write; replacing it instead of restoring into it needs \
         it unmounted (§14.4)"
            .to_owned()
    }
}

impl BtrfsProvider {
    /// Everything [`RecoveryProvider::restore_with`] does, answering only whether it was done.
    fn carry_out(
        &self,
        action: &PlanAction,
        asset: &RecoveryAsset,
        acceptance: &RestoreAcceptance,
    ) -> Result<(), ErrorValue> {
        let Execution::RecoveryOperation {
            provider,
            arguments,
            ..
        } = action.execution()
        else {
            return Err(core_error::recovery_apply_failed(
                action.summary(),
                "this action is not a Btrfs recovery operation",
            ));
        };
        if provider.as_ref() != PROVIDER_ID {
            return Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!("the action belongs to {provider}, not to {PROVIDER_ID}"),
            ));
        }
        let Some(reference) = SubvolumeRef::parse(asset.scope().domain()) else {
            return Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!(
                    "the recovery point's scope `{}` names no filesystem and subvolume, so no \
                     step could be tied to one (§56.2). Nothing was changed",
                    asset.scope().domain()
                ),
            ));
        };
        let operation = argument(arguments, ARG_OPERATION).unwrap_or_default();
        let source = argument(arguments, ARG_SOURCE).unwrap_or_default();
        let destination = argument(arguments, ARG_DESTINATION).unwrap_or_default();
        match operation.as_str() {
            OP_DERIVE_WRITABLE => {
                self.restore_derive(action, asset, &reference, &source, &destination)
            }
            OP_RESTORE_FILE => self.restore_file(action, &reference, &source, &destination),
            OP_REPLACE_SUBVOLUME => {
                self.restore_swap(action, asset, &reference, arguments, acceptance, true)
            }
            OP_SWAP_FOR_NEXT_BOOT => {
                self.restore_swap(action, asset, &reference, arguments, acceptance, false)
            }
            OP_SET_DEFAULT => {
                self.restore_set_default(action, asset, &reference, arguments, acceptance)
            }
            other => Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!("`{other}` is not an operation this provider performs"),
            )),
        }
    }
}
