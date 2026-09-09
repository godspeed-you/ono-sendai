//! Outcome tests for event references, `find event`, completion and historical place discovery
//! (spec v0.5 §11.6, §14.4, §20.3, §20.4).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_spatial_core::{PermissionState, SpatialId, SpatialType};
use ono_temporal_core::{
    EventId, EventKind, EventQuery, LedgerWrite, SessionLedger, TemporalCompleteness,
    TemporalEvent, TimeRange,
};
use ono_temporal_query::relevance::Horizon;
use ono_temporal_query::search::{
    DEFAULT_LIMIT, EventReferences, PresentAliases, ResolutionBasis, SearchHints, completions,
    find_events, find_place_at, plan,
};
use support::{changed, coverage, event, id, instant, ledger, subject};

/// Two events whose identities share their first two hex digits, and that shared prefix.
///
/// The digest is content-derived, so a test cannot choose a collision; it walks a series of
/// events until the pigeonhole produces one, which it must inside 257 of them.
fn colliding_pair() -> (TemporalEvent, TemporalEvent, String) {
    let mut seen: Vec<TemporalEvent> = Vec::new();
    for minute in 0..300 {
        let candidate = event(
            EventKind::ObjectObserved,
            &format!("2026-08-31T12:{:02}:{:02}Z", minute / 60, minute % 60),
            "linux.procfs",
            Some(subject(SpatialType::Process, &format!("p{minute}"))),
        );
        let prefix = candidate.event_id.as_str()[..3].to_owned();
        if let Some(earlier) = seen
            .iter()
            .find(|held| held.event_id.as_str().starts_with(&prefix))
        {
            return (earlier.clone(), candidate, prefix);
        }
        seen.push(candidate);
    }
    unreachable!("256 two-digit prefixes cannot hold 300 distinct events")
}

#[test]
fn should_resolve_a_short_reference_back_to_the_event_when_one_was_issued() {
    let shown = event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:17:51Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "nginx")),
    );
    let held = ledger(std::slice::from_ref(&shown));
    let mut references = EventReferences::new();
    let reference = references.reference(&shown);

    assert!(
        reference.to_string().starts_with("@e"),
        "§11.6 renders `@e42`"
    );
    assert!(reference.as_str().len() < shown.event_id.as_str().len());
    let found = references
        .resolve(&reference.to_string(), &held)
        .expect("the reference resolves");
    assert_eq!(
        found.map(|event| event.event_id),
        Some(shown.event_id),
        "§11.6: `inspect event`, `at event` and `why event` all mean the same event"
    );
}

#[test]
fn should_issue_the_same_reference_when_the_same_event_is_shown_twice() {
    let shown = event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:17:51Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "nginx")),
    );
    let mut references = EventReferences::new();
    let first = references.reference(&shown);
    let again = references.reference(&shown);
    assert_eq!(first, again);
    assert_eq!(references.len(), 1);
}

#[test]
fn should_keep_the_first_reference_when_a_later_event_shares_its_prefix() {
    let (earlier, later, _) = colliding_pair();
    let mut references = EventReferences::new();
    let first = references.reference(&earlier);
    let second = references.reference(&later);

    assert_ne!(first, second, "one reference names one event");
    assert_eq!(
        references
            .lookup(first.as_str())
            .map(ono_temporal_core::EventId::as_str),
        Some(earlier.event_id.as_str()),
        "a reference already shown to the user never changes meaning in the session"
    );
    assert!(
        second.as_str().len() > first.as_str().len(),
        "the later event takes a longer prefix rather than the shorter one"
    );
}

#[test]
fn should_resolve_a_reference_by_prefix_when_the_session_never_issued_it() {
    let shown = event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:17:51Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "nginx")),
    );
    let held = ledger(std::slice::from_ref(&shown));
    let references = EventReferences::new();
    let typed = format!("@{}", &shown.event_id.as_str()[..5]);
    let found = references
        .resolve(&typed, &held)
        .expect("a prefix reference resolves against the ledger");
    assert_eq!(found.map(|event| event.event_id), Some(shown.event_id));
}

