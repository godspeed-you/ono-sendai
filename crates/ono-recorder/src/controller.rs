//! The recorder itself: start, stop, report, and the maintenance nobody else drives.
//!
//! §10.8 gives the lifecycle two sentences and both are exact. "`start recorder` MUST be
//! idempotent" — starting a running recorder reports the running recorder rather than refusing.
//! "`stop recorder` MUST flush the ledger and stop cleanly" — the flush is
//! [`ono_temporal_core::LedgerWrite::flush`], which the ledger implements as a truncating
//! write-ahead-log checkpoint, so what was committed is durable and readable by a second process
//! rather than merely committed.
//!
//! Between those two sentences sits the one decision §10.8 leaves open, and ADR-0640 takes it: a
//! start that carries settings the running recorder is not using cannot be absorbed by
//! idempotency, because absorbing it would silently ignore what the operator asked for. It
//! answers `temporal.recorder_already_running` and names the settings that differ.
//!
//! # What this component is for
//!
//! The recorder is the one part of the temporal system that reads the clock and touches the
//! store, which is exactly why nothing below it does (§39.2, §39.3). Every instant here is a
//! parameter for the same reason a test needs it to be, and every timer is somebody's call into
//! [`Recorder::maintenance`] rather than a thread this crate started:
//!
//! - **Retention is driven.** `Ledger::sweep(now)` removes one bounded batch and says whether
//!   more remains (§31.8). [`Recorder::sweep`] drives it up to
//!   `RecorderSettings::max_sweep_passes` times and reports honestly when the bounds are still
//!   not met, so §31.9's "checkpoints MUST not block the interactive prompt" holds of retention
//!   too.
//! - **Checkpoints are projected off the prompt path.** [`Recorder::begin_checkpoint`] runs the
//!   bounded projection of §42.2 on a thread of its own and only the write touches the store.
//! - **Continuity is declared.** `declare_contiguous` is what turns a hole in a source's sequence
//!   into a §43.2 coverage gap, and the recorder is the only component that knows which sources
//!   promised continuity, so it declares them at every start.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use jiff::Timestamp;
use ono_spatial_core::SpatialScope;
use ono_temporal_core::{
    Appended, ClockDomain, Evidence, EvidenceSource, LedgerRead as _, LedgerWrite as _,
    TemporalCoverage, TemporalEvent, TemporalGap,
};
use ono_temporal_ledger::{Ledger, StoreOptions, Swept};
use ono_value::{ErrorValue, Value};

use crate::checkpoint::{CheckpointReason, CheckpointSchedule};
use crate::downtime::{self, RestartPlan};
use crate::ingest::Intake;
use crate::normalize::Normalizer;
use crate::privilege::{self, PrivilegeReport};
use crate::redact::Redaction;
use crate::settings::RecorderSettings;
use crate::source::SourceProfile;
use crate::status::{RecorderHealth, RecorderStatus};

use ono_temporal_reconstruct::{CheckpointPolicy, CheckpointRequest, project_checkpoint};

use ono_temporal_core::{ObjectState, RelationState};

/// How a recorder is configured before it is started.
#[derive(Debug, Clone)]
pub struct RecorderOptions {
    scope: SpatialScope,
    domain: ClockDomain,
    store: Option<PathBuf>,
    sources: Vec<SourceProfile>,
    checkpoints: CheckpointPolicy,
}

impl RecorderOptions {
    /// A recorder for `scope`, recording in `domain`.
    #[must_use]
    pub fn new(scope: SpatialScope, domain: ClockDomain) -> Self {
        Self {
            scope,
            domain,
            store: None,
            sources: Vec::new(),
            checkpoints: CheckpointPolicy::default_local(),
        }
    }

    /// The same options with the ledger at `path` (§31.1).
    #[must_use]
    pub fn with_store(mut self, path: PathBuf) -> Self {
        self.store = Some(path);
        self
    }

    /// The same options collecting from these sources (§10.6).
    #[must_use]
    pub fn with_sources(mut self, sources: Vec<SourceProfile>) -> Self {
        self.sources = sources;
        self
    }

    /// The same options checkpointing under `policy` (§42.2).
    #[must_use]
    pub fn with_checkpoint_policy(mut self, policy: CheckpointPolicy) -> Self {
        self.checkpoints = policy;
        self
    }

