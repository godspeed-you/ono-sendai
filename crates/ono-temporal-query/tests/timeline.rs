//! Outcome tests for the timeline planner, event density and temporal landmarks
//! (spec v0.5 §11, §19.4, §27).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::sync::Arc;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    CoverageSummary, EventKind, EventQuery, LedgerWrite, SessionLedger, TemporalCompleteness,
    TemporalContext, TimeRange,
};
use ono_temporal_query::landmark::{LandmarkRules, TemporalLandmarkKind, landmarks};
use ono_temporal_query::relevance::Horizon;
use ono_temporal_query::timeline::{
    DEFAULT_WINDOW, GroupReason, HISTORICAL_HALF_WINDOW, TimelineRequest, group, plan, timeline,
    window_of,
};
use ono_value::{Duration, Value};
use support::{changed, coverage, event, id, instant, ledger, relation, subject};

fn present() -> TemporalContext {
    TemporalContext::Present
}

fn historical(at: &str) -> TemporalContext {
    TemporalContext::Historical {
        requested: ono_temporal_core::TimeSelector::Absolute(instant(at)),
        requested_text: Arc::from(at),
        resolved_at: instant(at),
        coverage: CoverageSummary::default(),
        anchor_event: None,
    }
}

fn place_request() -> TimelineRequest {
    TimelineRequest::new(Horizon::at_place(
        support::scope(),
        id(SpatialType::Service, "nginx"),
        vec![id(SpatialType::Process, "nginx-worker")],
    ))
}

#[test]
fn should_end_the_default_window_at_now_when_the_session_is_in_the_present() {
    let now = instant("2026-08-31T13:00:00Z");
    let window = window_of(&place_request(), &present(), now);
    assert_eq!(window.until, now);
    assert_eq!(window.from, instant("2026-08-31T12:30:00Z"));
    assert_eq!(window.centre, None);
    assert_eq!(DEFAULT_WINDOW, Duration::parse("30m").expect("a duration"));
}

#[test]
fn should_centre_the_window_on_the_coordinate_when_historical_context_is_active() {
    let now = instant("2026-08-31T14:00:00Z");
    let window = window_of(&place_request(), &historical("2026-08-31T12:17:00Z"), now);
    assert_eq!(
        window.from,
        instant("2026-08-31T12:02:00Z"),
        "§11.8 centres the default window on the historical coordinate"
    );
    assert_eq!(window.until, instant("2026-08-31T12:32:00Z"));
    assert_eq!(window.centre, Some(instant("2026-08-31T12:17:00Z")));
    assert_eq!(
        HISTORICAL_HALF_WINDOW,
        Duration::parse("15m").expect("a duration")
    );
}

#[test]
fn should_honour_the_explicit_bounds_when_since_and_until_are_given() {
    let mut request = place_request();
    request.since = Some(instant("2026-08-31T12:00:00Z"));
    request.until = Some(instant("2026-08-31T12:10:00Z"));
    let window = window_of(&request, &present(), instant("2026-08-31T13:00:00Z"));
    assert_eq!(window.from, instant("2026-08-31T12:00:00Z"));
    assert_eq!(window.until, instant("2026-08-31T12:10:00Z"));
}

#[test]
fn should_push_the_window_the_subjects_and_the_kinds_into_the_query_when_a_plan_is_made() {
    let mut request = place_request();
    request.kinds = vec![EventKind::RelationAdded];
    request.limit = 42;
    let query: EventQuery = plan(&request, &present(), instant("2026-08-31T13:00:00Z"));
    assert_eq!(
        query.range,
        TimeRange::between(
            instant("2026-08-31T12:30:00Z"),
            instant("2026-08-31T13:00:00Z")
        ),
        "§32.3's budget needs the window pushed into the query rather than filtered afterwards"
    );
    assert_eq!(query.kinds, vec![EventKind::RelationAdded]);
    assert_eq!(
        query.subjects.len(),
        2,
        "the place and its directly relevant objects are the query's subjects"
    );
    assert_eq!(query.scope.as_ref(), Some(&support::scope()));
    assert_eq!(
        query.limit,
        Some(43),
        "one over the limit detects truncation"
    );
}

#[test]
fn should_leave_the_subjects_open_when_the_horizon_is_widened() {
    let request = TimelineRequest::new(Horizon::everything(support::scope()));
    let query = plan(&request, &present(), instant("2026-08-31T13:00:00Z"));
    assert!(
        query.subjects.is_empty(),
        "§11.3: `--all` requests the full visible scope"
    );
}

