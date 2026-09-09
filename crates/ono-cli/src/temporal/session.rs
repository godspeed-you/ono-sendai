//! The temporal state of one shell session: where in time it is standing, how it got there, and
//! the ledger that answers for the past (spec v0.5 §3.9, §4, §10.7, §12.4).
//!
//! §55.7 forbids `ono-cli` becoming the temporal engine, and this module is written to that: it
//! holds a coordinate, a trail, a ledger handle and the event references one session has issued.
//! Resolving a selector is `ono-temporal-core`'s, composing coverage is `ono-temporal-core`'s,
//! answering a query is `ono-temporal-query`'s, and reconstructing a world is
//! `ono-temporal-reconstruct`'s. What lives here is the fact that a session has one coordinate.
//!
//! # Why it is a process-wide static rather than a `Session` field
//!
//! Exactly the reason the spatial place is one (v0.4 §29.2, `crate::spatial::session`): a command
//! is handed an [`Invocation`], not the shell, so state a command must read has to be reachable
//! without one. A called script is another process and therefore another coordinate, which is the
//! strongest form of "a script MUST NOT silently change the caller's context".
//!
//! # Why the coordinate is published a second time
//!
//! [`crate::spatial::TemporalEvidence`] is synchronous — a spatial command asks what time it is
//! while it is already inside the async runtime — and a `tokio::sync::Mutex` cannot be read from
//! there without risking a deadlock. So every committed transition writes the context into
//! [`published`] as well, under the same lock, and the synchronous readers read that. It is a
//! projection with one writer, never a second coordinate: nothing writes it except
//! [`TemporalState::commit`].
//!
//! [`Invocation`]: ono_command::Invocation

use std::sync::{Arc, OnceLock, RwLock};

use jiff::{Timestamp, tz::TimeZone};
use ono_temporal_core::{LedgerRead, TemporalContext, TimeSelector};
use ono_temporal_ledger::Ledger;
use ono_temporal_query::search::EventReferences;
use tokio::sync::{Mutex, MutexGuard};

/// One move along the temporal trail of §12.4.
///
/// The trail is separate from the spatial one on purpose: §12.4 states that `back` remains spatial
/// navigation and must not become overloaded, so nothing here is reachable from `back`, `up` or
/// `trail`.
#[derive(Debug, Clone)]
pub struct TemporalStep {
    /// What the user typed — `-10m`, `event @e42`, `now`.
    pub requested: Arc<str>,
    /// The coordinate the session moved to. `None` for a return to the present.
    pub resolved_at: Option<Timestamp>,
    /// When the move was made, by the real clock, so the trail is legible in the present.
    pub moved_at: Timestamp,
}

/// What a session remembers about when it is (§3.9, §4, §12.4).
#[derive(Debug)]
pub struct TemporalState {
    context: Arc<TemporalContext>,
    started_at: Timestamp,
    trail: Vec<TemporalStep>,
    ledger: Arc<Ledger>,
    references: EventReferences,
    zone: TimeZone,
    show_source_tags: bool,
}

/// How many temporal moves the trail keeps.
///
/// The trail is a navigation aid rather than a record — §29.2 keeps the ledger and the command
/// history as the two things that are records — so it is bounded like the spatial trail is, and
/// the oldest step is dropped rather than the newest refused.
const TRAIL_DEPTH: usize = 64;

impl TemporalState {
    /// A session in the present, with §10.7's bounded in-memory ledger and nothing on disk.
    ///
    /// §32.1 budgets startup with recording disabled at no filesystem cost, and
    /// [`Ledger::default`] is that: it opens nothing, creates nothing and reads nothing.
    #[must_use]
    pub fn new(zone: TimeZone) -> Self {
        Self {
            context: Arc::new(TemporalContext::Present),
            started_at: Timestamp::now(),
            trail: Vec::new(),
            ledger: Arc::new(Ledger::default()),
            references: EventReferences::new(),
            zone,
            show_source_tags: true,
        }
    }

