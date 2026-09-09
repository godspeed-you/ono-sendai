//! v0.5 §56.6 and `docs/ACCEPTANCE.md` §4.11.8: events appended before a close are queryable
//! after a reopen, with every field they were written with.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_temporal_core::{CoverageQuery, EventKind, EventQuery, LedgerRead, LedgerWrite, TimeRange};
use ono_temporal_ledger::LedgerStore;

use common::{
    action, checkpoint, coverage, event, event_about, evidence_for, instant, link, path_in, scope,
    store_in,
};

#[test]
fn should_answer_with_the_same_events_when_the_store_is_closed_and_reopened() {
    let home = tempfile::tempdir().expect("a temporary home");
    let written = {
        let store = store_in(home.path());
        let events = vec![
            event_about(EventKind::ObjectAppeared, "2026-08-31T14:03:11Z", 1827),
            event_about(EventKind::ObjectChanged, "2026-08-31T14:03:13Z", 1827),
        ];
        store.append(&events, &[]).expect("an append succeeds");
        store.flush().expect("a flush succeeds");
        events
    };

    let store = LedgerStore::open(&path_in(home.path())).expect("the store reopens");
    let read = store
        .events(&EventQuery::in_range(TimeRange::all()))
        .expect("events answer");
    assert_eq!(read, written, "§56.6: events survive a restart intact");
}

#[test]
fn should_answer_with_the_same_evidence_links_coverage_actions_and_checkpoints_after_a_reopen() {
    let home = tempfile::tempdir().expect("a temporary home");
    let cause = event(
        EventKind::ActionExecuted,
        "2026-08-31T14:03:11Z",
        "ono.session",
    );
    let effect = event(
        EventKind::ObjectChanged,
        "2026-08-31T14:03:13Z",
        "linux.procfs",
    );
    let evidence = evidence_for("2026-08-31T14:03:13Z", 1827, "active");
    let causal = link(&cause, &effect, &evidence);
    let interval = coverage("2026-08-31T14:00:00Z", "2026-08-31T14:10:00Z");
    let recorded = action(
        "2026-08-31T14:03:11Z",
        ono_temporal_core::RedactedCommandSummary::of("restart", Some("service"), &[]),
    );
    let snapshot = checkpoint("2026-08-31T14:02:00Z");

    {
        let store = store_in(home.path());
        store
            .append(
                &[cause.clone(), effect.clone()],
                std::slice::from_ref(&evidence),
            )
            .expect("an append succeeds");
        store
            .append_links(std::slice::from_ref(&causal))
            .expect("links append");
        store
            .record_coverage(std::slice::from_ref(&interval))
            .expect("coverage records");
        store.record_action(&recorded).expect("an action records");
        store
            .write_checkpoint(&snapshot)
            .expect("a checkpoint writes");
        store.flush().expect("a flush succeeds");
    }

    let store = LedgerStore::open(&path_in(home.path())).expect("the store reopens");
    assert_eq!(
        store
            .evidence(std::slice::from_ref(&evidence.evidence_id))
            .expect("evidence answers"),
        vec![evidence]
    );
    assert_eq!(
        store.causal_links(&effect.event_id).expect("links answer"),
        vec![causal]
    );
    assert_eq!(
        store
            .coverage(&CoverageQuery::default())
            .expect("coverage answers"),
        vec![interval]
    );
    assert_eq!(
        store.actions(TimeRange::all()).expect("actions answer"),
        vec![recorded]
    );
    assert_eq!(
        store
            .checkpoint_before(&scope(), instant("2026-08-31T14:03:00Z"))
            .expect("checkpoints answer"),
        Some(snapshot)
    );
}

