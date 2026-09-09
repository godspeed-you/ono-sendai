//! Objects and collections at a historical coordinate (spec v0.5 §9.4, §9.6, §9.7, §28.2).
//!
//! Three sentences of §9 are what these tests hold the shell to, and each of them is about not
//! claiming more than the evidence carries:
//!
//! - **§9.6** — a historical collection "MUST return the best supported process set for `T` and
//!   attach collection-level coverage", and where enumeration cannot be proven the result "MUST
//!   NOT imply that the returned rows are the complete process list".
//! - **§9.4 and §28.2** — a reconstructed object "retains its canonical schema plus temporal
//!   metadata", so a historical `Process` is still an `ono.process/1` and a pipeline written
//!   against the present reads it unchanged. ADR-0613 fixes where the metadata rides: the
//!   reserved extension key `ono.temporal`, because this tree namespaces record extensions with
//!   dotted keys and has no `_temporal` field anywhere.
//! - **§9.7** — a place that did not exist at `T` says `place not known at requested time`, "with
//!   coverage explaining whether this means known-absent or simply unknown", and "MUST NOT
//!   automatically jump elsewhere unless the user asks to navigate".
//!
//! The evidence is written into the store the shell opens, because a reconstruction can only be
//! as good as what was recorded, and a test that seeded nothing would be asking the shell to
//! invent the past.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{PermissionState, Projection, SpatialScope, SpatialType};
use ono_temporal_core::{
    ClockDomain, EventKind, EventSeed, EventTimes, EvidenceSource, LedgerWrite, SpatialRef,
    TemporalCompleteness, TemporalCoverage, TemporalEvent,
};
use ono_temporal_ledger::{Ledger, StoreOptions};
use ono_testkit::{Run, Scratch};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};
use serde_yaml_ng::Value as Yaml;

use support::{field, json, text};

/// The pid of the process the fixture ledger holds, which no live process on the host wears.
const HISTORICAL_PID: i64 = 4242;

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

/// `now` shifted by `minutes`, negative for the past.
fn minutes_from_now(minutes: i64) -> Timestamp {
    Timestamp::now()
        .checked_add(jiff::Span::new().minutes(minutes))
        .expect("an instant minutes from now")
}

/// Where `temporal.recording.enabled` puts the store under `home` (§31.1).
fn store_path(home: &Scratch) -> PathBuf {
    home.path().join("data/ono/temporal/ledger.sqlite3")
}

/// Writes a whole fixture — the events and the coverage that qualifies them — into that store.
fn recorded(home: &Scratch, events: &[TemporalEvent], coverage: &[TemporalCoverage]) {
    let ledger = Ledger::persistent(&StoreOptions::at(&store_path(home)))
        .expect("the store the shell will open");
    ledger
        .append(events, &[])
        .expect("the store accepts the fixture events");
    ledger
        .record_coverage(coverage)
        .expect("the store accepts the fixture coverage");
}

/// An `ono.process/1` as a provider would have reported it.
fn process_record(pid: i64, name: &str, state: &str, started: Timestamp) -> RecordValue {
    let id = SchemaId::new("ono.process", 1);
    let schema = builtin_schemas()
        .get(&id)
        .expect("the contract is embedded");
    RecordValue::builder(schema, Provenance::local("ono.recorder", id))
        .set("pid", Value::Int(i128::from(pid)))
        .expect("a declared field")
        .set("name", Value::string(name))
        .expect("a declared field")
        .set("state", Value::string(state))
        .expect("a declared field")
        .set("started", Value::Timestamp(started))
        .expect("a declared field")
        .build()
}

