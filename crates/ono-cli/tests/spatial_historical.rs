//! The historical spatial world (spec v0.5 §9.7, §14.1, §14.3, §14.4, §14.5, §55.2, §55.9).
//!
//! Every test here is about one sentence: the past is answered from evidence about the past, and
//! nothing the present knows may reach it. §55.2 names the failure — "rendering today's graph
//! with an old timestamp is prohibited" — and §14.3 names the shape it takes, a current-only exit
//! appearing in a historical neighbourhood. The world under test is built from a ledger and from
//! nothing else, so the tests below can hand it a present index full of things that never existed
//! at `T` and watch none of them arrive.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_cli::spatial::historical::HistoricalWorld;
use ono_spatial_core::{
    BootIdentity, Confidence, PermissionState, Projection, SpatialId, SpatialScope, SpatialType,
};
use ono_spatial_query::NeighborhoodRequest;
use ono_temporal_core::{
    EventKind, EventSeed, EventTimes, EvidenceSource, LedgerWrite, SessionLedger, SpatialRef,
    TemporalCompleteness, TemporalCoverage, TemporalEvent,
};
use ono_temporal_reconstruct::Presence;
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

fn boot() -> BootIdentity {
    BootIdentity::new("testbox", "4d0a1f2b-0000-4000-8000-000000000001")
}

fn scope() -> SpatialScope {
    SpatialScope::host("testbox", boot())
}

fn instant(text: &str) -> Timestamp {
    text.parse().expect("the fixture instant parses")
}

fn provenance() -> Provenance {
    Provenance::local("ono.recorder", SchemaId::new("ono.temporal-event", 1))
}

fn recorder() -> EvidenceSource {
    EvidenceSource::recorder()
}

fn record(schema_name: &str, fields: &[(&str, Value)]) -> RecordValue {
    let id = SchemaId::new(schema_name, 1);
    let schema = builtin_schemas()
        .get(&id)
        .expect("the contract is embedded");
    let mut builder = RecordValue::builder(schema, Provenance::local("fixture", id));
    for (name, value) in fields {
        builder = builder.set(name, value.clone()).expect("a declared field");
    }
    builder.build()
}

fn process_record(pid: i64, name: &str, started: &str, state: &str) -> RecordValue {
    record(
        "ono.process",
        &[
            ("pid", Value::Int(i128::from(pid))),
            ("name", Value::string(name)),
            ("started", Value::Timestamp(instant(started))),
            ("state", Value::string(state)),
        ],
    )
}

fn service_record(name: &str, state: &str) -> RecordValue {
    record(
        "ono.service",
        &[
            ("name", Value::string(name)),
            ("state", Value::string(state)),
            ("provider", Value::string("systemd")),
        ],
    )
}

fn identity_of(record: &RecordValue, object_type: SpatialType, at: &str) -> SpatialId {
    Projection::new(scope(), instant(at))
        .project_as(record, object_type)
        .expect("the fixture record carries an identity")
        .spatial_id()
        .clone()
}

fn times(observed: &str) -> EventTimes {
    EventTimes {
        source_time: Some(instant(observed)),
        observed_at: instant(observed),
        ingested_at: instant(observed),
        source_sequence: None,
        monotonic_nanos: None,
        clock_uncertainty: None,
        domain: ono_temporal_core::ClockDomain::new(
            "testbox",
            Some("4d0a1f2b-0000-4000-8000-000000000001"),
        ),
    }
}

fn resolved(id: &SpatialId, object_type: SpatialType, label: &str) -> SpatialRef {
    SpatialRef::Resolved {
        id: id.clone(),
        object_type,
        label: Arc::from(label),
    }
}

/// An `object.observed` carrying the archived record, which is what gives a reconstructed object
/// a row rather than only a name.
fn observed(at: &str, subject: SpatialRef, archived: &RecordValue) -> TemporalEvent {
    EventSeed {
        kind: EventKind::ObjectObserved,
        subtype: None,
        scope: scope(),
        subject: Some(subject),
        related: Vec::new(),
        times: times(at),
        before: None,
        after: Some(Value::Record(Arc::new(archived.clone()))),
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance(),
    }
    .seal()
}

