//! `get <target>` at a temporal coordinate (spec v0.5 §4.5, §9.4, §9.6, §12.3, §55.2, §55.5,
//! §55.9).
//!
//! §9.6 is the sentence under test: `get process` in historical context "MUST return the best
//! supported process set for `T` and attach collection-level coverage. If the source cannot prove
//! complete enumeration, the result MUST NOT imply that the returned rows are the complete process
//! list."
//!
//! Three outcomes carry that, and each of them is observable from outside the shell:
//!
//! - the rows are the reconstruction's, so a process that is running **now** and was never
//!   recorded does not appear in an answer about **then** (§55.2, §55.9);
//! - the answer states its own coverage, so a reader can tell a proven enumeration from an
//!   unproven one (§9.6, §8.2), and it rides where ADR-0613 put every other piece of §9.4's
//!   temporal metadata — the reserved `ono.temporal` extension key;
//! - a target nothing can reconstruct is refused with a temporal code rather than answered with
//!   an empty list, which would be §55.5's silent gap.
//!
//! The evidence is written into the store the shell opens, because a reconstruction can only be
//! as good as what was recorded.

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

/// The pid the fixture ledger holds. No live process on the host wears it, which is what makes a
/// row carrying it provably reconstructed rather than observed.
const RECORDED_PID: i64 = 4242;

fn scope() -> SpatialScope {
    ono_cli::spatial::local_scope()
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

/// The window the fixture's evidence covers: an hour on either side of now, so a coordinate a
/// couple of minutes back sits inside it however long the shell takes to start.
fn fixture_window() -> (Timestamp, Timestamp) {
    let hour = jiff::Span::new().hours(1);
    let now = Timestamp::now();
    (
        now.checked_sub(hour).expect("an hour before now"),
        now.checked_add(hour).expect("an hour after now"),
    )
}

/// The instant the fixture's process was observed at — five minutes back, inside that window and
/// before any coordinate these tests stand at.
fn observed_at() -> Timestamp {
    let (from, _) = fixture_window();
    from.checked_add(jiff::Span::new().minutes(55))
        .expect("five minutes before now")
}

/// Where `temporal.recording.enabled` puts the store under `home` (§31.1).
fn store_path(home: &Scratch) -> PathBuf {
    home.path().join("data/ono/temporal/ledger.sqlite3")
}

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
fn process_record(pid: i64, name: &str) -> RecordValue {
    let id = SchemaId::new("ono.process", 1);
    let schema = builtin_schemas()
        .get(&id)
        .expect("the contract is embedded");
    RecordValue::builder(schema, Provenance::local("ono.recorder", id))
        .set("pid", Value::Int(i128::from(pid)))
        .expect("a declared field")
        .set("name", Value::string(name))
        .expect("a declared field")
        .set("state", Value::string("running"))
        .expect("a declared field")
        .set("started", Value::Timestamp(fixture_window().0))
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
        provenance: Provenance::local("ono.recorder", SchemaId::new("ono.temporal-event", 1)),
    }
    .seal()
}

/// Coverage of `capability` at `completeness` over `from`..`until`.
fn covering(
    capability: &str,
    completeness: TemporalCompleteness,
    from: Timestamp,
    until: Timestamp,
) -> TemporalCoverage {
    TemporalCoverage {
        scope: scope(),
        capability: Arc::from(capability),
        from,
        until,
        completeness,
        sampling_interval: None,
        source: EvidenceSource::recorder(),
        permission: PermissionState::Available,
    }
}

/// A home whose store holds one process observed five minutes ago, with complete coverage of the
/// class and of the fields the tests read back.
///
/// Complete coverage over the whole window is what §8.2 requires before an enumeration may be
/// called complete, and what §9.2 requires before a field observed at one instant may be held
/// forward to another.
fn home_with_a_recorded_process() -> Scratch {
    let home = ono_testkit::scratch();
    let record = process_record(RECORDED_PID, "fixture-proc");
    let (from, until) = fixture_window();
    recorded(
        &home,
        &[observed(observed_at(), &record, "fixture-proc")],
        &[
            covering(
                "process.existence",
                TemporalCompleteness::Complete,
                from,
                until,
            ),
            covering("process.name", TemporalCompleteness::Complete, from, until),
        ],
    );
    home
}

