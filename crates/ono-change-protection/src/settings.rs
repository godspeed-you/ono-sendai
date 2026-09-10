//! §53's configuration, typed, with its defaults exactly as the specification writes them.
//!
//! §53 prints a reference configuration and then ends with one sentence that outranks the rest of
//! it:
//!
//! > Configuration MUST NOT silently weaken explicit plan requirements.
//!
//! [`ChangeSettings::protection_mode_for`] is that sentence. A plan sealed with
//! `--protection require` (§17.3) stays `require` when the configuration says `prefer`, because
//! the effective mode is the stricter of the two rather than the last one read. The same rule
//! runs through [`ChangeSettings::policy_for`], which builds the [`ProtectionPolicy`] a plan runs
//! under out of the configuration and the plan's own explicit requirement.
//!
//! `change.profile` is the one key §53 does not print. It selects one of Appendix H's profiles, and
//! [`ChangeSettings::read`] applies it by tightening only (Appendix H.5, ADR-0810): the protection
//! mode is the stricter of the configured one and the profile's, retention the longer, the
//! free-space floor the stricter, opaque actions are disabled, and the risk gate is the profile's
//! `high+` or `moderate+`. No path through it lowers anything an operator or a plan stated, and
//! [`ChangeSettings::profile_expansion`] shows what it changed and what it left alone, because
//! Appendix H forbids a profile to hide semantics.
//!
//! Every key is read through a lookup function rather than from a file, so the CLI can wire the
//! real settings source later and a test can state the configuration it means. A key that is
//! absent takes its §53 default; a key whose value is not of the right shape is a structured
//! error, because a misread configuration is how a plan silently ends up with less protection
//! than the operator asked for.

use std::sync::Arc;
use std::time::Duration;

use ono_change_core::{
    DEFAULT_RETENTION, ProtectionLevel, ProtectionMode, RetentionPolicy, RiskClass, StrategyKind,
};
use ono_core::ErrorCode;
use ono_value::{ByteSize, ErrorValue, Percent, Value};

use crate::policy::{
    CostLimits, FreeSpaceFloor, Profile, ProtectionPolicy, effective_mode, mode_rank,
};

/// How Btrfs root recovery is carried out by default (§14.6, §53).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootRecovery {
    /// §14.6: put the wanted objects back while the system runs.
    OnlineSelectiveRestore,
    /// §14.6: replace the subvolume with the filesystem offline.
    OfflineSubvolumeReplacement,
    /// §14.6 and §53's default: swap the subvolume so the next boot uses it.
    NextBoot,
}

impl RootRecovery {
    /// Every method §14.6 distinguishes.
    pub const ALL: &'static [RootRecovery] = &[
        RootRecovery::OnlineSelectiveRestore,
        RootRecovery::OfflineSubvolumeReplacement,
        RootRecovery::NextBoot,
    ];

    /// The spelling §53 uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RootRecovery::OnlineSelectiveRestore => "online-selective",
            RootRecovery::OfflineSubvolumeReplacement => "offline-replacement",
            RootRecovery::NextBoot => "next-boot",
        }
    }

    /// Reads it back from configuration.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|method| method.as_str() == name)
    }
}

/// The §53 configuration keys, typed.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeSettings {
    default_protection: ProtectionMode,
    default_strategy: StrategyKind,
    high_risk_requires_ack: bool,
    critical_risk_requires_ack: bool,
    allow_opaque_actions: bool,
    bulk_warn_targets: usize,
    bulk_high_risk_targets: usize,
    retention: Duration,
    max_auto_snapshot_count: usize,
    min_filesystem_free: FreeSpaceFloor,
    prefer_read_only_snapshots: bool,
    zfs_enabled: bool,
    zfs_prefer_selective_restore: bool,
    zfs_allow_destructive_rollback: bool,
    btrfs_enabled: bool,
    btrfs_prefer_read_only_snapshots: bool,
    btrfs_root_recovery: RootRecovery,
    profile: Option<Profile>,
    configured: Configured,
    protection_stated: Stated,
}