fn relation_event(
    kind: EventKind,
    at: &str,
    from: SpatialRef,
    to: SpatialRef,
    relation: &str,
) -> TemporalEvent {
    EventSeed {
        kind,
        subtype: None,
        scope: scope(),
        subject: Some(from),
        related: vec![to],
        times: times(at),
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: Some(ono_temporal_core::relation_payload(
            relation,
            Confidence::Exact,
        )),
        provenance: provenance(),
    }
    .seal()
}

fn complete_coverage(capability: &str, from: &str, until: &str) -> TemporalCoverage {
    TemporalCoverage {
        scope: scope(),
        capability: Arc::from(capability),
        from: instant(from),
        until: instant(until),
        completeness: TemporalCompleteness::Complete,
        sampling_interval: None,
        source: recorder(),
        permission: PermissionState::Available,
    }
}

/// A ledger holding a service and one process, related at 12:00, plus a second process and a
/// second relation that only came into being at 12:30.
fn ledger_with_a_later_relation() -> (SessionLedger, SpatialId, SpatialId, SpatialId) {
    let ledger = SessionLedger::new();
    let service = service_record("nginx.service", "active");
    let early = process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running");
    let late = process_record(4711, "nginx-worker", "2026-08-31T12:29:00Z", "running");
    let service_id = identity_of(&service, SpatialType::Service, "2026-08-31T12:00:00Z");
    let early_id = identity_of(&early, SpatialType::Process, "2026-08-31T12:00:00Z");
    let late_id = identity_of(&late, SpatialType::Process, "2026-08-31T12:30:00Z");

    let events = vec![
        observed(
            "2026-08-31T12:00:00Z",
            resolved(&service_id, SpatialType::Service, "nginx.service"),
            &service,
        ),
        observed(
            "2026-08-31T12:00:00Z",
            resolved(&early_id, SpatialType::Process, "nginx"),
            &early,
        ),
        relation_event(
            EventKind::RelationAdded,
            "2026-08-31T12:00:00Z",
            resolved(&service_id, SpatialType::Service, "nginx.service"),
            resolved(&early_id, SpatialType::Process, "nginx"),
            "service.controls_process",
        ),
        observed(
            "2026-08-31T12:30:00Z",
            resolved(&late_id, SpatialType::Process, "nginx-worker"),
            &late,
        ),
        relation_event(
            EventKind::RelationAdded,
            "2026-08-31T12:30:00Z",
            resolved(&service_id, SpatialType::Service, "nginx.service"),
            resolved(&late_id, SpatialType::Process, "nginx-worker"),
            "service.controls_process",
        ),
    ];
    ledger.append(&events, &[]).expect("the ledger accepts");
    ledger
        .record_coverage(&[
            complete_coverage(
                "process.existence",
                "2026-08-31T11:55:00Z",
                "2026-08-31T13:00:00Z",
            ),
            complete_coverage(
                "service.existence",
                "2026-08-31T11:55:00Z",
                "2026-08-31T13:00:00Z",
            ),
            complete_coverage(
                "relation:service.controls_process",
                "2026-08-31T11:55:00Z",
                "2026-08-31T13:00:00Z",
            ),
        ])
        .expect("the ledger accepts coverage");
    (ledger, service_id, early_id, late_id)
}

#[test]
fn should_leave_out_a_relation_added_later_when_a_neighbourhood_is_built_at_an_earlier_instant() {
    // §14.3: "An exit shown by `look` or `near` at time `T` MUST correspond to a relation or
    // hierarchy supported at `T`. Current-only exits MUST not leak into a historical
    // neighborhood."
    let (ledger, service_id, early_id, late_id) = ledger_with_a_later_relation();
    let world = HistoricalWorld::reconstruct(&ledger, &scope(), instant("2026-08-31T12:15:00Z"))
        .expect("the reconstruction runs");

    let neighborhood = world.neighborhood(&service_id, &NeighborhoodRequest::new().all(true));
    let members: Vec<&SpatialId> = neighborhood
        .groups()
        .iter()
        .flat_map(|group| group.members().iter())
        .collect();

    assert!(
        members.contains(&&early_id),
        "the process that was related at 12:00 is missing from the 12:15 neighbourhood"
    );
    assert!(
        !members.contains(&&late_id),
        "a relation added at 12:30 leaked into the neighbourhood at 12:15"
    );
}

