//! Provider subscriptions, polled snapshots and what the recorder may claim from each
//! (v0.5 §21, §22.1, §6.1, §6.3, §6.4, §7.1, §30.3).

mod common;

use common::{domain, instant, process_record, procfs_profile, scope};
use ono_provider_api::ObjectEvent;
use ono_recorder::{Normalizer, Redaction, SNAPSHOT_DIFF, SnapshotDiff, kind_of};
use ono_spatial_core::Confidence;
use ono_temporal_core::{EventKind, EvidenceSource, relation_of};

fn normalizer() -> Normalizer {
    Normalizer::new(procfs_profile(), scope(), domain())
}

#[test]
fn should_name_the_evidence_source_class_when_a_provider_event_is_normalized() {
    let record = process_record(1842, "nginx", &["nginx"]);
    let observed = ObjectEvent::added(&record)
        .with_observed_at(instant("2026-08-31T12:00:00Z"))
        .with_sequence(7);

    let event = normalizer().from_provider(&observed, instant("2026-08-31T12:00:01Z"));

    assert_eq!(
        event.provenance.provider(),
        "linux.procfs",
        "§7.1: field-level source attribution reads the provenance provider, and an unrecognised \
         name falls back to `ono.session`"
    );
    assert_eq!(
        EvidenceSource::parse(event.provenance.provider()),
        EvidenceSource::builtin("linux.procfs"),
        "the name is one of §7.1's nine classes"
    );
    assert_eq!(event.kind, EventKind::ObjectAppeared);
    assert_eq!(event.times.source_sequence, Some(7));
    assert_eq!(event.times.ingested_at, instant("2026-08-31T12:00:01Z"));
    assert_eq!(
        event.times.source_time,
        Some(instant("2026-08-31T12:00:00Z"))
    );
}

#[test]
fn should_map_every_provider_event_kind_to_a_canonical_kind() {
    assert_eq!(
        kind_of(ono_provider_api::EventKind::Snapshot),
        EventKind::ObjectObserved
    );
    assert_eq!(
        kind_of(ono_provider_api::EventKind::Added),
        EventKind::ObjectAppeared
    );
    assert_eq!(
        kind_of(ono_provider_api::EventKind::Changed),
        EventKind::ObjectChanged
    );
    assert_eq!(
        kind_of(ono_provider_api::EventKind::Removed),
        EventKind::ObjectDisappeared
    );
}

#[test]
fn should_say_snapshot_diff_when_the_recorder_derived_the_event_itself() {
    let record = process_record(1842, "nginx", &["nginx"]);

    let event = normalizer().from_snapshot_diff(
        EventKind::ObjectAppeared,
        &record,
        instant("2026-08-31T12:00:00Z"),
        instant("2026-08-31T12:00:00Z"),
    );

    assert_eq!(
        event.provenance.source(),
        Some(SNAPSHOT_DIFF),
        "§22.1: provenance MUST say `snapshot_diff` for an event derived by comparing snapshots"
    );
    assert_eq!(event.provenance.provider(), "linux.procfs");
}

#[test]
fn should_report_an_appearance_when_an_object_enters_a_polled_snapshot() {
    let mut differ = SnapshotDiff::new(normalizer());
    let first = instant("2026-08-31T12:00:00Z");
    let _first_round = differ.observe(
        &[process_record(1842, "nginx", &["nginx"])],
        first,
        true,
        first,
    );

    let later = instant("2026-08-31T12:00:05Z");
    let round = differ.observe(
        &[
            process_record(1842, "nginx", &["nginx"]),
            process_record(2741, "worker", &["worker"]),
        ],
        later,
        true,
        later,
    );

    assert_eq!(round.events.len(), 1);
    assert_eq!(round.events[0].kind, EventKind::ObjectAppeared);
    assert_eq!(
        round.coverage.sampling_interval,
        Some(ono_value::Duration::from_nanoseconds(5_000_000_000)),
        "§22.1: coverage MUST reflect polling limitations"
    );
}

#[test]
fn should_withhold_a_disappearance_when_the_source_does_not_make_absence_meaningful() {
    let mut differ = SnapshotDiff::new(normalizer());
    let first = instant("2026-08-31T12:00:00Z");
    let _first_round = differ.observe(
        &[process_record(1842, "nginx", &["nginx"])],
        first,
        true,
        first,
    );

    let later = instant("2026-08-31T12:00:05Z");
    let round = differ.observe(&[], later, true, later);

    assert!(
        round.events.is_empty(),
        "§6.3: a polling gap MUST NOT emit a disappearance unless the provider contract makes \
         missing-from-a-complete-snapshot meaningful"
    );
    assert_eq!(round.withheld_disappearances, 1);
    assert!(
        round.gap.is_some(),
        "§43.2: an explicit gap rather than pretended continuity"
    );
}

