//! The temporal coordinate of one session, and the commands that move it (spec v0.5 §4, §12).
//!
//! §55.7 forbids the ledger, the reconstruction and the causal engine from being built into this
//! crate, and the module boundary here is that rule made structural. `ono-cli` parses, dispatches,
//! owns the active [`TemporalContext`], and integrates the current spatial place with the temporal
//! query — "nothing more", in §39's words. Every answer below is composed out of
//! `ono-temporal-core`, `-ledger`, `-query`, `-reconstruct` and `-render`.
//!
//! Three of the twelve commands are answered by the shell rather than by the command table:
//! `at` and `now` move state that lives in this process and is reached without an
//! [`ono_command::Invocation`], and `present` runs an external program through the session's own
//! executor. The other nine are ordinary [`ono_command::CommandImpl`]s, registered in
//! `crate::eval::native`.

pub mod coordinate;
pub mod events;
pub mod prompt;
pub mod recorder;
pub mod session;
pub mod views;

pub use events::{ActionLifecycle, ingest_changes, ingest_provider_events};
pub use prompt::{marker_segments, temporal_segments};
pub use recorder::{GetRecorder, RemoveTemporalHistory, StartRecorder, StopRecorder};
pub use session::{TemporalState, TemporalStep, temporal_session};
pub use views::{Changes, FindEvent, InspectEvent, Timeline, Why};

use jiff::Timestamp;
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::{Stage, StageHead};
use ono_value::{ActionResult, ActionStatus, ErrorValue, MapValue, SchemaId, Value, ValueRef};

use crate::eval::{Eval, Flow};
use crate::session::Session;

/// What a stage asks the temporal layer to answer (§4.2, §4.3, §4.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// `at <time-selector>` — enter historical context (§4.2).
    At,
    /// `now` — return to the present (§4.3).
    Now,
    /// `present <command…>` — run one external program in the real present (§4.8).
    Present,
}

/// Whether `stage` is one of the three commands this module answers.
///
/// Claimed by the head word alone, exactly as `enter` is: `ono:at` means the same thing, and a
/// program named `now` on `PATH` stays reachable as `exec:now`.
#[must_use]
pub fn claims(stage: &Stage) -> Option<Request> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    if !matches!(name.namespace.as_deref(), None | Some("ono")) {
        return None;
    }
    match name.name.as_str() {
        "at" => Some(Request::At),
        "now" => Some(Request::Now),
        "present" => Some(Request::Present),
        _ => None,
    }
}

/// The text a stage wrote after its head word, verbatim.
///
/// Verbatim because a time selector is not a shell word: `-10m` would lex as an option and
/// `2026-08-31 12:17:00` as two arguments, and §4.4 defines both as one selector. `explain` reads
/// its subject the same way and for the same reason.
fn tail_of(stage: &Stage, source: &str) -> String {
    let start = stage
        .arguments
        .first()
        .map_or(stage.span.end(), |argument| argument.span().start());
    source
        .get(start as usize..stage.span.end() as usize)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// The values `stage` produces (§4.2, §4.3, §4.8).
///
/// # Errors
///
/// The structured refusal of the command: `temporal.invalid_time`, `temporal.not_recorded` or
/// `temporal.out_of_retention` for `at`, and whatever the child process reported for `present`.
pub fn answer(
    session: &mut Session,
    stage: &Stage,
    source: &str,
    request: Request,
) -> Eval<Vec<Value>> {
    match request {
        Request::At => enter_at(session, &tail_of(stage, source)).map_err(Flow::Failed),
        Request::Now => return_to_now(session).map_err(Flow::Failed),
        Request::Present => run_present(session, stage, source),
    }
}

/// The runtime a temporal command runs on, built on first use like every other native command's.
fn runtime_handle(session: &mut Session) -> Result<tokio::runtime::Handle, ErrorValue> {
    session
        .runtime()
        .map(|runtime| runtime.handle().clone())
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                "the temporal commands need an async runtime, and this system refused to start one",
            )
        })
}

