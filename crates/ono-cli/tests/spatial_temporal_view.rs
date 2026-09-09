//! The map view's temporal cursor: pause, step, gap and return to now (spec v0.5 §18).
//!
//! The PTY suite proves the keys reach the view on a real screen. These are the outcomes that a
//! screen cannot show cheaply: that pausing freezes the view and nothing else, that `[` steps to
//! a significant event rather than to the next provider sample, that a cursor inside a coverage
//! gap has a gap to show, and that returning to now summarises through the canonical `changes`
//! engine rather than through a second comparison.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_cli::spatial::TemporalCursor;
use ono_spatial_core::{
    BootIdentity, PermissionState, Projection, SpatialId, SpatialScope, SpatialType,
};
use ono_temporal_core::{
    EventKind, EventSeed, EventTimes, EvidenceSource, LedgerRead, LedgerWrite, SessionLedger,
    SpatialRef, TemporalCompleteness, TemporalCoverage, TemporalEvent,
};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

fn scope() -> SpatialScope {
    SpatialScope::host(
        "testbox",
        BootIdentity::new("testbox", "4d0a1f2b-0000-4000-8000-000000000001"),
    )
}

fn instant(text: &str) -> Timestamp {
    text.parse().expect("the fixture instant parses")
}

fn process_record(pid: i64, name: &str, started: &str, state: &str) -> RecordValue {
    let id = SchemaId::new("ono.process", 1);
    let schema = builtin_schemas()
        .get(&id)
        .expect("the contract is embedded");
    RecordValue::builder(schema, Provenance::local("fixture", id))
        .set("pid", Value::Int(i128::from(pid)))
        .expect("pid")
        .set("name", Value::string(name))
        .expect("name")
        .set("started", Value::Timestamp(instant(started)))
        .expect("started")
        .set("state", Value::string(state))
        .expect("state")
        .build()
}

