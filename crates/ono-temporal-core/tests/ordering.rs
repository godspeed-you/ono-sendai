//! Ordering: v0.5 §26.1's happens-before, "supported by stronger evidence than wall time", and
//! §26.3's concurrency — "if neither A happens-before B nor B happens-before A, they are
//! potentially concurrent", and `inspect` must not claim otherwise.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_temporal_core::{EventKind, OrderEvidence, Ordering, happens_before, presentation_order};

use common::{domain, event, instant, seed, times};

#[test]
fn should_report_concurrent_when_only_the_wall_clocks_differ() {
    let earlier = event(
        EventKind::ObjectObserved,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
    );
    let later = event(
        EventKind::ObjectObserved,
        "2026-08-31T12:00:05Z",
        "linux.procfs",
    );
    assert_eq!(
        happens_before(&earlier, &later),
        (Ordering::Concurrent, None),
        "§26.1: wall time alone never establishes happens-before"
    );
}

#[test]
fn should_report_concurrent_when_two_clock_domains_have_no_shared_ordering_evidence() {
    let mut here = seed(
        EventKind::ObjectObserved,
        times("2026-08-31T12:00:00Z"),
        "linux.procfs",
    );
    here.times.source_sequence = Some(1);
    here.times.monotonic_nanos = Some(1_000);
    let mut there = seed(
        EventKind::ObjectObserved,
        times("2026-08-31T12:00:05Z"),
        "linux.procfs",
    );
    there.times.domain = domain("web01", Some("boot-b"));
    there.times.source_sequence = Some(2);
    there.times.monotonic_nanos = Some(2_000);
    assert_eq!(
        happens_before(&here.seal(), &there.seal()),
        (Ordering::Concurrent, None),
        "§26.4: a chain crosses hosts only when the evidence does"
    );
}

#[test]
fn should_report_before_when_one_source_stream_numbers_both_events() {
    let mut first = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:05Z"),
        "linux.netlink",
    );
    first.times.source_sequence = Some(41);
    let mut second = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:00Z"),
        "linux.netlink",
    );
    second.times.source_sequence = Some(42);
    assert_eq!(
        happens_before(&first.seal(), &second.seal()),
        (Ordering::Before, Some(OrderEvidence::SourceSequence)),
        "§25.4: the sequence orders the stream even when the wall clock jumped backwards"
    );
}

#[test]
fn should_report_after_when_the_source_sequence_runs_the_other_way() {
    let mut first = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:00Z"),
        "linux.netlink",
    );
    first.times.source_sequence = Some(42);
    let mut second = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:05Z"),
        "linux.netlink",
    );
    second.times.source_sequence = Some(41);
    assert_eq!(
        happens_before(&first.seal(), &second.seal()),
        (Ordering::After, Some(OrderEvidence::SourceSequence))
    );
}

#[test]
fn should_report_concurrent_when_two_sources_number_their_own_streams() {
    let mut netlink = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:00Z"),
        "linux.netlink",
    );
    netlink.times.source_sequence = Some(41);
    let mut journald = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:05Z"),
        "linux.journald",
    );
    journald.times.source_sequence = Some(42);
    assert_eq!(
        happens_before(&netlink.seal(), &journald.seal()),
        (Ordering::Concurrent, None),
        "two sequence counters that count different things do not compare"
    );
}

#[test]
fn should_report_before_when_a_monotonic_clock_orders_one_boot() {
    let mut first = seed(
        EventKind::ObjectAppeared,
        times("2026-08-31T12:00:09Z"),
        "linux.procfs",
    );
    first.times.monotonic_nanos = Some(1_000_000);
    let mut second = seed(
        EventKind::ObjectAppeared,
        times("2026-08-31T12:00:00Z"),
        "linux.journald",
    );
    second.times.monotonic_nanos = Some(2_000_000);
    assert_eq!(
        happens_before(&first.seal(), &second.seal()),
        (Ordering::Before, Some(OrderEvidence::Monotonic)),
        "§25.1: monotonic time orders reliably within one clock domain"
    );
}

#[test]
fn should_report_concurrent_when_a_monotonic_reading_has_no_boot_to_belong_to() {
    let mut first = seed(
        EventKind::ObjectAppeared,
        times("2026-08-31T12:00:00Z"),
        "linux.procfs",
    );
    first.times.domain = domain("testbox", None);
    first.times.monotonic_nanos = Some(1_000_000);
    let mut second = seed(
        EventKind::ObjectAppeared,
        times("2026-08-31T12:00:09Z"),
        "linux.procfs",
    );
    second.times.domain = domain("testbox", None);
    second.times.monotonic_nanos = Some(2_000_000);
    assert_eq!(
        happens_before(&first.seal(), &second.seal()),
        (Ordering::Concurrent, None),
        "§25.5: monotonic clocks reset across boot, so a reading with no boot orders nothing"
    );
}

#[test]
fn should_order_by_presentation_instant_when_a_timeline_is_drawn() {
    let mut events = vec![
        event(
            EventKind::ObjectChanged,
            "2026-08-31T12:00:05Z",
            "linux.procfs",
        ),
        event(
            EventKind::ObjectChanged,
            "2026-08-31T12:00:01Z",
            "linux.procfs",
        ),
        event(
            EventKind::ObjectChanged,
            "2026-08-31T12:00:03Z",
            "linux.procfs",
        ),
    ];
    presentation_order(&mut events);
    let instants: Vec<_> = events
        .iter()
        .map(|event| event.times.presentation_instant())
        .collect();
    assert_eq!(
        instants,
        vec![
            instant("2026-08-31T12:00:01Z"),
            instant("2026-08-31T12:00:03Z"),
            instant("2026-08-31T12:00:05Z"),
        ],
        "§26.3: a renderer may choose a stable display order without claiming one"
    );
}

#[test]
fn should_prefer_the_source_time_when_a_source_gave_one() {
    let mut seed = seed(
        EventKind::ObjectChanged,
        times("2026-08-31T12:00:09Z"),
        "linux.journald",
    );
    seed.times.source_time = Some(instant("2026-08-31T11:59:00Z"));
    assert_eq!(
        seed.seal().times.presentation_instant(),
        instant("2026-08-31T11:59:00Z"),
        "§3.3: a human navigates by when the source says it happened"
    );
}

#[test]
fn should_keep_a_stable_order_when_two_events_share_an_instant() {
    let mut events = vec![
        event(
            EventKind::ObjectChanged,
            "2026-08-31T12:00:01Z",
            "linux.netlink",
        ),
        event(
            EventKind::ObjectChanged,
            "2026-08-31T12:00:01Z",
            "linux.procfs",
        ),
    ];
    let mut reversed = vec![events[1].clone(), events[0].clone()];
    presentation_order(&mut events);
    presentation_order(&mut reversed);
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>(),
        reversed
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>(),
        "the same set draws the same way whatever order it arrived in"
    );
}