#[test]
fn should_return_only_the_place_and_its_neighbours_when_no_selector_was_given() {
    let held = ledger(&[
        changed(
            "2026-08-31T12:40:00Z",
            "linux.systemd-dbus",
            subject(SpatialType::Service, "nginx"),
            "active_state",
            Some("active"),
            Some("failed"),
        ),
        event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:41:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "nginx-worker")),
        ),
        event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:42:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "cron")),
        ),
    ]);
    let answer = timeline(
        &held,
        &place_request(),
        &present(),
        instant("2026-08-31T13:00:00Z"),
    )
    .expect("the timeline is planned");
    let labels: Vec<&str> = answer
        .events
        .iter()
        .filter_map(|event| {
            event
                .subject
                .as_ref()
                .map(ono_temporal_core::SpatialRef::label)
        })
        .collect();
    assert_eq!(labels, vec!["nginx", "nginx-worker"]);
}

#[test]
fn should_show_the_gap_when_events_exist_on_both_sides_of_it() {
    let held = SessionLedger::new();
    held.append(
        &[
            event(
                EventKind::ObjectAppeared,
                "2026-08-31T12:35:00Z",
                "linux.procfs",
                Some(subject(SpatialType::Service, "nginx")),
            ),
            event(
                EventKind::ObjectDisappeared,
                "2026-08-31T12:55:00Z",
                "linux.procfs",
                Some(subject(SpatialType::Service, "nginx")),
            ),
        ],
        &[],
    )
    .expect("the ledger appends");
    held.record_coverage(&[
        coverage(
            "process.existence",
            "2026-08-31T12:30:00Z",
            "2026-08-31T12:40:00Z",
            TemporalCompleteness::Complete,
        ),
        coverage(
            "process.existence",
            "2026-08-31T12:50:00Z",
            "2026-08-31T13:00:00Z",
            TemporalCompleteness::Complete,
        ),
    ])
    .expect("the ledger records coverage");

    let answer = timeline(
        &held,
        &place_request(),
        &present(),
        instant("2026-08-31T13:00:00Z"),
    )
    .expect("the timeline is planned");
    assert_eq!(
        answer.gaps.len(),
        1,
        "§11.7: a gap MUST not be hidden because events exist on both sides"
    );
    assert_eq!(answer.gaps[0].from, instant("2026-08-31T12:40:00Z"));
    assert_eq!(answer.gaps[0].until, instant("2026-08-31T12:50:00Z"));
    assert_eq!(
        answer.events.len(),
        2,
        "the events on both sides are still there"
    );
}

#[test]
fn should_report_truncation_when_the_limit_cut_the_answer_rather_than_the_window() {
    let events: Vec<ono_temporal_core::TemporalEvent> = (0..5)
        .map(|minute| {
            changed(
                &format!("2026-08-31T12:4{minute}:00Z"),
                "linux.systemd-dbus",
                subject(SpatialType::Service, "nginx"),
                "active_state",
                Some("active"),
                Some("reloading"),
            )
        })
        .collect();
    let held = ledger(&events);
    let mut request = place_request();
    request.limit = 2;
    let answer = timeline(&held, &request, &present(), instant("2026-08-31T13:00:00Z"))
        .expect("the timeline is planned");
    assert_eq!(answer.events.len(), 2);
    assert!(
        answer.truncated,
        "a reader must be able to tell a quiet interval from a truncated one"
    );
}

#[test]
fn should_render_the_timeline_as_its_own_schema_when_a_pipeline_consumes_it() {
    let held = ledger(&[event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:45:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Service, "nginx")),
    )]);
    let answer = timeline(
        &held,
        &place_request(),
        &present(),
        instant("2026-08-31T13:00:00Z"),
    )
    .expect("the timeline is planned");
    let record = answer.to_record().expect("the timeline record is built");
    assert_eq!(record.schema_id().to_string(), "ono.temporal-timeline/1");
    assert_eq!(
        record
            .get("truncated")
            .and_then(|value| value.as_bool().ok()),
        Some(false)
    );
    let events = record
        .get("events")
        .and_then(|value| value.as_list().ok().map(<[Value]>::to_vec))
        .expect("the events are a list");
    assert_eq!(events.len(), 1);
}

#[test]
fn should_group_repeated_changes_to_one_field_when_they_fall_in_one_window() {
    let events: Vec<ono_temporal_core::TemporalEvent> = (0..4)
        .map(|second| {
            changed(
                &format!("2026-08-31T12:40:0{second}Z"),
                "linux.procfs",
                subject(SpatialType::Process, "nginx-worker"),
                "rss",
                Some("100"),
                Some("101"),
            )
        })
        .collect();
    let groups = group(&events, Duration::parse("10s").expect("a duration"));
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].reason, Some(GroupReason::SameField));
    assert_eq!(
        groups[0].hidden, 3,
        "§19.4: grouping preserves hidden counts"
    );
    assert_eq!(
        groups[0].members.len(),
        4,
        "§19.5: a group expands to its members"
    );
    assert_eq!(groups[0].from, instant("2026-08-31T12:40:00Z"));
    assert_eq!(
        groups[0].until,
        instant("2026-08-31T12:40:03Z"),
        "§19.4: grouping preserves the time span"
    );
}