#[test]
fn should_report_a_disappearance_when_the_source_contract_makes_absence_meaningful() {
    let profile = procfs_profile().meaningful_disappearance(true);
    let mut differ = SnapshotDiff::new(Normalizer::new(profile, scope(), domain()));
    let first = instant("2026-08-31T12:00:00Z");
    let _first_round = differ.observe(
        &[process_record(1842, "nginx", &["nginx"])],
        first,
        true,
        first,
    );

    let later = instant("2026-08-31T12:00:05Z");
    let round = differ.observe(&[], later, true, later);

    assert_eq!(round.events.len(), 1);
    assert_eq!(round.events[0].kind, EventKind::ObjectDisappeared);
    assert_eq!(round.withheld_disappearances, 0);
    assert!(round.gap.is_none());
}

#[test]
fn should_withhold_a_disappearance_when_a_polling_round_was_missed() {
    let profile = procfs_profile().meaningful_disappearance(true);
    let mut differ = SnapshotDiff::new(Normalizer::new(profile, scope(), domain()));
    let first = instant("2026-08-31T12:00:00Z");
    let _first_round = differ.observe(
        &[process_record(1842, "nginx", &["nginx"])],
        first,
        true,
        first,
    );

    let much_later = instant("2026-08-31T12:04:00Z");
    let round = differ.observe(&[], much_later, true, much_later);

    assert!(
        round.events.is_empty(),
        "§6.3: the recorder was not looking, and its own absence is not the object's"
    );
    assert_eq!(round.withheld_disappearances, 1);
    assert!(round.gap.is_some());
    assert_eq!(
        differ.tracked(),
        1,
        "the object is still held, not forgotten"
    );
}

#[test]
fn should_refuse_a_disappearance_when_the_snapshot_was_not_complete() {
    let profile = procfs_profile().meaningful_disappearance(true);
    let mut differ = SnapshotDiff::new(Normalizer::new(profile, scope(), domain()));
    let first = instant("2026-08-31T12:00:00Z");
    let _first_round = differ.observe(
        &[process_record(1842, "nginx", &["nginx"])],
        first,
        true,
        first,
    );

    let later = instant("2026-08-31T12:00:05Z");
    let round = differ.observe(&[], later, false, later);

    assert!(round.events.is_empty());
    assert_eq!(
        round.coverage.completeness,
        ono_temporal_core::TemporalCompleteness::PointSample,
        "§7.4: a negative claim needs completeness the read did not have"
    );
    assert_eq!(
        round.gap.map(|gap| gap.reason),
        Some(ono_temporal_core::GapReason::ProviderUnavailable)
    );
}

#[test]
fn should_carry_the_relation_and_the_confidence_when_a_relation_event_is_built() {
    let from = ono_recorder::ResolvedSubject::new(
        ono_spatial_core::SpatialIdentity::lifetime(
            ono_spatial_core::SpatialType::Process,
            [("pid", "1842"), ("started", "1")],
        )
        .spatial_id(),
        ono_spatial_core::SpatialType::Process,
        "nginx",
    );
    let to = ono_recorder::ResolvedSubject::new(
        ono_spatial_core::SpatialIdentity::lifetime(
            ono_spatial_core::SpatialType::Socket,
            [("inode", "4711")],
        )
        .spatial_id(),
        ono_spatial_core::SpatialType::Socket,
        ":80",
    );
    let at = instant("2026-08-31T12:00:00Z");

    let event = normalizer().relation(
        true,
        &from,
        &to,
        "process.owns_socket",
        Confidence::Exact,
        at,
        at,
    );

    assert_eq!(event.kind, EventKind::RelationAdded);
    let (relation, confidence) =
        relation_of(&event).expect("§6.4: a relation event names its relation and its confidence");
    assert_eq!(&*relation, "process.owns_socket");
    assert_eq!(confidence, Confidence::Exact);
    assert_eq!(event.related.len(), 1, "both ends travel with the event");
}

#[test]
fn should_redact_before_a_record_becomes_an_event() {
    let normalizer = normalizer().with_redaction(Redaction::new(false));
    let record = process_record(1842, "psql", &["psql", "--password=hunter2"]);
    let at = instant("2026-08-31T12:00:00Z");

    let event = normalizer.from_snapshot_diff(EventKind::ObjectObserved, &record, at, at);

    let rendered = format!("{:?}", event.after);
    assert!(
        !rendered.contains("hunter2"),
        "§30.3: a secret is redacted before persistence, not at render time: {rendered}"
    );
    assert!(
        rendered.contains("Null"),
        "§30.4: raw argv is withheld, and a withheld field reads back as unknown"
    );
}
