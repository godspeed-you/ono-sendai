//! One sample of one of v0.5 §49's release measurements, taken in a process of its own.
//!
//! §37.3 forbids advertising a warm figure as a cold one, and a temporal query is warm the
//! instant it has been asked once: SQLite keeps its page cache, the operating system keeps the
//! file, and the planner keeps its prepared statement. So a sample is a whole process — the
//! parent re-runs `xtask perf --sample-temporal <operation>` and reads one figure off its
//! standard output — and each sample asks about a different window, so a repeat is a new question
//! rather than the same one answered twice.
//!
//! Everything here calls the shipped library exactly as a command would: `timeline::timeline`,
//! `changes::changes`, `Reconstructor::reconstruct`, `CausalEngine::explain`,
//! `search::find_events` and `LedgerStore::sweep`, over a real store. §32.2's rule that
//! "provider/planner code exercised by the benchmark MUST match production logic" is the whole
//! point of the exercise: a benchmark over a hand-rolled query would measure the benchmark.

use std::path::Path;
use std::time::Instant;

use jiff::Span;
use ono_temporal_core::{
    EventKind, EventQuery, LedgerRead, QueryOrder, SpatialRef, TemporalContext, TimeRange,
};
use ono_temporal_ledger::{LedgerStore, RetentionPolicy};
use ono_temporal_query::causal::{CausalContext, CausalEngine, WhyOptions, WhyRequest};
use ono_temporal_query::relevance::Horizon;
use ono_temporal_query::{changes, search, timeline};
use ono_temporal_reconstruct::{ReconstructionRequest, Reconstructor};

use super::TemporalOperation;
use super::fixture::FixtureLedger;

/// What one sample observed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// How long the operation took, in milliseconds.
    pub elapsed_ms: f64,
    /// How many values it produced.
    pub values: f64,
}

/// Takes one sample of `operation` against `fixture`, at the window `index` selects.
///
/// # Errors
///
/// Returns the reason the operation could not be performed, which the caller reports rather than
/// recording a figure nobody measured.
pub fn take(
    operation: TemporalOperation,
    index: u32,
    fixture: &FixtureLedger,
    binary: Option<&Path>,
) -> Result<Sample, String> {
    match operation {
        TemporalOperation::StartupWithLedgerPresent => startup(fixture, binary),
        TemporalOperation::RecorderIdle => recorder_idle(fixture, index),
        TemporalOperation::HistoricalMapL1 => Err(
            "no historical spatial world is in the tree, so there is no L1 map to draw at a past \
             instant"
                .to_owned(),
        ),
        TemporalOperation::Timeline15m => timeline_15m(fixture, index),
        TemporalOperation::Changes1h => changes_1h(fixture, index),
        TemporalOperation::RecentReconstruction => reconstruct_recent(fixture, index),
        TemporalOperation::Why => why(fixture, index),
        TemporalOperation::FindEvent => find_event(fixture, index),
        TemporalOperation::RetentionSweep => retention_sweep(fixture, index),
    }
}

/// v0.5 §32.1: what the shell's startup costs with the fixture ledger sitting at the canonical
/// path and recording disabled.
///
/// §32.1's second sentence is the one this measures: *"Temporal storage initialization MUST be
/// lazy when recording is disabled."* A store that is opened, migrated or integrity-checked at
/// startup would cost more the larger it is, and the way to find out is to put a million-event
/// store where the recorder would have left one and start the shell.
fn startup(fixture: &FixtureLedger, binary: Option<&Path>) -> Result<Sample, String> {
    let binary = binary.ok_or_else(|| "no `ono` binary is built to measure".to_owned())?;
    let home = fixture
        .path()
        .parent()
        .ok_or_else(|| "the fixture has no directory".to_owned())?
        .join("home");
    let store = home.join(".local/share/ono/temporal");
    std::fs::create_dir_all(&store)
        .map_err(|error| format!("cannot create {}: {error}", store.display()))?;
    let placed = store.join("ledger.sqlite3");
    if !placed.exists() {
        // A hard link rather than a copy: the same inode, so the shell sees the real fixture at
        // the real size without a second copy of it on disk.
        std::fs::hard_link(fixture.path(), &placed)
            .or_else(|_| std::fs::copy(fixture.path(), &placed).map(|_| ()))
            .map_err(|error| {
                format!("cannot place the fixture at {}: {error}", placed.display())
            })?;
    }

    let started = Instant::now();
    let output = std::process::Command::new(binary)
        .args(["-c", "echo ready"])
        .env("HOME", &home)
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("cannot start the shell: {error}"))?;
    let elapsed = started.elapsed();
    if !output.status.success() {
        return Err(format!(
            "the shell refused to start with a ledger present: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Sample {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        values: 1.0,
    })
}

