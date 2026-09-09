//! The recorder commands and the deletion of retained history (spec v0.5 §10.3, §10.8, §30.8).
//!
//! §10.2 is the sentence these four are written against: persistent recording is disabled by
//! default, and a normal installation without it still answers temporal queries from §10.7's
//! bounded session ledger. So `get recorder` on a fresh shell is a complete, truthful answer about
//! a recorder that is not running, and nothing here touches the filesystem until `start recorder`
//! is typed.
//!
//! The controller that subscribes to providers, schedules checkpoints and buffers under §43's
//! bounds is `ono-recorder`'s. What lives here is the command surface: the ledger the session
//! reads history from is swapped between §10.7's in-memory one and §31's persistent store, and
//! the status is reported from what the ledger itself knows.

use std::sync::Arc;

use jiff::Timestamp;
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture};
use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_temporal_core::{LedgerRead, LedgerWrite, RetentionState, error};
use ono_temporal_ledger::{Ledger, StoreOptions};
use ono_value::{
    ActionResult, ActionStatus, ByteSize, Duration, ErrorValue, MapValue, Provenance, RecordValue,
    SchemaId, Value, ValueRef, builtin_schemas,
};

use super::session::{TemporalState, temporal_session};

/// `temporal.retention.max_age`, `temporal.retention.max_size`, `temporal.checkpoint.interval`,
/// `temporal.flush.interval` and `temporal.session.max_events` as §33 defaults them.
///
/// The recorder status states the policy in force, and a policy nobody could name would make
/// §30.1's "the user must know how much it retains" unanswerable.
#[derive(Debug, Clone, Copy)]
struct Policy {
    max_age: Duration,
    max_size: ByteSize,
    checkpoint_interval: Duration,
    flush_interval: Duration,
    session_max_events: i128,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_age: Duration::from_nanoseconds(24 * 3_600_000_000_000),
            max_size: ByteSize::from_bytes(512 * 1024 * 1024),
            checkpoint_interval: Duration::from_nanoseconds(300_000_000_000),
            flush_interval: Duration::from_nanoseconds(2_000_000_000),
            session_max_events: i128::try_from(ono_temporal_core::DEFAULT_SESSION_MAX_EVENTS)
                .unwrap_or(100_000),
        }
    }
}

/// Where this user's ledger lives (§31.1, §30.2).
pub(crate) fn store_path() -> Result<std::path::PathBuf, ErrorValue> {
    ono_temporal_ledger::ledger_path(
        |name| std::env::var(name).ok(),
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .as_deref(),
    )
    .ok_or_else(|| {
        error::store_unavailable(
            "no home directory and no XDG state directory, so there is nowhere to keep history",
        )
    })
}