#[test]
fn should_hold_nothing_the_present_index_holds_when_a_historical_world_is_reconstructed() {
    // §55.2: "Rendering today's graph with an old timestamp is prohibited." The world is built
    // from the ledger and from nothing else, so an object that exists only today cannot be in it
    // however loudly the present says otherwise.
    let (ledger, _, _, late_id) = ledger_with_a_later_relation();
    let today = process_record(9999, "today-only", "2026-08-31T13:00:00Z", "running");
    let today_id = identity_of(&today, SpatialType::Process, "2026-08-31T13:00:00Z");

    let world = HistoricalWorld::reconstruct(&ledger, &scope(), instant("2026-08-31T12:15:00Z"))
        .expect("the reconstruction runs");

    assert!(
        !world.index().contains(&today_id),
        "an object nothing in the ledger ever observed reached the historical index"
    );
    assert!(
        !world.index().contains(&late_id),
        "an object first observed after the instant reached the historical index"
    );
    assert_eq!(
        world.at(),
        instant("2026-08-31T12:15:00Z"),
        "the world states the instant it was built for"
    );
}

#[test]
fn should_draw_no_edge_to_a_place_the_instant_has_no_evidence_for_when_a_map_is_projected() {
    // §14.3 again, through `map`: an edge whose far end is not a historical place would be an
    // exit leading nowhere the past knows about.
    let (ledger, service_id, early_id, late_id) = ledger_with_a_later_relation();
    let world = HistoricalWorld::reconstruct(&ledger, &scope(), instant("2026-08-31T12:15:00Z"))
        .expect("the reconstruction runs");
    let map = world.map(
        &service_id,
        &ono_spatial_query::MapRequest::new().all(true),
        100,
    );

    let drawn: Vec<String> = map.nodes.iter().map(|node| node.id.to_string()).collect();
    assert!(
        drawn.contains(&early_id.to_string()),
        "the historical map lost the process that was there, got {drawn:?}"
    );
    assert!(
        !drawn.contains(&late_id.to_string()),
        "the historical map drew a process that appeared later, got {drawn:?}"
    );
}

#[test]
fn should_tell_a_known_absence_from_an_unknown_one_when_a_place_is_not_in_the_reconstruction() {
    // §9.7: `look` reports `place not known at requested time` "with coverage explaining whether
    // this means known-absent or simply unknown".
    let (ledger, _, _, late_id) = ledger_with_a_later_relation();
    let world = HistoricalWorld::reconstruct(&ledger, &scope(), instant("2026-08-31T12:15:00Z"))
        .expect("the reconstruction runs");

    // Complete process coverage over the window can prove the later process was not there.
    assert_eq!(
        world.presence_of(&late_id, SpatialType::Process),
        Presence::Absent,
        "coverage capable of proving absence answered `unknown`"
    );

    let refusal = world
        .not_known_here(&late_id, SpatialType::Process)
        .expect("a place that was not there is refused");
    assert_eq!(refusal.code(), ono_core::ErrorCode::TemporalNotRecorded);
    assert!(
        refusal
            .message()
            .contains("place not known at requested time"),
        "§9.7's sentence is missing, got: {}",
        refusal.message()
    );
    assert_eq!(
        refusal
            .metadata()
            .get("presence")
            .and_then(|value| value.as_str().ok()),
        Some("absent"),
        "the refusal does not say which of the two answers it is"
    );
}

