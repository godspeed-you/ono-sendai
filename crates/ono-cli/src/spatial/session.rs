//! The spatial state of one shell session (spec v0.4 §46, §29.2, §33.1, §45.6).
//!
//! §46 lists what an interactive session must keep — the current place, the trail, the pins and
//! the view preferences — and §45.6 says where it lives: "`ono-cli` should parse/dispatch spatial
//! commands and own session current-place state". This is that state, and it is deliberately
//! nothing else: no graph selection, no identity reconciliation, no layout.
//!
//! Two rules shape it.
//!
//! **It is per process.** §29.2: "the current place is script-local unless explicitly operating
//! in the interactive shell session. A script MUST NOT silently change the caller's interactive
//! spatial context." A called script is another `ono` process and therefore another state, which
//! is the strongest form of that guarantee: there is no shared place to change. §46.1 fixes what
//! a fresh one starts as — the local SYSTEM root, with an empty trail.
//!
//! **It holds the index for the session.** §33.1 makes the spatial index an in-memory cache and
//! §33.2 makes the providers authoritative; §34 budgets a second `look` at 50 ms, which is only
//! reachable if the second `look` reads what the first one learned. So the index and the provider
//! bridge that fills it live here, for as long as the process does, and are handed to a command
//! for the duration of its run rather than rebuilt by it (ADR-0141, superseded here for the
//! session's own commands).

use std::collections::BTreeMap;
use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{
    Liveness, NavigationTrail, Projection, SpatialId, SpatialScope, Tombstone, TombstoneRegistry,
    space,
};
use ono_spatial_index::{Absorbed, FreshnessPolicy, PinRegistry, ProviderBridge, SpatialIndex};
use ono_value::RecordValue;
use tokio::sync::{Mutex, MutexGuard};

/// What a session remembers about where it is (§46).
#[derive(Debug)]
pub struct SpatialSessionState {
    trail: NavigationTrail,
    scope: SpatialScope,
    index: SpatialIndex,
    bridge: ProviderBridge,
    pins: PinRegistry,
    preferences: ViewPreferences,
    /// The thresholds the built-in landmark rules of §26.2 measure against (§26.3).
    thresholds: ono_spatial_query::LandmarkThresholds,
    /// What each place's neighborhood looked like when it was last asked about with
    /// `--changes` — the comparison snapshot of §25.4, and the only thing that lets §24.3's
    /// change section say anything without an event stream.
    baselines: BTreeMap<SpatialId, ono_spatial_events::PlaceSnapshot>,
    /// The places that went away while this session was watching, for as long as §10.3 keeps
    /// them (§20.3, §46). The index keeps their identity; this keeps the fact that they ended.
    tombstones: TombstoneRegistry,
    /// The last record each place was observed as. The index holds what a place *is* (§33.1);
    /// this holds what the provider last said about it, which is what the v0.2 relationship
    /// graph expands and what §24.1's summary is read from — neither may be re-read behind the
    /// provider's back (§2.16).
    records: BTreeMap<SpatialId, Arc<RecordValue>>,
}

/// The view settings a session carries between commands (§46's `view_preferences`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPreferences {
    /// How far back `look --changes` and `near --changed` look when the caller names no window.
    pub change_window: ono_value::Duration,
    /// How many nodes a map may draw (§34.2, §47's `spatial.map.node_budget`).
    pub map_node_budget: usize,
    /// How long a removed place remains reachable as a tombstone (§10.3).
    pub tombstone_lifetime: ono_value::Duration,
    /// How often a live view re-reads its sources where nothing subscribes (§25.1).
    pub live_interval: ono_value::Duration,
}

impl Default for ViewPreferences {
    fn default() -> Self {
        // §47: `spatial.look.change_window = "5m"`, `spatial.map.node_budget = 100`.
        Self {
            change_window: ono_value::Duration::from_nanoseconds(5 * 60 * 1_000_000_000),
            map_node_budget: ono_spatial_query::MAP_NODE_BUDGET,
            // §10.3 says "short-lived" and nothing more; a minute is long enough that a `back`
            // onto a process that has just exited arrives, and short enough that a place cannot
            // come back from the dead in the middle of an investigation (ADR-0179).
            tombstone_lifetime: ono_value::Duration::from_nanoseconds(60 * 1_000_000_000),
            // §34 budgets a view at well under a second, and §25.2 forbids activity that is not
            // change; half a second is fast enough that a connection opening is seen while it is
            // open, and slow enough to cost nothing anyone notices (ADR-0180).
            live_interval: ono_value::Duration::from_nanoseconds(500 * 1_000_000),
        }
    }
}

