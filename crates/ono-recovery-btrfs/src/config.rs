//! The `[recovery.btrfs]` settings of §53, as values the provider is constructed with.
//!
//! §53 gives this provider three keys — `enabled`, `prefer_read_only_snapshots` and
//! `root_recovery` — and one sentence that decides how they are modelled: *"Configuration MUST
//! NOT silently weaken explicit plan requirements."* So configuration selects between methods
//! that are all honest, and no key can turn a refusal into a claim. `root_recovery` chooses which
//! of §14.6's three root workflows the provider proposes; it cannot make a root recovery happen
//! without the reboot the chosen workflow needs.
//!
//! The snapshot location is here too, because Appendix D.8 makes it a policy decision with a
//! correctness rule attached: a predictable namespace on the same filesystem, and never
//! underneath a subvolume that is being snapshotted.

use std::sync::Arc;

/// Which of §14.6's three root workflows the provider proposes (§53's `root_recovery`).
///
/// §14.6 requires the three to be distinguished rather than blurred into one word, and the names
/// here are the spec's own. The default is §53's: `next-boot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RootRecovery {
    /// Read the wanted files out of the snapshot while the system keeps running (§14.6).
    OnlineSelectiveRestore,
    /// Replace the root subvolume with the filesystem unmounted (§14.6).
    OfflineSubvolumeReplacement,
    /// Point the next boot at a subvolume derived from the snapshot (§14.6, Appendix D.9).
    #[default]
    NextBoot,
}

impl RootRecovery {
    /// The token `recovery.btrfs.root_recovery` spells the setting with (§53).
    ///
    /// These are the spellings `ono-change-protection`'s settings accept and print —
    /// `online-selective`, `offline-replacement`, `next-boot` — so a value read from the
    /// configuration file is the same text this provider names in a plan.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            RootRecovery::OnlineSelectiveRestore => "online-selective",
            RootRecovery::OfflineSubvolumeReplacement => "offline-replacement",
            RootRecovery::NextBoot => "next-boot",
        }
    }

    /// The name of the §14.6 workflow the setting selects, as a plan shows it.
    #[must_use]
    pub const fn workflow(self) -> &'static str {
        match self {
            RootRecovery::OnlineSelectiveRestore => "online selective restore",
            RootRecovery::OfflineSubvolumeReplacement => "offline subvolume replacement",
            RootRecovery::NextBoot => "next-boot recovery",
        }
    }

    /// The setting a token names, or `None` when the token is not one of the three.
    ///
    /// The settings' spellings are the tokens; the longer `online-selective-restore` and
    /// `offline-subvolume-replacement` this provider used before are still read, so a value
    /// written against either reads the same. An unrecognised value is `None` rather than the
    /// default: §53 forbids configuration silently weakening a plan requirement, and quietly
    /// reading an unknown root policy as "next-boot" is exactly that.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "online-selective" | "online-selective-restore" => {
                Some(RootRecovery::OnlineSelectiveRestore)
            }
            "offline-replacement" | "offline-subvolume-replacement" => {
                Some(RootRecovery::OfflineSubvolumeReplacement)
            }
            "next-boot" => Some(RootRecovery::NextBoot),
            _ => None,
        }
    }

    /// Whether the workflow only takes effect after a reboot (§55.4 case 22).
    #[must_use]
    pub const fn requires_reboot(self) -> bool {
        matches!(self, RootRecovery::NextBoot)
    }

    /// Whether the workflow needs the filesystem unmounted first (§14.6).
    #[must_use]
    pub const fn requires_offline(self) -> bool {
        matches!(self, RootRecovery::OfflineSubvolumeReplacement)
    }
}

/// The recovery namespace snapshots are created in, and the policy around them (§53, Appendix D.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtrfsConfig {
    snapshot_location: Arc<str>,
    prefer_read_only_snapshots: bool,
    root_recovery: RootRecovery,
    offline_replacement_permitted: bool,
}

/// The recovery namespace the fixtures — and this provider by default — use (Appendix D.8).
///
/// It is a subvolume at the top level of the filesystem, so it is on the same Btrfs filesystem as
/// its sources (Appendix D.8's "same Btrfs filesystem") and underneath none of them.
pub const DEFAULT_SNAPSHOT_LOCATION: &str = "@snapshots";

impl Default for BtrfsConfig {
    fn default() -> Self {
        Self {
            snapshot_location: Arc::from(DEFAULT_SNAPSHOT_LOCATION),
            prefer_read_only_snapshots: true,
            root_recovery: RootRecovery::NextBoot,
            offline_replacement_permitted: true,
        }
    }
}