    /// The v0.4 boundary this recorder records about.
    #[must_use]
    pub const fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// The sources it collects from.
    #[must_use]
    pub fn sources(&self) -> &[SourceProfile] {
        &self.sources
    }
}

/// What a `start recorder` produced (§10.8, §44.1, §44.3).
#[derive(Debug)]
pub struct StartOutcome {
    /// The recorder's state after the start.
    pub status: RecorderStatus,
    /// §44.1's restart procedure, where a store was opened.
    pub plan: Option<RestartPlan>,
    /// §44.3's explicit diagnostic, where persistence could not be brought up.
    pub diagnostic: Option<ErrorValue>,
}

/// What one call of [`Recorder::maintenance`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Maintenance {
    /// Whether the flush interval had elapsed and the ledger was flushed (§10.4, §32.5).
    pub flushed: bool,
    /// What the bounded retention drive removed (§31.8).
    pub swept: Swept,
    /// Why a checkpoint is due, where one is (§31.9).
    pub checkpoint: Option<CheckpointReason>,
}

/// What writing one checkpoint produced (§42.2).
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointOutcome {
    /// The identity the checkpoint was stored under.
    pub checkpoint_id: ono_temporal_core::CheckpointId,
    /// When it was captured.
    pub captured_at: Timestamp,
    /// How many objects it holds.
    pub objects: usize,
    /// The coverage it declares, including the `<type>.existence` presence is gated on (§8.1).
    pub coverage: Vec<TemporalCoverage>,
    /// The state classes the policy left out, and why (§42.2).
    pub excluded: Vec<ono_temporal_reconstruct::ExcludedClass>,
}

/// A checkpoint being projected off the prompt path (§31.9).
#[derive(Debug)]
pub struct PendingCheckpoint {
    handle: Option<std::thread::JoinHandle<Result<CheckpointOutcome, ErrorValue>>>,
}

impl PendingCheckpoint {
    /// Waits for the projection and the write, and answers what they produced.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal, or `temporal.store_unavailable` where the projection thread
    /// did not finish.
    pub fn join(mut self) -> Result<CheckpointOutcome, ErrorValue> {
        match self.handle.take() {
            Some(handle) => handle.join().unwrap_or_else(|_| {
                Err(ono_temporal_core::error::store_unavailable(
                    "the checkpoint projection did not finish",
                ))
            }),
            None => Err(ono_temporal_core::error::store_unavailable(
                "the checkpoint projection was already joined",
            )),
        }
    }
}

/// What the recorder is doing now.
#[derive(Debug)]
struct State {
    running: bool,
    settings: RecorderSettings,
    since: Option<Timestamp>,
    store: Option<PathBuf>,
    schedule: CheckpointSchedule,
    dropped: u64,
    health: RecorderHealth,
    /// §44.3's diagnostic, kept so `get recorder` answers it after the fact as well.
    diagnostic: Option<std::sync::Arc<str>>,
    last_flush: Option<Timestamp>,
    declared: Vec<EvidenceSource>,
}

/// The temporal recorder of §10.
///
/// Cloning one is cloning a handle: the ledger, the state and the checkpoint counter are shared,
/// so a projection running on a thread of its own and the prompt asking for the status are the
/// same recorder (§31.9).
#[derive(Debug, Clone)]
pub struct Recorder {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    options: RecorderOptions,
    ledger: RwLock<Arc<Ledger>>,
    state: Mutex<State>,
    in_flight: std::sync::atomic::AtomicUsize,
}

impl Recorder {
    /// A recorder nobody has started (§10.2).
    ///
    /// Nothing is opened: the ledger is §10.7's in-memory one, which touches no filesystem, and
    /// that is what keeps §32.1's disabled path free.
    #[must_use]
    pub fn new(options: RecorderOptions) -> Self {
        let settings = RecorderSettings::default();
        Self {
            inner: Arc::new(Inner {
                ledger: RwLock::new(Arc::new(Ledger::session_with_capacity(
                    settings.session_max_events,
                ))),
                state: Mutex::new(State {
                    running: false,
                    settings: RecorderSettings {
                        enabled: false,
                        ..settings
                    },
                    since: None,
                    store: None,
                    schedule: CheckpointSchedule::default(),
                    dropped: 0,
                    health: RecorderHealth::Stopped,
                    diagnostic: None,
                    last_flush: None,
                    declared: Vec::new(),
                }),
                in_flight: std::sync::atomic::AtomicUsize::new(0),
                options,
            }),
        }
    }