#[test]
fn should_never_group_an_object_lifecycle_change_when_several_fall_in_one_window() {
    let events = vec![
        event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:40:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "worker-a")),
        ),
        event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:40:01Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "worker-b")),
        ),
        relation(
            EventKind::RelationAdded,
            "2026-08-31T12:40:02Z",
            "linux.procfs",
            subject(SpatialType::Service, "nginx"),
            subject(SpatialType::Process, "worker-a"),
            "service.owns_process",
        ),
    ];
    let groups = group(&events, Duration::parse("10s").expect("a duration"));
    assert_eq!(
        groups.len(),
        3,
        "grouping never hides an object or relation lifecycle change"
    );
    assert!(groups.iter().all(|group| group.reason.is_none()));
}

#[test]
fn should_group_repeated_samples_that_changed_nothing_when_they_repeat() {
    let events: Vec<ono_temporal_core::TemporalEvent> = (0..3)
        .map(|second| {
            event(
                EventKind::ObjectObserved,
                &format!("2026-08-31T12:40:0{second}Z"),
                "linux.procfs",
                Some(subject(SpatialType::Process, "nginx-worker")),
            )
        })
        .collect();
    let groups = group(&events, Duration::parse("10s").expect("a duration"));
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].reason, Some(GroupReason::UnchangedSample));
    assert_eq!(groups[0].hidden, 2);
}

#[test]
fn should_anchor_a_service_failure_and_its_recovery_when_the_state_moves() {
    let failure = changed(
        "2026-08-31T12:40:00Z",
        "linux.systemd-dbus",
        subject(SpatialType::Service, "nginx"),
        "active_state",
        Some("active"),
        Some("failed"),
    );
    let recovery = changed(
        "2026-08-31T12:45:00Z",
        "linux.systemd-dbus",
        subject(SpatialType::Service, "nginx"),
        "active_state",
        Some("failed"),
        Some("active"),
    );
    let found = landmarks(
        &[failure.clone(), recovery.clone()],
        &LandmarkRules::default(),
    );
    assert_eq!(
        found
            .iter()
            .map(|landmark| landmark.kind())
            .collect::<Vec<_>>(),
        vec![
            TemporalLandmarkKind::ServiceFailure,
            TemporalLandmarkKind::ServiceRecovery
        ]
    );
    assert_eq!(
        found[0].event(),
        &failure.event_id,
        "§27.3: a landmark keeps its event reference so `why event` and `at event` work"
    );
    assert_eq!(found[1].event(), &recovery.event_id);
}

#[test]
fn should_anchor_an_operator_action_without_claiming_severity() {
    let action = event(
        EventKind::ActionRequested,
        "2026-08-31T12:40:00Z",
        "ono.session",
        Some(subject(SpatialType::Service, "nginx")),
    );
    let found = landmarks(&[action], &LandmarkRules::default());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind(), TemporalLandmarkKind::OperatorAction);
    let rendered = found[0].to_record().expect("the landmark record is built");
    let record = rendered
        .as_record()
        .expect("the landmark renders as `ono.temporal-landmark/1`");
    record
        .validate()
        .expect("the landmark record satisfies the contract it declares");
    assert!(
        record.get("severity").is_none() && record.get("priority").is_none(),
        "§27.2: a landmark is a navigation anchor, never an incident alert, and the contract \
         declares no field that would let one claim to be"
    );
    assert_eq!(
        record.get("kind").and_then(|value| value.as_str().ok()),
        Some("operator_action")
    );
}

#[test]
fn should_anchor_a_recorder_gap_apart_from_an_ordinary_coverage_boundary() {
    let recorder_gap = event(
        EventKind::CoverageEnded,
        "2026-08-31T12:40:00Z",
        "ono.recorder",
        None,
    );
    let provider_boundary = event(
        EventKind::CoverageEnded,
        "2026-08-31T12:41:00Z",
        "linux.systemd-dbus",
        None,
    );
    let found = landmarks(
        &[recorder_gap, provider_boundary],
        &LandmarkRules::default(),
    );
    assert_eq!(
        found
            .iter()
            .map(|landmark| landmark.kind())
            .collect::<Vec<_>>(),
        vec![
            TemporalLandmarkKind::RecorderGap,
            TemporalLandmarkKind::CoverageBoundary
        ]
    );
}