/// A home whose evidence supports the process it holds without supporting the claim that it held
/// them all.
///
/// The store carries an observation from fifty minutes back, which is what the reconstruction
/// opens its window at, and complete coverage only over the last ten minutes. So the process
/// observed five minutes ago is provably there — the complete stretch spans from its observation
/// to the coordinate — while the window as a whole has a hole in it, and §8.2 forbids reading the
/// resulting list as the whole list.
fn home_with_an_unprovable_enumeration() -> Scratch {
    let home = ono_testkit::scratch();
    let record = process_record(RECORDED_PID, "fixture-proc");
    let (from, until) = fixture_window();
    let recent = from
        .checked_add(jiff::Span::new().minutes(50))
        .expect("ten minutes before now");
    let earlier = from
        .checked_add(jiff::Span::new().minutes(10))
        .expect("fifty minutes before now");
    recorded(
        &home,
        &[
            observed(earlier, &record, "fixture-proc"),
            observed(observed_at(), &record, "fixture-proc"),
        ],
        &[
            covering(
                "process.existence",
                TemporalCompleteness::Complete,
                recent,
                until,
            ),
            covering(
                "process.name",
                TemporalCompleteness::Complete,
                recent,
                until,
            ),
        ],
    );
    home
}

/// The `ono.temporal` metadata of one row, or a panic naming the row that carried none.
fn temporal_of(row: &Yaml) -> Yaml {
    row.get("ono.temporal")
        .unwrap_or_else(|| {
            panic!(
                "v0.5 §9.4, ADR-0613: a reconstructed row carries its temporal metadata under the \
                 reserved `ono.temporal` key. Got {row:?}"
            )
        })
        .clone()
}

/// What a historical answer says, with the instants left out: which objects it holds and whether
/// it claims to hold them all.
///
/// The instants cannot be compared between two runs — two shells resolve `-2m` two milliseconds
/// apart — and they are not what §4.5 is about. What it is about is that the two spellings reach
/// the same evidence and make the same claim over it.
fn answered(run: &Run) -> Vec<(Option<i64>, Option<bool>, String)> {
    support::last_json_rows(run)
        .iter()
        .map(|row| {
            let collection = support::field(&temporal_of(row), "collection");
            (
                row["pid"].as_i64(),
                collection["enumeration_proven"].as_bool(),
                support::text(&collection, "capability"),
            )
        })
        .collect()
}

/// The structured code an output carries, where it carries one.
fn code_of(run: &Run) -> Option<String> {
    run.output()
        .split_whitespace()
        .find(|word| word.starts_with("Ono-Sendai-E"))
        .map(str::to_owned)
}

// --- §55.2, §55.9: the rows are the reconstruction's -------------------------------------------

#[test]
fn should_list_only_what_the_reconstruction_supports_when_get_process_is_asked_at_a_past_instant() {
    // §55.9: "If `at -10m` changes the prompt but `look`/`map` still show present objects, the
    // feature is invalid", and §55.2 prohibits "rendering today's graph with an old timestamp".
    // A process this test started is running right now and nothing recorded it, so a historical
    // `get process` that mentions it consulted the live provider.
    let mut live = support::fixture_process();
    let live_pid = live.id();
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(&home, "at -2m\nget process | to json");
    let _ = live.kill();
    let _ = live.wait();

    let rows = support::last_json_rows(&run);
    let pids: Vec<i64> = rows.iter().filter_map(|row| row["pid"].as_i64()).collect();
    assert_eq!(
        pids,
        vec![RECORDED_PID],
        "v0.5 §9.6, §55.2: the historical process set is what the ledger supports at `T`, and \
         nothing else. Got {:?}",
        run.output()
    );
    assert!(
        !pids.contains(&i64::from(live_pid)),
        "v0.5 §55.2: a process running now that no source recorded cannot appear in an answer \
         about then. Got {:?}",
        run.output()
    );
}