/// The store, opened for reading with the retention policy `policy`.
fn open(fixture: &FixtureLedger, policy: RetentionPolicy) -> Result<LedgerStore, String> {
    fixture.open(policy)
}

/// The place one sample asks about, rotated by the sample index so twenty samples are twenty
/// questions rather than one question twenty times.
fn place(fixture: &FixtureLedger, index: u32) -> ono_spatial_core::SpatialScope {
    fixture.place_scope(index as usize % 16)
}

/// The instant a sample's window ends at: the fixture's horizon, walked backwards an hour per
/// sample so no two samples read the same rows.
fn coordinate(fixture: &FixtureLedger, index: u32) -> Result<jiff::Timestamp, String> {
    fixture
        .horizon()
        .checked_sub(Span::new().minutes(i64::from(index) * 61))
        .map_err(|error| format!("the sample window left the fixture: {error}"))
}

/// v0.5 §32.3: fifteen minutes of one place's timeline.
fn timeline_15m(fixture: &FixtureLedger, index: u32) -> Result<Sample, String> {
    let store = open(fixture, RetentionPolicy::unlimited())?;
    let scope = place(fixture, index);
    let until = coordinate(fixture, index)?;
    let since = until
        .checked_sub(Span::new().minutes(15))
        .map_err(|error| format!("the window left the fixture: {error}"))?;
    // `everything` scoped to the place: §11.3's "current place" is a scope, and the relevance
    // filter narrowing it further would measure the filter rather than the query.
    let request = timeline::TimelineRequest {
        since: Some(since),
        until: Some(until),
        ..timeline::TimelineRequest::new(Horizon::everything(scope))
    };
    let context = TemporalContext::Present;

    let started = Instant::now();
    let answer = timeline::timeline(&store, &request, &context, until)
        .map_err(|error| format!("the timeline refused: {error}"))?;
    let elapsed = started.elapsed();
    Ok(Sample {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        values: answer.events.len() as f64,
    })
}

/// v0.5 §32.3: one hour of one place's changes.
fn changes_1h(fixture: &FixtureLedger, index: u32) -> Result<Sample, String> {
    let store = open(fixture, RetentionPolicy::unlimited())?;
    let scope = place(fixture, index);
    let until = coordinate(fixture, index)?;
    let since = until
        .checked_sub(Span::new().hours(1))
        .map_err(|error| format!("the window left the fixture: {error}"))?;
    let request = changes::ChangesRequest {
        until: Some(until),
        ..changes::ChangesRequest::new(scope, since)
    };

    let started = Instant::now();
    let answer = changes::changes(&store, &request, until)
        .map_err(|error| format!("changes refused: {error}"))?;
    let elapsed = started.elapsed();
    Ok(Sample {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        values: answer.len() as f64,
    })
}

/// v0.5 §32.3: the state of the host at a recent checkpoint, replayed forward to the instant.
fn reconstruct_recent(fixture: &FixtureLedger, index: u32) -> Result<Sample, String> {
    let store = open(fixture, RetentionPolicy::unlimited())?;
    let at = fixture
        .horizon()
        .checked_sub(Span::new().seconds(90 + i64::from(index)))
        .map_err(|error| format!("the instant left the fixture: {error}"))?;
    let request = ReconstructionRequest::new(fixture.root_scope(), at);

    let started = Instant::now();
    let world = Reconstructor::new(&store)
        .reconstruct(&request)
        .map_err(|error| format!("the reconstruction refused: {error}"))?;
    let elapsed = started.elapsed();
    Ok(Sample {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        values: world.objects().count() as f64,
    })
}