fn identity_of(record: &RecordValue, at: &str) -> SpatialId {
    Projection::new(scope(), instant(at))
        .project_as(record, SpatialType::Process)
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

fn event(
    kind: EventKind,
    at: &str,
    id: &SpatialId,
    label: &str,
    archived: Option<&RecordValue>,
) -> TemporalEvent {
    EventSeed {
        kind,
        subtype: None,
        scope: scope(),
        subject: Some(SpatialRef::Resolved {
            id: id.clone(),
            object_type: SpatialType::Process,
            label: Arc::from(label),
        }),
        related: Vec::new(),
        times: times(at),
        before: None,
        after: archived.map(|record| Value::Record(Arc::new(record.clone()))),
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: Provenance::local("ono.recorder", SchemaId::new("ono.temporal-event", 1)),
    }
    .seal()
}

fn coverage(capability: &str, from: &str, until: &str) -> TemporalCoverage {
    TemporalCoverage {
        scope: scope(),
        capability: Arc::from(capability),
        from: instant(from),
        until: instant(until),
        completeness: TemporalCompleteness::Complete,
        sampling_interval: None,
        source: EvidenceSource::recorder(),
        permission: PermissionState::Available,
    }
}

/// A ledger with one process observed repeatedly and one that appeared at 12:20.
fn ledger() -> (Arc<SessionLedger>, SpatialId, SpatialId) {
    let ledger = Arc::new(SessionLedger::new());
    let held = process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running");
    let held_id = identity_of(&held, "2026-08-31T12:00:00Z");
    let arriving = process_record(4711, "worker", "2026-08-31T12:19:00Z", "running");
    let arriving_id = identity_of(&arriving, "2026-08-31T12:20:00Z");
    ledger
        .append(
            &[
                event(
                    EventKind::ObjectObserved,
                    "2026-08-31T11:30:00Z",
                    &held_id,
                    "nginx",
                    Some(&held),
                ),
                event(
                    EventKind::ObjectObserved,
                    "2026-08-31T12:00:00Z",
                    &held_id,
                    "nginx",
                    Some(&held),
                ),
                // A provider sample that changed nothing: §18.4 says `[` and `]` do not step to
                // one of these.
                event(
                    EventKind::ObjectObserved,
                    "2026-08-31T12:10:00Z",
                    &held_id,
                    "nginx",
                    Some(&held),
                ),
                event(
                    EventKind::ObjectAppeared,
                    "2026-08-31T12:20:00Z",
                    &arriving_id,
                    "worker",
                    Some(&arriving),
                ),
            ],
            &[],
        )
        .expect("the ledger accepts");
    (ledger, held_id, arriving_id)
}

fn cursor_at(ledger: &Arc<SessionLedger>, at: Option<&str>) -> TemporalCursor {
    let handle: Arc<dyn LedgerRead> = Arc::clone(ledger) as Arc<dyn LedgerRead>;
    TemporalCursor::over(scope(), Some(handle), at.map(instant))
}

#[test]
fn should_freeze_the_view_and_leave_the_ledger_ingesting_when_the_cursor_is_paused() {
    // §18.2: "Pausing does NOT stop providers, the recorder or the real system. It freezes only
    // the view's temporal cursor." The ledger keeps accepting events after the pause, and the
    // instant the view is showing does not move because of them.
    let (ledger, held_id, _) = ledger();
    // §9.2: a reading before `T` holds at `T` only where a source covered the stretch between.
    // The recorder covered the whole afternoon here, so the frozen instant has a world to show.
    ledger
        .record_coverage(&[coverage(
            "process.existence",
            "2026-08-31T11:00:00Z",
            "2026-08-31T13:00:00Z",
        )])
        .expect("the ledger accepts coverage");
    let mut cursor = cursor_at(&ledger, None);
    assert_eq!(cursor.at(), None, "a fresh cursor follows the present");

    let frozen = instant("2026-08-31T12:15:00Z");
    cursor.toggle_pause(frozen);
    assert!(cursor.is_paused());
    assert_eq!(cursor.at(), Some(frozen));

    let later = process_record(9999, "late", "2026-08-31T12:40:00Z", "running");
    let later_id = identity_of(&later, "2026-08-31T12:40:00Z");
    ledger
        .append(
            &[event(
                EventKind::ObjectAppeared,
                "2026-08-31T12:40:00Z",
                &later_id,
                "late",
                Some(&later),
            )],
            &[],
        )
        .expect("the ledger keeps ingesting while the view is paused");

    assert_eq!(
        cursor.at(),
        Some(frozen),
        "ingestion moved the view's cursor, which §18.2 forbids"
    );
    let world = cursor
        .world()
        .expect("the reconstruction runs")
        .expect("a paused cursor has a world");
    assert!(
        world.index().contains(&held_id),
        "the frozen instant lost the process that was there"
    );
    assert!(
        !world.index().contains(&later_id),
        "an object ingested after the pause appeared in the frozen view"
    );

    cursor.toggle_pause(instant("2026-08-31T12:50:00Z"));
    assert!(!cursor.is_paused());
    assert_eq!(
        cursor.at(),
        None,
        "releasing the pause did not return the view to the session's own coordinate"
    );
}

#[test]
fn should_step_to_a_significant_event_rather_than_to_a_provider_sample() {
    // §18.4: "`[` and `]` step through events relevant to the visible map horizon, not every raw
    // provider event." The 12:10 observation changed nothing; the 12:20 appearance is a node
    // lifecycle event, which is the first of §18.4's priority list.
    let (ledger, held_id, arriving_id) = ledger();
    let mut cursor = cursor_at(&ledger, Some("2026-08-31T12:05:00Z"));

    let stepped = cursor
        .step(
            &held_id,
            vec![arriving_id.clone()],
            true,
            instant("2026-08-31T13:00:00Z"),
        )
        .expect("the step runs")
        .expect("there is a significant event ahead");
    assert_eq!(stepped.kind, EventKind::ObjectAppeared);
    assert_eq!(
        cursor.at(),
        Some(instant("2026-08-31T12:20:00Z")),
        "the cursor landed on the provider sample rather than on the appearance"
    );

    let back = cursor
        .step(
            &held_id,
            vec![arriving_id],
            false,
            instant("2026-08-31T13:00:00Z"),
        )
        .expect("the step runs");
    assert!(
        back.is_none(),
        "stepping back reached a raw provider sample: {back:?}"
    );
}

#[test]
fn should_have_a_gap_to_show_when_the_cursor_stands_inside_one() {
    // §18.6: "If the user steps into a gap, the view MUST display the gap explicitly", and the
    // map "MUST NOT continue showing the last state with a silently advancing timestamp".
    let (ledger, _, _) = ledger();
    ledger
        .record_coverage(&[coverage(
            "process.existence",
            "2026-08-31T11:00:00Z",
            "2026-08-31T12:00:00Z",
        )])
        .expect("the ledger accepts coverage");

    let cursor = cursor_at(&ledger, Some("2026-08-31T12:05:00Z"));
    let world = cursor
        .world()
        .expect("the reconstruction runs")
        .expect("a historical cursor has a world");
    let gap = cursor
        .gap_in(&world)
        .expect("the cursor is standing in the uncovered stretch");
    assert_eq!(
        gap.from,
        instant("2026-08-31T12:00:00Z"),
        "the gap opens where the recorder's coverage ended"
    );
    assert_eq!(
        gap.until,
        instant("2026-08-31T12:05:00Z"),
        "the gap runs up to the instant the cursor asked about"
    );

    let frame = TemporalCursor::gap_frame(gap, 60).join("\n");
    assert!(frame.contains("HISTORY GAP"), "got:\n{frame}");
    assert!(
        frame.contains("last supported state shown at"),
        "§18.6's frame names the last instant anything was known, got:\n{frame}"
    );
}

#[test]
fn should_summarise_through_the_changes_engine_when_the_cursor_returns_to_now() {
    // §18.7: "This summary uses the canonical `changes` engine." One implementation, asked here
    // and by `look`'s change section alike (§13.5).
    let (ledger, _, _) = ledger();
    let mut cursor = cursor_at(&ledger, None);
    cursor.toggle_pause(instant("2026-08-31T12:05:00Z"));

    let changed = cursor
        .return_to_now(instant("2026-08-31T13:00:00Z"))
        .expect("the summary runs");
    assert!(
        changed
            .iter()
            .any(|change| change.class == ono_temporal_core::ChangeClass::Added),
        "the process that appeared at 12:20 is missing from the return-to-now summary"
    );
    assert_eq!(cursor.at(), None, "`N` did not return the view to now");

    let lines = TemporalCursor::summary_lines(&changed, 60).join("\n");
    assert!(
        lines.contains("returned to now"),
        "§18.7's own heading is missing, got:\n{lines}"
    );
}

#[test]
fn should_report_no_evidence_rather_than_an_empty_past_when_no_ledger_is_installed() {
    // §2.17: "nothing recorded" and "nothing happened" are two answers. A cursor with no ledger
    // behind it steps nowhere and says the first.
    let mut cursor = TemporalCursor::over(scope(), None, None);
    assert!(!cursor.has_evidence());
    let stepped = cursor
        .step(
            &ono_spatial_core::space::root().spatial_id(),
            Vec::new(),
            true,
            instant("2026-08-31T13:00:00Z"),
        )
        .expect("a cursor with no ledger still answers");
    assert!(stepped.is_none());
    assert!(
        cursor
            .world()
            .expect("a cursor with no ledger still answers")
            .is_none()
    );
}