/// Whether the operator wrote `change.default_protection`, which decides whether a plan may
/// lower it (§17.3, §53, ADR-0834).
///
/// Provenance, read through [`ChangeSettings::protection_mode_for`], and part of equality like
/// every other field. The reference check in `xtask/src/change.rs::check_settings` compares
/// [`ChangeSettings::entries`] rather than whole settings, so it does not see it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Stated(bool);

/// What the keys a profile governs said before the profile was applied (Appendix H.5).
///
/// Kept so the expansion can show where each value in force came from: an operator reading
/// `require` needs to know whether they wrote it or the profile raised it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Configured {
    protection: ProtectionMode,
    retention: Duration,
    floor: FreeSpaceFloor,
    opaque: bool,
    high_ack: bool,
    critical_ack: bool,
    strategy: StrategyKind,
}

/// Where a value in a profile's expansion came from (Appendix H).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The configuration already stated it, at least as strictly as the profile asks.
    Configuration,
    /// The profile stated it, or raised what the configuration stated.
    Profile(Profile),
}

/// One setting a profile expands to, with the value in force and its source (Appendix H).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSetting {
    /// The configuration key, or Appendix H's own label where the setting has no §53 key.
    pub key: &'static str,
    /// What the configuration said, where the setting is a configuration key.
    pub configured: Option<String>,
    /// What the profile asks for, in Appendix H's words.
    pub profile: String,
    /// The value in force, or `None` where another layer applies it: the strategy is the
    /// planner's (§28.4) and `remote unknown` the executor's (§29.3), and a value this reader
    /// does not apply is not one it may claim is in force.
    pub effective: Option<String>,
    /// Which of the two the value in force came from.
    pub source: Source,
}

/// Every key §53 defines, in the order it prints them, then `change.profile`, the extension
/// that selects an Appendix H profile.
pub const KEYS: &[&str] = &[
    "change.default_protection",
    "change.default_strategy",
    "change.high_risk_requires_ack",
    "change.critical_risk_requires_ack",
    "change.allow_opaque_actions",
    "change.bulk.warn_targets",
    "change.bulk.high_risk_targets",
    "recovery.retention",
    "recovery.max_auto_snapshot_count",
    "recovery.min_filesystem_free",
    "recovery.prefer_read_only_snapshots",
    "recovery.zfs.enabled",
    "recovery.zfs.prefer_selective_restore",
    "recovery.zfs.allow_destructive_rollback",
    "recovery.btrfs.enabled",
    "recovery.btrfs.prefer_read_only_snapshots",
    "recovery.btrfs.root_recovery",
    "change.profile",
];

impl Default for ChangeSettings {
    fn default() -> Self {
        Self::defaults()
    }
}

