//! `changes --since` at the shell's boundary (spec v0.5 §13.1, §13.2, §13.4, §28.1).
//!
//! §13.2 fixes the answer: `Stream<TemporalChange>` in five canonical classes — `added`,
//! `removed`, `changed`, `relation_added`, `relation_removed` — each an `ono.temporal-change/1`
//! rather than a rendered diff. §28.1 fixes what follows from that: a change is an ordinary Ono
//! value, so `changes --since 30m | group subject.object_type` is a pipeline like any other.
//!
//! The evidence these tests run against is written into the store the shell opens, because that
//! is the only honest way to ask the question through the binary: v0.5's answer is what was
//! recorded, and a shell that observed nothing must answer with nothing (§13.4, §55.5). Each test
//! therefore lays down a ledger, runs the real `ono`, and reads the JSON it printed.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{Confidence, PermissionState, Projection, SpatialScope, SpatialType};
use ono_temporal_core::{
    ChangeCertainty, ClockDomain, EventKind, EventSeed, EventTimes, EvidenceSource, FieldChange,
    LedgerWrite, SpatialRef, TemporalCompleteness, TemporalCoverage, TemporalEvent,
};
use ono_temporal_ledger::{Ledger, StoreOptions};
use ono_testkit::Scratch;
use ono_value::{Provenance, SchemaId, Value};
use serde_yaml_ng::Value as Yaml;

use support::{rows, text};

/// The scope the shell compares in — the host it runs on, as the shell itself reads it.
fn scope() -> SpatialScope {
    ono_cli::spatial::local_scope()
}

fn provenance() -> Provenance {
    Provenance::local("ono.recorder", SchemaId::new("ono.temporal-event", 1))
}

fn times(at: Timestamp) -> EventTimes {
    EventTimes {
        source_time: Some(at),
        observed_at: at,
        ingested_at: at,
        source_sequence: None,
        monotonic_nanos: None,
        clock_uncertainty: None,
        domain: ClockDomain::new(scope().host_scope().id(), None),
    }
}

/// A resolved reference to a fixture object, with an identity derived in this scope.
fn subject(object_type: SpatialType, key: &str, label: &str) -> SpatialRef {
    let id = Projection::new(scope(), Timestamp::now())
        .derive(
            object_type,
            SchemaId::new("ono.process", 1),
            "key",
            key,
            provenance(),
        )
        .spatial_id()
        .clone();
    SpatialRef::Resolved {
        id,
        object_type,
        label: Arc::from(label),
    }
}

fn lifecycle(kind: EventKind, at: Timestamp, about: SpatialRef) -> TemporalEvent {
    EventSeed {
        kind,
        subtype: None,
        scope: scope(),
        subject: Some(about),
        related: Vec::new(),
        times: times(at),
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance(),
    }
    .seal()
}

/// An `object.changed` carrying one typed field transition (§6.2).
fn field_moved(
    at: Timestamp,
    about: SpatialRef,
    field: &str,
    before: Value,
    after: Value,
) -> TemporalEvent {
    EventSeed {
        kind: EventKind::ObjectChanged,
        subtype: None,
        scope: scope(),
        subject: Some(about),
        related: Vec::new(),
        times: times(at),
        before: None,
        after: None,
        changed_fields: vec![FieldChange {
            field: Arc::from(field),
            before: Some(before),
            after: Some(after),
            certainty: ChangeCertainty::Observed,
        }],
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance(),
    }
    .seal()
}

fn relation(kind: EventKind, at: Timestamp, from: SpatialRef, to: SpatialRef) -> TemporalEvent {
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
            "service.controls_process",
            Confidence::Exact,
        )),
        provenance: provenance(),
    }
    .seal()
}

fn coverage(from: Timestamp, until: Timestamp, capability: &str) -> TemporalCoverage {
    TemporalCoverage {
        scope: scope(),
        capability: Arc::from(capability),
        from,
        until,
        completeness: TemporalCompleteness::Complete,
        sampling_interval: None,
        source: EvidenceSource::recorder(),
        permission: PermissionState::Available,
    }
}

/// Where `temporal.recording.enabled` puts the store under `home` (§31.1).
fn store_path(home: &Scratch) -> std::path::PathBuf {
    home.path().join("data/ono/temporal/ledger.sqlite3")
}

/// The store the shell opens under `home`, as `temporal.recording.enabled` makes it.
fn store_of(home: &Scratch) -> Ledger {
    Ledger::persistent(&StoreOptions::at(&store_path(home))).expect("the store opens")
}

/// `now` minus `minutes`.
fn minutes_ago(minutes: i64) -> Timestamp {
    Timestamp::now()
        .checked_sub(jiff::Span::new().minutes(minutes))
        .expect("an instant minutes back")
}

