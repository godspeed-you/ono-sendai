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
//! Every key is read through a lookup function rather than from a file, so the CLI can wire the
//! real settings source later and a test can state the configuration it means. A key that is
//! absent takes its §53 default; a key whose value is not of the right shape is a structured
//! error, because a misread configuration is how a plan silently ends up with less protection
//! than the operator asked for.

use std::sync::Arc;
use std::time::Duration;

use ono_change_core::{
    DEFAULT_RETENTION, ProtectionLevel, ProtectionMode, RetentionPolicy, StrategyKind,
};
use ono_core::ErrorCode;
use ono_value::{ByteSize, ErrorValue, Percent, Value};

use crate::policy::{CostLimits, FreeSpaceFloor, ProtectionPolicy, effective_mode};

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
}

/// Every key §53 defines, in the order it prints them.
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
        let mut settings = Self::defaults();
        if let Some(value) = lookup("change.default_protection") {
            let text = text_of("change.default_protection", &value)?;
            settings.default_protection = ProtectionMode::from_name(&text).ok_or_else(|| {
                invalid(
                    "change.default_protection",
                    &text,
                    "one of off, prefer, require or maximize (§17.2)",
                )
            })?;
        }
        if let Some(value) = lookup("change.default_strategy") {
            let text = text_of("change.default_strategy", &value)?;
            settings.default_strategy = StrategyKind::from_name(&text).ok_or_else(|| {
                invalid(
                    "change.default_strategy",
                    &text,
                    "one of sequential, batch, canary or parallel (§28.4)",
                )
            })?;
        }
        if let Some(value) = lookup("change.high_risk_requires_ack") {
            settings.high_risk_requires_ack = value.as_bool()?;
        }
        if let Some(value) = lookup("change.critical_risk_requires_ack") {
            settings.critical_risk_requires_ack = value.as_bool()?;
        }
        if let Some(value) = lookup("change.allow_opaque_actions") {
            settings.allow_opaque_actions = value.as_bool()?;
        }
        if let Some(value) = lookup("change.bulk.warn_targets") {
            settings.bulk_warn_targets = count_of("change.bulk.warn_targets", &value)?;
        }
        if let Some(value) = lookup("change.bulk.high_risk_targets") {
            settings.bulk_high_risk_targets = count_of("change.bulk.high_risk_targets", &value)?;
        }
        if let Some(value) = lookup("recovery.retention") {
            settings.retention = duration_of("recovery.retention", &value)?;
        }
        if let Some(value) = lookup("recovery.max_auto_snapshot_count") {
            settings.max_auto_snapshot_count =
                count_of("recovery.max_auto_snapshot_count", &value)?;
        }
        if let Some(value) = lookup("recovery.min_filesystem_free") {
            settings.min_filesystem_free = floor_of("recovery.min_filesystem_free", &value)?;
        }
        if let Some(value) = lookup("recovery.prefer_read_only_snapshots") {
            settings.prefer_read_only_snapshots = value.as_bool()?;
        }
        if let Some(value) = lookup("recovery.zfs.enabled") {
            settings.zfs_enabled = value.as_bool()?;
        }
        if let Some(value) = lookup("recovery.zfs.prefer_selective_restore") {
            settings.zfs_prefer_selective_restore = value.as_bool()?;
        }
        if let Some(value) = lookup("recovery.zfs.allow_destructive_rollback") {
            settings.zfs_allow_destructive_rollback = value.as_bool()?;
        }
        if let Some(value) = lookup("recovery.btrfs.enabled") {
            settings.btrfs_enabled = value.as_bool()?;
        }
        if let Some(value) = lookup("recovery.btrfs.prefer_read_only_snapshots") {
            settings.btrfs_prefer_read_only_snapshots = value.as_bool()?;
        }
        if let Some(value) = lookup("recovery.btrfs.root_recovery") {
            let text = text_of("recovery.btrfs.root_recovery", &value)?;
            settings.btrfs_root_recovery = RootRecovery::from_name(&text).ok_or_else(|| {
                invalid(
                    "recovery.btrfs.root_recovery",
                    &text,
                    "one of online-selective, offline-replacement or next-boot (§14.6)",
                )
            })?;
        }
        Ok(settings)
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

    /// The mode a plan actually runs in (§53's last line, §17.3).
    ///
    /// The stricter of the configured default and the plan's own explicit requirement. A plan
    /// sealed with `--protection require` keeps `require` under a configuration that says
    /// `prefer`, and a configuration that says `require` is not weakened by a plan that says
    /// nothing.
    #[must_use]
    pub fn protection_mode_for(&self, requested: Option<ProtectionMode>) -> ProtectionMode {
        effective_mode(self.default_protection, requested)
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