impl ChangeSettings {
    /// §53's reference configuration, verbatim.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            default_protection: ProtectionMode::Prefer,
            default_strategy: StrategyKind::Sequential,
            high_risk_requires_ack: true,
            critical_risk_requires_ack: true,
            allow_opaque_actions: false,
            bulk_warn_targets: 10,
            bulk_high_risk_targets: 50,
            retention: DEFAULT_RETENTION,
            max_auto_snapshot_count: 32,
            min_filesystem_free: FreeSpaceFloor::Share(Percent::new(10.0)),
            prefer_read_only_snapshots: true,
            zfs_enabled: true,
            zfs_prefer_selective_restore: true,
            zfs_allow_destructive_rollback: false,
            btrfs_enabled: true,
            btrfs_prefer_read_only_snapshots: true,
            btrfs_root_recovery: RootRecovery::NextBoot,
            profile: None,
            configured: Configured {
                protection: ProtectionMode::Prefer,
                retention: DEFAULT_RETENTION,
                floor: FreeSpaceFloor::Share(Percent::new(10.0)),
                opaque: false,
                high_ack: true,
                critical_ack: true,
                strategy: StrategyKind::Sequential,
            },
            protection_stated: Stated(false),
        }
    }

    /// Reads the settings a `lookup` provides, defaulting every key it does not answer.
    ///
    /// # Errors
    ///
    /// `type.mismatch` or `type.invalid_unit` when a key carries a value of the wrong shape. §53's
    /// closing sentence is why this is an error rather than a fallback: a configuration nobody can
    /// read is a configuration that quietly stops applying, and the one it stops applying might be
    /// the requirement that mattered.
    pub fn from_settings(lookup: &dyn Fn(&str) -> Option<Value>) -> Result<Self, ErrorValue> {
        let (settings, problems) = Self::read(lookup);
        match problems.into_iter().next() {
            Some(problem) => Err(problem),
            None => Ok(settings),
        }
    }

    /// Reads every §53 key `lookup` answers, keeping each one it can read and reporting the rest.
    ///
    /// §53 asks for a configuration nobody can read to be reported rather than silently ignored,
    /// and the report is the second half of the answer. A key that cannot be read keeps its
    /// default; every other key keeps the value the operator gave it. Throwing all of them away
    /// for one typo would quietly drop a protection requirement that was written correctly.
    #[must_use]
    pub fn read(lookup: &dyn Fn(&str) -> Option<Value>) -> (Self, Vec<ErrorValue>) {
        let mut settings = Self::defaults();
        let mut problems = Vec::new();
        let take = |key: &str| lookup(key);
        // Written at all, even unreadably: a mode the operator meant to state is not one a plan may
        // lower, and a refused value keeps the default without making it lowerable (ADR-0834).
        settings.protection_stated = Stated(take("change.default_protection").is_some());
        if let Some(mode) = field(&mut problems, take("change.default_protection"), |value| {
            let text = text_of("change.default_protection", value)?;
            ProtectionMode::from_name(&text).ok_or_else(|| {
                invalid(
                    "change.default_protection",
                    &text,
                    "one of off, prefer, require or maximize (§17.2)",
                )
            })
        }) {
            settings.default_protection = mode;
        }
        if let Some(kind) = field(&mut problems, take("change.default_strategy"), |value| {
            let text = text_of("change.default_strategy", value)?;
            // §28.4 writes a strategy with its parameter — `batch 2`, `canary 1 then 10%` — and
            // the kind is its first word. The parameters are the planner's to read.
            let kind = text.split_whitespace().next().unwrap_or_default();
            StrategyKind::from_name(kind).ok_or_else(|| {
                invalid(
                    "change.default_strategy",
                    &text,
                    "one of sequential, batch, canary or parallel (§28.4)",
                )
            })
        }) {
            settings.default_strategy = kind;
        }
        if let Some(flag) = field(
            &mut problems,
            take("change.high_risk_requires_ack"),
            flag_of,
        ) {
            settings.high_risk_requires_ack = flag;
        }
        if let Some(flag) = field(
            &mut problems,
            take("change.critical_risk_requires_ack"),
            flag_of,
        ) {
            settings.critical_risk_requires_ack = flag;
        }
        if let Some(flag) = field(&mut problems, take("change.allow_opaque_actions"), flag_of) {
            settings.allow_opaque_actions = flag;
        }
        if let Some(count) = field(&mut problems, take("change.bulk.warn_targets"), |value| {
            count_of("change.bulk.warn_targets", value)
        }) {
            settings.bulk_warn_targets = count;
        }
        if let Some(count) = field(
            &mut problems,
            take("change.bulk.high_risk_targets"),
            |value| count_of("change.bulk.high_risk_targets", value),
        ) {
            settings.bulk_high_risk_targets = count;
        }
        if let Some(retention) = field(&mut problems, take("recovery.retention"), |value| {
            duration_of("recovery.retention", value)
        }) {
            settings.retention = retention;
        }
        if let Some(count) = field(
            &mut problems,
            take("recovery.max_auto_snapshot_count"),
            |value| count_of("recovery.max_auto_snapshot_count", value),
        ) {
            settings.max_auto_snapshot_count = count;
        }
        if let Some(floor) = field(
            &mut problems,
            take("recovery.min_filesystem_free"),
            |value| floor_of("recovery.min_filesystem_free", value),
        ) {
            settings.min_filesystem_free = floor;
        }
        if let Some(flag) = field(
            &mut problems,
            take("recovery.prefer_read_only_snapshots"),
            flag_of,
        ) {
            settings.prefer_read_only_snapshots = flag;
        }
        if let Some(flag) = field(&mut problems, take("recovery.zfs.enabled"), flag_of) {
            settings.zfs_enabled = flag;
        }
        if let Some(flag) = field(
            &mut problems,
            take("recovery.zfs.prefer_selective_restore"),
            flag_of,
        ) {
            settings.zfs_prefer_selective_restore = flag;
        }
        if let Some(flag) = field(
            &mut problems,
            take("recovery.zfs.allow_destructive_rollback"),
            flag_of,
        ) {
            settings.zfs_allow_destructive_rollback = flag;
        }
        if let Some(flag) = field(&mut problems, take("recovery.btrfs.enabled"), flag_of) {
            settings.btrfs_enabled = flag;
        }
        if let Some(flag) = field(
            &mut problems,
            take("recovery.btrfs.prefer_read_only_snapshots"),
            flag_of,
        ) {
            settings.btrfs_prefer_read_only_snapshots = flag;
        }
        if let Some(method) = field(
            &mut problems,
            take("recovery.btrfs.root_recovery"),
            |value| {
                let text = text_of("recovery.btrfs.root_recovery", value)?;
                RootRecovery::from_name(&text).ok_or_else(|| {
                    invalid(
                        "recovery.btrfs.root_recovery",
                        &text,
                        "one of online-selective, offline-replacement or next-boot (§14.6)",
                    )
                })
            },
        ) {
            settings.btrfs_root_recovery = method;
        }
        if let Some(profile) = field(&mut problems, take("change.profile"), profile_of) {
            settings.profile = profile;
        }
        settings.configured = Configured {
            protection: settings.default_protection,
            retention: settings.retention,
            floor: settings.min_filesystem_free,
            opaque: settings.allow_opaque_actions,
            high_ack: settings.high_risk_requires_ack,
            critical_ack: settings.critical_risk_requires_ack,
            strategy: settings.default_strategy,
        };
        if let Some(profile) = settings.profile {
            settings.tighten_with(profile);
        }
        (settings, problems)
    }

    /// Applies `profile` in the one direction Appendix H.5 permits (ADR-0810).
    ///
    /// Every step is a maximum over the configured value and the profile's, so a stricter value
    /// the operator wrote survives and the profile only fills in where it asks for more. Opaque
    /// actions are disabled because H.1 – H.3 say so and none of the four says otherwise, and the
    /// `high+` gate every profile carries restores §19.4's acknowledgements a configuration
    /// switched off.
    fn tighten_with(&mut self, profile: Profile) {
        if mode_rank(profile.mode()) > mode_rank(self.default_protection) {
            self.default_protection = profile.mode();
        }
        self.retention = self.retention.max(profile.retention());
        self.min_filesystem_free = self
            .min_filesystem_free
            .stricter_of(profile.limits().min_filesystem_free());
        self.allow_opaque_actions = false;
        self.high_risk_requires_ack = true;
        self.critical_risk_requires_ack = true;
    }

    /// The configured default protection mode (§17.1).
    #[must_use]
    pub const fn default_protection(&self) -> ProtectionMode {
        self.default_protection
    }

    /// The configured default strategy (§28.4).
    #[must_use]
    pub const fn default_strategy(&self) -> StrategyKind {
        self.default_strategy
    }

    /// Whether a high-risk plan needs an acknowledgement (§19.4).
    #[must_use]
    pub const fn high_risk_requires_ack(&self) -> bool {
        self.high_risk_requires_ack
    }

    /// Whether a critical-risk plan needs an acknowledgement (§19.4).
    #[must_use]
    pub const fn critical_risk_requires_ack(&self) -> bool {
        self.critical_risk_requires_ack
    }

    /// Whether opaque actions may be planned at all (§6.2, §6.3).
    #[must_use]
    pub const fn allow_opaque_actions(&self) -> bool {
        self.allow_opaque_actions
    }

    /// How many targets make a bulk plan worth warning about (§28.3).
    #[must_use]
    pub const fn bulk_warn_targets(&self) -> usize {
        self.bulk_warn_targets
    }

    /// How many targets make a bulk plan high risk (§28.3).
    #[must_use]
    pub const fn bulk_high_risk_targets(&self) -> usize {
        self.bulk_high_risk_targets
    }

    /// How long recovery assets are retained after a successful verification (§37.1).
    #[must_use]
    pub const fn retention(&self) -> Duration {
        self.retention
    }

    /// The retention policy that follows from it (§37.1).
    #[must_use]
    pub const fn retention_policy(&self) -> RetentionPolicy {
        RetentionPolicy::of(self.retention)
    }

    /// How many automatic snapshots one plan may propose (§38.3).
    #[must_use]
    pub const fn max_auto_snapshot_count(&self) -> usize {
        self.max_auto_snapshot_count
    }

    /// The free-space floor automatic protection stays above (Appendix D.3).
    #[must_use]
    pub const fn min_filesystem_free(&self) -> FreeSpaceFloor {
        self.min_filesystem_free
    }

    /// Whether snapshots are made read-only where the mechanism allows it (§14.2).
    #[must_use]
    pub const fn prefer_read_only_snapshots(&self) -> bool {
        self.prefer_read_only_snapshots
    }

    /// Whether the ZFS recovery provider is enabled (§13).
    #[must_use]
    pub const fn zfs_enabled(&self) -> bool {
        self.zfs_enabled
    }

    /// Whether ZFS recovery prefers selective restore over rollback (§13.5, Appendix C.1).
    #[must_use]
    pub const fn zfs_prefer_selective_restore(&self) -> bool {
        self.zfs_prefer_selective_restore
    }

    /// Whether a destructive `zfs rollback` may be planned automatically (§13.6, §24.5).
    #[must_use]
    pub const fn zfs_allow_destructive_rollback(&self) -> bool {
        self.zfs_allow_destructive_rollback
    }

    /// Whether the Btrfs recovery provider is enabled (§14).
    #[must_use]
    pub const fn btrfs_enabled(&self) -> bool {
        self.btrfs_enabled
    }

    /// Whether Btrfs snapshots are made read-only (§14.2).
    #[must_use]
    pub const fn btrfs_prefer_read_only_snapshots(&self) -> bool {
        self.btrfs_prefer_read_only_snapshots
    }

    /// How Btrfs root recovery is carried out (§14.6).
    #[must_use]
    pub const fn btrfs_root_recovery(&self) -> RootRecovery {
        self.btrfs_root_recovery
    }

    /// The Appendix H profile in force, or `None` where `change.profile` chose none.
    #[must_use]
    pub const fn profile(&self) -> Option<Profile> {
        self.profile
    }

    /// Whether a command may stop and ask (Appendix H.4, §17.4, §40.3).
    ///
    /// `false` under `scripted`: every gate is then answered by its flag or refused, at a
    /// terminal as much as in a pipeline.
    #[must_use]
    pub fn prompts(&self) -> bool {
        self.profile.is_none_or(Profile::prompts)
    }

    /// Whether a plan of risk `class` needs §19.4's acknowledgement before it applies.
    ///
    /// HIGH and CRITICAL by §53's two keys; any class at or above the profile's risk gate by
    /// Appendix H — `moderate+` under `cautious` gates MODERATE and UNKNOWN too.
    #[must_use]
    pub fn requires_acknowledgement(&self, class: RiskClass) -> bool {
        let by_profile = self
            .profile
            .is_some_and(|profile| class.max_of(profile.risk_gate()) == class);
        let by_keys = match class {
            RiskClass::High => self.high_risk_requires_ack,
            RiskClass::Critical => self.critical_risk_requires_ack,
            RiskClass::Low | RiskClass::Moderate | RiskClass::Unknown => false,
        };
        by_profile || by_keys
    }

    /// The lowest risk class that needs an acknowledgement, or `None` where none does (§19.4,
    /// Appendix H's `risk gate`).
    ///
    /// [`ChangeSettings::requires_acknowledgement`] is the exact answer per class; this is the
    /// summary Appendix H prints, in §19.2's order where UNKNOWN sits between MODERATE and HIGH.
    #[must_use]
    pub fn risk_gate(&self) -> Option<RiskClass> {
        [
            RiskClass::Low,
            RiskClass::Moderate,
            RiskClass::Unknown,
            RiskClass::High,
            RiskClass::Critical,
        ]
        .into_iter()
        .find(|class| self.requires_acknowledgement(*class))
    }

    /// The strategy the profile names where it differs from §53's default, for the planner.
    ///
    /// Only `fleet` has one (Appendix H.3). It is a default rather than a requirement: a strategy
    /// is how a plan is scheduled, not how much of it is protected, so `--strategy` and an
    /// explicitly configured `change.default_strategy` both come before it, and the planner —
    /// which sees the configuration's layers and the plan's target count — is where it applies.
    #[must_use]
    pub fn profile_strategy(&self) -> Option<&'static str> {
        self.profile
            .map(Profile::strategy)
            .filter(|strategy| *strategy != StrategyKind::Sequential.as_str())
    }

    /// The profile in force, expanded into the settings it stands for, each with the value in
    /// force and where that value came from (Appendix H). Empty where no profile is chosen.
    #[must_use]
    pub fn profile_expansion(&self) -> Vec<ProfileSetting> {
        let Some(profile) = self.profile else {
            return Vec::new();
        };
        let before = self.configured;
        let row = |key: &'static str,
                   configured: Option<String>,
                   asked: String,
                   effective: Option<String>| {
            let source = if configured.is_some() && configured == effective {
                Source::Configuration
            } else {
                Source::Profile(profile)
            };
            ProfileSetting {
                key,
                configured,
                profile: asked,
                effective,
                source,
            }
        };
        let gate = self
            .risk_gate()
            .map_or_else(|| "none".to_owned(), |class| format!("{}+", class.as_str()));
        let mut rows = vec![
            row(
                "change.default_protection",
                Some(before.protection.as_str().to_owned()),
                profile.mode().as_str().to_owned(),
                Some(self.default_protection.as_str().to_owned()),
            ),
            row(
                "recovery.retention",
                Some(format!("{:?}", before.retention)),
                format!("{:?}", profile.retention()),
                Some(format!("{:?}", self.retention)),
            ),
            row(
                "recovery.min_filesystem_free",
                Some(before.floor.describe()),
                profile.limits().min_filesystem_free().describe(),
                Some(self.min_filesystem_free.describe()),
            ),
            row(
                "change.allow_opaque_actions",
                Some(before.opaque.to_string()),
                "false".to_owned(),
                Some(self.allow_opaque_actions.to_string()),
            ),
            row(
                "change.high_risk_requires_ack",
                Some(before.high_ack.to_string()),
                "true".to_owned(),
                Some(self.high_risk_requires_ack.to_string()),
            ),
            row(
                "change.critical_risk_requires_ack",
                Some(before.critical_ack.to_string()),
                "true".to_owned(),
                Some(self.critical_risk_requires_ack.to_string()),
            ),
            row(
                "risk gate",
                None,
                format!("{}+", profile.risk_gate().as_str()),
                Some(gate),
            ),
            row(
                "prompts",
                None,
                if profile.prompts() { "yes" } else { "no" }.to_owned(),
                Some(if self.prompts() { "yes" } else { "no" }.to_owned()),
            ),
            row(
                "change.default_strategy",
                Some(before.strategy.as_str().to_owned()),
                profile.strategy().to_owned(),
                None,
            ),
        ];
        if profile == Profile::Fleet {
            rows.push(row(
                "remote unknown",
                None,
                "stop new batches".to_owned(),
                None,
            ));
        }
        rows
    }

    /// The mode a plan actually runs in (§53's last line, §17.3, ADR-0834).
    ///
    /// Where the operator configured a mode — in a file, in the environment, or through a
    /// profile — the stricter of it and the plan's own explicit requirement. A plan sealed with
    /// `--protection require` keeps `require` under a configuration that says `prefer`, and a
    /// configured `prefer` is not weakened by a plan that says `off`.
    ///
    /// Where nothing was configured, the plan's own mode, and the built-in default only where the
    /// plan states none: §17.3 lets a plan override configuration, and §53's rule is about what an
    /// operator wrote, which a built-in default is not.
    #[must_use]
    pub fn protection_mode_for(&self, requested: Option<ProtectionMode>) -> ProtectionMode {
        if self.protection_stated.0 || self.profile.is_some() {
            effective_mode(self.default_protection, requested)
        } else {
            requested.unwrap_or(self.default_protection)
        }
    }

    /// The §38.3 cost limits these settings imply.
    #[must_use]
    pub fn limits(&self) -> CostLimits {
        CostLimits::default()
            .counted(self.max_auto_snapshot_count)
            .above_floor(self.min_filesystem_free)
    }

    /// The policy a plan runs under, given what the plan itself asked for (§17, §53).
    #[must_use]
    pub fn policy_for(&self, requested: Option<ProtectionMode>) -> ProtectionPolicy {
        let mut policy = ProtectionPolicy::of(self.protection_mode_for(requested))
            .requiring(ProtectionLevel::Protected)
            .retaining(self.retention_policy())
            .limited_by(self.limits());
        if let Some(asked) = requested {
            policy = policy.asked_for(asked);
        }
        policy
    }

    /// The settings as the key/value pairs §53 prints, for `get settings`.
    #[must_use]
    pub fn entries(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "change.default_protection",
                self.default_protection.as_str().to_owned(),
            ),
            (
                "change.default_strategy",
                self.default_strategy.as_str().to_owned(),
            ),
            (
                "change.high_risk_requires_ack",
                self.high_risk_requires_ack.to_string(),
            ),
            (
                "change.critical_risk_requires_ack",
                self.critical_risk_requires_ack.to_string(),
            ),
            (
                "change.allow_opaque_actions",
                self.allow_opaque_actions.to_string(),
            ),
            (
                "change.bulk.warn_targets",
                self.bulk_warn_targets.to_string(),
            ),
            (
                "change.bulk.high_risk_targets",
                self.bulk_high_risk_targets.to_string(),
            ),
            ("recovery.retention", format!("{:?}", self.retention)),
            (
                "recovery.max_auto_snapshot_count",
                self.max_auto_snapshot_count.to_string(),
            ),
            (
                "recovery.min_filesystem_free",
                self.min_filesystem_free.describe(),
            ),
            (
                "recovery.prefer_read_only_snapshots",
                self.prefer_read_only_snapshots.to_string(),
            ),
            ("recovery.zfs.enabled", self.zfs_enabled.to_string()),
            (
                "recovery.zfs.prefer_selective_restore",
                self.zfs_prefer_selective_restore.to_string(),
            ),
            (
                "recovery.zfs.allow_destructive_rollback",
                self.zfs_allow_destructive_rollback.to_string(),
            ),
            ("recovery.btrfs.enabled", self.btrfs_enabled.to_string()),
            (
                "recovery.btrfs.prefer_read_only_snapshots",
                self.btrfs_prefer_read_only_snapshots.to_string(),
            ),
            (
                "recovery.btrfs.root_recovery",
                self.btrfs_root_recovery.as_str().to_owned(),
            ),
            (
                "change.profile",
                self.profile.map_or("none", Profile::as_str).to_owned(),
            ),
        ]
    }
}