/// v0.5 §32.3: an explanation over at most a hundred candidate events.
///
/// The candidates are read from the ledger and their evidence with them, because that is what a
/// `why` costs: §16.3 selects the transition from the events at or before the coordinate, and
/// §15.2 admits a causal link only with the evidence its rule requires, so an explanation that
/// did not load evidence would be an explanation that could never claim a cause.
fn why(fixture: &FixtureLedger, index: u32) -> Result<Sample, String> {
    let store = open(fixture, RetentionPolicy::unlimited())?;
    let scope = place(fixture, index);
    let at = coordinate(fixture, index)?;

    let started = Instant::now();
    let candidates = store
        .events(&EventQuery {
            scope: Some(scope),
            range: TimeRange {
                from: None,
                until: Some(at),
            },
            limit: Some(100),
            order: QueryOrder::Descending,
            ..EventQuery::default()
        })
        .map_err(|error| format!("the candidate query refused: {error}"))?;
    let evidence_ids: Vec<_> = candidates
        .iter()
        .flat_map(|event| event.evidence.iter().cloned())
        .collect();
    let evidence = store
        .evidence(&evidence_ids)
        .map_err(|error| format!("the evidence query refused: {error}"))?;
    let context = CausalContext::new(evidence);

    let subject = candidates
        .iter()
        .find(|event| event.kind == EventKind::ObjectChanged)
        .and_then(|event| event.subject.as_ref())
        .and_then(|reference| match reference {
            SpatialRef::Resolved { id, label, .. } => Some((id.clone(), label.to_string())),
            SpatialRef::Unresolved { .. } => None,
        })
        .ok_or_else(|| "no candidate window holds an evidenced change to explain".to_owned())?;

    let explanation = CausalEngine::builtin()
        .explain(
            &WhyRequest::target(&subject.0, &subject.1),
            &candidates,
            &context,
            &WhyOptions::at(at).with_max_candidates(100),
        )
        .map_err(|error| format!("why refused: {error}"))?;
    let elapsed = started.elapsed();
    Ok(Sample {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        values: (explanation.causal_chain.len()
            + explanation.correlations.len()
            + explanation.preceding.len()) as f64,
    })
}

/// v0.5 §32.3: an indexed event search.
fn find_event(fixture: &FixtureLedger, index: u32) -> Result<Sample, String> {
    let store = open(fixture, RetentionPolicy::unlimited())?;
    let until = coordinate(fixture, index)?;
    let since = until
        .checked_sub(Span::new().hours(6))
        .map_err(|error| format!("the window left the fixture: {error}"))?;
    let hints = search::SearchHints {
        scope: Some(place(fixture, index)),
        subjects: Vec::new(),
        kinds: vec![EventKind::ObjectDisappeared],
        range: TimeRange::between(since, until),
        limit: None,
        order: QueryOrder::Descending,
    };

    let started = Instant::now();
    let found = search::find_events(&store, &hints)
        .map_err(|error| format!("find event refused: {error}"))?;
    let elapsed = started.elapsed();
    Ok(Sample {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        values: found.len() as f64,
    })
}

/// v0.5 §31.8: one bounded retention sweep of a ledger that is past its bounds.
///
/// The sweep runs against a copy, because it is the one measured operation that changes what it
/// measured: a sweep that removed the fixture's oldest hours would leave the next benchmark
/// asking about a window that is no longer there. The copy is made once and swept repeatedly —
/// twenty batches of 2 048 events is well inside the two hours of expired history the coordinate
/// puts outside the twenty-four-hour bound, so every sample has the same work to do.
fn retention_sweep(fixture: &FixtureLedger, index: u32) -> Result<Sample, String> {
    let copy = fixture
        .path()
        .parent()
        .ok_or_else(|| "the fixture has no directory".to_owned())?
        .join("sweep.sqlite3");
    if !copy.exists() {
        std::fs::copy(fixture.path(), &copy)
            .map_err(|error| format!("cannot copy the fixture to sweep it: {error}"))?;
    }
    // §10.4's defaults, which is what "retention cleanup" means: twenty-four hours or 512 MiB,
    // whichever removes data first, one bounded batch per call (§31.8).
    let store = LedgerStore::open_with(
        &ono_temporal_ledger::StoreOptions::at(&copy).with_retention(RetentionPolicy::default()),
    )
    .map_err(|error| format!("cannot open the sweep copy: {error}"))?;
    let now = fixture
        .horizon()
        .checked_add(Span::new().hours(2).seconds(i64::from(index)))
        .map_err(|error| format!("the sweep instant left the range: {error}"))?;

    let started = Instant::now();
    let swept = store
        .sweep(now)
        .map_err(|error| format!("the sweep refused: {error}"))?;
    let elapsed = started.elapsed();
    Ok(Sample {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        values: (swept.events + swept.evidence + swept.links + swept.actions + swept.coverage)
            as f64,
    })
}

