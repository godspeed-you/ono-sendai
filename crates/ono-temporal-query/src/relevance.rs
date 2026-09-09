//! Relevance ranking (spec v0.5 §11.3, §18.4): which events belong to a place and a horizon, in
//! the priority order significant-event stepping walks.
//!
//! Two sentences of the specification are what this module makes true.
//!
//! §11.3 scopes a timeline without a selector to "the current spatial place and its directly
//! relevant events", and at the root place to "high-significance events and current-session
//! actions rather than dumping every event from every object". [`is_default_scope`] is that
//! filter, and [`Horizon`] is the place it is asked about.
//!
//! §18.4 fixes the order `[` and `]` step in: node appearance and disappearance, relation
//! appearance and disappearance, service and container state changes, landmark changes, operator
//! actions, then other selected object changes. [`RelevanceClass`] is that list in that order,
//! and [`Relevance::rank`] is the number a planner sorts on.
//!
//! Both are computed from an event and a horizon alone. No provider is consulted, no clock is
//! read and no ledger is reached for, so a relevance decision is reproducible in a test that
//! builds two events by hand (§39.2, §39.3).

use ono_spatial_core::{SpatialId, SpatialScope, SpatialType};
use ono_temporal_core::{EventKind, EvidenceSource, SpatialRef, TemporalEvent};

/// The state fields whose change is a service or container state change (§18.4 rank three).
///
/// The names are the canonical ones the service and container providers already publish, so a
/// rule keyed on them needs no second vocabulary.
const STATE_FIELDS: &[&str] = &[
    "active_state",
    "sub_state",
    "state",
    "status",
    "load_state",
    "health",
    "result",
];

/// How an event relates to the place a timeline is drawn for (§11.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlaceRelation {
    /// The event's subject is the place itself.
    Place,
    /// The event touches an object directly related to the place — an exit of its neighbourhood.
    Neighbour,
    /// The event is inside the visible scope and touches neither.
    Scope,
    /// The event is outside the visible scope.
    Elsewhere,
}

impl PlaceRelation {
    /// Every relation, closest first.
    pub const ALL: &'static [PlaceRelation] = &[
        PlaceRelation::Place,
        PlaceRelation::Neighbour,
        PlaceRelation::Scope,
        PlaceRelation::Elsewhere,
    ];

    /// The name a renderer and `inspect` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PlaceRelation::Place => "place",
            PlaceRelation::Neighbour => "neighbour",
            PlaceRelation::Scope => "scope",
            PlaceRelation::Elsewhere => "elsewhere",
        }
    }

    /// Where it sits in the closeness order; lower is closer.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            PlaceRelation::Place => 0,
            PlaceRelation::Neighbour => 1,
            PlaceRelation::Scope => 2,
            PlaceRelation::Elsewhere => 3,
        }
    }
}

impl std::fmt::Display for PlaceRelation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What kind of significance an event carries (§18.4).
///
/// The order is §18.4's own stepping priority, and [`RelevanceClass::ALL`] states it so a test
/// can assert the list rather than the individual comparisons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelevanceClass {
    /// A node appeared or disappeared.
    NodeLifecycle,
    /// A relation appeared or disappeared.
    RelationLifecycle,
    /// A service or container changed state.
    ServiceState,
    /// A landmark became true or stopped being true (§27.1).
    LandmarkChange,
    /// The operator asked the shell for a mutation (§17.2).
    OperatorAction,
    /// Some other field of a selected object changed.
    ObjectChange,
    /// Bookkeeping a reader steps past — an observation that changed nothing, a checkpoint.
    Background,
}

impl RelevanceClass {
    /// Every class, in §18.4's stepping priority order.
    pub const ALL: &'static [RelevanceClass] = &[
        RelevanceClass::NodeLifecycle,
        RelevanceClass::RelationLifecycle,
        RelevanceClass::ServiceState,
        RelevanceClass::LandmarkChange,
        RelevanceClass::OperatorAction,
        RelevanceClass::ObjectChange,
        RelevanceClass::Background,
    ];

    /// The name a renderer and `inspect` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RelevanceClass::NodeLifecycle => "node_lifecycle",
            RelevanceClass::RelationLifecycle => "relation_lifecycle",
            RelevanceClass::ServiceState => "service_state",
            RelevanceClass::LandmarkChange => "landmark_change",
            RelevanceClass::OperatorAction => "operator_action",
            RelevanceClass::ObjectChange => "object_change",
            RelevanceClass::Background => "background",
        }
    }

    /// The class with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|class| class.as_str() == name)
    }

    /// Where it sits in §18.4's order; lower steps first.
    #[must_use]
    pub fn rank(self) -> u8 {
        Self::ALL
            .iter()
            .position(|class| *class == self)
            .unwrap_or(Self::ALL.len()) as u8
    }

    /// Whether §11.3 counts this as a high-significance event at the root place.
    ///
    /// Lifecycle, state and landmark changes orient a reader who has not chosen a place; a field
    /// moving by a byte does not, and showing it is the firehose §11.3 refuses.
    #[must_use]
    pub const fn is_high_significance(self) -> bool {
        matches!(
            self,
            RelevanceClass::NodeLifecycle
                | RelevanceClass::RelationLifecycle
                | RelevanceClass::ServiceState
                | RelevanceClass::LandmarkChange
        )
    }
}