#[test]
fn should_answer_unknown_rather_than_absent_when_no_coverage_could_have_proven_it() {
    // §7.4: an absence is a claim, and a claim needs coverage that could have carried it. Without
    // any coverage the same missing place is `unknown`, and §9.7's refusal says so.
    let ledger = SessionLedger::new();
    let held = process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running");
    let held_id = identity_of(&held, SpatialType::Process, "2026-08-31T12:00:00Z");
    ledger
        .append(
            &[observed(
                "2026-08-31T12:00:00Z",
                resolved(&held_id, SpatialType::Process, "nginx"),
                &held,
            )],
            &[],
        )
        .expect("the ledger accepts");

    let missing = process_record(4711, "other", "2026-08-31T11:00:00Z", "running");
    let missing_id = identity_of(&missing, SpatialType::Process, "2026-08-31T12:00:00Z");

    let world = HistoricalWorld::reconstruct(&ledger, &scope(), instant("2026-08-31T12:15:00Z"))
        .expect("the reconstruction runs");
    assert_eq!(
        world.presence_of(&missing_id, SpatialType::Process),
        Presence::Unknown,
        "an absence was claimed without coverage that could prove it"
    );
    let refusal = world
        .not_known_here(&missing_id, SpatialType::Process)
        .expect("a place with no evidence is refused");
    assert_eq!(
        refusal
            .metadata()
            .get("presence")
            .and_then(|value| value.as_str().ok()),
        Some("unknown"),
    );
}

#[test]
fn should_let_a_place_that_was_there_stand_when_the_instant_supports_it() {
    // §9.7 refuses only where the place was not there. A place the reconstruction supports is
    // looked at, and the refusal is `None`.
    let (ledger, service_id, _, _) = ledger_with_a_later_relation();
    let world = HistoricalWorld::reconstruct(&ledger, &scope(), instant("2026-08-31T12:15:00Z"))
        .expect("the reconstruction runs");
    assert_eq!(
        world.presence_of(&service_id, SpatialType::Service),
        Presence::Present
    );
    assert!(
        world
            .not_known_here(&service_id, SpatialType::Service)
            .is_none(),
        "a place that was there was refused"
    );
}

#[test]
fn should_refuse_historical_directory_contents_when_no_source_carries_the_structure() {
    // §14.5: "No generic v0.5 implementation may pretend that current directory contents
    // represent the past." A filesystem place with nothing but ordinary observations behind it
    // is not reconstructed, and asking for it says so rather than showing today's tree.
    let ledger = SessionLedger::new();
    let directory = record(
        "ono.file",
        &[
            (
                "path",
                Value::Path(std::path::Path::new("/etc/nginx").into()),
            ),
            ("name", Value::string("nginx")),
            ("kind", Value::string("dir")),
        ],
    );
    let directory_id = Projection::new(scope(), instant("2026-08-31T12:00:00Z"))
        .derive(
            SpatialType::Directory,
            SchemaId::new("ono.file", 1),
            "path",
            "/etc/nginx",
            provenance(),
        )
        .spatial_id()
        .clone();
    ledger
        .append(
            &[observed(
                "2026-08-31T12:00:00Z",
                resolved(&directory_id, SpatialType::Directory, "/etc/nginx"),
                &directory,
            )],
            &[],
        )
        .expect("the ledger accepts");

    let world = HistoricalWorld::reconstruct(&ledger, &scope(), instant("2026-08-31T12:15:00Z"))
        .expect("the reconstruction runs");
    assert!(
        !world.index().contains(&directory_id),
        "a directory with no snapshot, checkpoint or audit evidence behind it was reconstructed"
    );
    let refusal = world
        .structure_refusal(SpatialType::Directory)
        .expect("§14.5 refuses a historical directory tree");
    assert_eq!(
        refusal.code(),
        ono_core::ErrorCode::TemporalUnsupportedSource
    );
    assert!(
        refusal
            .message()
            .contains("historical filesystem structure"),
        "got: {}",
        refusal.message()
    );
    assert!(
        world.structure_refusal(SpatialType::Process).is_none(),
        "the filesystem refusal spread to a kind of place it is not about"
    );
}