/// `at <time-selector>`: resolve, then move (§4.2, §12.1).
fn enter_at(session: &mut Session, selector: &str) -> Result<Vec<Value>, ErrorValue> {
    if selector.is_empty() {
        return Err(ono_temporal_core::error::invalid_time(
            "",
            "`at` needs the instant to stand at: `at -10m`, `at 12:17`, `at event @e42`",
        ));
    }
    let handle = runtime_handle(session)?;
    handle.block_on(async move {
        let mut state = session::temporal_session().await;
        // The clock is read after the state exists, so the session's own start is never later
        // than the instant a selector is measured back from (§10.7, §39.2).
        let now = Timestamp::now();
        // §12.1: everything that can refuse has refused before this line, so a bad selector
        // leaves the coordinate untouched.
        let context = coordinate::resolve(&state, selector, now)?;
        state.commit(context.clone(), selector, now);
        session::install_evidence();
        coordinate::context_record(&context).map(|record| vec![record])
    })
}

/// `now`: restore the present, and say what happened to the place (§4.3).
fn return_to_now(session: &mut Session) -> Result<Vec<Value>, ErrorValue> {
    let now = Timestamp::now();
    let handle = runtime_handle(session)?;
    handle.block_on(async move {
        {
            let mut state = session::temporal_session().await;
            state.commit(ono_temporal_core::TemporalContext::Present, "now", now);
        }
        // §4.3: the place is kept where it still exists, and otherwise the session returns to the
        // nearest live canonical parent and reports the transition. A tombstoned place is never
        // silently reinterpreted as the live object that took its identity.
        let moved = restore_place(now).await;
        let record = coordinate::context_record(&ono_temporal_core::TemporalContext::Present)?;
        match moved {
            Some(report) => Ok(vec![record, report]),
            None => Ok(vec![record]),
        }
    })
}

/// Keeps the place if it is live, else climbs to the nearest live canonical parent (§4.3).
///
/// Answers the transition it made, so the caller can report it; `None` where the place survived.
async fn restore_place(now: Timestamp) -> Option<Value> {
    let mut spatial = crate::spatial::spatial_session().await;
    let place = spatial.current_place().clone();
    if spatial.liveness(&place, now).accepts_actions() {
        return None;
    }
    let mut candidate = ono_spatial_query::resolve::parent_of(spatial.index(), &place);
    while let Some(parent) = candidate {
        if spatial.liveness(&parent, now).accepts_actions() {
            let from = place.as_str().to_owned();
            let to = parent.as_str().to_owned();
            spatial.arrive_at(&parent, now);
            return Some(transition(&from, &to));
        }
        candidate = ono_spatial_query::resolve::parent_of(spatial.index(), &parent);
    }
    None
}

/// The report §4.3 requires when returning to the present moved the place.
fn transition(from: &str, to: &str) -> Value {
    let mut identity = MapValue::new();
    identity.insert("place".into(), Value::string(to));
    let target = ValueRef::object(SchemaId::new("ono.spatial-place", 1), identity);
    ActionResult::new(target, "ono.temporal.now", ActionStatus::Success)
        .changed(true)
        .with_message(&format!(
            "`{from}` has no live counterpart; the session is at `{to}` (v0.5 §4.3)"
        ))
        .into_value()
}

/// Whether §4.8 refuses this pipeline, and why.
///
/// `None` in the present, `None` inside `present`, and `None` for a pipeline the shell answers
/// itself — the caller has already claimed those, so what reaches here is the arbitrary external
/// program §4.8 is about.
#[must_use]
pub fn present_only_refusal(
    list: &ono_parser::StageList,
    source: &str,
) -> Option<ono_value::ErrorValue> {
    if present_bound() || !views::is_historical() {
        return None;
    }
    let spelling = list
        .stages
        .first()
        .and_then(|stage| stage.head.name())
        .map_or_else(|| source.trim().to_owned(), |name| name.to_owned());
    Some(ono_temporal_core::error::present_only(&spelling))
}

/// Whether the current thread is inside a `present` command (§4.8).
fn present_bound() -> bool {
    PRESENT_DEPTH.with(|depth| depth.get() > 0)
}