    /// The ledger this recorder writes to — persistent while running, in-memory otherwise.
    #[must_use]
    pub fn ledger(&self) -> Arc<Ledger> {
        self.inner.ledger.read().map_or_else(
            |poisoned| Arc::clone(&poisoned.into_inner()),
            |ledger| Arc::clone(&ledger),
        )
    }

    /// The sources this recorder collects from (§10.6).
    #[must_use]
    pub fn sources(&self) -> &[SourceProfile] {
        self.inner.options.sources()
    }

    /// A normalizer for one of them, with the redaction the running settings ask for (§30.4).
    #[must_use]
    pub fn normalizer(&self, profile: &SourceProfile) -> Normalizer {
        let argv = self.with_state(|state| state.settings.record_process_argv);
        Normalizer::new(
            profile.clone(),
            self.inner.options.scope.clone(),
            self.inner.options.domain.clone(),
        )
        .with_redaction(Redaction::new(argv))
    }

    /// `start recorder` (§10.8).
    ///
    /// Idempotent for a start that asks for what the running recorder is already doing, and
    /// `temporal.recorder_already_running` for one that does not — see ADR-0640.
    ///
    /// # Errors
    ///
    /// Returns `temporal.recorder_already_running` (E1314). A store that cannot be opened is not
    /// an error here: §44.3 requires the shell to keep working with temporal persistence disabled
    /// and an explicit diagnostic, which is what [`StartOutcome::diagnostic`] carries.
    pub fn start(
        &self,
        settings: &RecorderSettings,
        now: Timestamp,
    ) -> Result<StartOutcome, ErrorValue> {
        if let Some(refusal) = self.refuse_second_start(settings) {
            return Err(refusal);
        }
        if self.with_state(|state| state.running) {
            return Ok(StartOutcome {
                status: self.status(now),
                plan: None,
                diagnostic: None,
            });
        }

        let Some(path) = self.inner.options.store.clone() else {
            return Ok(self.degrade(
                settings,
                now,
                ono_temporal_core::error::store_unavailable(
                    "no ledger path was configured, so nothing can be retained across sessions",
                ),
            ));
        };

        let options = StoreOptions::at(&path).with_retention(settings.retention());
        let ledger = match Ledger::persistent(&options) {
            Ok(ledger) => Arc::new(ledger),
            Err(refusal) => return Ok(self.degrade(settings, now, refusal)),
        };

        // §44.1, in order, before anything new is written into the store.
        let plan = ledger.store().map_or(Ok(None), |store| {
            downtime::restart(
                store,
                &self.inner.options.scope,
                &self.inner.options.domain,
                self.inner.options.sources(),
                now,
            )
            .map(Some)
        });
        let plan = match plan {
            Ok(plan) => plan,
            Err(refusal) => return Ok(self.degrade(settings, now, refusal)),
        };

        let declared = self.declare_continuity(&ledger, now)?;
        if let Some(plan) = plan.as_ref() {
            self.record_gaps(&ledger, &plan.gaps, now)?;
        }
        self.open_coverage(&ledger, now)?;

        {
            let mut state = self.lock_state();
            state.running = true;
            state.enabled_from(settings);
            state.since = Some(now);
            state.store = Some(path);
            state.schedule = CheckpointSchedule::every(settings.checkpoint_interval);
            state.health = if state.dropped > 0 {
                RecorderHealth::Degraded
            } else {
                RecorderHealth::Healthy
            };
            state.last_flush = Some(now);
            state.declared = declared;
        }
        self.replace_ledger(ledger);

        Ok(StartOutcome {
            status: self.status(now),
            plan,
            diagnostic: None,
        })
    }

