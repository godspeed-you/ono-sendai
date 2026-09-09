//! Timeline planning (spec v0.5 §11): the chronological projection of temporal events for a
//! place and a window, and the relevance the default scope is built from.
//!
//! The planner is the part that makes `timeline` usable and the part that makes it fast. §32.3
//! budgets 100 ms p95 for a fifteen-minute timeline, which is reachable only by pushing the
//! window, the scope, the subjects and the kinds into the [`EventQuery`] and letting the ledger
//! answer a bounded question. [`plan`] builds that query; [`timeline`] runs it and frames the
//! answer.
//!
//! Three rules of §11 shape the result:
//!
//! - **§11.3, the default is a place rather than a firehose.** Without a selector the answer is
//!   the current place and its directly relevant events; at the root it is high-significance
//!   events and the operator's own actions. [`crate::relevance`] holds that judgement.
//! - **§11.7, a gap is shown even when events surround it.** The gaps come from composing the
//!   coverage over the window, never from looking at where the events are, so "events exist on
//!   both sides" cannot suppress one.
//! - **§11.8, historical context moves the window.** The default centres on the active
//!   coordinate, ±15 minutes, rather than ending at now.
//!
//! §19.4's density grouping lives here too, as [`group`]: it is a reading of the same event
//! stream rather than a second data model, and the renderer applies it to what the planner
//! already returned.

use std::collections::BTreeMap;
use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::SpatialScope;
use ono_temporal_core::{
    CoverageQuery, CoverageSummary, EventId, EventKind, EventQuery, LedgerRead, QueryOrder,
    SpatialRef, TemporalContext, TemporalEvent, TemporalGap, TimeRange, presentation_order, value,
};
use ono_value::{
    Duration, ErrorValue, RecordBuilder, RecordValue, SchemaId, Value, builtin_schemas,
};

use crate::relevance::{Horizon, is_default_scope};
use crate::search::EventReferences;

/// The default window a `timeline` with no bounds covers — `temporal.timeline.default_window`.
pub const DEFAULT_WINDOW: Duration = Duration::from_nanoseconds(1_800_000_000_000);

/// Half the window §11.8 centres on an active historical coordinate.
pub const HISTORICAL_HALF_WINDOW: Duration = Duration::from_nanoseconds(900_000_000_000);

/// How many events a timeline returns before it says it was cut.
///
/// §11.4 makes the answer a stream a pipeline filters, so the ceiling is about boundedness
/// rather than about what a screen holds: an unbounded answer would break §32.3's budget and
/// §43.1's prohibition on unbounded queues at the same time.
pub const DEFAULT_LIMIT: usize = 500;

/// The default aggregation window §19.4 groups inside.
pub const DEFAULT_AGGREGATION_WINDOW: Duration = Duration::from_nanoseconds(5_000_000_000);

/// What a caller asks `timeline` for (§11.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineRequest {
    /// What the timeline is scoped to — the place, a selector, the root or `--all` (§11.3).
    pub horizon: Horizon,
    /// What a person calls the place, so a rendered header needs no second lookup.
    pub place_label: Option<Arc<str>>,
    /// `--since`. `None` takes the default window (§11.8).
    pub since: Option<Timestamp>,
    /// `--until`. `None` ends the window at the active coordinate.
    pub until: Option<Timestamp>,
    /// `--kind`. Empty for every kind.
    pub kinds: Vec<EventKind>,
    /// How many events to return before reporting truncation.
    pub limit: usize,
}

impl TimelineRequest {
    /// A request for `horizon` over the default window with the default ceiling.
    #[must_use]
    pub fn new(horizon: Horizon) -> Self {
        Self {
            horizon,
            place_label: None,
            since: None,
            until: None,
            kinds: Vec::new(),
            limit: DEFAULT_LIMIT,
        }
    }

    /// The same request with the label a renderer draws for the place.
    #[must_use]
    pub fn labelled(mut self, label: &str) -> Self {
        self.place_label = Some(Arc::from(label));
        self
    }
}