thread_local! {
    /// How many `present` commands are running on this thread (§4.8).
    ///
    /// A depth rather than a flag, because a `present` whose command runs a function whose body
    /// runs another external command must not have the escape lifted halfway through by the
    /// inner one returning.
    static PRESENT_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Lifts §4.8's refusal for the dynamic extent of one `present`, and restores it after.
struct PresentGuard;

impl PresentGuard {
    fn enter() -> Self {
        PRESENT_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
        Self
    }
}

impl Drop for PresentGuard {
    fn drop(&mut self) {
        PRESENT_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// `present <command…>`: one external program, in the real present (§4.8).
///
/// The temporal coordinate is not touched — before, during or after — and the result says so,
/// which is §4.8's "the HUD MUST make the present-bound execution visible at least once in the
/// command result metadata".
fn run_present(session: &mut Session, stage: &Stage, source: &str) -> Eval<Vec<Value>> {
    let spelling = tail_of(stage, source);
    if spelling.is_empty() {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                "`present` needs the command to run in the present",
            )
            .with_help("`present git status` runs it in the real current environment (§4.8)"),
        ));
    }
    let started = std::time::Instant::now();
    let parsed = ono_parser::parse(&spelling);
    let pipeline = parsed
        .program()
        .statements
        .first()
        .and_then(ono_parser::Statement::as_pipeline)
        .cloned()
        .ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::ParseSyntax,
                format!("`present {spelling}` is not a command"),
            ))
        })?;
    let status = {
        let _present = PresentGuard::enter();
        crate::eval::run_pipeline(session, &pipeline, &spelling)?
    };
    let elapsed = ono_value::Duration::from_nanoseconds(
        i128::try_from(started.elapsed().as_nanos()).unwrap_or(i128::MAX),
    );
    Ok(vec![present_result(&spelling, status, elapsed)])
}

/// The one value `present` produces: what ran, that it ran in the present, and where the session
/// still is (§4.8).
fn present_result(spelling: &str, status: ExitStatus, elapsed: ono_value::Duration) -> Value {
    let coordinate = session::coordinate();
    let mut identity = MapValue::new();
    identity.insert("command".into(), Value::string(spelling));
    identity.insert("context".into(), Value::string("present"));
    let target = ValueRef::object(SchemaId::new("ono.temporal-context", 1), identity);
    let outcome = if status.is_success() {
        ActionStatus::Success
    } else {
        ActionStatus::Failed
    };
    let held = match coordinate.prompt_marker() {
        Some(marker) => format!("the session stays in the past {marker}"),
        None => "the session is in the present".to_owned(),
    };
    ActionResult::new(target, "ono.temporal.present", outcome)
        .changed(false)
        .with_message(&format!(
            "`{spelling}` ran in the present, against the live system; {held}"
        ))
        .with_duration(elapsed)
        .into_value()
}

/// Reads the `temporal.*` settings the shell honours and hands them to the session state (§33).
pub fn configure_from(settings: &crate::settings::Settings) {
    let show_source_tags = settings
        .flag("temporal.ui.show_source_tags")
        .unwrap_or(true);
    session::set_show_source_tags(show_source_tags);
    let zone = jiff::tz::TimeZone::system();
    let recording = settings.flag("temporal.recording.enabled").unwrap_or(false);
    if let Ok(mut state) = session::session_state().try_lock() {
        state.configure(zone, show_source_tags);
        // §10.2 makes persistent recording opt-in and off by default, and §33 makes
        // `temporal.recording.enabled` the switch. A session that finds it on opens the store,
        // which is what makes §56.6 true: events recorded by one `ono` are queryable from the
        // next. `start recorder` turns it on for the running session; the setting is how a user
        // says "and for the next one too".
        //
        // The store is opened only when the setting says so, so §32.1's disabled path still
        // touches no filesystem: below this line, a shell with recording off has done nothing.
        if recording && !state.ledger().is_persistent() {
            match recorder::store_path().and_then(|path| {
                ono_temporal_ledger::Ledger::persistent(&ono_temporal_ledger::StoreOptions::at(
                    &path,
                ))
            }) {
                Ok(ledger) => {
                    state.set_ledger(ledger);
                    recorder::note_started(Some(jiff::Timestamp::now()));
                }
                // §31.7 and §44.3: a store that cannot be opened leaves the shell working with
                // persistence off. The session ledger of §10.7 is what remains, and `get
                // recorder` reports the health rather than the prompt reporting a failure the
                // user did not ask for.
                Err(_) => recorder::note_started(None),
            }
        }
    }
    session::install_evidence();
}