/// Writes one of each of §13.2's five change classes into the store `home`'s shell will open.
///
/// The instants are minutes back from now rather than fixed, because `temporal.retention.max_age`
/// is 24h and a `--since` outside it is refused (§10.4, §34): the fixture has to be recent for the
/// question to be answerable at all.
fn seed_five_classes(home: &Scratch) {
    let ledger = store_of(home);

    let arrived = subject(SpatialType::Process, "fixture-arrived", "backup");
    let departed = subject(SpatialType::Process, "fixture-departed", "old-backup");
    let moved = subject(SpatialType::Service, "fixture-moved", "backup.service");
    let holder = subject(SpatialType::Service, "fixture-holder", "nginx.service");
    let held = subject(SpatialType::Process, "fixture-held", "nginx");
    let dropped = subject(SpatialType::Process, "fixture-dropped", "nginx-worker");

    let events = vec![
        lifecycle(EventKind::ObjectAppeared, minutes_ago(4), arrived),
        lifecycle(EventKind::ObjectDisappeared, minutes_ago(4), departed),
        field_moved(
            minutes_ago(3),
            moved,
            "state",
            Value::string("running"),
            Value::string("failed"),
        ),
        relation(
            EventKind::RelationAdded,
            minutes_ago(2),
            holder.clone(),
            held,
        ),
        relation(EventKind::RelationRemoved, minutes_ago(2), holder, dropped),
    ];
    ledger
        .append(&events, &[])
        .expect("the store accepts the fixture events");
    let now = Timestamp::now();
    ledger
        .record_coverage(&[
            coverage(minutes_ago(10), now, "process.existence"),
            coverage(minutes_ago(10), now, "service.existence"),
            coverage(minutes_ago(10), now, "relation:service.controls_process"),
        ])
        .expect("the store accepts the fixture coverage");
}

/// The `kind` of every change the shell printed.
fn kinds(document: &[Yaml]) -> Vec<String> {
    document.iter().map(|row| text(row, "kind")).collect()
}

#[test]
fn should_report_every_canonical_change_class_when_the_ledger_holds_one_of_each() {
    // §13.2: "Canonical change classes: added, removed, changed, relation_added,
    // relation_removed." All five belong to one answer, and each of them is a value.
    let home = ono_testkit::scratch();
    seed_five_classes(&home);
    let run = support::recording_shell(&home, "changes --since 10m | to json");
    let document = rows(&run);
    let seen = kinds(&document);
    for class in [
        "added",
        "removed",
        "changed",
        "relation_added",
        "relation_removed",
    ] {
        assert!(
            seen.iter().any(|kind| kind == class),
            "v0.5 §13.2: `changes --since` reports the `{class}` class. Got {seen:?}; output {:?}",
            run.output()
        );
    }
}

#[test]
fn should_answer_with_the_temporal_change_schema_when_changes_are_reported() {
    // §13.2's output type is `Stream<TemporalChange>`, and §35.4 says what a `TemporalChange` is:
    // `change_id`, `kind`, `subject`, both ends of the window, the field changes and the coverage
    // that qualifies them. A row missing one of those is a rendering rather than a typed value.
    let home = ono_testkit::scratch();
    seed_five_classes(&home);
    let run = support::recording_shell(&home, "changes --since 10m | to json");
    let document = rows(&run);
    assert!(
        !document.is_empty(),
        "v0.5 §13.2: the seeded window holds changes. Got {:?}",
        run.output()
    );
    for row in &document {
        for field in [
            "change_id",
            "kind",
            "subject",
            "from_time",
            "to_time",
            "field_changes",
            "coverage",
        ] {
            assert!(
                !row[field].is_null(),
                "v0.5 §35.4: an `ono.temporal-change/1` carries `{field}` on every row, got \
                 {row:?}"
            );
        }
        assert!(
            row["from_time"].as_str().is_some() && row["to_time"].as_str().is_some(),
            "v0.5 §13.1: a change names both ends of the window it compares, got {row:?}"
        );
        assert!(
            !text(&row["subject"], "object_type").is_empty(),
            "v0.5 §35.4: the subject is the canonical identity with its type, not a label, got \
             {row:?}"
        );
    }
}

#[test]
fn should_report_a_field_transition_as_two_typed_sides_when_an_object_changed() {
    // §13.2's `changed` class carries §6.2's typed field changes rather than a sentence, so a
    // reader gets the two sides as values. §13.4 adds the certainty that says how well each side
    // is known — `observed` only where something observed it.
    let home = ono_testkit::scratch();
    seed_five_classes(&home);
    let run = support::recording_shell(&home, "changes --since 10m | to json");
    let document = rows(&run);
    let changed = document
        .iter()
        .find(|row| text(row, "kind") == "changed")
        .unwrap_or_else(|| panic!("v0.5 §13.2: a `changed` row, got {:?}", run.output()));
    let fields = changed["field_changes"]
        .as_sequence()
        .unwrap_or_else(|| panic!("v0.5 §6.2: `field_changes` is a list, got {changed:?}"));
    let state = fields
        .iter()
        .find(|field| text(field, "field") == "state")
        .unwrap_or_else(|| panic!("v0.5 §6.2: the field that moved is named, got {fields:?}"));
    assert_eq!(
        state["before"].as_str(),
        Some("running"),
        "v0.5 §6.2: the earlier side is the value that was observed, got {state:?}"
    );
    assert_eq!(
        state["after"].as_str(),
        Some("failed"),
        "v0.5 §6.2: the later side is the value that was observed, got {state:?}"
    );
    assert_eq!(
        state["certainty"].as_str(),
        Some("observed"),
        "v0.5 §13.4: a side something observed is `observed`; only an unobserved one is \
         `unknown`, and neither is ever fabricated. Got {state:?}"
    );
}