/// The text of a configuration value, whatever shape it arrived in.
fn text_of(key: &str, value: &Value) -> Result<Arc<str>, ErrorValue> {
    value
        .as_str()
        .map(Arc::from)
        .map_err(|error| error.with_help(format!("v0.6 §53: `{key}` is written as a string")))
}

/// A non-negative count.
fn count_of(key: &str, value: &Value) -> Result<usize, ErrorValue> {
    let number = value
        .as_int()
        .map_err(|error| error.with_help(format!("v0.6 §53: `{key}` is written as an integer")))?;
    usize::try_from(number)
        .map_err(|_| invalid(key, &number.to_string(), "a count of zero or more"))
}

/// A duration, from a duration value or from §53's `"24h"` text.
fn duration_of(key: &str, value: &Value) -> Result<Duration, ErrorValue> {
    let parsed = match value {
        Value::Duration(duration) => *duration,
        Value::String(text) => ono_value::Duration::parse(text)?,
        other => {
            return Err(invalid(
                key,
                other.type_name(),
                "a duration such as `24h` (§37.1)",
            ));
        }
    };
    let nanoseconds = u64::try_from(parsed.nanoseconds()).map_err(|_| {
        invalid(
            key,
            &parsed.exact(),
            "a duration that does not run backwards",
        )
    })?;
    Ok(Duration::from_nanos(nanoseconds))
}

