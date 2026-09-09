//! Outcome tests for the relevance planner (spec v0.5 §11.3, §18.4).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_spatial_core::{BootIdentity, SpatialScope, SpatialType};
use ono_temporal_core::EventKind;
use ono_temporal_query::relevance::{
    Horizon, PlaceRelation, RelevanceClass, classify, is_default_scope, rank_events,
};
use support::{changed, event, id, relation, subject};

fn place_horizon() -> Horizon {
    Horizon::at_place(
        support::scope(),
        id(SpatialType::Service, "nginx"),
        vec![id(SpatialType::Process, "nginx-worker")],
    )
}

#[test]
fn should_rank_a_node_appearance_above_a_field_change_when_both_are_relevant() {
    let horizon = place_horizon();
    let appeared = event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "nginx-worker")),
    );
    let touched = changed(
        "2026-08-31T12:00:01Z",
        "linux.systemd-dbus",
        subject(SpatialType::Service, "nginx"),
        "memory_current",
        Some("10"),
        Some("11"),
    );

    assert_eq!(
        classify(&appeared, &horizon).class,
        RelevanceClass::NodeLifecycle,
        "§18.4 puts node appearance and disappearance first"
    );
    assert_eq!(
        classify(&touched, &horizon).class,
        RelevanceClass::ObjectChange
    );
    assert!(
        classify(&appeared, &horizon).rank() < classify(&touched, &horizon).rank(),
        "a lower rank steps first"
    );
}

#[test]
fn should_order_the_six_classes_exactly_as_the_stepping_priority_names_them() {
    let ranks: Vec<RelevanceClass> = RelevanceClass::ALL.to_vec();
    assert_eq!(
        ranks,
        vec![
            RelevanceClass::NodeLifecycle,
            RelevanceClass::RelationLifecycle,
            RelevanceClass::ServiceState,
            RelevanceClass::LandmarkChange,
            RelevanceClass::OperatorAction,
            RelevanceClass::ObjectChange,
            RelevanceClass::Background,
        ],
        "§18.4 fixes the priority order of the relevance planner"
    );
}

#[test]
fn should_report_the_relation_to_the_place_when_an_event_touches_a_neighbour() {
    let horizon = place_horizon();
    let here = changed(
        "2026-08-31T12:00:00Z",
        "linux.systemd-dbus",
        subject(SpatialType::Service, "nginx"),
        "active_state",
        Some("activating"),
        Some("active"),
    );
    let neighbour = event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "nginx-worker")),
    );
    let stranger = event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "cron")),
    );

    assert_eq!(classify(&here, &horizon).relation, PlaceRelation::Place);
    assert_eq!(
        classify(&neighbour, &horizon).relation,
        PlaceRelation::Neighbour
    );
    assert_eq!(classify(&stranger, &horizon).relation, PlaceRelation::Scope);
}

#[test]
fn should_report_elsewhere_when_the_event_belongs_to_another_scope() {
    let far = event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "nginx-worker")),
    );
    let elsewhere = Horizon::at_place(
        SpatialScope::host("web01", BootIdentity::unknown_boot("web01")),
        id(SpatialType::Service, "nginx"),
        Vec::new(),
    );
    assert_eq!(
        classify(&far, &elsewhere).relation,
        PlaceRelation::Elsewhere
    );
    assert!(
        !is_default_scope(&far, &elsewhere),
        "an event from another host is outside the visible scope"
    );
}

#[test]
fn should_exclude_an_unrelated_object_when_the_timeline_is_scoped_to_a_place() {
    let horizon = place_horizon();
    let stranger = changed(
        "2026-08-31T12:00:00Z",
        "linux.procfs",
        subject(SpatialType::Process, "cron"),
        "rss",
        Some("100"),
        Some("120"),
    );
    let neighbour = event(
        EventKind::ObjectDisappeared,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "nginx-worker")),
    );

    assert!(
        !is_default_scope(&stranger, &horizon),
        "§11.3 scopes the default timeline to the current place and its directly relevant events"
    );
    assert!(is_default_scope(&neighbour, &horizon));
}

#[test]
fn should_show_high_significance_events_and_session_actions_when_the_place_is_the_root() {
    let horizon = Horizon::at_root(support::scope());
    let noise = changed(
        "2026-08-31T12:00:00Z",
        "linux.procfs",
        subject(SpatialType::Process, "cron"),
        "rss",
        Some("100"),
        Some("120"),
    );
    let failure = changed(
        "2026-08-31T12:00:01Z",
        "linux.systemd-dbus",
        subject(SpatialType::Service, "nginx"),
        "active_state",
        Some("active"),
        Some("failed"),
    );
    let action = event(
        EventKind::ActionRequested,
        "2026-08-31T12:00:02Z",
        "ono.session",
        Some(subject(SpatialType::Service, "nginx")),
    );

    assert!(
        !is_default_scope(&noise, &horizon),
        "§11.3: the root place does not dump every event from every object"
    );
    assert!(is_default_scope(&failure, &horizon));
    assert!(
        is_default_scope(&action, &horizon),
        "§11.3 keeps current-session actions at the root"
    );
}

#[test]
fn should_widen_to_the_whole_scope_when_the_horizon_is_all() {
    let horizon = Horizon::everything(support::scope());
    let stranger = changed(
        "2026-08-31T12:00:00Z",
        "linux.procfs",
        subject(SpatialType::Process, "cron"),
        "rss",
        Some("100"),
        Some("120"),
    );
    assert!(
        is_default_scope(&stranger, &horizon),
        "§11.3: `--all` requests the full visible scope"
    );
}

#[test]
fn should_rank_a_relation_change_second_when_it_touches_the_place() {
    let horizon = place_horizon();
    let edge = relation(
        EventKind::RelationAdded,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
        subject(SpatialType::Service, "nginx"),
        subject(SpatialType::Process, "nginx-worker"),
        "service.owns_process",
    );
    assert_eq!(
        classify(&edge, &horizon).class,
        RelevanceClass::RelationLifecycle
    );
}

#[test]
fn should_produce_the_same_order_when_the_same_events_arrive_in_a_different_order() {
    let horizon = place_horizon();
    let appeared = event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:00:05Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "nginx-worker")),
    );
    let failure = changed(
        "2026-08-31T12:00:05Z",
        "linux.systemd-dbus",
        subject(SpatialType::Service, "nginx"),
        "active_state",
        Some("active"),
        Some("failed"),
    );
    let forwards = rank_events(&[appeared.clone(), failure.clone()], &horizon);
    let backwards = rank_events(&[failure, appeared], &horizon);
    assert_eq!(
        forwards
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>(),
        backwards
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>(),
        "relevance ranking is deterministic"
    );
    assert_eq!(
        forwards.first().map(|event| event.kind),
        Some(EventKind::ObjectAppeared),
        "§18.4 steps to a node appearance before a service state change"
    );
}
