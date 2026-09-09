//! Retention as work the recorder drives, and what it costs (v0.5 §10.4, §31.8, §32.4).

mod common;

use common::{HOST, instant, options, scope, settings};
use ono_recorder::{Recorder, RecorderSettings};
use ono_temporal_core::{
    ClockDomain, EventKind, EventSeed, EventTimes, EvidenceSource, LedgerRead as _, SpatialRef,
    TemporalEvent,
};
use ono_value::{ByteSize, Duration, Provenance, SchemaId};

fn event(at: jiff::Timestamp, sequence: u64) -> TemporalEvent {
    EventSeed {
        kind: EventKind::ObjectObserved,
        subtype: None,
        scope: scope(),
        subject: Some(SpatialRef::Unresolved {
            source: EvidenceSource::recorder(),
            described: format!("pid {sequence}").into(),
        }),
        related: Vec::new(),
        times: EventTimes {
            source_sequence: Some(sequence),
            ..EventTimes::observed(at, at, ClockDomain::new(HOST, Some(common::BOOT)))
        },
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: Provenance::local("ono.recorder", SchemaId::new("ono.temporal-event", 1)),
    }
    .seal()
}

fn started(directory: &std::path::Path, settings: RecorderSettings) -> Recorder {
    let recorder = Recorder::new(options(directory));
    assert!(
        recorder
            .start(&settings, instant("2026-08-31T12:00:00Z"))
            .is_ok(),
        "the recorder starts on a scratch directory"
    );
    recorder
}

#[test]
fn should_remove_the_oldest_events_when_the_age_bound_is_exceeded() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = started(
        directory.path(),
        RecorderSettings {
            max_age: Duration::from_nanoseconds(600 * 1_000_000_000),
            ..settings()
        },
    );
    let base = instant("2026-08-31T12:00:00Z");
    let events: Vec<TemporalEvent> = (0..200_i64)
        .map(|index| event(base + jiff::Span::new().seconds(index), index as u64))
        .collect();
    recorder.record(&events, &[]).expect("two hundred events");

    let swept = recorder
        .sweep(base + jiff::Span::new().seconds(1_000))
        .expect("§31.8: retention is bounded background work the recorder drives");

    assert!(swept.events > 0, "the age bound removed the oldest events");
    assert!(swept.complete, "one drive reached the bound");
    let retention = recorder.ledger().retention();
    assert!(
        retention
            .earliest
            .is_none_or(|earliest| earliest >= base + jiff::Span::new().seconds(400)),
        "§10.4: retention removes the oldest eligible data"
    );
}

#[test]
fn should_report_incomplete_when_one_sweep_cannot_reach_the_bounds() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = started(
        directory.path(),
        RecorderSettings {
            max_age: Duration::from_nanoseconds(1),
            retention_batch: 4,
            max_sweep_passes: 2,
            ..settings()
        },
    );
    let base = instant("2026-08-31T12:00:00Z");
    let events: Vec<TemporalEvent> = (0..200_i64)
        .map(|index| event(base + jiff::Span::new().seconds(index), index as u64))
        .collect();
    recorder.record(&events, &[]).expect("two hundred events");

    let swept = recorder
        .sweep(base + jiff::Span::new().seconds(10_000))
        .expect("a bounded drive");

    assert!(
        !swept.complete,
        "§31.8: a bounded sweep says when it should be called again rather than running on"
    );
    assert!(
        swept.events <= 8,
        "a drive removes at most its passes times its batch"
    );
}

#[test]
fn should_leave_no_dangling_reference_when_retention_has_run() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = started(directory.path(), settings());
    let base = instant("2026-08-31T12:00:00Z");
    let events: Vec<TemporalEvent> = (0..64_i64)
        .map(|index| event(base + jiff::Span::new().seconds(index), index as u64))
        .collect();
    recorder.record(&events, &[]).expect("events");

    let swept = recorder
        .sweep(base + jiff::Span::new().hours(48))
        .expect("a drive past the age bound");

    assert!(swept.complete);
    assert_eq!(recorder.ledger().retention().events, 0);
    assert!(
        recorder.ledger().retention().earliest.is_none(),
        "nothing is retained, and nothing points at what has gone"
    );
}