#[test]
fn should_report_the_retained_boundary_when_a_reopened_store_is_asked() {
    let home = tempfile::tempdir().expect("a temporary home");
    {
        let store = store_in(home.path());
        store
            .append(
                &[
                    event(
                        EventKind::ObjectObserved,
                        "2026-08-31T14:00:00Z",
                        "linux.procfs",
                    ),
                    event(
                        EventKind::ObjectObserved,
                        "2026-08-31T14:30:00Z",
                        "linux.procfs",
                    ),
                ],
                &[],
            )
            .expect("an append succeeds");
    }
    let store = LedgerStore::open(&path_in(home.path())).expect("the store reopens");
    let retention = store.retention();
    assert_eq!(retention.earliest, Some(instant("2026-08-31T14:00:00Z")));
    assert_eq!(retention.latest, Some(instant("2026-08-31T14:30:00Z")));
    assert_eq!(retention.events, 2);
    assert!(
        retention.stored_size.is_some_and(|size| size.bytes() > 0),
        "a persistent store knows how much space it occupies (§10.4)"
    );
}

/// §31.4: "unknown future fields MUST be handled according to Ono schema evolution rules", which
/// starts with the encoding reading back exactly what it wrote. A provider payload may hold any
/// value, so the round trip is over one of every kind.
#[test]
fn should_read_back_every_kind_of_value_when_a_payload_carried_one() {
    use std::sync::Arc;

    use ono_value::{
        ByteSize, Decimal, Duration, IpNetwork, MapValue, Percent, RegexValue, Uuid, Value,
    };

    let mut map = MapValue::new();
    map.insert(Arc::from("nested"), Value::Bool(true));
    let every = vec![
        Value::Null,
        Value::Bool(false),
        Value::Int(i128::MIN),
        Value::Int(i128::MAX),
        Value::Float(-0.5),
        Value::Decimal(Decimal::parse("1.25").expect("a decimal literal")),
        Value::string("text with \u{1f} separators"),
        Value::Bytes(vec![0xff, 0x00, 0x80].into()),
        Value::Path(Arc::from(std::path::Path::new("/proc/1/cmdline"))),
        Value::Timestamp(instant("2026-08-31T14:03:13.123456789Z")),
        Value::Duration(Duration::from_nanoseconds(-1_500_000_000)),
        Value::ByteSize(ByteSize::from_bytes(512 * 1024 * 1024)),
        Value::Percent(Percent::new(24.8)),
        Value::Regex(Arc::new(RegexValue::new("^ngin[x]$").expect("a pattern"))),
        Value::Uuid(Uuid::parse("4d0a1f2b-0000-4000-8000-000000000001").expect("a uuid")),
        Value::Ip("fe80::1".parse().expect("an address")),
        Value::IpNetwork(
            IpNetwork::new("10.0.0.0".parse().expect("an address"), 8).expect("a network"),
        ),
        Value::Port(443),
        Value::Map(Arc::new(map)),
        Value::Record(Arc::new(common::object_record(1827))),
        Value::Error(Arc::new(
            ono_value::ErrorValue::new(ono_core::ErrorCode::IoNotFound, "gone")
                .with_help("look elsewhere")
                .with_retryable(false)
                .with_target(ono_value::ValueRef::name("nginx"))
                .with_metadata("attempts", Value::Int(3)),
        )),
    ];
    let payload = Value::list(every);

    let home = tempfile::tempdir().expect("a temporary home");
    let written = {
        let store = store_in(home.path());
        let mut seed = common::seed(
            EventKind::ProviderEvent,
            common::times("2026-08-31T14:03:13Z"),
            "linux.systemd-dbus",
        );
        seed.payload = Some(payload.clone());
        seed.before = Some(Value::string("inactive"));
        seed.after = Some(Value::string("active"));
        seed.changed_fields = vec![ono_temporal_core::FieldChange {
            field: Arc::from("active_state"),
            before: Some(Value::string("inactive")),
            after: Some(Value::string("active")),
            certainty: ono_temporal_core::ChangeCertainty::Observed,
        }];
        seed.subtype = Some(Arc::from("systemd.unit.state"));
        let event = seed.seal();
        store
            .append(std::slice::from_ref(&event), &[])
            .expect("an append succeeds");
        store.flush().expect("a flush succeeds");
        event
    };

    let store = LedgerStore::open(&path_in(home.path())).expect("the store reopens");
    let read = store
        .events(&EventQuery::in_range(TimeRange::all()))
        .expect("events answer");
    assert_eq!(read, vec![written], "every value kind reads back exactly");
}