impl std::fmt::Display for RelevanceClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a timeline is scoped to (§11.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Horizon {
    scope: SpatialScope,
    place: Option<SpatialId>,
    neighbours: Vec<SpatialId>,
    selected: Vec<SpatialId>,
    widened: bool,
    root: bool,
}

impl Horizon {
    /// The horizon of a session standing at `place`, with `neighbours` as its directly related
    /// objects (§11.3).
    #[must_use]
    pub fn at_place(scope: SpatialScope, place: SpatialId, neighbours: Vec<SpatialId>) -> Self {
        Self {
            scope,
            place: Some(place),
            neighbours,
            selected: Vec::new(),
            widened: false,
            root: false,
        }
    }

    /// The horizon of a session standing at the root system place (§11.3).
    #[must_use]
    pub fn at_root(scope: SpatialScope) -> Self {
        Self {
            scope,
            place: None,
            neighbours: Vec::new(),
            selected: Vec::new(),
            widened: false,
            root: true,
        }
    }

    /// The horizon `timeline --all` asks for: the whole visible scope (§11.3).
    ///
    /// Retention and permission still bound the answer; they are the ledger's to apply, and this
    /// type only says that the *planner* stops narrowing.
    #[must_use]
    pub fn everything(scope: SpatialScope) -> Self {
        Self {
            scope,
            place: None,
            neighbours: Vec::new(),
            selected: Vec::new(),
            widened: true,
            root: false,
        }
    }

    /// The horizon of an explicit selector — `timeline service nginx` (§11.2).
    #[must_use]
    pub fn selecting(scope: SpatialScope, selected: Vec<SpatialId>) -> Self {
        Self {
            scope,
            place: None,
            neighbours: Vec::new(),
            selected,
            widened: false,
            root: false,
        }
    }

    /// The same horizon widened by `--all`.
    #[must_use]
    pub fn widened(mut self) -> Self {
        self.widened = true;
        self
    }

    /// The visible scope.
    #[must_use]
    pub fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// The place the session stands at, or `None` at the root and under `--all`.
    #[must_use]
    pub fn place(&self) -> Option<&SpatialId> {
        self.place.as_ref()
    }

    /// The objects directly related to the place (§11.3).
    #[must_use]
    pub fn neighbours(&self) -> &[SpatialId] {
        &self.neighbours
    }

    /// The identities the planner pushes into an [`ono_temporal_core::EventQuery`].
    ///
    /// Empty where the horizon is not keyed on identity, which is what `--all` and the root
    /// place are: there the window and the significance rule bound the answer instead.
    #[must_use]
    pub fn subjects(&self) -> Vec<SpatialId> {
        if self.widened {
            return Vec::new();
        }
        if !self.selected.is_empty() {
            return self.selected.clone();
        }
        match &self.place {
            Some(place) => std::iter::once(place.clone())
                .chain(self.neighbours.iter().cloned())
                .collect(),
            None => Vec::new(),
        }
    }

    /// Whether `--all` widened this horizon (§11.3).
    #[must_use]
    pub const fn is_widened(&self) -> bool {
        self.widened
    }

    /// Whether the session stands at the root system place (§11.3).
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.root
    }
}

/// How relevant one event is to one horizon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relevance {
    /// What kind of significance it carries (§18.4).
    pub class: RelevanceClass,
    /// How close it is to the place (§11.3).
    pub relation: PlaceRelation,
}

impl Relevance {
    /// The stepping rank of §18.4; lower steps first.
    #[must_use]
    pub fn rank(&self) -> u8 {
        self.class.rank()
    }

    /// The key a deterministic ranking sorts on: significance first, then closeness.
    #[must_use]
    pub fn sort_key(&self) -> (u8, u8) {
        (self.class.rank(), self.relation.rank())
    }
}

/// What kind of significance `event` carries, and how close it is to `horizon` (§11.3, §18.4).
#[must_use]
pub fn classify(event: &TemporalEvent, horizon: &Horizon) -> Relevance {
    Relevance {
        class: class_of(event),
        relation: relation_of(event, horizon),
    }
}