/// The window a timeline covers, and the instant it is centred on (§11.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineWindow {
    /// The start of the window.
    pub from: Timestamp,
    /// The end of the window.
    pub until: Timestamp,
    /// The active historical coordinate, where there is one (§11.8).
    pub centre: Option<Timestamp>,
}

impl TimelineWindow {
    /// The half-open range a ledger query uses.
    #[must_use]
    pub const fn range(&self) -> TimeRange {
        TimeRange {
            from: Some(self.from),
            until: Some(self.until),
        }
    }
}

/// The window `request` asks for under `context`, with `now` supplied by the caller (§39.2).
///
/// Explicit bounds win. Otherwise historical context centres the window on its coordinate
/// (§11.8), and the present ends it at `now`.
#[must_use]
pub fn window_of(
    request: &TimelineRequest,
    context: &TemporalContext,
    now: Timestamp,
) -> TimelineWindow {
    let coordinate = context.instant().unwrap_or(now);
    let centre = context.instant();
    match (request.since, request.until) {
        (Some(from), Some(until)) => TimelineWindow {
            from,
            until,
            centre,
        },
        (Some(from), None) => TimelineWindow {
            from,
            until: coordinate.max(from),
            centre,
        },
        (None, Some(until)) => TimelineWindow {
            from: shift(until, -DEFAULT_WINDOW.nanoseconds()),
            until,
            centre,
        },
        (None, None) => match centre {
            Some(at) => TimelineWindow {
                from: shift(at, -HISTORICAL_HALF_WINDOW.nanoseconds()),
                until: shift(at, HISTORICAL_HALF_WINDOW.nanoseconds()),
                centre,
            },
            None => TimelineWindow {
                from: shift(now, -DEFAULT_WINDOW.nanoseconds()),
                until: now,
                centre: None,
            },
        },
    }
}

/// The bounded ledger query `request` becomes (§32.3).
///
/// Everything the horizon knows is pushed down: the scope, the subjects, the kinds and the
/// window. The limit is one over what the caller asked for, so [`timeline`] can tell a window
/// that ended from an answer that was cut without a second query.
#[must_use]
pub fn plan(request: &TimelineRequest, context: &TemporalContext, now: Timestamp) -> EventQuery {
    let window = window_of(request, context, now);
    EventQuery {
        scope: Some(request.horizon.scope().clone()),
        subjects: request.horizon.subjects(),
        kinds: request.kinds.clone(),
        range: window.range(),
        limit: Some(request.limit.saturating_add(1)),
        order: QueryOrder::Ascending,
    }
}

/// A window of events for a place, with the coverage that backs it (`ono.temporal-timeline/1`).
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    /// The boundary the timeline covers.
    pub scope: SpatialScope,
    /// The place it is scoped to, where it is scoped to one (§11.3).
    pub place: Option<Arc<str>>,
    /// What a person calls that place.
    pub place_label: Option<Arc<str>>,
    /// The start of the window (§11.8).
    pub from: Timestamp,
    /// The end of the window.
    pub until: Timestamp,
    /// The instant the window is centred on, where historical context is active (§11.8).
    pub centre: Option<Timestamp>,
    /// The events, in presentation order (§26.3).
    pub events: Vec<TemporalEvent>,
    /// The coverage gaps inside the window (§11.7).
    pub gaps: Vec<TemporalGap>,
    /// The composed coverage over the window (§8.5).
    pub coverage: CoverageSummary,
    /// Whether the limit cut the answer rather than the window.
    pub truncated: bool,
    /// §19.4's density rows over [`Timeline::events`], formed by [`group`].
    ///
    /// Grouping is a judgement about events rather than a way of drawing them, so it is made
    /// once, here, and a renderer draws these rows. Every row enumerates its members, so §19.5's
    /// expansion reaches the retained individuals with no second query.
    pub groups: Vec<EventGroup>,
    /// The short reference a session minted for each event (§11.6).
    ///
    /// Empty for a timeline produced outside a session, which is why the record's `reference`
    /// field is nullable: a reference is one session's word for an event.
    pub references: BTreeMap<EventId, Arc<str>>,
}

