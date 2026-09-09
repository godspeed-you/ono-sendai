//! Source sequence continuity (v0.5 §25.4, §43.2, §44.1) and the ordering property of §47.2.
//!
//! §25.4: "NTP corrections or manual wall-clock jumps MUST NOT reorder events inside a source
//! sequence. The ledger MUST use sequence/monotonic evidence where available." §43.2: a source that
//! exceeded its capacity gets "an explicit coverage gap over pretending continuity".

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_temporal_core::{
    EventKind, EvidenceSource, GapReason, LedgerWrite, Ordering, happens_before,
};

use common::{instant, scope, seed, sequenced, store_in};

fn netlink(observed: &str, sequence: u64) -> ono_temporal_core::TemporalEvent {
    seed(
        EventKind::ProviderEvent,
        sequenced(observed, sequence),
        "linux.netlink",
    )
    .seal()
}

#[test]
fn should_report_the_highest_sequence_seen_when_a_source_is_asked_after_a_reopen() {
    let home = tempfile::tempdir().expect("a temporary home");
    {
        let store = store_in(home.path());
        store
            .append(
                &[
                    netlink("2026-08-31T14:00:00Z", 17),
                    netlink("2026-08-31T14:00:01Z", 18),
                    netlink("2026-08-31T14:00:02Z", 19),
                ],
                &[],
            )
            .expect("an append succeeds");
    }
    let store = ono_temporal_ledger::LedgerStore::open(&common::path_in(home.path()))
        .expect("the store reopens");
    let sequences = store.source_sequences().expect("sequences answer");
    assert_eq!(sequences.len(), 1);
    let sequence = &sequences[0];
    assert_eq!(sequence.highest, 19, "§44.1: a restart resumes from here");
    assert_eq!(sequence.lowest, 17);
    assert_eq!(sequence.seen, 3);
    assert!(sequence.contiguous);
    assert_eq!(sequence.missing(), 0);
    assert_eq!(sequence.source.as_str(), "linux.netlink");
}

#[test]
fn should_report_a_break_when_a_declared_contiguous_sequence_skips_a_number() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let source = EvidenceSource::builtin("linux.netlink").expect("a built-in source");
    store
        .declare_contiguous(
            &source,
            "testbox",
            Some("4d0a1f2b"),
            &scope(),
            instant("2026-08-31T14:00:00Z"),
        )
        .expect("a source declares its contiguity");
    store
        .append(
            &[
                netlink("2026-08-31T14:00:00Z", 17),
                netlink("2026-08-31T14:00:03Z", 20),
            ],
            &[],
        )
        .expect("an append succeeds");

    let sequence = store
        .source_sequence(&source, "testbox", Some("4d0a1f2b"))
        .expect("the sequence answers")
        .expect("the source has a sequence");
    assert!(!sequence.contiguous);
    assert_eq!(sequence.missing(), 2, "18 and 19 never arrived");
    assert!(
        sequence.has_lost_events(),
        "§43.2: a source that declared contiguity and skipped a number lost events"
    );

    let gaps = store.gaps().expect("gaps answer");
    assert_eq!(gaps.len(), 1, "§43.2 prefers an explicit gap to continuity");
    assert_eq!(gaps[0].reason, GapReason::NotRecorded);
    assert_eq!(gaps[0].source.as_str(), "linux.netlink");
}

#[test]
fn should_report_no_break_when_a_source_never_claimed_its_sequence_was_contiguous() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    store
        .append(
            &[
                netlink("2026-08-31T14:00:00Z", 17),
                netlink("2026-08-31T14:00:03Z", 90),
            ],
            &[],
        )
        .expect("an append succeeds");
    assert!(
        store.gaps().expect("gaps answer").is_empty(),
        "a hole is a loss only where the source promised there would be none"
    );
}

#[test]
fn should_keep_sequences_apart_when_the_clock_domain_changes() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let mut after_reboot = seed(
        EventKind::ProviderEvent,
        sequenced("2026-08-31T14:05:00Z", 1),
        "linux.netlink",
    );
    after_reboot.times.domain = common::domain("testbox", Some("boot-b"));
    store
        .append(
            &[netlink("2026-08-31T14:00:00Z", 4_000), after_reboot.seal()],
            &[],
        )
        .expect("an append succeeds");

    let sequences = store.source_sequences().expect("sequences answer");
    assert_eq!(
        sequences.len(),
        2,
        "§25.5: a monotonic sequence does not cross a boot boundary"
    );
    assert!(store.gaps().expect("gaps answer").is_empty());
}

/// §47.2: "ordering is stable under wall-clock jumps when source sequence exists."
///
/// The property is exercised over every backward jump a corrected clock could make: whatever the
/// wall clock does, two events in one sequence stay ordered by their sequence numbers, and the
/// store reports the same highest sequence.
#[test]
fn should_keep_the_sequence_order_when_the_wall_clock_jumps_backwards() {
    for jump_seconds in [1_i64, 5, 60, 3_600, 86_400] {
        let home = tempfile::tempdir().expect("a temporary home");
        let store = store_in(home.path());
        let base = instant("2026-08-31T14:00:00Z");
        let jumped = base - jiff::Span::new().seconds(jump_seconds);

        let mut first = seed(
            EventKind::ProviderEvent,
            sequenced("2026-08-31T14:00:00Z", 100),
            "linux.netlink",
        );
        first.times.observed_at = base;
        let mut second = seed(
            EventKind::ProviderEvent,
            sequenced("2026-08-31T14:00:00Z", 101),
            "linux.netlink",
        );
        // The clock was corrected backwards between the two reports.
        second.times.observed_at = jumped;
        let (first, second) = (first.seal(), second.seal());

        let (order, evidence) = happens_before(&first, &second);
        assert_eq!(
            order,
            Ordering::Before,
            "§25.4: a backward jump of {jump_seconds}s must not reorder one sequence"
        );
        assert!(
            evidence.is_some(),
            "the order rests on the sequence rather than on wall time (§26.1)"
        );

        store
            .append(&[second, first], &[])
            .expect("an append succeeds");
        let sequences = store.source_sequences().expect("sequences answer");
        assert_eq!(sequences[0].highest, 101);
        assert_eq!(sequences[0].lowest, 100);
        assert!(sequences[0].contiguous);
    }
}