/// A free-space floor, from a percentage, a byte size or §53's `"10%"` text.
fn floor_of(key: &str, value: &Value) -> Result<FreeSpaceFloor, ErrorValue> {
    match value {
        Value::Percent(share) => Ok(FreeSpaceFloor::Share(*share)),
        Value::ByteSize(size) => Ok(FreeSpaceFloor::Absolute(*size)),
        Value::String(text) if text.trim_end().ends_with('%') => {
            Ok(FreeSpaceFloor::Share(Percent::parse(text)?))
        }
        Value::String(text) => Ok(FreeSpaceFloor::Absolute(ByteSize::parse(text)?)),
        other => Err(invalid(
            key,
            other.type_name(),
            "a percentage such as `10%` or a size such as `20GiB` (§53)",
        )),
    }
}

/// The refusal for a value §53 does not permit.
fn invalid(key: &str, found: &str, expected: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!("the configuration key `{key}` carries `{found}`, which is not {expected}"),
    )
    .with_help(
        "v0.6 §53: configuration MUST NOT silently weaken explicit plan requirements, so a key \
         Ono cannot read is refused rather than ignored",
    )
    .with_metadata("key", Value::string(key))
    .with_metadata("found", Value::string(found))
}

/// One key's value, read by `parse`; a value it refuses is reported and answers nothing.
fn field<T>(
    problems: &mut Vec<ErrorValue>,
    value: Option<Value>,
    parse: impl FnOnce(&Value) -> Result<T, ErrorValue>,
) -> Option<T> {
    let value = value?;
    match parse(&value) {
        Ok(parsed) => Some(parsed),
        Err(problem) => {
            problems.push(problem);
            None
        }
    }
}

/// `change.profile`: one of Appendix H's four names, or `none` for no profile.
fn profile_of(value: &Value) -> Result<Option<Profile>, ErrorValue> {
    let text = text_of("change.profile", value)?;
    let name = text.trim();
    if name.is_empty() || name == "none" {
        return Ok(None);
    }
    Profile::from_name(name).map(Some).ok_or_else(|| {
        invalid(
            "change.profile",
            &text,
            "one of none, interactive, cautious, fleet or scripted (Appendix H)",
        )
    })
}

/// A boolean key, refusing a value of another shape (§53).
fn flag_of(value: &Value) -> Result<bool, ErrorValue> {
    value.as_bool()
}