/// What the user's configuration says, seeded once before the first spatial command runs (§47).
///
/// The command table is built from the shell's session, which is the only place the resolved
/// settings live; the spatial state is a `static` that outlives any one command (§29.2). This is
/// how the one reaches the other, and it is set once, so a later `set config` in the same session
/// does not silently redefine what an already-drawn view meant.
static CONFIGURED: std::sync::OnceLock<(ViewPreferences, ono_spatial_query::LandmarkThresholds)> =
    std::sync::OnceLock::new();

/// A configured duration as the span the tombstone registry counts in.
fn span_of(duration: ono_value::Duration) -> jiff::Span {
    let seconds = i64::try_from(duration.nanoseconds() / 1_000_000_000).unwrap_or(60);
    jiff::Span::new().seconds(seconds.max(1))
}

/// Records the settings the spatial layer reads (§26.3, §34.2, §47).
pub fn configure(preferences: ViewPreferences, thresholds: ono_spatial_query::LandmarkThresholds) {
    let _ = CONFIGURED.set((preferences, thresholds));
}

impl SpatialSessionState {
    /// A fresh session: standing at the local SYSTEM root, with an empty trail (§46.1).
    #[must_use]
    pub fn new(scope: SpatialScope, now: Timestamp) -> Self {
        let (preferences, thresholds) = CONFIGURED.get().cloned().unwrap_or_else(|| {
            (
                ViewPreferences::default(),
                ono_spatial_query::LandmarkThresholds::default(),
            )
        });
        let tombstones = TombstoneRegistry::new(span_of(preferences.tombstone_lifetime));
        Self {
            trail: NavigationTrail::new(space::root().spatial_id()),
            index: SpatialIndex::new(FreshnessPolicy::recommended()),
            bridge: ProviderBridge::new(Projection::new(scope.clone(), now)),
            scope,
            pins: PinRegistry::new(),
            preferences,
            thresholds,
            tombstones,
            baselines: BTreeMap::new(),
            records: BTreeMap::new(),
        }
    }

    /// Where the session is standing (§46's `current_place`).
    #[must_use]
    pub fn current_place(&self) -> &SpatialId {
        self.trail.current()
    }

    /// The navigation history (§20.1, §46).
    #[must_use]
    pub fn trail(&self) -> &NavigationTrail {
        &self.trail
    }

    /// The navigation history, to record a movement in.
    pub fn trail_mut(&mut self) -> &mut NavigationTrail {
        &mut self.trail
    }

    /// The host and boot every observation of this session belongs to (§3.2, §10.2).
    #[must_use]
    pub fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// What the session has learned (§33.1). A cache; the providers stay authoritative (§33.2).
    #[must_use]
    pub fn index(&self) -> &SpatialIndex {
        &self.index
    }

    /// The index and the bridge together, which is how anything is added to it.
    pub fn absorb_with(&mut self) -> (&mut SpatialIndex, &mut ProviderBridge) {
        (&mut self.index, &mut self.bridge)
    }

    /// Registers what a provider answered: the places the records are, the records themselves,
    /// and the landmarks the built-in rules of §26.2 find on them (§33.1, §33.2).
    ///
    /// The landmark engine runs here rather than in a view, because a landmark is a fact about
    /// the object and not about the projection that happens to show it: `look`, `near` and `map`
    /// must all agree about what deserves attention, and ranking needs it before any of them
    /// chooses what to show (§26.1, §3.6).
    pub fn absorb(&mut self, records: &[RecordValue], at: Timestamp) -> Absorbed {
        for record in records {
            if let Ok(object) = self.bridge.project(record) {
                // §33.2: the providers are authoritative. A place one of them answers for is a
                // live place, whatever this session remembers about it having gone quiet.
                self.tombstones.forget(object.spatial_id());
                self.records
                    .insert(object.spatial_id().clone(), Arc::new(record.clone()));
            }
        }
        let absorbed = self.bridge.absorb(&mut self.index, records, at);
        self.promote(records, at);
        absorbed
    }

    /// Runs the built-in landmark rules over what was just observed (§26.2).
    fn promote(&mut self, records: &[RecordValue], at: Timestamp) {
        if !self.thresholds.enabled {
            return;
        }
        for record in records {
            let Ok(object) = self.bridge.project(record) else {
                continue;
            };
            let landmarks = ono_spatial_query::landmarks_of_object(
                &object,
                Some(record),
                &self.thresholds,
                &self.scope,
                at,
            );
            if !landmarks.is_empty() {
                self.index.set_landmarks(object.spatial_id(), landmarks);
            }
        }
    }

    /// Replaces what this session last saw around `id`, and answers with what it saw before
    /// (§25.4).
    ///
    /// Returns `None` the first time a place is asked about: §24.3 forbids inventing a change
    /// summary where no comparison snapshot exists, and "there was nothing to compare to" is a
    /// different answer from "nothing changed" (§2.17).
    pub fn rebase(
        &mut self,
        id: &SpatialId,
        snapshot: ono_spatial_events::PlaceSnapshot,
    ) -> Option<ono_spatial_events::PlaceSnapshot> {
        self.baselines.insert(id.clone(), snapshot)
    }

