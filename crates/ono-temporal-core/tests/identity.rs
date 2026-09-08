//! Event identity: the content digest of v0.5 §3.3 and the deduplication of §6.8, which "MUST
//! NOT collapse two distinct source events merely because their rendered text is equal".

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_temporal_core::{EventId, EventKind, EventTimes, SpatialRef};

use common::{instant, provenance, scope, seed, subject, times};

#[test]
fn should_produce_one_identity_when_one_source_reports_one_observation_twice() {
    let first = seed(
        EventKind::ObjectObserved,
        times("2026-08-31T12:00:00Z"),
        "linux.procfs",
    );
    let second = seed(
        EventKind::ObjectObserved,
        times("2026-08-31T12:00:00Z"),
        "linux.procfs",
    );
    assert_eq!(
        EventId::of(&first),
        EventId::of(&second),
        "the same observation from the same source is one event (§6.8)"
    );
}

#[test]
fn should_produce_two_identities_when_two_distinct_source_events_render_the_same_text() {
    // Same kind, same subject, same rendered row — different observations, one second apart.
    let mut first = seed(
        EventKind::ObjectObserved,
        times("2026-08-31T12:00:00Z"),
        "linux.procfs",
    );
    first.subject = Some(SpatialRef::Resolved {
        id: subject(1842),
        object_type: ono_spatial_core::SpatialType::Process,
        label: "nginx".into(),
    });
    let mut second = seed(
        EventKind::ObjectObserved,
        times("2026-08-31T12:00:01Z"),
        "linux.procfs",
    );
    second.subject = first.subject.clone();
    assert_ne!(
        EventId::of(&first),
        EventId::of(&second),
        "§6.8: equal rendered text is not equal identity"
    );
}

#[test]
fn should_produce_two_identities_when_two_sources_report_the_same_instant() {
    let procfs = seed(
        EventKind::ObjectObserved,
        times("2026-08-31T12:00:00Z"),
        "linux.procfs",
    );
    let journald = seed(
        EventKind::ObjectObserved,
        times("2026-08-31T12:00:00Z"),
        "linux.journald",
    );
    assert_ne!(
        EventId::of(&procfs),
        EventId::of(&journald),
        "the source is part of what makes an observation that observation (§3.4)"
    );
}

#[test]
fn should_separate_identities_when_two_events_differ_only_in_their_source_sequence() {
    let mut a = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:00Z"),
        "linux.netlink",
    );
    a.times.source_sequence = Some(41);
    let mut b = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:00Z"),
        "linux.netlink",
    );
    b.times.source_sequence = Some(42);
    assert_ne!(EventId::of(&a), EventId::of(&b));
}

#[test]
fn should_separate_identities_when_two_events_differ_only_in_their_boot() {
    let mut a = seed(
        EventKind::ObjectAppeared,
        times("2026-08-31T12:00:00Z"),
        "linux.procfs",
    );
    a.times.domain = common::domain("testbox", Some("boot-a"));
    let mut b = seed(
        EventKind::ObjectAppeared,
        times("2026-08-31T12:00:00Z"),
        "linux.procfs",
    );
    b.times.domain = common::domain("testbox", Some("boot-b"));
    assert_ne!(
        EventId::of(&a),
        EventId::of(&b),
        "§25.5: a boot boundary separates clock domains, and therefore observations"
    );
}

#[test]
fn should_separate_identities_when_two_events_carry_different_payloads() {
    let mut a = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:00Z"),
        "linux.journald",
    );
    a.payload = Some(ono_value::Value::string("job 4821 succeeded"));
    let mut b = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:00Z"),
        "linux.journald",
    );
    b.payload = Some(ono_value::Value::string("job 4822 succeeded"));
    assert_ne!(EventId::of(&a), EventId::of(&b));
}

#[test]
fn should_render_an_event_reference_a_later_command_can_use() {
    let event = seed(
        EventKind::ObjectChanged,
        times("2026-08-31T12:00:00Z"),
        "ono.recorder",
    )
    .seal();
    let rendered = event.event_id.to_string();
    assert!(
        rendered.starts_with("@e"),
        "§11.6 spells an event reference `@e…`: {rendered}"
    );
    assert_eq!(
        EventId::parse(&rendered).as_ref(),
        Some(&event.event_id),
        "the rendering parses back to the same identity"
    );
    assert_eq!(
        EventId::parse(event.event_id.as_str()).as_ref(),
        Some(&event.event_id),
        "the bare form parses too"
    );
}

#[test]
fn should_refuse_a_reference_when_it_is_not_an_event_identity() {
    for text in [
        "",
        "@e",
        "@x0123",
        "@e0123456789abcdef012345678",
        "@eZZZZ",
        "e 42",
    ] {
        assert!(
            EventId::parse(text).is_none(),
            "`{text}` is not an event reference"
        );
    }
}

#[test]
fn should_keep_the_identity_of_a_sealed_event_when_it_is_sealed() {
    let seed = seed(
        EventKind::ObjectChanged,
        times("2026-08-31T12:00:00Z"),
        "ono.recorder",
    );
    let expected = EventId::of(&seed);
    assert_eq!(seed.seal().event_id, expected);
}

#[test]
fn should_separate_identities_when_a_subject_is_named_rather_than_resolved() {
    let mut resolved = seed(
        EventKind::ObjectAppeared,
        times("2026-08-31T12:00:00Z"),
        "linux.journald",
    );
    resolved.subject = Some(SpatialRef::Resolved {
        id: subject(1842),
        object_type: ono_spatial_core::SpatialType::Process,
        label: "nginx".into(),
    });
    let mut unresolved = seed(
        EventKind::ObjectAppeared,
        times("2026-08-31T12:00:00Z"),
        "linux.journald",
    );
    unresolved.subject = Some(SpatialRef::Unresolved {
        source: common::source(),
        described: "pid 1842".into(),
    });
    assert_ne!(
        EventId::of(&resolved),
        EventId::of(&unresolved),
        "§5.5: a subject Ono could not reconcile is not the subject it might have been"
    );
}

#[test]
fn should_ignore_the_ingest_time_when_computing_identity() {
    // Two ledgers ingesting one source report at different moments must agree on the id, or
    // §6.8's deduplication cannot work across a restart.
    let base = times("2026-08-31T12:00:00Z");
    let early = EventTimes {
        ingested_at: instant("2026-08-31T12:00:01Z"),
        ..base.clone()
    };
    let late = EventTimes {
        ingested_at: instant("2026-08-31T18:44:02Z"),
        ..base
    };
    let mut a = seed(EventKind::ObjectObserved, early, "linux.procfs");
    a.scope = scope();
    a.provenance = provenance("linux.procfs");
    let mut b = seed(EventKind::ObjectObserved, late, "linux.procfs");
    b.scope = scope();
    b.provenance = provenance("linux.procfs");
    assert_eq!(EventId::of(&a), EventId::of(&b));
}