#[test]
fn should_name_a_present_day_match_a_resolution_aid_when_the_ledger_never_saw_it() {
    // §14.4: where a present-day alias helps resolve a historical object, the implementation
    // "MUST distinguish resolution aid from historical existence evidence".
    let (ledger, _, _, _) = ledger_with_a_later_relation();
    let world = HistoricalWorld::reconstruct(&ledger, &scope(), instant("2026-08-31T12:15:00Z"))
        .expect("the reconstruction runs");

    let mut present = ono_spatial_index::SpatialIndex::new(
        ono_spatial_index::FreshnessPolicy::uniform(jiff::Span::new().hours(1)),
    );
    let today = process_record(9999, "today-only", "2026-08-31T13:00:00Z", "running");
    let object = Projection::new(scope(), instant("2026-08-31T13:00:00Z"))
        .project_as(&today, SpatialType::Process)
        .expect("the record carries an identity");
    present
        .register(object, instant("2026-08-31T13:00:00Z"))
        .expect("the index accepts it");

    let found = world
        .find(&ledger, "today-only", &present)
        .expect("the search runs");
    assert_eq!(found.len(), 1, "the alias did not reach the candidate");
    assert!(
        !found[0].is_evidence_of_existence(),
        "a name found in today's index was reported as evidence the object existed then"
    );
    assert_eq!(found[0].existed_at, None);

    let evidenced = world
        .find(&ledger, "nginx", &present)
        .expect("the search runs");
    assert!(
        evidenced
            .iter()
            .any(|found| found.is_evidence_of_existence()),
        "an event that named the object at the active time is not evidence of existence"
    );
}

#[test]
fn should_enter_a_lifetime_that_has_since_ended_when_the_instant_is_inside_it() {
    // §5.4: "historical navigation MAY enter the object's known lifetime." A process that ended
    // before now is a place at an instant inside its lifetime, and `enter`'s resolution reaches
    // it there — the tombstone is a fact about the present, and the past is a different question.
    let ledger = SessionLedger::new();
    let gone = process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running");
    let gone_id = identity_of(&gone, SpatialType::Process, "2026-08-31T12:00:00Z");
    ledger
        .append(
            &[
                observed(
                    "2026-08-31T12:00:00Z",
                    resolved(&gone_id, SpatialType::Process, "nginx"),
                    &gone,
                ),
                {
                    let mut ended = observed(
                        "2026-08-31T12:40:00Z",
                        resolved(&gone_id, SpatialType::Process, "nginx"),
                        &gone,
                    );
                    ended.kind = EventKind::ObjectDisappeared;
                    EventSeed {
                        kind: EventKind::ObjectDisappeared,
                        subtype: None,
                        scope: scope(),
                        subject: ended.subject.clone(),
                        related: Vec::new(),
                        times: times("2026-08-31T12:40:00Z"),
                        before: None,
                        after: None,
                        changed_fields: Vec::new(),
                        evidence: Vec::new(),
                        causal_parents: Vec::new(),
                        payload: None,
                        provenance: provenance(),
                    }
                    .seal()
                },
            ],
            &[],
        )
        .expect("the ledger accepts");
    ledger
        .record_coverage(&[complete_coverage(
            "process.existence",
            "2026-08-31T11:55:00Z",
            "2026-08-31T13:00:00Z",
        )])
        .expect("the ledger accepts coverage");

    let inside = HistoricalWorld::reconstruct(&ledger, &scope(), instant("2026-08-31T12:20:00Z"))
        .expect("the reconstruction runs");
    assert_eq!(
        inside.presence_of(&gone_id, SpatialType::Process),
        Presence::Present,
        "an instant inside a known lifetime did not reach the object"
    );
    // The same context `enter` builds: standing at the root of this host, resolving on this
    // host (§27.1 step 4).
    let context =
        ono_spatial_query::SelectorContext::at(ono_spatial_core::space::root().spatial_id_in(None))
            .on_host(ono_spatial_query::resolve::locality(Some(&scope())).to_owned());
    // `1842` is what the process answers to: v0.4's identity rule makes the pid an alias, and the
    // historical index carries the aliases the archived record carried.
    let found = inside.resolve("1842", &context);
    assert!(
        matches!(found, ono_spatial_query::Resolution::Resolved(_)),
        "§5.4: `enter 1842` at an instant inside its lifetime resolves, got {found:?}"
    );

    let after = HistoricalWorld::reconstruct(&ledger, &scope(), instant("2026-08-31T12:50:00Z"))
        .expect("the reconstruction runs");
    assert_eq!(
        after.presence_of(&gone_id, SpatialType::Process),
        Presence::Absent,
        "§7.4: coverage that could prove the absence did not report one after the lifetime ended"
    );
    assert!(
        after
            .not_known_here(&gone_id, SpatialType::Process)
            .is_some(),
        "§9.7: an instant after the lifetime reports rather than shows the object"
    );
}