/// The temporal coordinate one stage evaluates at (§4, §4.5).
///
/// The session's coordinate, unless the stage wrote `--at`. §4.5 requires `--at` to "use the same
/// reconstruction engine as `at` context" and forbids a second historical code path, and this is
/// where that is kept: both spellings call [`coordinate::resolve`], the one function that parses a
/// selector, resolves it against the session's zone and ledger, composes coverage and refuses.
///
/// # Errors
///
/// Whatever `at` would have refused with for the same selector: `temporal.invalid_time`,
/// `temporal.not_recorded` or `temporal.out_of_retention`.
pub async fn invocation_context(
    arguments: &ono_command::BoundArguments,
) -> Result<std::sync::Arc<ono_temporal_core::TemporalContext>, ErrorValue> {
    let given = match arguments.option("at") {
        Some(Value::String(text)) => Some(text.to_string()),
        Some(Value::Null) | None => None,
        Some(other) => ono_value::canonical_text(other).ok(),
    };
    let state = session::temporal_session().await;
    let now = Timestamp::now();
    match given {
        Some(text) if !text.trim().is_empty() => {
            let context = coordinate::resolve(&state, text.trim(), now)?;
            Ok(std::sync::Arc::new(context))
        }
        _ => Ok(state.shared_context()),
    }
}

/// Records one Ono mutation's §17.2 lifecycle in the session ledger.
///
/// This is the T3 bridge's action half, at the seam where the shell knows what was asked and what
/// came back. §17.1: the shell "knows exactly which actions the operator requested", and §17.3
/// requires the [`ono_temporal_core::ActionId`] to exist before execution — so the identity is
/// minted here, before `CommandTable::run` is called, and the outcome is stapled to it afterwards.
///
/// Recording never fails a command. A ledger that refused an append would be a reason to lose
/// history, and §16.5 does not make it a reason to lose the mutation's own result.
pub async fn record_action(
    spelling: &str,
    operation: &str,
    arguments: &[String],
    requested_at: Timestamp,
    outcome: Result<usize, &ono_value::ErrorValue>,
) {
    let scope = crate::spatial::local_scope();
    let actor = format!("uid:{}", ono_process::effective_uid());
    let state = session::temporal_session().await;
    let command = events::redact(spelling, None, arguments);
    let mut lifecycle = events::ActionLifecycle::requested(
        scope,
        &state.session_id(),
        &actor,
        operation,
        None,
        command,
        requested_at,
    );
    let now = Timestamp::now();
    lifecycle.authorized(
        ono_temporal_core::AuthorizationSummary {
            decision: ono_temporal_core::AuthorizationDecision::Allowed,
            risk: std::sync::Arc::from("mutate"),
            capability: None,
            reason: None,
        },
        requested_at,
    );
    lifecycle.executed(requested_at);
    match outcome {
        Ok(count) => lifecycle.completed(now, Some(&format!("{count} target(s)"))),
        Err(error) => lifecycle.failed(now, Some(error.message()), Some(error.code().name())),
    }
    // A session ledger is what §10.7 gives every session, recording or not, so this costs no
    // filesystem access when `temporal.recording.enabled` is false (§32.1).
    let _ = record_into(state.ledger(), &lifecycle);
}

/// Appends a lifecycle to a ledger, discarding the refusal a full or unavailable store raises.
fn record_into(
    ledger: &ono_temporal_ledger::Ledger,
    lifecycle: &events::ActionLifecycle,
) -> Result<(), ErrorValue> {
    lifecycle.record(ledger)
}