#[test]
fn should_keep_the_objects_own_schema_when_a_process_is_listed_in_the_past() {
    // §28.2: "A historical `Process` remains a `Process` with temporal metadata, not a separate
    // `HistoricalProcess` type." So a pipeline written against the present reads it unchanged.
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(
        &home,
        "at -2m\nget process | where pid == 4242 | select pid name started | to json",
    );
    let rows = support::last_json_rows(&run);
    assert_eq!(
        rows.len(),
        1,
        "v0.5 §28.2: the reconstructed row flows through the same transforms the present uses. \
         Got {:?}",
        run.output()
    );
    assert_eq!(
        support::text(&rows[0], "name"),
        "fixture-proc",
        "v0.5 §9.4: the object keeps its own fields, so `select` over them answers. Got {rows:?}"
    );
    assert!(
        rows[0]["started"].as_str().is_some(),
        "v0.5 §9.4: and its identity fields, which is what makes it the same object. Got {rows:?}"
    );
}

// --- §9.6: the collection states its own coverage ---------------------------------------------

#[test]
fn should_state_that_enumeration_is_unproven_when_the_source_cannot_prove_the_whole_set() {
    // §9.6's second sentence: "If the source cannot prove complete enumeration, the result MUST
    // NOT imply that the returned rows are the complete process list." The evidence supports the
    // one process it saw and has a hole earlier in the window, so the rows are real and the set
    // is not provably whole — and the answer says which of the two it is rather than leaving a
    // row count to be read as a total.
    let home = home_with_an_unprovable_enumeration();
    let run = support::recording_shell(&home, "at -2m\nget process | to json");
    let rows = support::last_json_rows(&run);
    assert!(
        !rows.is_empty(),
        "v0.5 §9.6: the best supported set is still returned where enumeration is unproven. Got \
         {:?}",
        run.output()
    );
    let metadata = temporal_of(&rows[0]);
    let collection = support::field(&metadata, "collection");
    assert_eq!(
        collection["enumeration_proven"].as_bool(),
        Some(false),
        "v0.5 §9.6: a hole in the window means the enumeration cannot be proven complete, and \
         the answer states it rather than implying the rows are the whole list. Got {metadata:?}"
    );
    assert_eq!(
        support::text(&collection, "capability"),
        "process.existence",
        "v0.5 §8.1: the collection names the capability its completeness was read from. Got \
         {collection:?}"
    );
}

#[test]
fn should_state_that_enumeration_is_proven_when_coverage_is_complete_over_the_window() {
    // The other side of §9.6, and §8.2's: complete coverage of the class over the interval is
    // exactly the case in which the returned rows *are* the whole list, and saying so is as much
    // a part of the contract as refusing to say it.
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(&home, "at -2m\nget process | to json");
    let rows = support::last_json_rows(&run);
    let metadata = temporal_of(&rows[0]);
    let collection = support::field(&metadata, "collection");
    assert_eq!(
        collection["enumeration_proven"].as_bool(),
        Some(true),
        "v0.5 §8.2, §9.6: complete coverage of `process.existence` proves the enumeration. Got \
         {metadata:?}"
    );
    assert_eq!(
        metadata["reconstructed"].as_bool(),
        Some(true),
        "v0.5 §9.4: a row built from the ledger says it was reconstructed. Got {metadata:?}"
    );
    assert!(
        !run.stdout().contains("_temporal"),
        "ADR-0613: the metadata is the namespaced key `ono.temporal`, and this tree has no \
         underscore-prefixed field anywhere. Got {:?}",
        run.output()
    );
}

// --- §12.3, §55.5: a target nothing reconstructs is refused ------------------------------------

