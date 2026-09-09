//! Temporal landmarks (spec v0.5 §27): the significant transitions that orient a timeline. A
//! landmark is a navigation anchor and never a claim of operational severity.
//!
//! §27.1 lists the built-in candidates. Each one is a deterministic rule over events here: the
//! same events produce the same anchors in the same order, on any machine, with no provider
//! consulted and no clock read. That is what lets `[` and `]` land twice on the same instant.
//!
//! §27.2 is a prohibition the type enforces: "a temporal landmark is a navigation anchor, not an
//! incident alert" and "the shell MUST NOT claim operational severity beyond the underlying
//! rule". [`TemporalLandmark`] therefore carries no severity, no priority and no ranking. What it
//! carries is the rule that fired and the event it fired on, and a reader who wants to know how
//! bad it is goes and looks.
//!
//! §27.3 is what makes an anchor usable: every landmark keeps its [`ono_temporal_core::EventId`],
//! so `why event @e42`, `at event @e42` and `map --at event @e42` all reach the same event.

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{SpatialScope, SpatialType};
use ono_value::{Duration, ErrorValue, Provenance, RecordValue, SchemaId, Value};

use ono_temporal_core::{
    EventId, EventKind, EvidenceSource, SpatialRef, TemporalEvent, presentation_order, value,
};

/// What a landmark record says produced it (§25.2's provenance).
const PROVIDER: &str = "ono.temporal";

/// The state a service manager reports for a unit that gave up.
///
/// Public because the causal layer needs the same vocabulary: §15.5's and §16.6's worked example
/// is a *service* failing, which reaches the ledger as an `object.changed` moving to one of these
/// words rather than as an `action.failed`. Two copies of this list would let a correlation rule
/// and a landmark rule disagree about what a failure is.
pub const FAILED_STATES: &[&str] = &["failed", "error", "dead-failed"];

/// The state a service manager reports for a unit that gave up.
const FAILED: &[&str] = FAILED_STATES;

/// The states a recovered unit reports.
const HEALTHY: &[&str] = &["active", "running", "listening", "healthy"];

/// The states a unit passes through on its way back up, which is what a restart loop repeats.
const RESTARTING: &[&str] = &["activating", "restarting", "auto-restart", "starting"];

/// The states an interface reports.
const LINK_STATES: &[&str] = &["up", "down", "lowerlayerdown", "no-carrier"];

/// The fields carrying a managed object's state.
const STATE_FIELDS: &[&str] = &["active_state", "sub_state", "state", "status", "health"];

/// The fields carrying a filesystem's read-only flag.
const READ_ONLY_FIELDS: &[&str] = &["read_only", "ro", "readonly"];

/// The candidate list of §27.1, as the rules that emit each anchor.
///
/// §27.1 spells "service failure/recovery" as one line and "container start/stop/failure" as
/// another. A failure and a recovery are different anchors to navigate to, so they are two kinds
/// here and one line there; the container line stays one kind because start, stop and failure are
/// the same lifecycle transition seen from three sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TemporalLandmarkKind {
    /// A managed unit reported failure.
    ServiceFailure,
    /// A managed unit came back from failure.
    ServiceRecovery,
    /// A unit re-entered its start-up state often enough to be a loop (§43.4).
    RestartLoop,
    /// A mount appeared or went away.
    MountChange,
    /// A filesystem became read-only.
    FilesystemReadOnly,
    /// An interface went up or down.
    InterfaceState,
    /// A route appeared or went away.
    RouteChange,
    /// A container started, stopped or failed.
    ContainerLifecycle,
    /// The operator asked the shell for a mutation (§17.2).
    OperatorAction,
    /// A source started or stopped covering a capability, or a permission changed (§8.1).
    CoverageBoundary,
    /// The recorder stopped covering, which is where a history gap begins (§7.5, §43.4).
    RecorderGap,
    /// A link to another host was lost or came back (§24.5).
    RemoteLink,
}