#[test]
fn should_carry_the_edge_that_moved_when_a_relation_changed() {
    // §13.2 keeps the two relation classes apart from the object classes because §6.4 does: the
    // value carries the edge itself — both ends and the relation type — rather than folding it
    // into a field change.
    let home = ono_testkit::scratch();
    seed_five_classes(&home);
    let run = support::recording_shell(&home, "changes --since 10m | to json");
    let document = rows(&run);
    let added = document
        .iter()
        .find(|row| text(row, "kind") == "relation_added")
        .unwrap_or_else(|| panic!("v0.5 §13.2: a `relation_added` row, got {:?}", run.output()));
    assert!(
        !added["relation"].is_null(),
        "v0.5 §6.4, §35.4: a relation change carries the edge that changed, got {added:?}"
    );
    assert_eq!(
        text(&added["relation"], "relation"),
        "service.controls_process",
        "v0.5 §6.4: the edge names its relation type, got {added:?}"
    );
    assert!(
        added["field_changes"]
            .as_sequence()
            .is_some_and(Vec::is_empty),
        "v0.5 §35.4: a relation change has no field changes — the edge is the change, got \
         {added:?}"
    );
}

#[test]
fn should_group_changes_by_a_field_of_the_subject_when_piped_into_group() {
    // §28.1's own example: `changes --since 30m | group subject.object_type`. A change is an
    // ordinary value, so an ordinary stage reads a field of it — no temporal-specific syntax, and
    // nothing rendered in between.
    let home = ono_testkit::scratch();
    seed_five_classes(&home);
    let run = support::recording_shell(
        &home,
        "changes --since 30m | group subject.object_type | to json",
    );
    let document = rows(&run);
    assert!(
        !document.is_empty(),
        "v0.5 §28.1: `changes --since 30m | group subject.object_type` answers as a pipeline. \
         Got {:?}",
        run.output()
    );
    let keys: Vec<String> = document
        .iter()
        .filter_map(|row| row["key"].as_str().map(str::to_owned))
        .collect();
    assert!(
        keys.iter().any(|key| key == "Process"),
        "v0.5 §28.1: `group` follows `subject.object_type` into the subject, so the process \
         changes land under v0.4's `Process`. Got {keys:?}; output {:?}",
        run.output()
    );
    assert!(
        keys.iter().any(|key| key == "Service"),
        "v0.5 §28.1: and the service changes land under `Service`. Got {keys:?}"
    );
}

#[test]
fn should_stay_a_stream_a_later_stage_can_filter_when_changes_are_piped() {
    // §28.1 again, through `where`: the class is a readable field on the value, so a stage that
    // has never heard of the temporal engine narrows the answer.
    let home = ono_testkit::scratch();
    seed_five_classes(&home);
    let run = support::recording_shell(
        &home,
        "changes --since 10m | where kind == \"added\" | to json",
    );
    let document = rows(&run);
    assert!(
        !document.is_empty(),
        "v0.5 §28.1: a `where` over `changes` is an ordinary pipeline stage. Got {:?}",
        run.output()
    );
    assert!(
        document.iter().all(|row| text(row, "kind") == "added"),
        "v0.5 §28.1: the filter selected on the value's own field, got {:?}",
        kinds(&document)
    );
}

#[test]
fn should_leave_out_a_change_that_happened_before_the_window_when_since_is_narrowed() {
    // §13.1: the window is the question. A comparison that answered with changes from outside it
    // would be reporting about an interval the user did not ask about.
    let home = ono_testkit::scratch();
    seed_five_classes(&home);
    let run = support::recording_shell(&home, "changes --since 1m | to json");
    let document = rows(&run);
    assert!(
        document.is_empty(),
        "v0.5 §13.1: every seeded change is at least two minutes old, so a one-minute window \
         holds none of them. Got {:?}",
        kinds(&document)
    );
}

#[test]
fn should_refuse_rather_than_invent_a_window_when_since_is_omitted() {
    // §13.1: `changes [selector] --since <time-selector>`. A comparison needs two ends, and the
    // earlier one is not the shell's to guess.
    let home = ono_testkit::scratch();
    let run = support::recording_shell(&home, "changes");
    assert!(
        run.output().contains("--since"),
        "v0.5 §13.1: `changes` without `--since` names the missing end rather than comparing \
         against an invented one. Got {:?}",
        run.output()
    );
}