#[test]
fn should_refuse_rather_than_answer_an_empty_set_when_nothing_reconstructs_the_target() {
    // §55.5 forbids the silent gap, and §8.2 makes "there were none" a claim only complete
    // coverage can support. The fixture covers `process.existence` and nothing else, so a
    // historical `get socket` has no evidence either way — and an empty list would say there were
    // no sockets, which nothing observed.
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(&home, "at -2m\nget socket | count");
    assert_eq!(
        code_of(&run).as_deref(),
        Some("Ono-Sendai-E1302"),
        "v0.5 §12.3, §55.5: no source covers sockets at that instant, so the answer is \
         `temporal.not_recorded` rather than an empty list. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_filesystem_walk_rather_than_read_it_live_when_the_coordinate_is_historical() {
    // §14.5 and ADR-0653: a current reading is not one of the four supports for historical path
    // structure, so `find file` at a past instant may not answer from today's filesystem — with
    // today's `accessed` and `modified` on it — under a `[PAST?]` prompt (§55.2, §55.9). Nothing
    // in the fixture carries the tree, so §34's `temporal.unsupported_source` is the answer, and
    // the refusal says what would have to exist for there to be one.
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(&home, "at -2m\nfind file /etc | to json");
    assert_eq!(
        code_of(&run).as_deref(),
        Some("Ono-Sendai-E1310"),
        "v0.5 §14.5, §55.9: a historical `find file` is reconstructed or refused, never read from \
         the live filesystem. Got {:?}",
        run.output()
    );
    assert!(
        !run.stdout().contains("hostname"),
        "v0.5 §55.2: nothing of today's tree reaches an answer about then. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_target_no_source_can_reconstruct_when_the_coordinate_is_historical() {
    // §21.1: a source answers about the past only where it said it can. A package database is not
    // a kind of thing the reconstruction carries at all, and §34's `temporal.unsupported_source`
    // is the word for that — a different refusal from "nothing was recorded about this one".
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(&home, "at -2m\nget package | count");
    assert_eq!(
        code_of(&run).as_deref(),
        Some("Ono-Sendai-E1310"),
        "v0.5 §21.1, §34: a target the reconstruction cannot carry is refused as an unsupported \
         source, never answered emptily. Got {:?}",
        run.output()
    );
}

// --- §4.5: `--at` is the same engine, spelled per command --------------------------------------

#[test]
fn should_accept_the_at_option_on_get_and_evaluate_at_that_instant() {
    // §4.5: "Read-only Ono-native commands that support historical evaluation SHOULD accept
    // `--at <time-selector>`. This evaluates the command at that time without changing session
    // context." The session never leaves the present here, and the answer is still the past's.
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(&home, "get process --at -2m | to json");
    assert!(
        !run.output().contains("has no option"),
        "v0.5 §4.5: `get` accepts `--at`. Got {:?}",
        run.output()
    );
    let pids: Vec<i64> = support::last_json_rows(&run)
        .iter()
        .filter_map(|row| row["pid"].as_i64())
        .collect();
    assert_eq!(
        pids,
        vec![RECORDED_PID],
        "v0.5 §4.5: `--at` evaluates the command at that instant. Got {:?}",
        run.output()
    );
}

#[test]
fn should_answer_the_same_from_at_context_and_from_the_at_option_when_a_collection_is_asked_for() {
    // §4.5: "`--at` MUST use the same reconstruction engine as `at` context. It MUST NOT implement
    // a separate historical code path." Two observations prove it: the same selector answers with
    // the same rows through both spellings, and an instant neither can reach is refused with the
    // same code through both.
    let home = home_with_an_unprovable_enumeration();
    let with_context = support::recording_shell(&home, "at -2m\nget process | to json");
    let with_option = support::recording_shell(&home, "get process --at -2m | to json");
    assert_eq!(
        answered(&with_context),
        answered(&with_option),
        "v0.5 §4.5: one engine, so both spellings answer the same set and say the same thing \
         about how completely it is known. Got {:?} and {:?}",
        with_context.output(),
        with_option.output()
    );

    let refused_by_context = support::recording_shell(&home, "at -3d\nget process | count");
    let refused_by_option = support::recording_shell(&home, "get process --at -3d | count");
    assert_eq!(
        code_of(&refused_by_context),
        code_of(&refused_by_option),
        "v0.5 §4.5: an instant `at` refuses is one `--at` refuses, with the same code. Got {:?} \
         and {:?}",
        refused_by_context.output(),
        refused_by_option.output()
    );
}

#[test]
fn should_leave_the_session_in_the_present_when_get_was_asked_with_the_at_option() {
    // §4.5's "without changing session context": the option is a coordinate for one command, and
    // the prompt after it carries no past marker.
    let home = home_with_a_recorded_process();
    let run = support::recording_shell(&home, "get process --at -2m | count\nlook");
    assert!(
        !run.output().contains("[PAST"),
        "v0.5 §4.5: `--at` does not move the session. Got {:?}",
        run.output()
    );
}
