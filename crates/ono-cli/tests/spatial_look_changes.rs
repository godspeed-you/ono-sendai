//! v0.4's recent-change section, answered by the v0.5 temporal engine (spec v0.5 §13.5, §8.2).
//!
//! §13.5: "`look`'s v0.4 `changed` section SHOULD be backed by the same `TemporalChange` engine
//! when v0.5 evidence exists. It MUST NOT retain a separate ad-hoc snapshot comparison
//! implementation once the temporal engine is available." One engine, not two — and the
//! observable consequence is that the section can only say what the ledger can support.
//!
//! ADR-0775 fixes the four things it may say, and each of them is a different claim:
//!
//! - `unsupported` — no temporal evidence is installed, so nothing was watching;
//! - `unknown`, with a **null** source — nothing was found and the window is not covered end to
//!   end, so an absence cannot be claimed (§8.2, v0.4 §24.3);
//! - `empty` — the window **is** covered and nothing in it differs. The one state that asserts
//!   that nothing happened;
//! - `available` — changes were found, and they are the entries.
//!
//! `snapshot_comparison` is never the source of any of them: the variant survives in
//! `ono-spatial-events` for the live map that legitimately compares snapshots, and `look` no
//! longer answers from one (ADR-0775's spec deviation against v0.4 §25.4).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{PermissionState, SpatialId, SpatialScope, SpatialType, space};
use ono_temporal_core::{
    ChangeCertainty, ClockDomain, EventKind, EventSeed, EventTimes, EvidenceSource, FieldChange,
    LedgerWrite, SpatialRef, TemporalCompleteness, TemporalCoverage,
};
use ono_temporal_ledger::{Ledger, StoreOptions};
use ono_testkit::{Run, Scratch};
use ono_value::{Provenance, SchemaId, Value};
use serde_yaml_ng::Value as Yaml;

use support::{field, json, text};

fn scope() -> SpatialScope {
    ono_cli::spatial::local_scope()
}