impl Timeline {
    /// Mints a session reference for every event in the window (§11.6, ADR-0660).
    ///
    /// The reference a row prints and the reference `at event`, `inspect event` and `why event`
    /// accept are then the same string, because both come out of `references`. Minting is in
    /// presentation order, so the shortest references go to the events read first.
    #[must_use]
    pub fn with_references(mut self, references: &mut EventReferences) -> Self {
        self.references = self
            .events
            .iter()
            .map(|event| {
                let minted = references.reference(event);
                (event.event_id.clone(), Arc::from(minted.as_str()))
            })
            .collect();
        self
    }

    /// The `ono.temporal-timeline/1` record a pipeline consumes (§28.1).
    ///
    /// # Errors
    ///
    /// Returns `ono.provider_schema_violation` where the contract is not in this build, which
    /// the `ono-value` contract test and `cargo xtask spec-check` both prevent from shipping.
    pub fn to_record(&self) -> Result<RecordValue, ErrorValue> {
        let schema_id = SchemaId::new("ono.temporal-timeline", 1);
        let schema = builtin_schemas().get(&schema_id).ok_or_else(|| {
            ErrorValue::new(
                ono_core::ErrorCode::ProviderSchemaViolation,
                "the `ono.temporal-timeline/1` contract is not in this build".to_owned(),
            )
        })?;
        let provenance = ono_value::Provenance::local("ono.temporal", schema_id);
        let mut events = Vec::with_capacity(self.events.len());
        for event in &self.events {
            let reference = self.references.get(&event.event_id).map(Arc::as_ref);
            events.push(Value::Record(Arc::new(value::event_record_with_reference(
                event, reference,
            )?)));
        }
        let mut gaps = Vec::with_capacity(self.gaps.len());
        for gap in &self.gaps {
            gaps.push(Value::Record(Arc::new(value::gap_record(gap)?)));
        }
        let builder = RecordValue::builder(schema, provenance.clone());
        let builder = put(builder, "scope", Value::string(&self.scope.to_string()));
        let builder = put(
            builder,
            "place",
            self.place.as_deref().map_or(Value::Null, Value::string),
        );
        let builder = put(
            builder,
            "place_label",
            self.place_label
                .as_deref()
                .map_or(Value::Null, Value::string),
        );
        let builder = put(builder, "from", Value::Timestamp(self.from));
        let builder = put(builder, "until", Value::Timestamp(self.until));
        let builder = put(
            builder,
            "centre",
            self.centre.map_or(Value::Null, Value::Timestamp),
        );
        let builder = put(builder, "events", Value::list(events));
        let builder = put(
            builder,
            "groups",
            Value::list(self.groups.iter().map(group_value).collect::<Vec<_>>()),
        );
        let builder = put(builder, "gaps", Value::list(gaps));
        let builder = put(
            builder,
            "coverage",
            value::coverage_summary(&self.coverage)?,
        );
        let builder = put(builder, "truncated", Value::Bool(self.truncated));
        let builder = put(builder, "provenance", provenance_value(&provenance));
        Ok(builder.build())
    }
}

/// The timeline `request` asks `ledger` for, evaluated at `now` (§11).
///
/// # Errors
///
/// Returns whatever §34 refusal the ledger raises — `temporal.store_unavailable`,
/// `temporal.store_corrupt` or `temporal.permission_denied`.
pub fn timeline(
    ledger: &dyn LedgerRead,
    request: &TimelineRequest,
    context: &TemporalContext,
    now: Timestamp,
) -> Result<Timeline, ErrorValue> {
    let window = window_of(request, context, now);
    let query = plan(request, context, now);
    let found = ledger.events(&query)?;

    let mut events: Vec<TemporalEvent> = found
        .into_iter()
        .filter(|event| is_default_scope(event, &request.horizon))
        .collect();
    presentation_order(&mut events);
    let truncated = events.len() > request.limit;
    events.truncate(request.limit);

    let intervals = ledger.coverage(&CoverageQuery {
        scope: Some(request.horizon.scope().clone()),
        capabilities: Vec::new(),
        range: window.range(),
    })?;
    let coverage = CoverageSummary::compose(&intervals, window.range());

    let groups = group(&events, DEFAULT_AGGREGATION_WINDOW);

    Ok(Timeline {
        scope: request.horizon.scope().clone(),
        place: request.horizon.place().map(|id| Arc::from(id.as_str())),
        place_label: request.place_label.clone(),
        from: window.from,
        until: window.until,
        centre: window.centre,
        events,
        gaps: coverage.gaps().to_vec(),
        coverage,
        truncated,
        groups,
        references: BTreeMap::new(),
    })
}