/// v0.5 §32.4: what one turn of an idle recorder's own maintenance costs.
///
/// §32.4 states the budget as a rate rather than a latency — *"recorder CPU overhead SHOULD
/// average below 1% of one CPU core and memory below 100 MiB excluding OS page cache"* — and the
/// thing that consumes it on an idle machine is [`Recorder::maintenance`], the periodic turn a
/// caller drives from a timer: flush if the flush interval has elapsed, drive one bounded
/// retention sweep, and say whether a checkpoint is due. With `temporal.flush.interval` at two
/// seconds, 1% of one core is 20 ms of maintenance per turn, so the latency recorded here and
/// §32.4's percentage are the same number read two ways. The memory half is the `peak_rss_bytes`
/// the sample's own process reports.
///
/// The recorder runs over a copy of §49's whole fixture rather than over an empty store, because
/// an idle recorder that has been running has a day of history behind it and the sweep it drives
/// has a million rows to find nothing in. Two settings are stated rather than defaulted:
///
/// - `max_age` stays §10.4's twenty-four hours and the instant is the fixture's horizon, so
///   nothing is expired and the sweep is genuinely idle work.
/// - `max_size` is lifted, because §49's ledger is larger than §10.4's 512 MiB bound. A recorder
///   under the default bound would be continuously compacting and would never be idle at all,
///   which is a finding about §10.4 against §49 rather than a measurement of §32.4.
fn recorder_idle(fixture: &FixtureLedger, index: u32) -> Result<Sample, String> {
    use ono_recorder::{Recorder, RecorderOptions, RecorderSettings};

    let copy = fixture
        .path()
        .parent()
        .ok_or_else(|| "the fixture has no directory".to_owned())?
        .join("recorder.sqlite3");
    if !copy.exists() {
        std::fs::copy(fixture.path(), &copy)
            .map_err(|error| format!("cannot copy the fixture for the recorder: {error}"))?;
    }

    let recorder = Recorder::new(
        RecorderOptions::new(
            fixture.root_scope(),
            ono_temporal_core::ClockDomain::new("fixture-host", Some("fixture-boot")),
        )
        .with_store(copy),
    );
    let settings = RecorderSettings {
        max_size: ono_value::ByteSize::from_bytes(64 * 1024 * 1024 * 1024),
        ..RecorderSettings::default()
    };
    let now = fixture
        .horizon()
        .checked_sub(Span::new().seconds(i64::from(index)))
        .map_err(|error| format!("the idle instant left the fixture: {error}"))?;
    recorder
        .start(&settings, now)
        .map_err(|error| format!("the recorder refused to start: {error}"))?;
    // One turn to get past the first flush, so what is timed is the steady state rather than the
    // start-up transient §44.1 puts in front of it.
    recorder
        .maintenance(now)
        .map_err(|error| format!("the first maintenance turn refused: {error}"))?;

    let later = now
        .checked_add(Span::new().seconds(2))
        .map_err(|error| format!("the idle interval left the fixture: {error}"))?;
    let started = Instant::now();
    let turn = recorder
        .maintenance(later)
        .map_err(|error| format!("the maintenance turn refused: {error}"))?;
    let elapsed = started.elapsed();
    let _ = recorder.stop(later);
    Ok(Sample {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        // An idle recorder produced nothing, which is what makes it idle. The swept counts are
        // the only values a turn can report, and on an idle one they are zero (§2.6 keeps a
        // measured zero a measured zero).
        values: (turn.swept.events + turn.swept.evidence + turn.swept.actions) as f64,
    })
}