/// Whether `event` belongs in a `timeline` with no selector (§11.3).
///
/// At a place: the place and its directly relevant objects. At the root: high-significance
/// events and the operator's own actions in this session. Under `--all`: everything the visible
/// scope holds, which the ledger has already bounded by retention and permission.
#[must_use]
pub fn is_default_scope(event: &TemporalEvent, horizon: &Horizon) -> bool {
    let relevance = classify(event, horizon);
    if relevance.relation == PlaceRelation::Elsewhere {
        return false;
    }
    if horizon.is_widened() {
        return true;
    }
    if horizon.is_root() {
        return relevance.class.is_high_significance() || is_session_action(event);
    }
    matches!(
        relevance.relation,
        PlaceRelation::Place | PlaceRelation::Neighbour
    )
}

/// `events` in relevance order: §18.4's priority, then closeness, then presentation instant.
///
/// The identity breaks the last tie, so the same set produces the same order however it arrived
/// — which is what makes `[` and `]` land on the same event twice (§18.4, §26.3).
#[must_use]
pub fn rank_events(events: &[TemporalEvent], horizon: &Horizon) -> Vec<TemporalEvent> {
    let mut ranked: Vec<TemporalEvent> = events.to_vec();
    ranked.sort_by(|a, b| {
        classify(a, horizon)
            .sort_key()
            .cmp(&classify(b, horizon).sort_key())
            .then_with(|| {
                a.times
                    .presentation_instant()
                    .cmp(&b.times.presentation_instant())
            })
            .then_with(|| a.event_id.as_str().cmp(b.event_id.as_str()))
    });
    ranked
}

/// Whether the operator's own session produced the event (§11.3's current-session actions).
fn is_session_action(event: &TemporalEvent) -> bool {
    event.kind.is_action() && event.provenance.provider() == EvidenceSource::session().as_str()
}

/// §18.4's class for one event, read from the kind, the subject type and the changed fields.
fn class_of(event: &TemporalEvent) -> RelevanceClass {
    match event.kind {
        EventKind::ObjectAppeared | EventKind::ObjectDisappeared => RelevanceClass::NodeLifecycle,
        EventKind::RelationAdded | EventKind::RelationRemoved => RelevanceClass::RelationLifecycle,
        EventKind::LandmarkAdded | EventKind::LandmarkRemoved => RelevanceClass::LandmarkChange,
        EventKind::ActionRequested
        | EventKind::ActionAuthorized
        | EventKind::ActionExecuted
        | EventKind::ActionCompleted
        | EventKind::ActionFailed => RelevanceClass::OperatorAction,
        EventKind::ObjectChanged => {
            if is_state_change(event) {
                RelevanceClass::ServiceState
            } else {
                RelevanceClass::ObjectChange
            }
        }
        EventKind::ObjectObserved
        | EventKind::ProviderEvent
        | EventKind::CoverageStarted
        | EventKind::CoverageEnded
        | EventKind::CheckpointCreated => RelevanceClass::Background,
    }
}

/// Whether a change is a service or container state change (§18.4 rank three).
fn is_state_change(event: &TemporalEvent) -> bool {
    let managed = event
        .subject
        .as_ref()
        .and_then(subject_type)
        .is_some_and(is_managed_type);
    managed
        && event
            .changed_fields
            .iter()
            .any(|change| STATE_FIELDS.contains(&&*change.field))
}

/// Whether a type is one whose lifecycle a service manager or a runtime owns.
fn is_managed_type(object_type: SpatialType) -> bool {
    matches!(
        object_type,
        SpatialType::Service
            | SpatialType::Job
            | SpatialType::Workload
            | SpatialType::Container
            | SpatialType::Cgroup
    )
}

/// The canonical type of a resolved subject.
fn subject_type(subject: &SpatialRef) -> Option<SpatialType> {
    match subject {
        SpatialRef::Resolved { object_type, .. } => Some(*object_type),
        SpatialRef::Unresolved { .. } => None,
    }
}

/// How close `event` sits to the place `horizon` describes (§11.3).
fn relation_of(event: &TemporalEvent, horizon: &Horizon) -> PlaceRelation {
    if !horizon.scope.contains(&event.scope) {
        return PlaceRelation::Elsewhere;
    }
    let touches = |wanted: &SpatialId| {
        event
            .subject
            .iter()
            .chain(event.related.iter())
            .filter_map(SpatialRef::spatial_id)
            .any(|id| id == wanted)
    };
    if horizon.place.as_ref().is_some_and(&touches) || horizon.selected.iter().any(&touches) {
        return PlaceRelation::Place;
    }
    if horizon.neighbours.iter().any(&touches) {
        return PlaceRelation::Neighbour;
    }
    PlaceRelation::Scope
}