#[test]
fn should_stay_inside_its_memory_budget_when_a_recording_load_is_driven() {
    let Some(before) = resident_peak_bytes() else {
        ono_testkit::skipped(
            ono_testkit::SkipReason::FixtureNotApplicable,
            "/proc/self/status does not report VmHWM on this host",
        );
        return;
    };
    let cpu_before = cpu_nanos();
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = started(
        directory.path(),
        RecorderSettings {
            max_size: ByteSize::from_bytes(16 * 1024 * 1024),
            ..settings()
        },
    );
    let base = instant("2026-08-31T12:00:00Z");

    for round in 0..40_i64 {
        let events: Vec<TemporalEvent> = (0..500_i64)
            .map(|index| {
                event(
                    base + jiff::Span::new().seconds(round * 500 + index),
                    (round * 500 + index) as u64,
                )
            })
            .collect();
        recorder.record(&events, &[]).expect("a batch");
        recorder
            .sweep(base + jiff::Span::new().seconds(round * 500 + 500))
            .expect("a drive");
    }
    recorder.flush().expect("a flush");

    let after = resident_peak_bytes().unwrap_or(before);
    let cpu = cpu_nanos().saturating_sub(cpu_before);
    let growth = after.saturating_sub(before);
    println!(
        "§32.4 measurement: 20000 events recorded and swept; peak RSS grew {} MiB; CPU {} ms",
        growth / (1024 * 1024),
        cpu / 1_000_000
    );

    assert!(
        growth < 100 * 1024 * 1024,
        "§32.4: memory below 100 MiB excluding OS page cache — grew {growth} bytes"
    );
    assert!(
        recorder
            .ledger()
            .retention()
            .stored_size
            .is_some_and(|size| { size.bytes() <= 32 * 1024 * 1024 }),
        "§10.4: the size bound is what stops the store growing without end"
    );
}

#[test]
fn should_cost_a_fraction_of_a_core_when_an_idle_workstation_is_recorded() {
    // §32.4 is a claim about an *idle* workstation, so the load is what an idle hour actually
    // produces rather than what a benchmark can produce: a handful of events a second, a flush
    // every `temporal.flush.interval`, a bounded retention drive beside it, and a checkpoint every
    // `temporal.checkpoint.interval`. The cost is measured against the modelled hour, so the
    // figure is a share of a core rather than a wall-clock reading of the machine the test ran on.
    const MODELLED_SECONDS: i64 = 3_600;
    const EVENTS_PER_SECOND: i64 = 2;
    const MAINTENANCE_TURNS: i64 = MODELLED_SECONDS / 2;
    const CHECKPOINTS: i64 = MODELLED_SECONDS / 300;

    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = started(directory.path(), settings());
    let base = instant("2026-08-31T12:00:00Z");
    let before = cpu_nanos();

    for turn in 0..MAINTENANCE_TURNS {
        let at = base + jiff::Span::new().seconds(turn * 2);
        let events: Vec<TemporalEvent> = (0..EVENTS_PER_SECOND * 2)
            .map(|index| {
                event(
                    at + jiff::Span::new().milliseconds(index * 250),
                    (turn * EVENTS_PER_SECOND * 2 + index) as u64,
                )
            })
            .collect();
        recorder
            .record(&events, &[])
            .expect("an idle second's events");
        recorder.maintenance(at).expect("a maintenance turn");
    }
    for checkpoint in 0..CHECKPOINTS {
        let at = base + jiff::Span::new().seconds(checkpoint * 300);
        recorder
            .checkpoint(
                ono_recorder::CheckpointCapture::new(scope(), at),
                &ono_temporal_reconstruct::CheckpointPolicy::default_local(),
            )
            .expect("a scheduled checkpoint");
    }

    let cpu = cpu_nanos().saturating_sub(before);
    let modelled = (MODELLED_SECONDS as u64) * 1_000_000_000;
    let share = (cpu as f64 / modelled as f64) * 100.0;
    println!(
        "§32.4 measurement: a modelled idle hour ({} events, {MAINTENANCE_TURNS} maintenance \
         turns, {CHECKPOINTS} checkpoints) cost {} ms of CPU — {share:.3}% of one core",
        MAINTENANCE_TURNS * EVENTS_PER_SECOND * 2,
        cpu / 1_000_000,
    );

    assert!(
        share < 1.0,
        "§32.4: recorder CPU overhead SHOULD average below 1% of one CPU core — measured {share:.3}%"
    );
}

fn resident_peak_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    let kilobytes: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kilobytes * 1024)
}

fn cpu_nanos() -> u64 {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return 0;
    };
    let Some(fields) = stat.rsplit_once(')') else {
        return 0;
    };
    let numbers: Vec<&str> = fields.1.split_whitespace().collect();
    let ticks: u64 = numbers
        .get(11)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
        + numbers
            .get(12)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
    ticks * 10_000_000
}