/// Where a session stands before it navigates — the root place `look` describes (v0.4 §7.1).
fn here() -> SpatialId {
    space::root().spatial_id_in(None)
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

/// The fixture window: `minutes` behind now and the same distance ahead of it.
///
/// A coverage interval a test installs has to span the window the shell will ask about, and the
/// shell asks about it a moment later than the fixture was written — so the far end is ahead of
/// the writing rather than at it, and neither end is a fixed instant that would age out of
/// `temporal.retention.max_age` (§10.4).
fn window(minutes: i64) -> (Timestamp, Timestamp) {
    let span = jiff::Span::new().minutes(minutes);
    let now = Timestamp::now();
    (
        now.checked_sub(span).expect("an instant minutes back"),
        now.checked_add(span).expect("an instant minutes ahead"),
    )
}

/// Where `temporal.recording.enabled` puts the store under `home` (§31.1).
fn store_path(home: &Scratch) -> std::path::PathBuf {
    home.path().join("data/ono/temporal/ledger.sqlite3")
}

/// The store the shell opens under `home`, as `temporal.recording.enabled` makes it.
fn store_of(home: &Scratch) -> Ledger {
    Ledger::persistent(&StoreOptions::at(&store_path(home))).expect("the store opens")
}

/// Coverage a recorder claims complete over the whole fixture window.
fn covered(home: &Scratch) {
    let (from, until) = window(30);
    let ledger = store_of(home);
    let intervals: Vec<TemporalCoverage> = ["process.existence", "service.existence", "existence"]
        .into_iter()
        .map(|capability| TemporalCoverage {
            scope: scope(),
            capability: Arc::from(capability),
            from,
            until,
            completeness: TemporalCompleteness::Complete,
            sampling_interval: None,
            source: EvidenceSource::recorder(),
            permission: PermissionState::Available,
        })
        .collect();
    ledger
        .record_coverage(&intervals)
        .expect("the store accepts the fixture coverage");
}

/// One observed field transition of the place the session is standing in, two minutes ago.
fn seed_change_here(home: &Scratch) {
    let at = window(2).0;
    let ledger = store_of(home);
    let event = EventSeed {
        kind: EventKind::ObjectChanged,
        subtype: None,
        scope: scope(),
        subject: Some(SpatialRef::Resolved {
            id: here(),
            object_type: SpatialType::System,
            label: Arc::from("system"),
        }),
        related: Vec::new(),
        times: times(at),
        before: None,
        after: None,
        changed_fields: vec![FieldChange {
            field: Arc::from("state"),
            before: Some(Value::string("nominal")),
            after: Some(Value::string("degraded")),
            certainty: ChangeCertainty::Observed,
        }],
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance(),
    }
    .seal();
    ledger
        .append(&[event], &[])
        .expect("the store accepts the fixture event");
}

/// One change named by its class and its subject, which is what two surfaces must agree on.
fn named(change: &Yaml) -> String {
    format!(
        "{}:{}",
        text(change, "kind"),
        text(&change["subject"], "label")
    )
}

/// The `changed` section of the `PlaceView` a `look --json` printed (v0.4 §24.3).
fn change_section(run: &Run) -> Yaml {
    let document = json(run.stdout().trim());
    let section = field(&document, "changed");
    assert!(
        !section.is_null(),
        "v0.4 §24.3: `look --changes` carries a change section. Got {:?}",
        run.output()
    );
    section
}

#[test]
fn should_report_unknown_with_no_source_when_nothing_was_found_and_the_window_is_uncovered() {
    // ADR-0775, from §8.2 and v0.4 §24.3: "No fake change summary may be generated when no event
    // source or comparison snapshot exists." A ledger that found nothing over a window it did not
    // cover has observed nothing, and reporting that as `empty` would state that nothing
    // happened. The source is null because there is no comparison to name.
    let home = ono_testkit::scratch();
    let run = support::recording_shell(&home, "look --json --changes 1m");
    let section = change_section(&run);
    assert_eq!(
        text(&section, "state"),
        "unknown",
        "v0.5 §13.5, §8.2: a window nothing covered end to end answers `unknown`, never \
         `empty`. Got {section:?}"
    );
    assert!(
        section["source"].is_null(),
        "v0.4 §25.4, ADR-0775: an `unknown` section names no source, because it made no \
         comparison. Got {section:?}"
    );
    assert!(
        section["entries"].as_sequence().is_some_and(Vec::is_empty),
        "v0.5 §13.5: an `unknown` section reports no entries it did not find. Got {section:?}"
    );
}

#[test]
fn should_report_nothing_changed_only_when_a_source_covered_the_whole_window() {
    // ADR-0775: `empty` is "the only state that asserts that nothing happened", and §8.2 fixes
    // what earns it — coverage complete over the whole interval, for the capabilities a change
    // would have come from. This is the branch the ADR said should become reachable and, as first
    // written, could not: the session's own always-partial interval was composed into every
    // window and made `headline()` partial whatever a recorder had covered (ADR-0777).
    //
    // Its opposite number is `should_report_unknown_with_no_source_when_nothing_was_found_and_the_window_is_uncovered`,
    // and the pair is the contract: an absence is asserted where coverage backs it and nowhere
    // else.
    let home = ono_testkit::scratch();
    covered(&home);
    let run = support::recording_shell(&home, "look --json --changes 1m");
    let section = change_section(&run);
    assert_eq!(
        text(&section, "state"),
        "empty",
        "v0.5 §8.2, §13.5, ADR-0775: a window a source covered end to end, with nothing in it, is \
         the one case in which the shell may say nothing changed. Got {section:?}; output {:?}",
        run.output()
    );
    assert_eq!(
        text(&section, "source"),
        "ono.temporal-ledger",
        "v0.4 §25.4: a section that made a comparison names what it made it from. Got {section:?}"
    );
    assert!(
        section["entries"]
            .as_sequence()
            .is_some_and(std::vec::Vec::is_empty),
        "v0.4 §24.3: `empty` is an empty list of changes rather than a prose claim. Got \
         {section:?}"
    );
}

#[test]
fn should_report_the_change_the_ledger_holds_when_something_here_moved() {
    // §13.5: the section is backed by `changes`, so a transition the ledger holds about this
    // place is what the section reports — as entries rather than as prose.
    let home = ono_testkit::scratch();
    seed_change_here(&home);
    let run = support::recording_shell(&home, "look --json --changes 5m");
    let section = change_section(&run);
    assert_eq!(
        text(&section, "state"),
        "available",
        "v0.5 §13.5: a change the ledger found is reported. Got {section:?}; output {:?}",
        run.output()
    );
    assert_eq!(
        text(&section, "source"),
        "ono.temporal-ledger",
        "v0.5 §13.5, ADR-0775: the sources are the temporal ledger, or none. Got {section:?}"
    );
    let entries = section["entries"]
        .as_sequence()
        .unwrap_or_else(|| panic!("v0.4 §24.3: `entries` is a list, got {section:?}"));
    assert_eq!(
        entries.len(),
        1,
        "v0.5 §13.5: the one seeded transition is the one entry. Got {entries:?}"
    );
    assert_eq!(
        text(&entries[0], "kind"),
        "changed",
        "v0.5 §13.2: the entry carries the canonical change class. Got {:?}",
        entries[0]
    );
    assert_eq!(
        field(&entries[0], "provenance.schema").as_str(),
        Some("ono.temporal-change/1"),
        "v0.5 §13.5, §35.4: the entries are the engine's own `ono.temporal-change/1` values, not \
         a second shape composed for the view. Got {:?}",
        entries[0]
    );
}

#[test]
fn should_report_a_change_under_partial_coverage_when_it_was_observed() {
    // ADR-0775: "A change is still reported under partial coverage, so enabling the recorder adds
    // absence claims rather than adding findings." Coverage gates what may be said about
    // *absence*; an observation is evidence of itself.
    let home = ono_testkit::scratch();
    seed_change_here(&home);
    let run = support::recording_shell(&home, "look --json --changes 5m");
    let section = change_section(&run);
    assert_eq!(
        text(&section, "state"),
        "available",
        "v0.5 §8.2: withholding an observed change because the rest of the window is unknown \
         would lose a fact the shell holds. Got {section:?}"
    );
}

#[test]
fn should_never_name_a_snapshot_comparison_as_the_source_of_the_change_section() {
    // ADR-0775's spec deviation against v0.4 §25.4: "a snapshot comparison is never the source of
    // `look`'s change section. The sources are the temporal ledger, or none." §13.5 is what makes
    // it so — the ad-hoc implementation may not be retained once the engine exists.
    let home = ono_testkit::scratch();
    seed_change_here(&home);
    let found = support::recording_shell(&home, "look --json --changes 5m");
    let quiet = ono_testkit::scratch();
    let nothing = support::recording_shell(&quiet, "look --json --changes 5m");
    for run in [&found, &nothing] {
        let section = change_section(run);
        let source = section["source"].as_str().unwrap_or("null");
        assert!(
            source == "null" || source == "ono.temporal-ledger",
            "v0.5 §13.5, ADR-0775: the change section's source is the temporal ledger or \
             nothing. Got {source:?}"
        );
        assert!(
            !run.output().contains("snapshot_comparison"),
            "v0.5 §13.5: the prohibited ad-hoc snapshot comparison names itself nowhere in a \
             `look`. Got {:?}",
            run.output()
        );
    }
}

#[test]
fn should_answer_the_change_section_from_the_same_engine_that_answers_changes() {
    // §13.5's requirement stated as an outcome: one implementation means the two surfaces cannot
    // disagree. The same window, asked through `look` and through `changes`, is the same
    // statement about the same machine, down to the change identity.
    let home = ono_testkit::scratch();
    seed_change_here(&home);
    let looked = support::recording_shell(&home, "look --json --changes 5m");
    let asked = support::recording_shell(&home, "changes --since 5m | to json");

    let section = change_section(&looked);
    let entries = section["entries"]
        .as_sequence()
        .unwrap_or_else(|| panic!("v0.4 §24.3: `entries` is a list, got {section:?}"));
    let listed = support::rows(&asked);

    // The subject and the class, not the change identity: `change_id` covers the window a change
    // was computed over, and two runs a moment apart compute over two windows. What §13.5 asks is
    // that the two surfaces report the same changes, and that is what is compared.
    let from_look: Vec<String> = entries.iter().map(named).collect();
    let from_changes: Vec<String> = listed.iter().map(named).collect();
    assert!(
        !from_look.is_empty(),
        "v0.5 §13.5: the seeded change reaches the section. Got {:?}",
        looked.output()
    );
    assert_eq!(
        from_look, from_changes,
        "v0.5 §13.5: one change implementation, not two — `look`'s section and `changes` answer \
         the same window with the same changes"
    );
}

#[test]
fn should_leave_the_change_section_out_when_no_window_was_asked_for() {
    // v0.4 §24.3: the section answers a question about an interval, and without `--changes` no
    // interval was named. A section invented for a window nobody asked about would be the fake
    // summary §24.3 forbids, in a second spelling.
    let home = ono_testkit::scratch();
    let run = support::recording_shell(&home, "look --json");
    let document = json(run.stdout().trim());
    let section = field(&document, "changed");
    assert!(
        section.is_null(),
        "v0.4 §24.3: `look` without `--changes` reports no change section. Got {section:?}"
    );
}