    /// Records that the providers no longer answer for the place at `id` (§10.3, §33.2).
    ///
    /// The index keeps the entry, because §10.3 makes a tombstone the *same* place — "the
    /// identity is retained" is what tells it from a place that never existed (§40) — and §20.3
    /// makes `back` arrive at one. What changes is that the object's lifetime closes and the
    /// session remembers that it ended, so nothing reads the last live answer back as current.
    pub fn record_removed(&mut self, id: &SpatialId, at: Timestamp) {
        if self.tombstones.recorded(id) {
            return;
        }
        let Some(entry) = self.index.get(id) else {
            return;
        };
        let tombstone = Tombstone::new(
            id.clone(),
            entry.object().object_type(),
            entry.object().display_name(),
            at,
        );
        self.tombstones.record(tombstone);
        self.index.mark_ended(id, at);
        self.index.forget_edges(id);
        self.tombstones.prune(at);
    }

    /// Whether the place at `id` is still there (§10.3, §20.3, §33.2).
    #[must_use]
    pub fn liveness(&self, id: &SpatialId, now: Timestamp) -> Liveness {
        self.tombstones
            .resolve(id, !self.tombstones.recorded(id), now)
    }

    /// The tombstone of `id`, where one is still held (§10.3).
    #[must_use]
    pub fn tombstone_of(&self, id: &SpatialId, now: Timestamp) -> Option<&Tombstone> {
        self.tombstones.get(id, now)
    }

    /// Replaces the landmark thresholds with the ones the user configured (§26.3).
    pub fn set_thresholds(&mut self, thresholds: ono_spatial_query::LandmarkThresholds) {
        self.thresholds = thresholds;
    }

    /// Which place a record is, without registering it (§45.2).
    ///
    /// # Errors
    ///
    /// `spatial.identity_conflict` when the record declares no identity §3.1 can be derived from.
    pub fn projection_of(&self, record: &RecordValue) -> Result<SpatialId, ono_value::ErrorValue> {
        self.bridge
            .project(record)
            .map(|object| object.spatial_id().clone())
    }

    /// The place a record is, as the bridge projects it (§45.2).
    ///
    /// # Errors
    ///
    /// `spatial.identity_conflict` when the record declares no identity §3.1 can be derived from.
    pub fn projection_of_object(
        &self,
        record: &RecordValue,
    ) -> Result<ono_spatial_core::SpatialObject, ono_value::ErrorValue> {
        self.bridge.project(record)
    }

    /// The place another record's reference names — a pid, a unit name, an interface (§45.2).
    ///
    /// This is the bridge's `resolve`, not the alias index: it answers what a *record* names,
    /// which is what a `enter <target> <identity>` argument is.
    #[must_use]
    pub fn reference(
        &self,
        object_type: ono_spatial_core::SpatialType,
        key: &str,
    ) -> Option<SpatialId> {
        self.bridge.resolve(object_type, key).cloned()
    }

    /// What the provider last said about the object at `id`, where this session has seen it.
    #[must_use]
    pub fn record_of(&self, id: &SpatialId) -> Option<&Arc<RecordValue>> {
        self.records.get(id)
    }

    /// The pins this session knows about (§20.4, §26.4).
    #[must_use]
    pub fn pins(&self) -> &PinRegistry {
        &self.pins
    }

    /// Replaces the pins with what the store holds, once per command that needs them (§46.1).
    pub fn set_pins(&mut self, pins: PinRegistry) {
        self.pins = pins;
    }

    /// The view settings (§46's `view_preferences`).
    #[must_use]
    pub fn preferences(&self) -> &ViewPreferences {
        &self.preferences
    }

    /// Replaces the view settings with the ones the user configured (§47).
    pub fn set_preferences(&mut self, preferences: ViewPreferences) {
        self.preferences = preferences;
    }
}

/// The one spatial state of this process (§29.2, §46).
///
/// A `static` rather than a field of [`crate::session::Session`] because the command table the
/// evaluator builds is itself static: a spatial command is handed an [`Invocation`], not the
/// shell. The lock is asynchronous because the commands that hold it reach providers while they
/// do, and a blocking guard held across an `await` would be a deadlock waiting to happen.
///
/// [`Invocation`]: ono_command::Invocation
pub fn session_state() -> &'static Mutex<SpatialSessionState> {
    static STATE: std::sync::OnceLock<Arc<Mutex<SpatialSessionState>>> = std::sync::OnceLock::new();
    STATE.get_or_init(|| {
        Arc::new(Mutex::new(SpatialSessionState::new(
            crate::spatial::local_scope(),
            Timestamp::now(),
        )))
    })
}

/// Borrows this process's spatial state.
pub async fn spatial_session() -> MutexGuard<'static, SpatialSessionState> {
    session_state().lock().await
}