impl TemporalLandmarkKind {
    /// Every kind, in the order §27.1 lists its candidates.
    pub const ALL: &'static [TemporalLandmarkKind] = &[
        TemporalLandmarkKind::ServiceFailure,
        TemporalLandmarkKind::ServiceRecovery,
        TemporalLandmarkKind::RestartLoop,
        TemporalLandmarkKind::MountChange,
        TemporalLandmarkKind::FilesystemReadOnly,
        TemporalLandmarkKind::InterfaceState,
        TemporalLandmarkKind::RouteChange,
        TemporalLandmarkKind::ContainerLifecycle,
        TemporalLandmarkKind::OperatorAction,
        TemporalLandmarkKind::CoverageBoundary,
        TemporalLandmarkKind::RecorderGap,
        TemporalLandmarkKind::RemoteLink,
    ];

    /// The name a renderer and a filter spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            TemporalLandmarkKind::ServiceFailure => "service_failure",
            TemporalLandmarkKind::ServiceRecovery => "service_recovery",
            TemporalLandmarkKind::RestartLoop => "restart_loop",
            TemporalLandmarkKind::MountChange => "mount_change",
            TemporalLandmarkKind::FilesystemReadOnly => "filesystem_read_only",
            TemporalLandmarkKind::InterfaceState => "interface_state",
            TemporalLandmarkKind::RouteChange => "route_change",
            TemporalLandmarkKind::ContainerLifecycle => "container_lifecycle",
            TemporalLandmarkKind::OperatorAction => "operator_action",
            TemporalLandmarkKind::CoverageBoundary => "coverage_boundary",
            TemporalLandmarkKind::RecorderGap => "recorder_gap",
            TemporalLandmarkKind::RemoteLink => "remote_link",
        }
    }

    /// The kind with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_str() == name)
    }
}

impl std::fmt::Display for TemporalLandmarkKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The thresholds the rules of §27.1 need, so a rule states its own numbers (§33).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandmarkRules {
    /// How many entries into a start-up state make a restart loop rather than a restart.
    pub restart_loop_threshold: usize,
    /// The span the entries must fall inside.
    pub restart_loop_window: Duration,
}

impl Default for LandmarkRules {
    /// Three start-ups within five minutes, matching the checkpoint cadence of §33.
    fn default() -> Self {
        Self {
            restart_loop_threshold: 3,
            restart_loop_window: Duration::from_nanoseconds(300_000_000_000),
        }
    }
}

/// A navigation anchor on the timeline (§27).
///
/// It carries the rule that fired and the event it fired on, and nothing that ranks it: §27.2
/// forbids claiming operational severity beyond the rule, so there is no field to claim it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalLandmark {
    kind: TemporalLandmarkKind,
    event: EventId,
    at: Timestamp,
    scope: SpatialScope,
    subject: Option<SpatialRef>,
    detail: Arc<str>,
}

impl TemporalLandmark {
    /// The rule that fired.
    #[must_use]
    pub const fn kind(&self) -> TemporalLandmarkKind {
        self.kind
    }

    /// The event the anchor points at (§27.3).
    ///
    /// `why event`, `at event` and `map --at event` all take this reference, which is why a
    /// landmark is a way of navigating rather than a notification.
    #[must_use]
    pub const fn event(&self) -> &EventId {
        &self.event
    }

    /// The instant the anchor sits at.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }

    /// The boundary the anchor belongs to.
    #[must_use]
    pub const fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// What the transition was about, where a single object owns it.
    #[must_use]
    pub const fn subject(&self) -> Option<&SpatialRef> {
        self.subject.as_ref()
    }

    /// The rule's own words about what it saw — never a severity (§27.2).
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// The landmark as an Ono value a pipeline consumes (§28.1).
    ///
    /// It is a map rather than a record of its own: §35 registers no landmark schema, and
    /// inventing one here would put a contract in a crate rather than in `docs/contracts`.
    ///
    /// # Errors
    ///
    /// Returns `ono.provider_schema_violation` where the event contract is not in this build.
    pub fn to_record(&self) -> Result<Value, ErrorValue> {
        let schema = ono_value::builtin_schemas()
            .get(&SchemaId::new("ono.temporal-landmark", 1))
            .ok_or_else(|| {
                ErrorValue::new(
                    ono_core::ErrorCode::ResolveTargetNotFound,
                    "the workspace carries no `ono.temporal-landmark/1` contract",
                )
            })?;
        let record = RecordValue::builder(
            schema,
            Provenance::local(PROVIDER, SchemaId::new("ono.temporal-landmark", 1)),
        )
        .set("kind", Value::string(self.kind.as_str()))?
        .set("event", Value::string(&self.event.to_string()))?
        .set("at", Value::Timestamp(self.at))?
        .set("scope", Value::string(&self.scope.to_string()))?
        .set(
            "subject",
            self.subject
                .as_ref()
                .map_or(Value::Null, value::spatial_ref),
        )?
        .set("detail", Value::string(&self.detail))?
        .build();
        Ok(Value::Record(Arc::new(record)))
    }
}

