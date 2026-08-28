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
use ono_spatial_core::{NavigationTrail, Projection, SpatialId, SpatialScope, space};
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
    /// The last record each place was observed as. The index holds what a place *is* (§33.1);
    /// this holds what the provider last said about it, which is what the v0.2 relationship
    /// graph expands and what §24.1's summary is read from — neither may be re-read behind the
    /// provider's back (§2.16).
    records: BTreeMap<SpatialId, Arc<RecordValue>>,
}

/// The view settings a session carries between commands (§46's `view_preferences`).
///
/// §47's `spatial.look.change_window` is the only one a command of this phase reads; the map
/// preferences of §23 arrive with the renderer that has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPreferences {
    /// How far back `look --changes` and `near --changed` look when the caller names no window.
    pub change_window: ono_value::Duration,
}

impl Default for ViewPreferences {
    fn default() -> Self {
        // §47: `spatial.look.change_window = "5m"`.
        Self {
            change_window: ono_value::Duration::from_nanoseconds(5 * 60 * 1_000_000_000),
        }
    }
}

impl SpatialSessionState {
    /// A fresh session: standing at the local SYSTEM root, with an empty trail (§46.1).
    #[must_use]
    pub fn new(scope: SpatialScope, now: Timestamp) -> Self {
        Self {
            trail: NavigationTrail::new(space::root().spatial_id()),
            index: SpatialIndex::new(FreshnessPolicy::recommended()),
            bridge: ProviderBridge::new(Projection::new(scope.clone(), now)),
            scope,
            pins: PinRegistry::new(),
            preferences: ViewPreferences::default(),
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

    /// Registers what a provider answered: the places the records are, and the records
    /// themselves (§33.1, §33.2).
    pub fn absorb(&mut self, records: &[RecordValue], at: Timestamp) -> Absorbed {
        for record in records {
            if let Ok(object) = self.bridge.project(record) {
                self.records
                    .insert(object.spatial_id().clone(), Arc::new(record.clone()));
            }
        }
        self.bridge.absorb(&mut self.index, records, at)
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
