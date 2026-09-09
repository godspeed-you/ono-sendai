//! The recorder commands and the deletion of retained history (spec v0.5 §10.3, §10.8, §30.8).
//!
//! §10.2 is the sentence these four are written against: persistent recording is disabled by
//! default, and a normal installation without it still answers temporal queries from §10.7's
//! bounded session ledger. So `get recorder` on a fresh shell is a complete, truthful answer about
//! a recorder that is not running, and nothing here touches the filesystem until `start recorder`
//! is typed.
//!
//! What sits behind these commands is [`ono_recorder::Recorder`], not a stand-in. §44.1's five
//! start steps, §43.2's coverage declarations and §31.8's bounded retention are that component's,
//! and the command layer's job is the one `crates/ono-recorder/src/lib.rs` documents: build the
//! [`RecorderOptions`] this host can honestly offer, call `start`, `status` and `stop`, and give
//! `maintenance` a turn whenever the shell has one to give (ADR-0777).

use std::sync::{Arc, OnceLock, RwLock};

use jiff::Timestamp;
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture};
use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_recorder::{Recorder, RecorderOptions, RecorderSettings, SourceProfile};
use ono_spatial_core::SpatialType;
use ono_temporal_core::{ClockDomain, EvidenceSource, LedgerRead, error};
use ono_temporal_ledger::Ledger;
use ono_value::{
    ActionResult, ActionStatus, ByteSize, Duration, ErrorValue, MapValue, SchemaId, Value, ValueRef,
};

use super::session::{TemporalState, temporal_session};

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

/// The one recorder of this process (§10, §39.2).
///
/// Built on first use and never before: [`Recorder::new`] opens nothing, but the scope it is built
/// from reads the host's identity, and §32.1 budgets a disabled shell at no cost at all. Every
/// caller below is a command the user typed or a setting they switched on.
pub(crate) fn recorder() -> &'static Recorder {
    static RECORDER: OnceLock<Recorder> = OnceLock::new();
    RECORDER.get_or_init(|| Recorder::new(options()))
}

/// How this host's recorder is configured (§10.4, §10.6, §31.1).
fn options() -> RecorderOptions {
    let scope = crate::spatial::local_scope();
    let host = scope.host_scope().id().to_owned();
    let domain = ClockDomain::new(&host, boot_id().as_deref());
    let options = RecorderOptions::new(scope, domain).with_sources(profiles());
    match store_path() {
        Ok(path) => options.with_store(path),
        // §44.3: a shell with nowhere to keep history still works, with persistence off and an
        // explicit diagnostic. `Recorder::start` produces exactly that from a store-less option
        // set, so the refusal is carried rather than raised here.
        Err(_) => options,
    }
}

/// The kernel's boot identity, which is what makes two observations comparable (§25.5).
fn boot_id() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// What this shell can honestly say it collects from (§10.6, §21.5, §22.1).
///
/// One profile per kind of place the session's own sweeps yield, all of them sourced `ono.session`
/// — because that is what the evidence actually is. §21.5 forbids advertising `exhaustive_events`
/// for a source that is asked rather than subscribed to, and §6.3 forbids reading a missing object
/// as a disappearance unless the source promised a complete snapshot; a shell that observes when a
/// command is typed promises neither, so both are declined here rather than in a comment.
///
/// `ono.recorder` is not among them: the recorder is not a source of system observations, it is
/// what writes down the coverage and the gaps of the ones it holds (§44.1).
fn profiles() -> Vec<SourceProfile> {
    [SpatialType::Process, SpatialType::Service]
        .into_iter()
        .map(|object_type| {
            SourceProfile::new(EvidenceSource::session(), "ono.session", object_type)
                .polled(observation_interval())
                .exhaustive(false)
                .meaningful_disappearance(false)
        })
        .collect()
}

/// How often the shell's own observation refreshes, as §33.3's freshness policy fixes it.
///
/// It is the sampling interval §22.1 requires a snapshot source's coverage to carry: a reader of
/// that coverage learns that between two commands nobody looked.
fn observation_interval() -> Duration {
    Duration::from_nanoseconds(5_000_000_000)
}

/// When the running recorder was started, so `get recorder` can report it (§10.3).
///
/// One slot, in one place. Two function-local statics of the same name are two different statics,
/// and a writer and a reader that each declared their own could never agree.
fn started() -> &'static RwLock<Option<Timestamp>> {
    static STARTED: OnceLock<RwLock<Option<Timestamp>>> = OnceLock::new();
    STARTED.get_or_init(|| RwLock::new(None))
}

/// When the running recorder was started, for `since` (§10.3).
fn started_at() -> Option<Timestamp> {
    started().read().ok().and_then(|held| *held)
}

/// Records that the recorder started, or stopped.
pub(crate) fn note_started(at: Option<Timestamp>) {
    if let Ok(mut held) = started().write() {
        *held = at;
    }
}

/// The §10.4 and §10.7 limits this session was configured with (§33).
///
/// Read once, from the resolved settings, and kept for every later `start recorder`: the settings
/// a start applies are the session's, and a recorder that used the built-in defaults would report
/// a `session_max_events` nobody asked for as though it were in force.
fn settings() -> &'static RwLock<RecorderSettings> {
    static SETTINGS: OnceLock<RwLock<RecorderSettings>> = OnceLock::new();
    SETTINGS.get_or_init(|| RwLock::new(RecorderSettings::default()))
}