/// An `object.observed` carrying the archived record, which is what gives a reconstructed object
/// a row rather than only a name.
fn observed(at: Timestamp, record: &RecordValue, label: &str) -> TemporalEvent {
    let id = Projection::new(scope(), at)
        .project_as(record, SpatialType::Process)
        .expect("the fixture record carries an identity")
        .spatial_id()
        .clone();
    EventSeed {
        kind: EventKind::ObjectObserved,
        subtype: None,
        scope: scope(),
        subject: Some(SpatialRef::Resolved {
            id,
            object_type: SpatialType::Process,
            label: Arc::from(label),
        }),
        related: Vec::new(),
        times: times(at),
        before: None,
        after: Some(Value::Record(Arc::new(record.clone()))),
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance(),
    }
    .seal()
}

/// Coverage of `capability` a recorder claims complete over the whole fixture window.
///
/// Complete is the state §7.4 requires before an absence may be claimed, so a fixture that wants
/// `absent` rather than `unknown` has to install it.
fn complete(capability: &str) -> TemporalCoverage {
    TemporalCoverage {
        scope: scope(),
        capability: Arc::from(capability),
        from: minutes_from_now(-30),
        until: minutes_from_now(30),
        completeness: TemporalCompleteness::Complete,
        sampling_interval: None,
        source: EvidenceSource::recorder(),
        permission: PermissionState::Available,
    }
}

/// A home whose store holds one process observed five minutes ago, with the coverage that can
/// prove what else was and was not there.
fn home_with_a_recorded_process() -> Scratch {
    let home = ono_testkit::scratch();
    let record = process_record(
        HISTORICAL_PID,
        "fixture-proc",
        "running",
        minutes_from_now(-30),
    );
    recorded(
        &home,
        &[observed(minutes_from_now(-5), &record, "fixture-proc")],
        &[complete("process.existence")],
    );
    home
}

/// The `PlaceView` a `look --json` printed.
fn view(run: &Run) -> Yaml {
    let text = run.stdout();
    let line = text
        .lines()
        .rfind(|line| line.trim_start().starts_with('{'))
        .unwrap_or_else(|| {
            panic!(
                "v0.4 §6.1: `look --json` prints a place view, got {:?}",
                run.output()
            )
        });
    json(line.trim())
}

// --- §9.4, §28.2: a reconstructed object keeps its schema and gains temporal metadata ---------

#[test]
fn should_keep_the_objects_own_schema_when_a_process_is_looked_at_in_the_past() {
    // §28.2: "A historical `Process` remains a `Process` with temporal metadata, not a separate
    // `HistoricalProcess` type. This preserves pipeline compatibility." So the place the shell
    // describes at `T` answers to `ono.process/1` and carries the process's own fields — the pid
    // it had and the instant it started — rather than a shape invented for the past.
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(
        &home,
        &format!("at -2m\nenter {HISTORICAL_PID}\nlook --json"),
    );
    let place = field(&view(&run), "place");
    assert_eq!(
        text(&place, "object_type"),
        "ono.process/1",
        "v0.5 §28.2: a reconstructed process is an `ono.process/1`, not a second historical type. \
         Got {place:?}; output {:?}",
        run.output()
    );
    assert_eq!(
        field(&place, "provenance.schema").as_str(),
        Some("ono.process/1"),
        "v0.5 §9.4: the object retains its canonical schema, so its provenance names that schema. \
         Got {place:?}"
    );
    assert_eq!(
        place["pid"].as_i64(),
        Some(HISTORICAL_PID),
        "v0.5 §9.4: the reconstructed object keeps its own fields — the pid is the one that was \
         recorded. Got {place:?}"
    );
    assert!(
        place["started"].as_str().is_some(),
        "v0.5 §9.4: and the rest of them, so a reader of the present reads the past. Got {place:?}"
    );
}