/// One §19.4 row as the sub-record `ono.temporal-timeline/1` carries.
///
/// The hidden count and the span are the two things §19.4 requires grouping to preserve, and
/// `members` is what §19.5's expansion reads, so all three are stated rather than recomputable.
fn group_value(group: &EventGroup) -> Value {
    let mut map = ono_value::MapValue::new();
    map.insert(
        "event_id".into(),
        Value::string(group.representative.event_id.as_str()),
    );
    map.insert(
        "members".into(),
        Value::list(
            group
                .members
                .iter()
                .map(|id| Value::string(id.as_str()))
                .collect::<Vec<_>>(),
        ),
    );
    map.insert(
        "hidden".into(),
        Value::Int(i128::try_from(group.hidden).unwrap_or(i128::MAX)),
    );
    map.insert("from".into(), Value::Timestamp(group.from));
    map.insert("until".into(), Value::Timestamp(group.until));
    map.insert(
        "reason".into(),
        group
            .reason
            .map_or(Value::Null, |reason| Value::string(reason.as_str())),
    );
    Value::Map(Arc::new(map))
}

/// Why several events were drawn as one (§19.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupReason {
    /// The same object's same field moved repeatedly inside the window.
    SameField,
    /// A connection churned inside the aggregation window.
    ConnectionChurn,
    /// A source repeated a sample that changed no canonical state.
    UnchangedSample,
    /// The provider had already aggregated a cluster of events into one.
    ProviderCluster,
}

impl GroupReason {
    /// Every reason, in the order §19.4 lists its allowed grouping dimensions.
    pub const ALL: &'static [GroupReason] = &[
        GroupReason::SameField,
        GroupReason::ConnectionChurn,
        GroupReason::UnchangedSample,
        GroupReason::ProviderCluster,
    ];

    /// The name a renderer spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            GroupReason::SameField => "same_field",
            GroupReason::ConnectionChurn => "connection_churn",
            GroupReason::UnchangedSample => "unchanged_sample",
            GroupReason::ProviderCluster => "provider_cluster",
        }
    }

    /// The reason with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|reason| reason.as_str() == name)
    }
}

impl std::fmt::Display for GroupReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One row of a dense timeline: an event, or several that stand for one another (§19.4).
#[derive(Debug, Clone, PartialEq)]
pub struct EventGroup {
    /// The event a renderer draws — the first of the run.
    pub representative: TemporalEvent,
    /// Every member, so §19.5's expansion reaches the retained individuals.
    pub members: Vec<EventId>,
    /// How many members are not the representative (§19.4: grouping preserves hidden counts).
    pub hidden: usize,
    /// The start of the span the group covers (§19.4: grouping preserves time spans).
    pub from: Timestamp,
    /// The end of that span.
    pub until: Timestamp,
    /// Why they were grouped. `None` for an event standing alone.
    pub reason: Option<GroupReason>,
}

