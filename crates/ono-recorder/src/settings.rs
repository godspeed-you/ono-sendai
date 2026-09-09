//! What the recorder was asked to do (v0.5 §10.4, §33).
//!
//! §10.4 fixes five figures a first `start recorder` uses, and §33 names the settings they live
//! in. They are values here rather than reads of a configuration file because the recorder is
//! handed its settings by the command layer, and because a start that carries settings the
//! running recorder is not using has to be able to say *which* (§10.8, ADR-0640).

use ono_temporal_ledger::RetentionPolicy;
use ono_value::{ByteSize, Duration};

/// `temporal.recording.enabled` — §10.2 defaults it to false.
pub const SETTING_ENABLED: &str = "temporal.recording.enabled";
/// `temporal.retention.max_age` — §10.4.
pub const SETTING_MAX_AGE: &str = "temporal.retention.max_age";
/// `temporal.retention.max_size` — §10.4.
pub const SETTING_MAX_SIZE: &str = "temporal.retention.max_size";
/// `temporal.checkpoint.interval` — §10.4, §31.9.
pub const SETTING_CHECKPOINT_INTERVAL: &str = "temporal.checkpoint.interval";
/// `temporal.flush.interval` — §10.4, §32.5.
pub const SETTING_FLUSH_INTERVAL: &str = "temporal.flush.interval";
/// `temporal.session.max_events` — §10.7.
pub const SETTING_SESSION_MAX_EVENTS: &str = "temporal.session.max_events";
/// `temporal.record.process_argv` — §30.4, off by default.
pub const SETTING_PROCESS_ARGV: &str = "temporal.record.process_argv";

/// §10.4's `24h`.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_nanoseconds(24 * 3_600 * 1_000_000_000);
/// §10.4's `512MiB`.
pub const DEFAULT_MAX_SIZE: ByteSize = ByteSize::from_bytes(512 * 1024 * 1024);
/// §10.4's `5m`, which §31.9 also states as the checkpoint cadence.
pub const DEFAULT_CHECKPOINT_INTERVAL: Duration =
    Duration::from_nanoseconds(5 * 60 * 1_000_000_000);
/// §10.4's `2s`, which §32.5 makes the batching window.
pub const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_nanoseconds(2 * 1_000_000_000);
/// §10.7's `100000`.
pub const DEFAULT_SESSION_MAX_EVENTS: usize = 100_000;
/// How many events one source's bounded queue holds before §43.2's gap begins.
pub const DEFAULT_INTAKE_CAPACITY: usize = 1_024;
/// How many bounded sweeps one drive of retention performs (§31.8).
pub const DEFAULT_MAX_SWEEP_PASSES: usize = 8;

/// The settings one running recorder is using (§33).
#[derive(Debug, Clone, PartialEq)]
pub struct RecorderSettings {
    /// `temporal.recording.enabled`.
    pub enabled: bool,
    /// `temporal.retention.max_age`.
    pub max_age: Duration,
    /// `temporal.retention.max_size`.
    pub max_size: ByteSize,
    /// `temporal.checkpoint.interval`.
    pub checkpoint_interval: Duration,
    /// `temporal.flush.interval`.
    pub flush_interval: Duration,
    /// `temporal.session.max_events`.
    pub session_max_events: usize,
    /// `temporal.record.process_argv` (§30.4).
    pub record_process_argv: bool,
    /// How many events one source's bounded queue holds (§43.1).
    pub intake_capacity: usize,
    /// How many events one bounded sweep removes (§31.8).
    pub retention_batch: usize,
    /// How many bounded sweeps one drive performs before yielding (§31.8).
    pub max_sweep_passes: usize,
}

impl Default for RecorderSettings {
    /// The values §10.4 gives a user who starts the recorder without custom settings.
    ///
    /// `enabled` is true here because these are the settings a *start* applies; §10.2's default
    /// is that no start has happened, which [`crate::Recorder::new`] represents by not running.
    fn default() -> Self {
        Self {
            enabled: true,
            max_age: DEFAULT_MAX_AGE,
            max_size: DEFAULT_MAX_SIZE,
            checkpoint_interval: DEFAULT_CHECKPOINT_INTERVAL,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
            session_max_events: DEFAULT_SESSION_MAX_EVENTS,
            record_process_argv: false,
            intake_capacity: DEFAULT_INTAKE_CAPACITY,
            retention_batch: ono_temporal_ledger::DEFAULT_RETENTION_BATCH,
            max_sweep_passes: DEFAULT_MAX_SWEEP_PASSES,
        }
    }
}

impl RecorderSettings {
    /// The retention policy these settings are, for the store to enforce (§10.4).
    #[must_use]
    pub fn retention(&self) -> RetentionPolicy {
        RetentionPolicy::default()
            .with_max_age(Some(self.max_age))
            .with_max_size(Some(self.max_size))
            .with_batch(self.retention_batch)
    }

    /// Every setting `requested` states differently from these (§10.8).
    ///
    /// This is what makes an idempotent start distinguishable from one that cannot be satisfied:
    /// an empty answer means the running recorder is already doing what was asked.
    #[must_use]
    pub fn differences(&self, requested: &Self) -> Vec<SettingDifference> {
        let mut differences = Vec::new();
        let mut note = |setting: &'static str, running: String, wanted: String| {
            if running != wanted {
                differences.push(SettingDifference {
                    setting,
                    running,
                    requested: wanted,
                });
            }
        };
        note(
            SETTING_ENABLED,
            self.enabled.to_string(),
            requested.enabled.to_string(),
        );
        note(
            SETTING_MAX_AGE,
            self.max_age.to_string(),
            requested.max_age.to_string(),
        );
        note(
            SETTING_MAX_SIZE,
            self.max_size.to_string(),
            requested.max_size.to_string(),
        );
        note(
            SETTING_CHECKPOINT_INTERVAL,
            self.checkpoint_interval.to_string(),
            requested.checkpoint_interval.to_string(),
        );
        note(
            SETTING_FLUSH_INTERVAL,
            self.flush_interval.to_string(),
            requested.flush_interval.to_string(),
        );
        note(
            SETTING_SESSION_MAX_EVENTS,
            self.session_max_events.to_string(),
            requested.session_max_events.to_string(),
        );
        note(
            SETTING_PROCESS_ARGV,
            self.record_process_argv.to_string(),
            requested.record_process_argv.to_string(),
        );
        differences
    }
}

/// One setting a running recorder holds differently from what a start asked for (§10.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingDifference {
    /// The §33 key.
    pub setting: &'static str,
    /// What the running recorder is using.
    pub running: String,
    /// What the start asked for.
    pub requested: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_name_the_setting_when_a_start_asks_for_something_else() {
        let running = RecorderSettings::default();
        let requested = RecorderSettings {
            max_size: ByteSize::from_bytes(1024),
            ..RecorderSettings::default()
        };

        let differences = running.differences(&requested);

        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].setting, SETTING_MAX_SIZE);
        assert!(running.differences(&running).is_empty());
    }
}