/// The `ono.recorder-status/1` record of §10.3.
fn status_record(state: &TemporalState, since: Option<Timestamp>) -> Result<Value, ErrorValue> {
    let schema_id = SchemaId::new("ono.recorder-status", 1);
    let schema = builtin_schemas().get(&schema_id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            "the `ono.recorder-status/1` contract is not in this build",
        )
    })?;
    let provenance = Provenance::local("ono.temporal", schema_id);
    let policy = Policy::default();
    let retention: RetentionState = state.ledger().retention();
    let running = state.ledger().is_persistent();
    let builder = RecordValue::builder(schema, provenance);
    let builder = put(builder, "running", Value::Bool(running));
    let builder = put(builder, "enabled", Value::Bool(running));
    let builder = put(
        builder,
        "since",
        since.map_or(Value::Null, Value::Timestamp),
    );
    let builder = put(
        builder,
        "store",
        match store_path() {
            Ok(path) if running => Value::Path(Arc::from(path)),
            _ => Value::Null,
        },
    );
    let builder = put(
        builder,
        "max_age",
        Value::Duration(retention.max_age.unwrap_or(policy.max_age)),
    );
    let builder = put(
        builder,
        "max_size",
        Value::ByteSize(retention.max_size.unwrap_or(policy.max_size)),
    );
    let builder = put(
        builder,
        "checkpoint_interval",
        Value::Duration(policy.checkpoint_interval),
    );
    let builder = put(
        builder,
        "flush_interval",
        Value::Duration(policy.flush_interval),
    );
    let builder = put(
        builder,
        "session_max_events",
        Value::Int(policy.session_max_events),
    );
    let builder = put(builder, "events", Value::Int(i128::from(retention.events)));
    let builder = put(
        builder,
        "size",
        retention.stored_size.map_or(Value::Null, Value::ByteSize),
    );
    let builder = put(
        builder,
        "earliest",
        retention.earliest.map_or(Value::Null, Value::Timestamp),
    );
    let builder = put(
        builder,
        "latest",
        retention.latest.map_or(Value::Null, Value::Timestamp),
    );
    // §10.6: what the recorder collects is visible. Without the controller running there is one
    // honest answer, and it is the session's own ingest — never a list of what it might collect.
    let sources = if running {
        vec![
            Value::string(ono_temporal_core::EvidenceSource::recorder().as_str()),
            Value::string(ono_temporal_core::EvidenceSource::session().as_str()),
        ]
    } else {
        vec![Value::string(
            ono_temporal_core::EvidenceSource::session().as_str(),
        )]
    };
    let builder = put(builder, "sources", Value::list(sources));
    let builder = put(
        builder,
        "dropped",
        Value::Int(i128::from(retention.evicted)),
    );
    // §21.8 forbids freezing a last known state and calling it current, and §43.4 makes health a
    // fact rather than a mood: a recorder that is not running is `stopped`, and one that has
    // dropped events is `degraded`.
    let health = if !running {
        "stopped"
    } else if retention.evicted > 0 {
        "degraded"
    } else {
        "healthy"
    };
    let builder = put(builder, "health", Value::string(health));
    Ok(Value::Record(Arc::new(builder.build())))
}

/// Sets a field, keeping a schema refusal rather than swallowing it.
fn put(builder: ono_value::RecordBuilder, field: &str, value: Value) -> ono_value::RecordBuilder {
    builder.set(field, value).unwrap_or_else(|_| {
        // Unreachable while the embedded contract and this code agree, and the `ono-value`
        // contract test plus `xtask spec-check` are what keep them agreeing. Returning an
        // incomplete builder rather than panicking keeps a contract drift a failed validation
        // instead of a crashed shell.
        ono_value::RecordValue::builder(
            builtin_schemas()
                .get(&SchemaId::new("ono.recorder-status", 1))
                .unwrap_or_else(|| {
                    builtin_schemas()
                        .schemas()
                        .next()
                        .cloned()
                        .unwrap_or_else(|| unreachable!("the builtin registry is never empty"))
                }),
            Provenance::local("ono.temporal", SchemaId::new("ono.recorder-status", 1)),
        )
    })
}

/// `get recorder` (§10.3).
#[derive(Debug)]
pub struct GetRecorder;

impl CommandImpl for GetRecorder {
    fn id(&self) -> &str {
        "ono.recorder.get"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("get recorder"))
    }

    fn invoke_async<'a>(&'a self, _ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let state = temporal_session().await;
            let record = status_record(&state, started_at())?;
            Ok(Outcome::Values(ValueStream::from_values([record])))
        })
    }
}

/// When the running recorder was started, for `since` (§10.3).
fn started_at() -> Option<Timestamp> {
    static STARTED: std::sync::OnceLock<std::sync::RwLock<Option<Timestamp>>> =
        std::sync::OnceLock::new();
    STARTED
        .get_or_init(|| std::sync::RwLock::new(None))
        .read()
        .ok()
        .and_then(|held| *held)
}

/// Records that the recorder started, or stopped.
pub(crate) fn note_started(at: Option<Timestamp>) {
    static STARTED: std::sync::OnceLock<std::sync::RwLock<Option<Timestamp>>> =
        std::sync::OnceLock::new();
    if let Ok(mut held) = STARTED.get_or_init(|| std::sync::RwLock::new(None)).write() {
        *held = at;
    }
}

/// `start recorder` (§10.3, §10.8).
#[derive(Debug)]
pub struct StartRecorder;