/// The anchors `events` produce under `rules` (§27.1).
///
/// The events are read in presentation order, so the answer does not depend on the order they
/// arrived in, and each event contributes at most one anchor: the first rule that matches wins,
/// in the order §27.1 lists them, so a failing container is a container lifecycle anchor rather
/// than a second service failure.
#[must_use]
pub fn landmarks(events: &[TemporalEvent], rules: &LandmarkRules) -> Vec<TemporalLandmark> {
    let mut ordered = events.to_vec();
    presentation_order(&mut ordered);

    let mut found = Vec::new();
    let mut restarts: Vec<(Option<Arc<str>>, Timestamp)> = Vec::new();
    let mut looping: Vec<Arc<str>> = Vec::new();

    for event in &ordered {
        if let Some(landmark) = anchor(event) {
            found.push(landmark);
        }
        if let Some(loop_anchor) = restart_loop(event, rules, &mut restarts, &mut looping) {
            found.push(loop_anchor);
        }
    }
    found
}

/// The first rule of §27.1 that `event` matches.
fn anchor(event: &TemporalEvent) -> Option<TemporalLandmark> {
    let object_type = event.subject.as_ref().and_then(|subject| match subject {
        SpatialRef::Resolved { object_type, .. } => Some(*object_type),
        SpatialRef::Unresolved { .. } => None,
    });

    if event.kind.is_action() {
        return Some(build(
            event,
            TemporalLandmarkKind::OperatorAction,
            "the operator requested a mutation through the shell",
        ));
    }
    if matches!(
        event.kind,
        EventKind::CoverageStarted | EventKind::CoverageEnded
    ) {
        let recorder = event.provenance.provider() == EvidenceSource::recorder().as_str();
        let kind = if recorder {
            TemporalLandmarkKind::RecorderGap
        } else if event.scope.is_remote() {
            TemporalLandmarkKind::RemoteLink
        } else {
            TemporalLandmarkKind::CoverageBoundary
        };
        return Some(build(event, kind, coverage_detail(event.kind)));
    }
    if event.scope.is_remote() && object_type == Some(SpatialType::Host) {
        return Some(build(
            event,
            TemporalLandmarkKind::RemoteLink,
            "a link to another host changed state",
        ));
    }

    let lifecycle = matches!(
        event.kind,
        EventKind::ObjectAppeared | EventKind::ObjectDisappeared
    );
    match object_type {
        Some(SpatialType::Container) => {
            if lifecycle || state_moved_to(event, FAILED) {
                return Some(build(
                    event,
                    TemporalLandmarkKind::ContainerLifecycle,
                    "a container started, stopped or failed",
                ));
            }
        }
        Some(SpatialType::Mount) if lifecycle => {
            return Some(build(
                event,
                TemporalLandmarkKind::MountChange,
                "a mount appeared or went away",
            ));
        }
        Some(SpatialType::Route) if lifecycle => {
            return Some(build(
                event,
                TemporalLandmarkKind::RouteChange,
                "a route appeared or went away",
            ));
        }
        _ => {}
    }

    if matches!(
        object_type,
        Some(SpatialType::Filesystem | SpatialType::Mount)
    ) && became_read_only(event)
    {
        return Some(build(
            event,
            TemporalLandmarkKind::FilesystemReadOnly,
            "the filesystem became read-only",
        ));
    }
    if object_type == Some(SpatialType::Interface) && state_moved_to(event, LINK_STATES) {
        return Some(build(
            event,
            TemporalLandmarkKind::InterfaceState,
            "the interface changed link state",
        ));
    }
    if is_managed(object_type) {
        if state_moved_to(event, FAILED) {
            return Some(build(
                event,
                TemporalLandmarkKind::ServiceFailure,
                "the unit reported failure",
            ));
        }
        if state_moved_from(event, FAILED) && state_moved_to(event, HEALTHY) {
            return Some(build(
                event,
                TemporalLandmarkKind::ServiceRecovery,
                "the unit came back from failure",
            ));
        }
    }
    None
}