/// Groups `events` for a dense window, inside an aggregation window of `aggregation` (§19.4).
///
/// A lifecycle change is never folded away: §19.4 allows grouping "repeated connection churn",
/// and every other object appearance, disappearance, relation addition and relation removal
/// stands alone, because a reader who cannot see one of those is reading a different history.
/// Every group enumerates its members, so §19.5's expansion needs no second query.
#[must_use]
pub fn group(events: &[TemporalEvent], aggregation: Duration) -> Vec<EventGroup> {
    let mut ordered = events.to_vec();
    presentation_order(&mut ordered);

    let mut groups: Vec<EventGroup> = Vec::new();
    for event in ordered {
        let reason = groupable(&event);
        let at = event.times.presentation_instant();
        let joined = groups.iter_mut().rev().find(|group| {
            group.reason == reason
                && reason.is_some()
                && same_run(&group.representative, &event)
                && at.as_nanosecond() - group.from.as_nanosecond() <= aggregation.nanoseconds()
        });
        match joined {
            Some(group) => {
                group.members.push(event.event_id.clone());
                group.hidden += 1;
                group.until = group.until.max(at);
            }
            None => groups.push(EventGroup {
                members: vec![event.event_id.clone()],
                // A provider that already folded a cluster reports how many it folded; every
                // other group starts with only its representative and counts up.
                hidden: aggregated_count(&event),
                from: at,
                until: at,
                reason,
                representative: event,
            }),
        }
    }
    for group in &mut groups {
        if group.members.len() == 1 && group.reason != Some(GroupReason::ProviderCluster) {
            group.reason = None;
        }
    }
    groups
}

/// Which of §19.4's dimensions `event` may be grouped under, or `None` where it may not be.
fn groupable(event: &TemporalEvent) -> Option<GroupReason> {
    if aggregated_count(event) > 0 {
        return Some(GroupReason::ProviderCluster);
    }
    match event.kind {
        EventKind::ObjectChanged if event.changed_fields.len() == 1 => Some(GroupReason::SameField),
        EventKind::ObjectObserved => Some(GroupReason::UnchangedSample),
        EventKind::ObjectAppeared | EventKind::ObjectDisappeared if is_connection(event) => {
            Some(GroupReason::ConnectionChurn)
        }
        _ => None,
    }
}

/// Whether two events are the same run: one subject, one kind and one field.
fn same_run(a: &TemporalEvent, b: &TemporalEvent) -> bool {
    a.kind == b.kind
        && a.subject.as_ref().and_then(SpatialRef::spatial_id)
            == b.subject.as_ref().and_then(SpatialRef::spatial_id)
        && a.changed_fields
            .iter()
            .map(|change| &change.field)
            .eq(b.changed_fields.iter().map(|change| &change.field))
}

/// Whether the subject is one of the connection-shaped objects §19.4 names.
fn is_connection(event: &TemporalEvent) -> bool {
    matches!(
        event.subject.as_ref(),
        Some(SpatialRef::Resolved {
            object_type: ono_spatial_core::SpatialType::Connection
                | ono_spatial_core::SpatialType::Socket,
            ..
        })
    )
}

/// How many events a provider had already folded into this one (§19.4's cluster events).
fn aggregated_count(event: &TemporalEvent) -> usize {
    event
        .payload
        .as_ref()
        .and_then(|payload| payload.as_map().ok())
        .and_then(|map| map.get("aggregated_count"))
        .and_then(|value| value.as_int().ok())
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or(0)
}

/// `instant` moved by `nanos`, saturating at the representable range rather than wrapping.
fn shift(instant: Timestamp, nanos: i128) -> Timestamp {
    instant
        .as_nanosecond()
        .checked_add(nanos)
        .and_then(|moved| Timestamp::from_nanosecond(moved).ok())
        .unwrap_or(instant)
}

/// Sets a field the schema declares; an undeclared name is a bug here rather than a caller's.
fn put(builder: RecordBuilder, name: &str, value: Value) -> RecordBuilder {
    let fallback = builder.clone();
    builder.set(name, value).unwrap_or(fallback)
}

/// Provenance as the nested record every schema of §35 declares (v0.2 §25.2).
fn provenance_value(provenance: &ono_value::Provenance) -> Value {
    let mut map = ono_value::MapValue::new();
    map.insert("provider".into(), Value::string(provenance.provider()));
    map.insert(
        "observed".into(),
        provenance.observed().map_or(Value::Null, Value::Timestamp),
    );
    map.insert(
        "source".into(),
        provenance.source().map_or(Value::Null, Value::string),
    );
    map.insert("link".into(), Value::string(&provenance.link().to_string()));
    map.insert(
        "schema".into(),
        Value::string(&provenance.schema().to_string()),
    );
    Value::Map(Arc::new(map))
}