    /// `stop recorder` (§10.8).
    ///
    /// # Errors
    ///
    /// Returns `temporal.recorder_not_running` (E1313) where nothing is running.
    pub fn stop(&self, now: Timestamp) -> Result<RecorderStatus, ErrorValue> {
        if !self.with_state(|state| state.running) {
            return Err(ono_temporal_core::error::recorder_not_running());
        }
        let ledger = self.ledger();
        self.close_coverage(&ledger, now)?;
        ledger.flush()?;
        {
            let mut state = self.lock_state();
            state.running = false;
            state.settings.enabled = false;
            state.since = None;
            state.store = None;
            state.health = RecorderHealth::Stopped;
            state.diagnostic = None;
        }
        let capacity = self.with_state(|state| state.settings.session_max_events);
        self.replace_ledger(Arc::new(Ledger::session_with_capacity(capacity)));
        Ok(self.status(now))
    }

    /// `get recorder` (§10.3, §43.4).
    ///
    /// `now` is what makes the health honest. §43.4 asks that "a persistent recorder that is
    /// falling behind SHOULD become a temporal/system landmark", and a recorder whose last
    /// maintenance turn is [`STALL_INTERVALS`] flush intervals old is behind whether or not it has
    /// dropped anything yet. Reporting `healthy` there would be §21.8's own failure in the
    /// recorder's domain: freezing the last known state and calling it current.
    #[must_use]
    pub fn status(&self, now: Timestamp) -> RecorderStatus {
        let retention = self.ledger().retention();
        let state = self.lock_state();
        let health = if state.running && state.is_stalled(now) {
            RecorderHealth::Degraded
        } else {
            state.health
        };
        RecorderStatus {
            running: state.running,
            enabled: state.settings.enabled,
            since: state.since,
            store: state.store.clone(),
            settings: state.settings.clone(),
            events: retention.events,
            size: retention.stored_size,
            earliest: retention.earliest,
            latest: retention.latest,
            sources: self
                .inner
                .options
                .sources()
                .iter()
                .map(|profile| profile.source.clone())
                .collect(),
            dropped: state.dropped,
            health,
            checkpoints_in_flight: self
                .inner
                .in_flight
                .load(std::sync::atomic::Ordering::Relaxed),
            diagnostic: state.diagnostic.clone(),
        }
    }