#[test]
fn should_anchor_a_restart_loop_when_the_threshold_is_crossed_inside_the_window() {
    let events: Vec<ono_temporal_core::TemporalEvent> = (0..3)
        .map(|minute| {
            changed(
                &format!("2026-08-31T12:4{minute}:00Z"),
                "linux.systemd-dbus",
                subject(SpatialType::Service, "flaky"),
                "active_state",
                Some("failed"),
                Some("activating"),
            )
        })
        .collect();
    let found = landmarks(&events, &LandmarkRules::default());
    assert_eq!(
        found
            .iter()
            .filter(|landmark| landmark.kind() == TemporalLandmarkKind::RestartLoop)
            .count(),
        1,
        "the loop is one anchor at the instant the threshold is crossed"
    );
}

#[test]
fn should_produce_no_landmark_when_an_ordinary_field_moves() {
    let quiet = changed(
        "2026-08-31T12:40:00Z",
        "linux.procfs",
        subject(SpatialType::Process, "nginx-worker"),
        "rss",
        Some("100"),
        Some("101"),
    );
    assert!(landmarks(&[quiet], &LandmarkRules::default()).is_empty());
}

#[test]
fn should_carry_the_producers_groups_when_a_dense_window_becomes_a_record() {
    let events: Vec<ono_temporal_core::TemporalEvent> = (0..4)
        .map(|second| {
            changed(
                &format!("2026-08-31T12:40:0{second}Z"),
                "linux.procfs",
                subject(SpatialType::Process, "nginx-worker"),
                "rss",
                Some("100"),
                Some("101"),
            )
        })
        .collect();
    let held = ledger(&events);
    let answer = timeline(
        &held,
        &TimelineRequest::new(Horizon::everything(support::scope())),
        &present(),
        instant("2026-08-31T13:00:00Z"),
    )
    .expect("the timeline is planned");
    let record = answer.to_record().expect("the timeline record is built");

    let Some(Value::List(groups)) = record.get("groups") else {
        panic!("§19.4's grouping is a judgement the producer makes, and it travels");
    };
    assert_eq!(groups.len(), 1, "one run of one field is one row");
    let Some(Value::Map(row)) = groups.first() else {
        panic!("a group row is a sub-record");
    };
    assert_eq!(
        row.get("hidden"),
        Some(&Value::Int(3)),
        "§19.4: grouping preserves hidden counts"
    );
    assert_eq!(
        row.get("from"),
        Some(&Value::Timestamp(instant("2026-08-31T12:40:00Z")))
    );
    assert_eq!(
        row.get("until"),
        Some(&Value::Timestamp(instant("2026-08-31T12:40:03Z"))),
        "§19.4: grouping preserves the time span"
    );
    let Some(Value::List(members)) = row.get("members") else {
        panic!("§19.5: a grouped event expands to its retained individuals");
    };
    assert_eq!(members.len(), 4);
    assert_eq!(row.get("reason"), Some(&Value::string("same_field")));
}

#[test]
fn should_print_the_reference_the_session_minted_when_a_timeline_becomes_a_record() {
    let held = ledger(&[event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:45:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Service, "nginx")),
    )]);
    let mut references = ono_temporal_query::search::EventReferences::new();
    let answer = timeline(
        &held,
        &place_request(),
        &present(),
        instant("2026-08-31T13:00:00Z"),
    )
    .expect("the timeline is planned")
    .with_references(&mut references);
    let record = answer.to_record().expect("the timeline record is built");

    let Some(Value::List(events)) = record.get("events") else {
        panic!("the events are a list");
    };
    let Some(Value::Record(first)) = events.first() else {
        panic!("an event is a record");
    };
    let Some(Value::String(printed)) = first.get("reference") else {
        panic!("§11.6: a rendered event carries the reference the session minted");
    };
    // §11.6 requires the rendered reference to work in `at event`, `inspect event` and `why
    // event`, so it must be the string the session's own resolver accepts.
    let resolved = references
        .resolve(printed, &held)
        .expect("the reference resolves")
        .expect("it names an event this session showed");
    assert_eq!(
        Some(&resolved.event_id),
        answer.events.first().map(|event| &event.event_id)
    );
}

#[test]
fn should_leave_the_reference_null_when_no_session_minted_one() {
    let held = ledger(&[event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:45:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Service, "nginx")),
    )]);
    let record = timeline(
        &held,
        &place_request(),
        &present(),
        instant("2026-08-31T13:00:00Z"),
    )
    .expect("the timeline is planned")
    .to_record()
    .expect("the timeline record is built");

    let Some(Value::List(events)) = record.get("events") else {
        panic!("the events are a list");
    };
    let Some(Value::Record(first)) = events.first() else {
        panic!("an event is a record");
    };
    assert_eq!(first.get("reference"), Some(&Value::Null));
}