    /// The identity §17.4 records an action against, and §29.2 lets the two histories share.
    ///
    /// Derived from the process and the instant it started observing, so it is stable for the
    /// life of the session and different from every other session's without a registry.
    #[must_use]
    pub fn session_id(&self) -> String {
        format!(
            "s{:x}{:x}",
            std::process::id(),
            self.started_at.as_nanosecond().unsigned_abs()
        )
    }

    /// When this session started observing (§10.7).
    ///
    /// The bounded in-memory ledger exists from here on, so this is the earliest instant the
    /// session itself is evidence for — and the boundary §12.3 refuses beyond when nothing else
    /// can reach further back.
    #[must_use]
    pub fn started_at(&self) -> Timestamp {
        self.started_at
    }

    /// Where in time the session is standing (§3.9).
    #[must_use]
    pub fn context(&self) -> &TemporalContext {
        &self.context
    }

    /// The same coordinate, shared, for handing to an [`ono_command::Invocation`].
    #[must_use]
    pub fn shared_context(&self) -> Arc<TemporalContext> {
        Arc::clone(&self.context)
    }

    /// Moves the session to `context` and records the move on the temporal trail (§12.4).
    ///
    /// This is the only writer of [`published`], which is what keeps the synchronous readers and
    /// the asynchronous ones from ever disagreeing about what time it is.
    pub fn commit(&mut self, context: TemporalContext, requested: &str, moved_at: Timestamp) {
        self.trail.push(TemporalStep {
            requested: Arc::from(requested),
            resolved_at: context.instant(),
            moved_at,
        });
        if self.trail.len() > TRAIL_DEPTH {
            self.trail.remove(0);
        }
        let shared = Arc::new(context);
        self.context = Arc::clone(&shared);
        if let Ok(mut held) = published().write() {
            *held = shared;
        }
    }

    /// The temporal trail, oldest first (§12.4).
    #[must_use]
    pub fn trail(&self) -> &[TemporalStep] {
        &self.trail
    }

    /// The ledger this session reads history from (§3.1, §10.7).
    #[must_use]
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// The same ledger, shared, for a spatial reconstruction that outlives the borrow.
    #[must_use]
    pub fn ledger_handle(&self) -> Arc<dyn LedgerRead> {
        Arc::clone(&self.ledger) as Arc<dyn LedgerRead>
    }

    /// Replaces the ledger, which is what `start recorder` and `stop recorder` do (§10.8).
    pub fn set_ledger(&mut self, ledger: Ledger) {
        let shared = Arc::new(ledger);
        self.ledger = Arc::clone(&shared);
        if let Ok(mut held) = published_ledger().write() {
            *held = shared;
        }
    }

    /// The event references this session has issued (§11.6).
    ///
    /// A reference is one session's word for an event, which is why it lives here rather than in
    /// the ledger: `@e42` means the forty-second event *this reader saw*.
    #[must_use]
    pub fn references(&mut self) -> &mut EventReferences {
        &mut self.references
    }

    /// The zone wall clocks are displayed in (§25.3).
    #[must_use]
    pub fn zone(&self) -> &TimeZone {
        &self.zone
    }

    /// `temporal.ui.show_source_tags` (§33), as the renderers need it.
    #[must_use]
    pub fn show_source_tags(&self) -> bool {
        self.show_source_tags
    }

    /// Applies the session's resolved configuration (§33).
    pub fn configure(&mut self, zone: TimeZone, show_source_tags: bool) {
        self.zone = zone;
        self.show_source_tags = show_source_tags;
    }

    /// The session's offset from UTC at `instant`, as a renderer wants it (§25.3).
    #[must_use]
    pub fn utc_offset(&self, instant: Timestamp) -> ono_value::Duration {
        let seconds = i128::from(self.zone.to_offset(instant).seconds());
        ono_value::Duration::from_nanoseconds(seconds.saturating_mul(1_000_000_000))
    }