    /// The sources this recorder has told the store number their events contiguously (§43.2).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn declared_contiguous(&self) -> Result<Vec<EvidenceSource>, ErrorValue> {
        Ok(self.with_state(|state| state.declared.clone()))
    }

    /// Every interval the ledger can say nothing about (§7.5, §31.7, §43.2).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn gaps(&self) -> Result<Vec<TemporalGap>, ErrorValue> {
        let ledger = self.ledger();
        let Some(store) = ledger.store() else {
            return Ok(Vec::new());
        };
        let mut gaps = store.gaps()?;
        gaps.extend(self.recorded_gaps(&ledger)?);
        Ok(gaps)
    }

    /// Appends events and the evidence that supports them (§6.7).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn record(
        &self,
        events: &[TemporalEvent],
        evidence: &[Evidence],
    ) -> Result<Appended, ErrorValue> {
        self.ledger().append(events, evidence)
    }

    /// Records what one source could observe over an interval (§8.1, §10.6).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn record_coverage(&self, intervals: &[TemporalCoverage]) -> Result<(), ErrorValue> {
        self.ledger().record_coverage(intervals)
    }

    /// Records one observation of an object, redacted as §10.6 requires.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn record_observation(
        &self,
        record: &ono_value::RecordValue,
        at: Timestamp,
    ) -> Result<Appended, ErrorValue> {
        let profile = self
            .inner
            .options
            .sources()
            .first()
            .cloned()
            .unwrap_or_else(|| default_profile(&self.inner.options.domain));
        let normalizer = self.normalizer(&profile);
        let event = normalizer.from_snapshot_diff(
            ono_temporal_core::EventKind::ObjectObserved,
            record,
            at,
            at,
        );
        self.record(&[event], &[])
    }

    /// Records one of Ono's own actions, with the redacted command summary (§17.4, §17.5).
    ///
    /// §10.6 lists action events among what the recorder SHOULD collect, and §17.1 says why they
    /// matter more than their number suggests: the shell "knows exactly which actions the operator
    /// requested", which is the one causal fact external monitoring usually lacks. The summary is
    /// a [`RedactedCommandSummary`](ono_temporal_core::RedactedCommandSummary), which has no
    /// constructor taking rendered text, so §17.5's semantic redaction is upstream of this call
    /// rather than inside it.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn record_action(&self, action: &ono_temporal_core::ActionEvent) -> Result<(), ErrorValue> {
        self.ledger().record_action(action)
    }

    /// Writes down what a bounded queue lost, as §43.2's explicit coverage gap.
    ///
    /// The gap reaches the ledger as a `coverage.ended` event and a closed coverage interval, so a
    /// timeline over the stretch draws a gap rather than a quiet morning (§55.5).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn note_overflow(&self, intake: &Intake, now: Timestamp) -> Result<(), ErrorValue> {
        let Some(gap) = intake.overflow_gap() else {
            return Ok(());
        };
        {
            let mut state = self.lock_state();
            state.dropped = state.dropped.saturating_add(intake.dropped());
            if state.running {
                state.health = RecorderHealth::Degraded;
            }
        }
        let ledger = self.ledger();
        self.record_gaps(&ledger, std::slice::from_ref(&gap), now)?;
        intake.clear_overflow();
        Ok(())
    }

    /// Drives retention until the bounds hold or the pass budget is spent (§31.8).
    ///
    /// `complete: false` in the answer means the caller should drive it again; nothing here
    /// loops without a bound, because §31.8 makes retention bounded background work and the
    /// prompt is what the bound protects.
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn sweep(&self, now: Timestamp) -> Result<Swept, ErrorValue> {
        let passes = self.with_state(|state| state.settings.max_sweep_passes.max(1));
        let ledger = self.ledger();
        let mut total = Swept {
            complete: true,
            ..Swept::default()
        };
        for _ in 0..passes {
            let swept = ledger.sweep(now)?;
            total.events += swept.events;
            total.evidence += swept.evidence;
            total.links += swept.links;
            total.checkpoints += swept.checkpoints;
            total.actions += swept.actions;
            total.coverage += swept.coverage;
            total.complete = swept.complete;
            if swept.complete {
                break;
            }
        }
        Ok(total)
    }

    /// The truncating write-ahead-log checkpoint §10.8 asks `stop recorder` for (§31.5).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn flush(&self) -> Result<(), ErrorValue> {
        self.ledger().flush()
    }

    /// One turn of the recorder's own maintenance: flush, sweep, and say whether a checkpoint is
    /// due (§10.4, §31.8, §31.9).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn maintenance(&self, now: Timestamp) -> Result<Maintenance, ErrorValue> {
        let (due_flush, interval) = self.with_state(|state| {
            (
                state
                    .last_flush
                    .is_none_or(|last| elapsed(last, now) >= state.settings.flush_interval),
                state.settings.flush_interval,
            )
        });
        let _ = interval;
        let mut flushed = false;
        if due_flush {
            self.flush()?;
            self.lock_state().last_flush = Some(now);
            flushed = true;
        }
        let swept = self.sweep(now)?;
        let checkpoint = self.with_state(|state| state.schedule.due(now));
        Ok(Maintenance {
            flushed,
            swept,
            checkpoint,
        })
    }

    /// Whether a change earns an extra checkpoint at `now` (§31.9).
    pub fn note_change(&self, event: &TemporalEvent, now: Timestamp) -> Option<CheckpointReason> {
        self.lock_state().schedule.on_change(event, now)
    }

    /// Projects and writes one checkpoint (§42.2).
    ///
    /// # Errors
    ///
    /// Returns a §34 store refusal.
    pub fn checkpoint(
        &self,
        capture: CheckpointCapture,
        policy: &CheckpointPolicy,
    ) -> Result<CheckpointOutcome, ErrorValue> {
        let request = self.request_of(capture);
        let projection = project_checkpoint(&request, policy);
        let excluded = projection.excluded().to_vec();
        let checkpoint = projection.into_checkpoint();
        self.ledger().write_checkpoint(&checkpoint)?;
        self.lock_state().schedule.taken(checkpoint.captured_at);
        Ok(CheckpointOutcome {
            checkpoint_id: checkpoint.checkpoint_id.clone(),
            captured_at: checkpoint.captured_at,
            objects: checkpoint.objects.len(),
            coverage: checkpoint.coverage.clone(),
            excluded,
        })
    }

    /// Projects and writes one checkpoint off the prompt path (§31.9).
    ///
    /// §31.9: "checkpoints MUST not block the interactive prompt." The bounded projection of
    /// §42.2 is the expensive half and it runs on a thread of its own; only the write touches the
    /// store, and [`Recorder::status`] never waits for either.
    #[must_use]
    pub fn begin_checkpoint(
        &self,
        capture: CheckpointCapture,
        policy: CheckpointPolicy,
    ) -> PendingCheckpoint {
        let recorder = self.clone();
        recorder
            .inner
            .in_flight
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let handle = std::thread::spawn(move || {
            let outcome = recorder.checkpoint(capture, &policy);
            recorder
                .inner
                .in_flight
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            outcome
        });
        PendingCheckpoint {
            handle: Some(handle),
        }
    }

    /// Discards the whole local ledger (§30.8's `remove temporal-history`).
    ///
    /// # Errors
    ///
    /// Returns `temporal.recorder_not_running` where there is no store, and a §34 store refusal
    /// where there is one and it will not empty.
    pub fn remove_history(&self) -> Result<(), ErrorValue> {
        let ledger = self.ledger();
        let Some(store) = ledger.store() else {
            return Err(ono_temporal_core::error::recorder_not_running());
        };
        store.remove_all()
    }

    /// What privilege the recorder is running with (§10.5).
    #[must_use]
    pub fn privilege(&self) -> PrivilegeReport {
        let store = self.with_state(|state| state.store.clone());
        privilege::inspect(std::path::Path::new("/proc"), store.as_deref())
    }

    /// The refusal a second start earns, or `None` where idempotency absorbs it (§10.8).
    fn refuse_second_start(&self, requested: &RecorderSettings) -> Option<ErrorValue> {
        let (running, current) = self.with_state(|state| (state.running, state.settings.clone()));
        if !running {
            return None;
        }
        let differences = current.differences(requested);
        if differences.is_empty() {
            return None;
        }
        let names: Vec<Value> = differences
            .iter()
            .map(|difference| Value::string(difference.setting))
            .collect();
        let rendered: Vec<String> = differences
            .iter()
            .map(|difference| {
                format!(
                    "{} is `{}` and the start asked for `{}`",
                    difference.setting, difference.running, difference.requested
                )
            })
            .collect();
        Some(
            ono_temporal_core::error::recorder_already_running()
                .with_help(format!(
                    "the running recorder cannot take these settings: {}; `stop recorder` then \
                     `start recorder` applies them",
                    rendered.join("; ")
                ))
                .with_metadata("settings", Value::List(names.into())),
        )
    }

    /// §44.3: persistence is off, the shell keeps working, and the diagnostic is explicit.
    fn degrade(
        &self,
        settings: &RecorderSettings,
        now: Timestamp,
        diagnostic: ErrorValue,
    ) -> StartOutcome {
        {
            let mut state = self.lock_state();
            state.running = false;
            state.settings = RecorderSettings {
                enabled: false,
                ..settings.clone()
            };
            state.since = None;
            state.store = None;
            state.health = RecorderHealth::Failed;
            state.diagnostic = Some(diagnostic.render_terse().into());
        }
        StartOutcome {
            status: self.status(now),
            plan: None,
            diagnostic: Some(diagnostic),
        }
    }

    /// Tells the store which sources promised sequence continuity (§21.5, §43.2).
    fn declare_continuity(
        &self,
        ledger: &Ledger,
        now: Timestamp,
    ) -> Result<Vec<EvidenceSource>, ErrorValue> {
        let Some(store) = ledger.store() else {
            return Ok(Vec::new());
        };
        let mut declared = Vec::new();
        for profile in self.inner.options.sources() {
            if !profile.promises_continuity() {
                continue;
            }
            store.declare_contiguous(
                &profile.source,
                &self.inner.options.domain.host,
                self.inner.options.domain.boot_id.as_deref(),
                &self.inner.options.scope,
                now,
            )?;
            declared.push(profile.source.clone());
        }
        Ok(declared)
    }

    /// Opens a coverage interval per source, so a stored stretch is distinguishable from an
    /// unwatched one (§8.1, §10.6).
    fn open_coverage(&self, ledger: &Ledger, now: Timestamp) -> Result<(), ErrorValue> {
        let mut intervals = Vec::new();
        let mut markers = Vec::new();
        for profile in self.inner.options.sources() {
            let coverage = profile.coverage(
                &self.inner.options.scope,
                now,
                now,
                ono_spatial_core::PermissionState::Available,
            );
            markers.push(self.normalizer(profile).coverage_started(&coverage, now));
            intervals.push(coverage);
        }
        if intervals.is_empty() {
            return Ok(());
        }
        ledger.record_coverage(&intervals)?;
        ledger.append(&markers, &[])?;
        Ok(())
    }

    /// Closes each source's coverage at `now`, so the interval the recorder covered ends where
    /// the recorder did (§8.1, §44.1).
    fn close_coverage(&self, ledger: &Ledger, now: Timestamp) -> Result<(), ErrorValue> {
        let since = self.with_state(|state| state.since).unwrap_or(now);
        let mut intervals = Vec::new();
        for profile in self.inner.options.sources() {
            intervals.push(profile.coverage(
                &self.inner.options.scope,
                since,
                now,
                ono_spatial_core::PermissionState::Available,
            ));
        }
        if intervals.is_empty() {
            return Ok(());
        }
        ledger.record_coverage(&intervals)
    }

    /// Writes gaps down as coverage the store can answer with, and as the events §11.7 draws.
    fn record_gaps(
        &self,
        ledger: &Ledger,
        gaps: &[TemporalGap],
        now: Timestamp,
    ) -> Result<(), ErrorValue> {
        if gaps.is_empty() {
            return Ok(());
        }
        let profile = self
            .inner
            .options
            .sources()
            .first()
            .cloned()
            .unwrap_or_else(|| default_profile(&self.inner.options.domain));
        let normalizer = self.normalizer(&profile);
        let markers: Vec<TemporalEvent> = gaps
            .iter()
            .map(|gap| normalizer.coverage_ended(gap, now))
            .collect();
        let intervals: Vec<TemporalCoverage> = gaps
            .iter()
            .map(|gap| TemporalCoverage {
                scope: gap.scope.clone(),
                capability: Arc::clone(&gap.capability),
                from: gap.from,
                until: gap.until,
                completeness: ono_temporal_core::TemporalCompleteness::Unavailable,
                sampling_interval: None,
                source: gap.source.clone(),
                permission: ono_spatial_core::PermissionState::Unknown,
            })
            .collect();
        ledger.record_coverage(&intervals)?;
        ledger.append(&markers, &[])?;
        Ok(())
    }

    /// The gaps the recorder itself wrote down, read back out of the ledger (§43.2, §55.5).
    ///
    /// They are read from the `coverage.ended` markers rather than recomputed from the coverage
    /// intervals beside them: the marker carries the reason and the words §11.7 renders, and a
    /// reason inferred from an interval would be a guess about why it is empty.
    fn recorded_gaps(&self, ledger: &Ledger) -> Result<Vec<TemporalGap>, ErrorValue> {
        let events = ledger.events(&ono_temporal_core::EventQuery {
            scope: Some(self.inner.options.scope.clone()),
            subjects: Vec::new(),
            kinds: vec![ono_temporal_core::EventKind::CoverageEnded],
            range: ono_temporal_core::TimeRange {
                from: None,
                until: None,
            },
            limit: None,
            order: ono_temporal_core::QueryOrder::Ascending,
        })?;
        Ok(events.iter().filter_map(crate::normalize::gap_of).collect())
    }

    /// Builds the checkpoint request, adding the `<type>.existence` coverage object presence is
    /// gated on (§8.1, §9.5).
    ///
    /// A checkpoint plus events with no existence coverage reconstructs to `Presence::Unknown`
    /// rather than to a present object, so the recorder declares it for every type the capture
    /// carries and for every source it holds. `ono_temporal_reconstruct::capability` builds the
    /// name and nothing here spells it.
    fn request_of(&self, capture: CheckpointCapture) -> CheckpointRequest {
        let captured_at = capture.captured_at;
        let mut coverage = capture.coverage;
        let mut types: Vec<ono_spatial_core::SpatialType> = capture
            .objects
            .iter()
            .map(|object| object.object_type)
            .collect();
        types.extend(
            self.inner
                .options
                .sources()
                .iter()
                .map(|profile| profile.object_type),
        );
        types.sort_by_key(|object_type| object_type.as_str());
        types.dedup_by_key(|object_type| object_type.as_str());

        for object_type in types {
            let capability = ono_temporal_reconstruct::capability::existence(object_type);
            if coverage
                .iter()
                .any(|interval| interval.capability == capability)
            {
                continue;
            }
            let profile = self
                .inner
                .options
                .sources()
                .iter()
                .find(|profile| profile.object_type == object_type)
                .cloned()
                .unwrap_or_else(|| default_profile(&self.inner.options.domain));
            coverage.push(profile.coverage_of(
                capability,
                &self.inner.options.scope,
                captured_at,
                captured_at,
                ono_spatial_core::PermissionState::Available,
            ));
        }

        CheckpointRequest::new(capture.scope, captured_at)
            .with_objects(capture.objects)
            .with_relations(capture.relations)
            .with_coverage(coverage)
            .with_provenance(ono_value::Provenance::local(
                EvidenceSource::recorder().as_str(),
                ono_value::SchemaId::new("ono.temporal-event", 1),
            ))
    }

    fn replace_ledger(&self, ledger: Arc<Ledger>) {
        match self.inner.ledger.write() {
            Ok(mut held) => *held = ledger,
            Err(poisoned) => *poisoned.into_inner() = ledger,
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn with_state<T>(&self, read: impl FnOnce(&State) -> T) -> T {
        read(&self.lock_state())
    }
}

/// How many flush intervals may pass before a running recorder is behind (§43.4).
///
/// The flush interval is 2s by default, so ten seconds without a maintenance turn. That is long
/// enough that an ordinary scheduling hiccup does not read as a stall, and short enough that a
/// recorder nobody is driving says so before the next checkpoint would have been due.
pub const STALL_INTERVALS: i128 = 5;

impl State {
    /// Whether the recorder has gone [`STALL_INTERVALS`] flush intervals without a turn (§43.4).
    fn is_stalled(&self, now: Timestamp) -> bool {
        let Some(last) = self.last_flush else {
            return false;
        };
        let interval = self.settings.flush_interval.nanoseconds().max(1);
        elapsed(last, now).nanoseconds() > interval * STALL_INTERVALS
    }

    fn enabled_from(&mut self, settings: &RecorderSettings) {
        self.settings = RecorderSettings {
            enabled: true,
            ..settings.clone()
        };
    }
}

/// A source for a recorder that was given none, so its own coverage still has a name.
fn default_profile(domain: &ClockDomain) -> SourceProfile {
    let _ = domain;
    SourceProfile::new(
        EvidenceSource::recorder(),
        "ono.recorder",
        ono_spatial_core::SpatialType::Process,
    )
}

/// How long passed between two instants.
fn elapsed(from: Timestamp, to: Timestamp) -> ono_value::Duration {
    ono_value::Duration::from_nanoseconds(to.as_nanosecond() - from.as_nanosecond())
}

/// What a caller offers a checkpoint (§42.2).
///
/// It is the recorder's own shape rather than `CheckpointRequest` because the recorder adds the
/// `<type>.existence` coverage of §8.1 before the projection sees it, and a request that has
/// already been sealed cannot be read back to find out what it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointCapture {
    scope: SpatialScope,
    captured_at: Timestamp,
    objects: Vec<ObjectState>,
    relations: Vec<RelationState>,
    coverage: Vec<TemporalCoverage>,
}

impl CheckpointCapture {
    /// A capture of `scope` at `captured_at`, holding nothing yet.
    #[must_use]
    pub fn new(scope: SpatialScope, captured_at: Timestamp) -> Self {
        Self {
            scope,
            captured_at,
            objects: Vec::new(),
            relations: Vec::new(),
            coverage: Vec::new(),
        }
    }

    /// The objects the capture saw.
    #[must_use]
    pub fn with_objects(mut self, objects: Vec<ObjectState>) -> Self {
        self.objects = objects;
        self
    }

    /// The relationships the capture saw (§9.5).
    #[must_use]
    pub fn with_relations(mut self, relations: Vec<RelationState>) -> Self {
        self.relations = relations;
        self
    }

    /// What the sources behind the capture were able to observe (§42.4).
    #[must_use]
    pub fn with_coverage(mut self, coverage: Vec<TemporalCoverage>) -> Self {
        self.coverage = coverage;
        self
    }

    /// When it was captured.
    #[must_use]
    pub const fn captured_at(&self) -> Timestamp {
        self.captured_at
    }

    /// How many objects it holds.
    #[must_use]
    pub fn objects(&self) -> &[ObjectState] {
        &self.objects
    }
}