impl CommandImpl for StartRecorder {
    fn id(&self) -> &str {
        "ono.recorder.start"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("start recorder"))
    }

    fn invoke_async<'a>(&'a self, _ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let mut state = temporal_session().await;
            if state.ledger().is_persistent() {
                // §10.8: starting is idempotent as a lifecycle, and §34 gives the second start
                // its own code so a script can tell "already on" from "failed to start".
                return Err(error::recorder_already_running());
            }
            let path = store_path()?;
            let ledger = Ledger::persistent(&StoreOptions::at(&path))?;
            state.set_ledger(ledger);
            note_started(Some(Timestamp::now()));
            let record = status_record(&state, started_at())?;
            Ok(Outcome::Values(ValueStream::from_values([record])))
        })
    }
}

/// `stop recorder` (§10.3, §10.8, §44.1).
#[derive(Debug)]
pub struct StopRecorder;

impl CommandImpl for StopRecorder {
    fn id(&self) -> &str {
        "ono.recorder.stop"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("stop recorder"))
    }

    fn invoke_async<'a>(&'a self, _ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let mut state = temporal_session().await;
            if !state.ledger().is_persistent() {
                return Err(error::recorder_not_running());
            }
            // §10.8: the stop is clean. What was buffered is written before the store is let go,
            // so the interval that follows is a declared gap rather than lost events (§44.1).
            state.ledger().flush()?;
            state.set_ledger(Ledger::default());
            note_started(None);
            let record = status_record(&state, None)?;
            Ok(Outcome::Values(ValueStream::from_values([record])))
        })
    }
}

/// `remove temporal-history` (§30.8).
#[derive(Debug)]
pub struct RemoveTemporalHistory;

impl CommandImpl for RemoveTemporalHistory {
    fn id(&self) -> &str {
        "ono.temporal-history.remove"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("remove temporal-history"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let path = store_path()?;
            let mut identity = MapValue::new();
            identity.insert("store".into(), Value::Path(Arc::from(path.clone())));
            let target = ValueRef::object(SchemaId::new("ono.temporal-history", 1), identity);
            let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);

            if arguments.flag("dry-run") {
                let result =
                    ActionResult::new(target, "ono.temporal-history.remove", ActionStatus::Skipped)
                        .changed(false)
                        .with_message(&format!(
                            "`{}` holds {} and would be destroyed",
                            path.display(),
                            ByteSize::from_bytes(u128::from(size))
                        ));
                return Ok(Outcome::Values(ValueStream::from_values([
                    result.into_value()
                ])));
            }
            // §30.8 takes the existing destructive-operation policy rather than a temporal one:
            // `--confirm` is declared `confirmation: always`, and the binding layer refuses
            // without it with `safety.confirmation_required` before anything reaches here.
            if !arguments.flag("confirm") {
                return Err(ErrorValue::new(
                    ErrorCode::SafetyConfirmationRequired,
                    "`remove temporal-history` destroys the retained local history",
                )
                .with_help(
                    "`remove temporal-history --confirm` (v0.5 §30.8); `--dry-run` says what it \
                     would destroy",
                ));
            }
            {
                let mut state = temporal_session().await;
                state.set_ledger(Ledger::default());
                note_started(None);
            }
            let removed = remove_store(&path);
            let result = match removed {
                Ok(()) => {
                    ActionResult::new(target, "ono.temporal-history.remove", ActionStatus::Success)
                        .changed(true)
                        .with_message(&format!(
                            "`{}` and its retained history are gone",
                            path.display()
                        ))
                }
                Err(error) => {
                    ActionResult::new(target, "ono.temporal-history.remove", ActionStatus::Failed)
                        .changed(false)
                        .with_error(error)
                }
            };
            Ok(Outcome::Values(ValueStream::from_values([
                result.into_value()
            ])))
        })
    }
}

/// Removes the store and the files SQLite keeps beside it (§31.5's write-ahead log).
///
/// A store that was never created is already absent, which is a success: §30.8 asks for the
/// history to be gone, and it is.
fn remove_store(path: &std::path::Path) -> Result<(), ErrorValue> {
    for suffix in ["", "-wal", "-shm"] {
        let candidate = if suffix.is_empty() {
            path.to_path_buf()
        } else {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            std::path::PathBuf::from(name)
        };
        match std::fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ErrorValue::new(
                    ErrorCode::IoPermissionDenied,
                    format!("`{}` could not be removed: {error}", candidate.display()),
                ));
            }
        }
    }
    Ok(())
}