    /// The selector the session is standing at, for a message that quotes it back (§4.2).
    #[must_use]
    pub fn requested(&self) -> Option<&TimeSelector> {
        match self.context.as_ref() {
            TemporalContext::Present => None,
            TemporalContext::Historical { requested, .. } => Some(requested),
        }
    }
}

/// The one temporal state of this process (§4, v0.4 §29.2).
///
/// Asynchronous because the commands that hold it reach the ledger and the reconstruction engine
/// while they do, and because a blocking guard taken inside the runtime is a deadlock waiting for
/// a reason.
pub fn session_state() -> &'static Mutex<TemporalState> {
    static STATE: OnceLock<Arc<Mutex<TemporalState>>> = OnceLock::new();
    STATE.get_or_init(|| Arc::new(Mutex::new(TemporalState::new(TimeZone::system()))))
}

/// Borrows this process's temporal state.
pub async fn temporal_session() -> MutexGuard<'static, TemporalState> {
    session_state().lock().await
}

/// The committed coordinate, readable without the lock.
///
/// Written only by [`TemporalState::commit`], while the lock is held.
fn published() -> &'static RwLock<Arc<TemporalContext>> {
    static PUBLISHED: OnceLock<RwLock<Arc<TemporalContext>>> = OnceLock::new();
    PUBLISHED.get_or_init(|| RwLock::new(Arc::new(TemporalContext::Present)))
}

/// The ledger behind the committed coordinate, readable without the lock.
fn published_ledger() -> &'static RwLock<Arc<Ledger>> {
    static LEDGER: OnceLock<RwLock<Arc<Ledger>>> = OnceLock::new();
    LEDGER.get_or_init(|| RwLock::new(Arc::new(Ledger::default())))
}

/// Where in time this process is standing, without taking the lock (§4.1).
///
/// A poisoned lock answers the present, because the present is the state in which every v0.2–v0.4
/// behaviour is the one that runs: degrading towards "no historical claim" is the only safe
/// direction for a coordinate to fail in.
#[must_use]
pub fn coordinate() -> Arc<TemporalContext> {
    published()
        .read()
        .map(|held| Arc::clone(&held))
        .unwrap_or_else(|_| Arc::new(TemporalContext::Present))
}

/// The ledger behind the current coordinate, without taking the lock.
#[must_use]
pub fn ledger_handle() -> Arc<dyn LedgerRead> {
    published_ledger()
        .read()
        .map_or_else(|_| Arc::new(Ledger::default()), |held| Arc::clone(&held))
        as Arc<dyn LedgerRead>
}

/// `temporal.ui.show_source_tags` (§33), readable by a renderer that has no session in hand.
static SHOW_SOURCE_TAGS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Records the resolved `temporal.ui.show_source_tags`, so `crate::sink` can hand it to a
/// renderer rather than let the renderer read the configuration itself (§39.2).
pub fn set_show_source_tags(show: bool) {
    SHOW_SOURCE_TAGS.store(show, std::sync::atomic::Ordering::Relaxed);
}

/// Whether a rendered event carries its abbreviated source tag (§11.5, §33).
#[must_use]
pub fn show_source_tags() -> bool {
    SHOW_SOURCE_TAGS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The bridge §14.1 reads the coordinate through, so the spatial layer keeps no second one.
#[derive(Debug, Clone, Copy)]
pub struct SessionEvidence;

impl crate::spatial::TemporalEvidence for SessionEvidence {
    fn coordinate(&self) -> Option<Timestamp> {
        coordinate().instant()
    }

    fn ledger(&self) -> Arc<dyn LedgerRead> {
        ledger_handle()
    }
}

/// Installs the session's coordinate as what the spatial layer evaluates against (§14.1, §55.9).
///
/// Without this, `at -10m` would move the prompt and leave `look` and `map` showing the present,
/// which §55.9 names as the failure that makes the whole feature invalid. It is idempotent.
pub fn install_evidence() {
    crate::spatial::historical::install(Arc::new(SessionEvidence));
}