#[test]
fn should_carry_the_temporal_metadata_under_the_reserved_extension_key_when_a_place_is_historical()
{
    // §9.4 lists the five members the metadata carries — `as_of`, `coverage`, `reconstructed`,
    // `sources`, `gaps` — and requires that it "MUST NOT collide with provider fields". ADR-0613
    // resolves where it rides: the reserved extension key `ono.temporal`, which cannot collide
    // with a provider field by construction, rather than the `_temporal` field the spec writes
    // and this tree has nowhere.
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(
        &home,
        &format!("at -2m\nenter {HISTORICAL_PID}\nlook --json"),
    );
    let document = view(&run);
    let metadata = document
        .get("ono.temporal")
        .unwrap_or_else(|| {
            panic!(
                "v0.5 §9.4, ADR-0613: a reconstructed object carries its temporal metadata under \
                 the reserved `ono.temporal` key. Got {:?}",
                run.output()
            )
        })
        .clone();
    for member in ["as_of", "coverage", "reconstructed", "sources", "gaps"] {
        assert!(
            metadata.get(member).is_some(),
            "v0.5 §9.4: the temporal metadata carries `{member}`. Got {metadata:?}"
        );
    }
    assert_eq!(
        metadata["reconstructed"].as_bool(),
        Some(true),
        "v0.5 §9.4: a view built from the ledger says it was reconstructed. Got {metadata:?}"
    );
    assert!(
        !run.stdout().contains("_temporal"),
        "ADR-0613's deviation from §9.4: the metadata is the namespaced key `ono.temporal`, and \
         this tree has no underscore-prefixed field anywhere. Got {:?}",
        run.output()
    );
}

// --- §9.6: a historical collection says whether it is the complete set ------------------------

#[test]
fn should_state_the_coverage_behind_a_historical_collection_capability_by_capability() {
    // §9.6 requires collection-level coverage to be attached to a historical answer, and §8.5
    // requires it composed per capability rather than reduced to one global label. Both together
    // are what lets a reader tell what the set they are looking at is backed by — and the gaps
    // are part of that, because §7.5 makes a gap a reportable fact rather than a silence.
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(&home, "at -2m\nlook --json");
    let coverage = field(&view(&run), "ono.temporal.coverage");
    assert!(
        !coverage.is_null(),
        "v0.5 §9.6: a historical view attaches the coverage of the set it describes. Got {:?}",
        run.output()
    );
    assert!(
        !text(&coverage, "headline").is_empty(),
        "v0.5 §8.5: the composition carries the one word a renderer may use. Got {coverage:?}"
    );
    let capabilities = coverage["capabilities"].as_mapping().unwrap_or_else(|| {
        panic!("v0.5 §8.5: coverage is composed per capability, got {coverage:?}")
    });
    assert!(
        capabilities.contains_key(Yaml::from("process.existence")),
        "v0.5 §8.1, §9.6: the capability the process set rests on is named in the composition, so \
         the collection's completeness is stated rather than assumed. Got {coverage:?}"
    );
    assert!(
        coverage["gaps"].as_sequence().is_some(),
        "v0.5 §7.5: the gaps that qualify the set travel with it. Got {coverage:?}"
    );
}

#[test]
fn should_list_only_what_the_reconstruction_supports_when_a_neighbourhood_is_asked_for_at_a_past_instant()
 {
    // §9.6's second sentence, in its strongest form: a historical answer may not be the present
    // one wearing an old timestamp (§55.2). The ledger holds one process and no relations, so the
    // neighbourhood at `T` is what that supports — and it is emphatically not the neighbourhood
    // the same command answers with in the present.
    let home = home_with_a_recorded_process();
    let past = support::recording_shell(&home, "at -2m\nnear | to json");
    let present = support::recording_shell(&home, "near | to json");

    let listed = support::last_json_rows(&past);
    assert!(
        listed.is_empty(),
        "v0.5 §9.6, §55.2: nothing in the ledger supports a neighbour at that instant, so nothing \
         is listed. Got {listed:?}"
    );
    assert!(
        !support::last_json_rows(&present).is_empty(),
        "the same `near` in the present answers from the live index, which is what makes the \
         previous assertion mean something. Got {:?}",
        present.output()
    );
}

// --- §9.7: a place that did not exist at `T` says so and stays where it is --------------------