impl BtrfsConfig {
    /// Sets the recovery namespace, as a path inside the filesystem tree (Appendix D.8).
    #[must_use]
    pub fn snapshots_in(mut self, location: impl Into<Arc<str>>) -> Self {
        self.snapshot_location = location.into();
        self
    }

    /// Sets §53's `prefer_read_only_snapshots`.
    #[must_use]
    pub const fn preferring_read_only(mut self, prefer: bool) -> Self {
        self.prefer_read_only_snapshots = prefer;
        self
    }

    /// Sets §53's `root_recovery`.
    #[must_use]
    pub const fn recovering_root_by(mut self, policy: RootRecovery) -> Self {
        self.root_recovery = policy;
        self
    }

    /// Sets whether a non-root subvolume may be recovered by taking it offline (§14.4).
    ///
    /// Where it may not, a domain restore falls back to materialising the snapshot as a separate
    /// writable subvolume and copying state out of it — Appendix C.1's `CLONE_AND_COPY`, which
    /// leaves the live subvolume mounted and in place.
    #[must_use]
    pub const fn permitting_offline_replacement(mut self, permitted: bool) -> Self {
        self.offline_replacement_permitted = permitted;
        self
    }

    /// The recovery namespace, as a path inside the filesystem tree.
    #[must_use]
    pub fn snapshot_location(&self) -> &str {
        &self.snapshot_location
    }

    /// Whether retained snapshots are read-only (§14.5).
    #[must_use]
    pub const fn prefers_read_only_snapshots(&self) -> bool {
        self.prefer_read_only_snapshots
    }

    /// The root workflow (§14.6).
    #[must_use]
    pub const fn root_recovery(&self) -> RootRecovery {
        self.root_recovery
    }

    /// Whether a non-root subvolume may be replaced offline.
    #[must_use]
    pub const fn permits_offline_replacement(&self) -> bool {
        self.offline_replacement_permitted
    }

    /// Whether the recovery namespace sits underneath `subvolume` (Appendix D.8).
    ///
    /// The comparison is by path component, so `@snapshots` is not read as living inside `@snap`.
    #[must_use]
    pub fn is_nested_in(&self, subvolume: &str) -> bool {
        let location = self.snapshot_location.trim_matches('/');
        let source = subvolume.trim_matches('/');
        if source.is_empty() {
            // The filesystem's top level contains everything, and Appendix D.8's confusion is
            // about a *source subvolume*: a snapshot of the top level is not something this
            // provider takes.
            return false;
        }
        location
            .strip_prefix(source)
            .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// Sanitises a provider-generated snapshot name (§43.6).
///
/// §43.6 requires generated asset names to carry no user-controlled command syntax. The rule here
/// is a whitelist rather than an escape: letters, digits, `-`, `_` and `.` survive, everything
/// else becomes `-`, and a leading `.` or `-` is dropped so the name can never look like an
/// option or a hidden file. A name that empties out becomes `ono`, because an empty path
/// component is the one result that would make the snapshot land on its parent directory.
///
/// The argument vector of [`ono_change_core::ToolRunner`] already means nothing here is
/// interpreted by a shell (§12.3). This is the second half: the name is also safe to print, to
/// store and to hand back to `btrfs` as a path component.
#[must_use]
pub fn sanitised_name(raw: &str) -> String {
    let mapped: String = raw
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = mapped.trim_start_matches(['.', '-']).trim_end_matches('.');
    if trimmed.is_empty() {
        "ono".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// The snapshot name for `subvolume` under `plan`, as Appendix D.6's `destination_path` needs it.
///
/// The shape is `ono-<plan>-<subvolume>`, which is the fixtures' own (`ono-a82f-root`,
/// `ono-a82f-var`). It is predictable, which is what Appendix D.8 asks of the namespace, and it
/// carries the plan id, so §37's cleanup can tell one plan's assets from another's.
///
/// The whole tree path goes into the name, each `/` becoming `-`, so `@var/cache` and `@cache`
/// are `ono-<plan>-var-cache` and `ono-<plan>-cache`. Two subvolumes whose paths sanitise to the
/// same text (`@var/cache` and `@var-cache`) still collide; `btrfs subvolume snapshot` then
/// refuses the second one because the destination exists (`snapshot-exists.txt`), so a
/// collision is a refused protection rather than one snapshot standing in for two.
#[must_use]
pub fn snapshot_name(plan: &str, subvolume: &str) -> String {
    let bare = subvolume.trim_matches('/').trim_start_matches('@');
    // `@` is the conventional name of the root subvolume and strips to nothing; the fixtures'
    // own `ono-a82f-root` is what a person reads it as.
    let name = if bare.is_empty() { "root" } else { bare };
    format!("ono-{}-{}", sanitised_name(plan), sanitised_name(name))
}