/// Whether `event` crosses the restart-loop threshold, and the anchor if it does (§27.1, §43.4).
///
/// The loop is one anchor at the instant the threshold is crossed: a unit that keeps restarting
/// is one thing to navigate to, and emitting an anchor per restart would turn an orientation aid
/// into the firehose §11.3 refuses.
fn restart_loop(
    event: &TemporalEvent,
    rules: &LandmarkRules,
    restarts: &mut Vec<(Option<Arc<str>>, Timestamp)>,
    looping: &mut Vec<Arc<str>>,
) -> Option<TemporalLandmark> {
    let object_type = event.subject.as_ref().and_then(|subject| match subject {
        SpatialRef::Resolved { object_type, .. } => Some(*object_type),
        SpatialRef::Unresolved { .. } => None,
    });
    if !is_managed(object_type) || !state_moved_to(event, RESTARTING) {
        return None;
    }
    let key: Option<Arc<str>> = event
        .subject
        .as_ref()
        .and_then(SpatialRef::spatial_id)
        .map(|id| Arc::from(id.as_str()));
    let at = event.times.presentation_instant();
    restarts.push((key.clone(), at));

    let earliest = at.as_nanosecond() - rules.restart_loop_window.nanoseconds();
    let count = restarts
        .iter()
        .filter(|(seen, when)| *seen == key && when.as_nanosecond() >= earliest)
        .count();
    let identity: Arc<str> = key.clone().unwrap_or_else(|| Arc::from(""));
    if count < rules.restart_loop_threshold || looping.contains(&identity) {
        return None;
    }
    looping.push(identity);
    Some(build(
        event,
        TemporalLandmarkKind::RestartLoop,
        "the unit re-entered start-up often enough to be a loop",
    ))
}

/// A landmark on `event`.
fn build(event: &TemporalEvent, kind: TemporalLandmarkKind, detail: &str) -> TemporalLandmark {
    TemporalLandmark {
        kind,
        event: event.event_id.clone(),
        at: event.times.presentation_instant(),
        scope: event.scope.clone(),
        subject: event.subject.clone(),
        detail: Arc::from(detail),
    }
}

/// What a coverage event changed.
fn coverage_detail(kind: EventKind) -> &'static str {
    match kind {
        EventKind::CoverageStarted => "a source started covering a capability",
        _ => "a source stopped covering a capability",
    }
}

/// Whether the object's lifecycle is a service manager's or a runtime's to own.
fn is_managed(object_type: Option<SpatialType>) -> bool {
    matches!(
        object_type,
        Some(SpatialType::Service | SpatialType::Job | SpatialType::Workload)
    )
}

/// Whether a state field of `event` moved *to* one of `states`.
pub fn state_moved_to(event: &TemporalEvent, states: &[&str]) -> bool {
    event.changed_fields.iter().any(|change| {
        STATE_FIELDS.contains(&&*change.field)
            && change
                .after
                .as_ref()
                .and_then(|value| value.as_str().ok())
                .is_some_and(|text| states.contains(&text))
    })
}

/// Whether a state field of `event` moved *from* one of `states`.
fn state_moved_from(event: &TemporalEvent, states: &[&str]) -> bool {
    event.changed_fields.iter().any(|change| {
        STATE_FIELDS.contains(&&*change.field)
            && change
                .before
                .as_ref()
                .and_then(|value| value.as_str().ok())
                .is_some_and(|text| states.contains(&text))
    })
}

/// Whether a read-only flag became true.
fn became_read_only(event: &TemporalEvent) -> bool {
    event.changed_fields.iter().any(|change| {
        READ_ONLY_FIELDS.contains(&&*change.field)
            && change.after.as_ref().is_some_and(|value| match value {
                Value::Bool(flag) => *flag,
                other => matches!(other.as_str(), Ok("true" | "ro" | "read-only")),
            })
    })
}