#[test]
fn should_report_that_a_place_was_not_there_when_coverage_can_prove_its_absence() {
    // §9.7: "If the current v0.4 place did not yet exist at `T`, `look` MUST report `place not
    // known at requested time` with coverage explaining whether this means known-absent or simply
    // unknown." Complete process coverage over the window is what makes it known-absent, and the
    // refusal says which of the two it is rather than leaving the reader to guess.
    let home = home_with_a_recorded_process();
    let mut fixture = support::fixture_process();
    let run = support::recording_shell(&home, &format!("enter {}\nat -2m\nlook", fixture.id()));
    let _ = fixture.kill();
    let _ = fixture.wait();

    let output = run.output();
    assert!(
        output.contains("place not known at requested time"),
        "v0.5 §9.7: the sentence the specification fixes is the one the shell says. Got {output:?}"
    );
    assert!(
        output.contains("temporal.not_recorded"),
        "v0.5 §34: it is a structured refusal, not prose. Got {output:?}"
    );
    assert!(
        output.contains("prove it was not there"),
        "v0.5 §9.7, §7.4: with coverage capable of carrying the claim, the refusal says the place \
         was known to be absent rather than merely unknown. Got {output:?}"
    );
}

#[test]
fn should_report_that_nothing_is_known_either_way_when_no_coverage_could_prove_an_absence() {
    // The other half of §9.7's "known-absent or simply unknown", and §7.4's rule underneath it:
    // an absence is a claim, and a claim needs a source that could have carried it. With events
    // in the ledger but no coverage, the same missing place is unknown, and the refusal says so.
    let home = ono_testkit::scratch();
    let record = process_record(
        HISTORICAL_PID,
        "fixture-proc",
        "running",
        minutes_from_now(-30),
    );
    recorded(
        &home,
        &[observed(minutes_from_now(-5), &record, "fixture-proc")],
        &[],
    );
    let mut fixture = support::fixture_process();
    let run = support::recording_shell(&home, &format!("enter {}\nat -2m\nlook", fixture.id()));
    let _ = fixture.kill();
    let _ = fixture.wait();

    let output = run.output();
    assert!(
        output.contains("place not known at requested time"),
        "v0.5 §9.7: the same sentence answers both cases. Got {output:?}"
    );
    assert!(
        output.contains("nothing is known either way"),
        "v0.5 §9.7, §7.4: without coverage that could prove an absence, the refusal reports an \
         unknown rather than claiming the place was gone. Got {output:?}"
    );
    assert!(
        !output.contains("prove it was not there"),
        "v0.5 §7.4: an absence nothing could have observed is never reported as proven. Got \
         {output:?}"
    );
}

#[test]
fn should_stay_where_it_is_when_the_place_was_not_known_at_the_requested_time() {
    // §9.7: "It MUST NOT automatically jump elsewhere unless the user asks to navigate." The
    // refusal is an answer, not a move: returning to the present finds the session standing
    // exactly where it stood before it asked about the past.
    let home = home_with_a_recorded_process();
    let mut fixture = support::fixture_process();
    let pid = fixture.id();
    let run = support::recording_shell(
        &home,
        &format!("enter {pid}\nat -2m\nlook\nnow\nlook --json"),
    );
    let _ = fixture.kill();
    let _ = fixture.wait();

    let output = run.output();
    assert!(
        output.contains("place not known at requested time"),
        "v0.5 §9.7: the refusal is what this test is about. Got {output:?}"
    );
    let place = field(&view(&run), "place");
    assert_eq!(
        text(&place, "spatial_type"),
        "Process",
        "v0.5 §9.7: the shell navigated nowhere on its own, so the place after `now` is still the \
         process that was entered — not the root it could have fallen back to. Got {place:?}; \
         output {output:?}"
    );
    assert_eq!(
        place["pid"].as_u64(),
        Some(u64::from(pid)),
        "v0.5 §9.7: and it is that process, not another one. Got {place:?}"
    );
}