/// The settings in force for this session (§10.4, §33).
pub(crate) fn configured_settings() -> RecorderSettings {
    settings()
        .read()
        .map_or_else(|_| RecorderSettings::default(), |held| held.clone())
}

/// Applies the `temporal.*` limits the session resolved (§10.4, §10.7, §33).
pub(crate) fn configure(resolved: RecorderSettings) {
    if let Ok(mut held) = settings().write() {
        *held = resolved;
    }
}

/// The bounded in-memory ledger of §10.7, at the capacity this session was configured with.
pub(crate) fn session_ledger() -> Ledger {
    Ledger::session_with_capacity(configured_settings().session_max_events)
}

/// Starts the recorder and gives the session the ledger it writes to (§10.8, §44.1).
///
/// Every one of §44.1's five steps runs inside [`Recorder::start`], which is the whole reason this
/// is a call rather than a ledger swap: the interval between the last retained event and this
/// start is filed as a gap under the `<type>.existence` capability a reconstruction gates object
/// presence on, and the coverage each source declares is written down beside it.
///
/// # Errors
///
/// Returns `temporal.recorder_already_running` where a second start asks for settings the running
/// recorder is not using (§10.8, ADR-0640).
pub(crate) fn start_recorder(
    state: &mut TemporalState,
    now: Timestamp,
) -> Result<ono_recorder::StartOutcome, ErrorValue> {
    let outcome = recorder().start(&configured_settings(), now)?;
    state.set_shared_ledger(recorder().ledger());
    note_started(outcome.status.since);
    Ok(outcome)
}

/// One turn of the recorder's own maintenance, where the shell has a turn to give (§10.4, §31.8).
///
/// §39.2 keeps every timer in this system a caller's call, and a command that has just finished
/// its work is the caller with a moment to spare. A refusal is dropped rather than raised: losing
/// a flush is not a reason to lose the answer the user asked for (§16.5).
pub(crate) fn maintain(now: Timestamp) {
    if started_at().is_none() {
        return;
    }
    let _ = recorder().maintenance(now);
}

/// The `ono.recorder-status/1` record of §10.3, as the running recorder answers it.
///
/// The retention figures are read from the ledger the *session* holds, because that is the ledger
/// every command in this shell reads and writes: §10.7's in-memory one while nothing is recording,
/// and the recorder's own store once something is. `sources` is §10.6's list, and it names only
/// what actually collects — the shell's own observations always, and the recorder itself once it
/// is writing coverage and gaps of its own (ADR-0777).
fn status_record(state: &TemporalState, now: Timestamp) -> Result<Value, ErrorValue> {
    let mut status = recorder().status(now);
    let retention = state.ledger().retention();
    status.events = retention.events;
    status.size = retention.stored_size;
    status.earliest = retention.earliest;
    status.latest = retention.latest;
    status.since = status.since.or_else(started_at);
    // §33: the limits the status states are the session's resolved ones. A recorder nobody has
    // started is still holding a session ledger bounded by `temporal.session.max_events`, and
    // reporting the built-in default there would state a ceiling that is not the one in force.
    if !status.running {
        status.settings = configured_settings();
    }
    // §10.6: the list names what collects, and nothing else. One profile per kind of place is one
    // source seen twice, not two sources; and a shell that is not recording still collects its own
    // observations into §10.7's session ledger, so the list is the same list either way.
    status
        .sources
        .sort_by(|left, right| left.as_str().cmp(right.as_str()));
    status
        .sources
        .dedup_by(|left, right| left.as_str() == right.as_str());
    if status.sources.is_empty() {
        status.sources = vec![EvidenceSource::session()];
    }
    Ok(Value::Record(std::sync::Arc::new(status.to_record()?)))
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
            let now = Timestamp::now();
            // §10.4, §31.8, §31.9: asking what the recorder is doing is a turn the shell can spare,
            // and the recorder has no thread of its own to take one on.
            maintain(now);
            let record = status_record(&state, now)?;
            Ok(Outcome::Values(ValueStream::from_values([record])))
        })
    }
}

/// `start recorder` (§10.3, §10.8, §44.1).
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
            let now = Timestamp::now();
            // §10.8 makes the lifecycle idempotent and ADR-0640 draws the one line inside it: a
            // start that asks for settings the running recorder is not using answers E1314 rather
            // than silently ignoring what the operator asked for. Both are `Recorder::start`'s.
            let outcome = start_recorder(&mut state, now)?;
            // §44.3: persistence that could not be brought up leaves the shell working and says
            // so, on the diagnostic stream, once — the status carries the same words afterwards.
            if let Some(diagnostic) = outcome.diagnostic.as_ref() {
                crate::report::Reporter::new(ono_render::Presentation::choose(
                    std::io::IsTerminal::is_terminal(&std::io::stderr()),
                    &[],
                ))
                .note(&diagnostic.render_terse());
            }
            let record = status_record(&state, now)?;
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
            let now = Timestamp::now();
            // §10.8: the stop is clean. The coverage the recorder opened is closed at this
            // instant and what was buffered is written before the store is let go, so the
            // interval that follows is a declared gap rather than lost events (§44.1).
            recorder().stop(now)?;
            state.set_shared_ledger(recorder().ledger());
            note_started(None);
            let record = status_record(&state, now)?;
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
                // The recorder lets the store go before the file does: §30.8 destroys the
                // retained history, and a recorder still holding an open handle to it would keep
                // writing into a database nobody can find.
                let mut state = temporal_session().await;
                let _ = recorder().stop(Timestamp::now());
                state.set_ledger(session_ledger());
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
