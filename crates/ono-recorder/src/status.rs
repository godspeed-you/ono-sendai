//! `get recorder` — what Ono is retaining, under which limits (v0.5 §10.3, §30.1, §43.4).
//!
//! §30.1's intent is the reason every number is here rather than only the ones a user asked
//! about: persistent history must be "impossible to confuse with hidden surveillance", so the
//! status states what is being kept, where, how much of it there is, how far back it reaches,
//! which sources fed it and what has been lost. §43.4 adds the last field: a recorder that is
//! falling behind is visible rather than quietly degraded.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::Timestamp;
use ono_temporal_core::EvidenceSource;
use ono_value::{ByteSize, ErrorValue, Provenance, RecordBuilder, RecordValue, SchemaId, Value};

use crate::settings::RecorderSettings;

/// §43.4's recorder health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderHealth {
    /// Collecting, with nothing lost.
    Healthy,
    /// Collecting, and something was lost or a source stopped answering (§21.8, §43.2).
    Degraded,
    /// Not collecting, because nobody asked it to.
    Stopped,
    /// Not collecting, because it could not (§31.7, §44.3).
    Failed,
}

impl RecorderHealth {
    /// Every value `ono.recorder-status/1` declares.
    pub const ALL: &'static [RecorderHealth] = &[
        RecorderHealth::Healthy,
        RecorderHealth::Degraded,
        RecorderHealth::Stopped,
        RecorderHealth::Failed,
    ];

    /// The name the schema spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RecorderHealth::Healthy => "healthy",
            RecorderHealth::Degraded => "degraded",
            RecorderHealth::Stopped => "stopped",
            RecorderHealth::Failed => "failed",
        }
    }

    /// The health with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|health| health.as_str() == name)
    }
}

/// The `ono.recorder-status/1` record of §10.3.
#[derive(Debug, Clone, PartialEq)]
pub struct RecorderStatus {
    /// Whether the recorder is collecting now (§10.8).
    pub running: bool,
    /// Whether `temporal.recording.enabled` is on, which is a different question (§10.2).
    pub enabled: bool,
    /// When the running recorder started; `None` when it is not running.
    pub since: Option<Timestamp>,
    /// Where retained history lives; `None` when nothing is retained beyond the session (§30.2).
    pub store: Option<PathBuf>,
    /// The limits in force (§10.4).
    pub settings: RecorderSettings,
    /// How many events are retained now.
    pub events: u64,
    /// How much space they occupy, where they occupy any.
    pub size: Option<ByteSize>,
    /// The earliest instant still retained — the boundary `at` refuses beyond (§12.3).
    pub earliest: Option<Timestamp>,
    /// The most recent instant retained.
    pub latest: Option<Timestamp>,
    /// The §7.1 sources the recorder collects from, so §10.6's policy is visible.
    pub sources: Vec<EvidenceSource>,
    /// How many events the bounded queues of §43.1 discarded (§43.2).
    pub dropped: u64,
    /// §43.4's health.
    pub health: RecorderHealth,
    /// How many checkpoint projections are running off the prompt path (§31.9).
    pub checkpoints_in_flight: usize,
    /// Why history stopped, where it did (§44.3, §31.7).
    ///
    /// `health: failed` says persistence is off and the shell is still working; this says what
    /// went wrong, so that a user who runs `get recorder` after the fact learns the same thing
    /// the start told them. `None` whenever there is nothing to explain.
    pub diagnostic: Option<Arc<str>>,
}

impl RecorderStatus {
    /// The status of a recorder nobody has started (§10.2).
    #[must_use]
    pub fn stopped(settings: RecorderSettings, sources: Vec<EvidenceSource>) -> Self {
        Self {
            running: false,
            enabled: false,
            since: None,
            store: None,
            settings,
            events: 0,
            size: None,
            earliest: None,
            latest: None,
            sources,
            dropped: 0,
            health: RecorderHealth::Stopped,
            checkpoints_in_flight: 0,
            diagnostic: None,
        }
    }

    /// Where the store is, for a caller that wants to check §30.2's modes itself.
    #[must_use]
    pub fn store_path(&self) -> Option<&Path> {
        self.store.as_deref()
    }

    /// The `ono.recorder-status/1` record `get recorder` returns (§10.3).
    ///
    /// # Errors
    ///
    /// Returns `ono.provider_schema_violation` where the contract is not in this build, which the
    /// `ono-value` contract test and `cargo xtask spec-check` both prevent from shipping.
    pub fn to_record(&self) -> Result<RecordValue, ErrorValue> {
        let schema_id = SchemaId::new("ono.recorder-status", 1);
        let schema = ono_value::builtin_schemas()
            .get(&schema_id)
            .ok_or_else(|| {
                ErrorValue::new(
                    ono_core::ErrorCode::ProviderSchemaViolation,
                    "the `ono.recorder-status/1` contract is not in this build",
                )
            })?;
        let provenance = Provenance::local("ono.recorder", schema_id);
        let sources: Vec<Value> = self
            .sources
            .iter()
            .map(|source| Value::string(source.as_str()))
            .collect();

        let builder = RecordValue::builder(schema, provenance);
        let builder = put(builder, "running", Value::Bool(self.running));
        let builder = put(builder, "enabled", Value::Bool(self.enabled));
        let builder = put(
            builder,
            "since",
            self.since.map_or(Value::Null, Value::Timestamp),
        );
        let builder = put(
            builder,
            "store",
            self.store
                .as_ref()
                .map_or(Value::Null, |path| Value::Path(path.as_path().into())),
        );
        let builder = put(builder, "max_age", Value::Duration(self.settings.max_age));
        let builder = put(builder, "max_size", Value::ByteSize(self.settings.max_size));
        let builder = put(
            builder,
            "checkpoint_interval",
            Value::Duration(self.settings.checkpoint_interval),
        );
        let builder = put(
            builder,
            "flush_interval",
            Value::Duration(self.settings.flush_interval),
        );
        let builder = put(
            builder,
            "session_max_events",
            Value::Int(i128::try_from(self.settings.session_max_events).unwrap_or(i128::MAX)),
        );
        let builder = put(builder, "events", Value::Int(i128::from(self.events)));
        let builder = put(
            builder,
            "size",
            self.size.map_or(Value::Null, Value::ByteSize),
        );
        let builder = put(
            builder,
            "earliest",
            self.earliest.map_or(Value::Null, Value::Timestamp),
        );
        let builder = put(
            builder,
            "latest",
            self.latest.map_or(Value::Null, Value::Timestamp),
        );
        let builder = put(builder, "sources", Value::List(sources.into()));
        let builder = put(builder, "dropped", Value::Int(i128::from(self.dropped)));
        let builder = put(builder, "health", Value::string(self.health.as_str()));
        let builder = put(
            builder,
            "diagnostic",
            self.diagnostic
                .as_deref()
                .map_or(Value::Null, Value::string),
        );
        Ok(builder.build())
    }
}

/// Sets a field the schema declares; a name it does not is a bug here rather than a caller's.
fn put(builder: RecordBuilder, name: &str, value: Value) -> RecordBuilder {
    let fallback = builder.clone();
    builder.set(name, value).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_every_health_word_back_when_it_is_rendered() {
        for health in RecorderHealth::ALL {
            assert_eq!(RecorderHealth::from_name(health.as_str()), Some(*health));
        }
        assert_eq!(RecorderHealth::from_name("busy"), None);
    }
}