/// §5.5: a subject a source named but Ono could not reconcile stays visibly unresolved.
#[test]
fn should_keep_an_unresolved_subject_unresolved_when_it_is_read_back() {
    use std::sync::Arc;

    let home = tempfile::tempdir().expect("a temporary home");
    let mut seed = common::seed(
        EventKind::ProviderEvent,
        common::times("2026-08-31T14:03:13Z"),
        "linux.journald",
    );
    seed.subject = Some(ono_temporal_core::SpatialRef::Unresolved {
        source: ono_temporal_core::EvidenceSource::builtin("linux.journald")
            .expect("a built-in source"),
        described: Arc::from("pid 1842"),
    });
    seed.related = vec![ono_temporal_core::SpatialRef::Resolved {
        id: common::subject(1842),
        object_type: ono_spatial_core::SpatialType::Process,
        label: Arc::from("nginx"),
    }];
    let written = seed.seal();

    {
        let store = store_in(home.path());
        store
            .append(std::slice::from_ref(&written), &[])
            .expect("an append succeeds");
    }
    let store = LedgerStore::open(&path_in(home.path())).expect("the store reopens");
    assert_eq!(
        store
            .events(&EventQuery::in_range(TimeRange::all()))
            .expect("events answer"),
        vec![written]
    );
}

/// §32.1: "temporal storage initialization MUST be lazy when recording is disabled", and the
/// disabled path must add less than 5 ms p95 to startup. Nothing here is timed — the outcome that
/// makes the budget reachable is that no file exists.
#[test]
fn should_touch_no_filesystem_when_recording_is_disabled() {
    let home = tempfile::tempdir().expect("a temporary home");
    let expected = path_in(home.path());

    let ledger = ono_temporal_ledger::Ledger::default();
    assert!(
        !ledger.is_persistent(),
        "§10.2: recording is off by default"
    );
    assert!(ledger.store().is_none());

    ledger
        .append(
            std::slice::from_ref(&event(
                EventKind::ObjectObserved,
                "2026-08-31T14:00:00Z",
                "linux.procfs",
            )),
            &[],
        )
        .expect("the session ledger still records for this session (§10.7)");
    assert_eq!(ledger.retention().events, 1);
    assert!(
        !expected.exists(),
        "§32.1: the disabled path opens no store; {} was created",
        expected.display()
    );
    assert!(
        !expected
            .parent()
            .expect("the store has a directory")
            .exists(),
        "§32.1: nor its directory"
    );
}

/// The two refusals of §34 are different facts: `temporal.out_of_retention` says history existed
/// and expired, `temporal.not_recorded` says it was never there.
#[test]
fn should_claim_expiry_only_when_there_is_retained_history_to_be_older_than() {
    let home = tempfile::tempdir().expect("a temporary home");
    let empty = ono_temporal_ledger::Ledger::default();
    assert!(
        !empty.is_out_of_retention(instant("2020-01-01T00:00:00Z")),
        "a ledger holding nothing cannot claim something expired"
    );
    assert_eq!(empty.earliest_retained(), None);

    let ledger = ono_temporal_ledger::Ledger::persistent(&common::options(&path_in(home.path())))
        .expect("the store opens");
    ledger
        .append(
            std::slice::from_ref(&event(
                EventKind::ObjectObserved,
                "2026-08-31T14:00:00Z",
                "linux.procfs",
            )),
            &[],
        )
        .expect("an append succeeds");
    assert_eq!(
        ledger.earliest_retained(),
        Some(instant("2026-08-31T14:00:00Z"))
    );
    assert!(ledger.is_out_of_retention(instant("2026-08-31T13:59:59Z")));
    assert!(!ledger.is_out_of_retention(instant("2026-08-31T14:00:01Z")));
}