#[test]
fn should_refuse_a_shortened_reference_when_it_names_more_than_one_event() {
    let (earlier, later, prefix) = colliding_pair();
    let held = ledger(&[earlier, later]);
    let references = EventReferences::new();
    let refusal = references
        .resolve(&format!("@{prefix}"), &held)
        .expect_err("an ambiguous reference is refused");
    assert_eq!(refusal.code(), ErrorCode::TemporalAmbiguousEvent);
}

#[test]
fn should_answer_nothing_when_a_reference_names_no_event() {
    let held = ledger(&[event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:17:51Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "nginx")),
    )]);
    let references = EventReferences::new();
    assert!(
        references
            .resolve("@e000000000000000000000ff", &held)
            .expect("a reference naming nothing is not a store failure")
            .is_none()
    );
}

#[test]
fn should_refuse_text_that_is_not_an_event_reference_at_all() {
    let held = ledger(&[]);
    let references = EventReferences::new();
    let refusal = references
        .resolve("yesterday", &held)
        .expect_err("only an event reference resolves to an event");
    assert_eq!(refusal.code(), ErrorCode::TemporalInvalidTime);
}

#[test]
fn should_push_every_hint_into_the_query_when_a_search_is_planned() {
    let hints = SearchHints {
        scope: Some(support::scope()),
        subjects: vec![id(SpatialType::Service, "nginx")],
        kinds: vec![EventKind::ActionFailed],
        range: TimeRange::since(instant("2026-08-31T12:00:00Z")),
        limit: Some(20),
        ..SearchHints::default()
    };
    let query: EventQuery = plan(&hints);
    assert_eq!(query.kinds, vec![EventKind::ActionFailed]);
    assert_eq!(query.subjects.len(), 1);
    assert_eq!(
        query.range,
        TimeRange::since(instant("2026-08-31T12:00:00Z"))
    );
    assert_eq!(query.limit, Some(20));
}

#[test]
fn should_bound_the_answer_when_the_caller_named_no_limit() {
    let query = plan(&SearchHints::default());
    assert_eq!(
        query.limit,
        Some(DEFAULT_LIMIT),
        "§32.3 and §43.1 both need a bounded answer"
    );
}

#[test]
fn should_return_the_matching_events_and_leave_the_predicate_to_the_pipeline() {
    let held = ledger(&[
        event(
            EventKind::ActionFailed,
            "2026-08-31T12:05:00Z",
            "ono.session",
            Some(subject(SpatialType::Service, "nginx")),
        ),
        event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:06:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "cron")),
        ),
    ]);
    let hints = SearchHints {
        kinds: vec![EventKind::ActionFailed],
        ..SearchHints::default()
    };
    let found = find_events(&held, &hints).expect("the search runs");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, EventKind::ActionFailed);
}

#[test]
fn should_prioritise_recent_relevant_events_when_completing_a_reference() {
    let held = ledger(&[
        changed(
            "2026-08-31T12:05:00Z",
            "linux.procfs",
            subject(SpatialType::Process, "nginx-worker"),
            "rss",
            Some("10"),
            Some("11"),
        ),
        event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:06:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "nginx-worker")),
        ),
        event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:07:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "cron")),
        ),
    ]);
    let horizon = Horizon::at_place(
        support::scope(),
        id(SpatialType::Service, "nginx"),
        vec![id(SpatialType::Process, "nginx-worker")],
    );
    let mut references = EventReferences::new();
    let offered = completions(
        &held,
        &mut references,
        &horizon,
        instant("2026-08-31T12:30:00Z"),
        10,
    )
    .expect("the completions are computed");

    assert_eq!(
        offered.len(),
        2,
        "§20.4 prioritises events relevant to the current place"
    );
    assert_eq!(
        offered[0].kind,
        EventKind::ObjectAppeared,
        "a node appearance orients a reader before a field moving by a byte"
    );
    assert!(offered[0].reference.to_string().starts_with("@e"));
}

#[test]
fn should_offer_nothing_from_history_this_user_may_not_read() {
    let held = SessionLedger::new();
    held.append(
        &[event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:06:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "nginx-worker")),
        )],
        &[],
    )
    .expect("the ledger appends");
    let mut denied = coverage(
        "process.existence",
        "2026-08-31T12:00:00Z",
        "2026-08-31T12:30:00Z",
        TemporalCompleteness::PermissionDenied,
    );
    denied.permission = PermissionState::PermissionDenied;
    held.record_coverage(&[denied])
        .expect("the ledger records coverage");

    let horizon = Horizon::at_place(
        support::scope(),
        id(SpatialType::Service, "nginx"),
        vec![id(SpatialType::Process, "nginx-worker")],
    );
    let mut references = EventReferences::new();
    let offered = completions(
        &held,
        &mut references,
        &horizon,
        instant("2026-08-31T12:30:00Z"),
        10,
    )
    .expect("the completions are computed");
    assert!(
        offered.is_empty(),
        "§20.4: completion MUST NOT enumerate hidden or unauthorized history"
    );
}

/// A present-day index that answers by name, standing in for the live spatial index.
struct Today(Vec<(SpatialId, SpatialType, Arc<str>)>);

impl PresentAliases for Today {
    fn resolve_alias(&self, text: &str) -> Vec<(SpatialId, SpatialType, Arc<str>)> {
        self.0
            .iter()
            .filter(|(_, _, label)| label.contains(text))
            .cloned()
            .collect()
    }
}

#[test]
fn should_answer_from_the_historical_index_when_an_event_supports_the_object_at_that_time() {
    let held = ledger(&[event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:05:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Service, "nginx")),
    )]);
    let found = find_place_at(
        &held,
        &support::scope(),
        instant("2026-08-31T12:30:00Z"),
        "nginx",
        &Today(Vec::new()),
    )
    .expect("the historical search runs");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].basis, ResolutionBasis::HistoricalEvidence);
    assert!(found[0].is_evidence_of_existence());
    assert_eq!(found[0].existed_at, Some(instant("2026-08-31T12:05:00Z")));
    assert!(found[0].anchor.is_some(), "§27.3 keeps the event reference");
}

#[test]
fn should_mark_a_present_day_name_as_a_resolution_aid_rather_than_historical_existence() {
    let held = ledger(&[]);
    let today = Today(vec![(
        id(SpatialType::Service, "nginx"),
        SpatialType::Service,
        Arc::from("nginx"),
    )]);
    let found = find_place_at(
        &held,
        &support::scope(),
        instant("2026-08-31T12:30:00Z"),
        "nginx",
        &today,
    )
    .expect("the historical search runs");
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].basis,
        ResolutionBasis::ResolutionAid,
        "§14.4: finding a name today is not evidence the thing existed then"
    );
    assert!(!found[0].is_evidence_of_existence());
    assert_eq!(found[0].existed_at, None);
    assert_eq!(found[0].anchor, None);
}

#[test]
fn should_keep_the_historical_evidence_when_a_present_day_name_helped_find_it() {
    let held = ledger(&[event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:05:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Service, "nginx")),
    )]);
    let today = Today(vec![(
        id(SpatialType::Service, "nginx"),
        SpatialType::Service,
        Arc::from("nginx"),
    )]);
    let found = find_place_at(
        &held,
        &support::scope(),
        instant("2026-08-31T12:30:00Z"),
        "nginx",
        &today,
    )
    .expect("the historical search runs");
    assert_eq!(
        found.len(),
        1,
        "one object is one match however it was found"
    );
    assert_eq!(found[0].basis, ResolutionBasis::HistoricalEvidence);
}

#[test]
fn should_ignore_an_event_after_the_active_time_when_searching_the_historical_index() {
    let held = ledger(&[event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:45:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Service, "nginx")),
    )]);
    let found = find_place_at(
        &held,
        &support::scope(),
        instant("2026-08-31T12:30:00Z"),
        "nginx",
        &Today(Vec::new()),
    )
    .expect("the historical search runs");
    assert!(
        found.is_empty(),
        "§14.4 searches the historical index for the active time"
    );
}

#[test]
fn should_carry_the_full_identity_when_a_reference_is_asked_for_it() {
    let shown = event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:17:51Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "nginx")),
    );
    let mut references = EventReferences::new();
    let reference = references.reference(&shown);
    assert_eq!(reference.event_id(), &shown.event_id);
    assert_eq!(
        EventId::parse(reference.as_str()).map(|id| id.as_str().to_owned()),
        Some(reference.as_str().to_owned()),
        "a short reference is itself an event reference"
    );
}
