//! The gap frame, and the frames that are never drawn (spec v0.5 §18.5, §18.6, §45.1, §55.5).
//!
//! §18.6: "The map MUST NOT continue showing the last state with a silently advancing
//! timestamp." §45.1 forbids fake tape-scrubbing, random visual noise in historical mode,
//! invented frames between unsupported states and `DECRYPTING PAST...` by name. A renderer that
//! is a pure function of a record and a width can invent none of them, and these tests are how
//! that stays true.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_temporal_render::{RenderOptions, gap_frame};

mod support;
use support::gap_record;

#[test]
fn should_name_the_gap_and_the_last_supported_state_when_the_cursor_steps_into_a_gap() {
    // §18.6's own frame: what is missing, when it starts and ends, why, and the last instant
    // anything was actually known.
    let record = gap_record(
        "12:40:18",
        "12:44:30",
        "source_disconnected",
        Some("recorder disconnected"),
    );
    let lines = gap_frame(&record, 60, &RenderOptions::default());
    let rendered = lines.join("\n");

    assert!(
        lines
            .first()
            .is_some_and(|line| line.contains("HISTORY GAP")),
        "§18.6: the frame opens by naming what it is, got {rendered}"
    );
    assert!(
        rendered.contains("12:40:18") && rendered.contains("12:44:30"),
        "§18.6: the frame states the interval, got {rendered}"
    );
    assert!(
        rendered.contains("recorder disconnected"),
        "§7.5: the frame says why nothing is known there, got {rendered}"
    );
    assert!(
        rendered.contains("last supported state shown at 12:40:18"),
        "§18.6: the frame names the last instant anything was known, got {rendered}"
    );
}

#[test]
fn should_fall_back_to_the_reason_when_the_gap_carries_no_detail() {
    // `ono.temporal-gap/1` makes `detail` nullable, and §35.3 of v0.2 forbids inventing a value
    // for what is unknown. The closed reason vocabulary of §7.5 is what remains, in words.
    let record = gap_record("12:40:18", "12:44:30", "retention_expired", None);
    let rendered = gap_frame(&record, 60, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("retention expired"),
        "§7.5: the reason is what the frame says when there is no detail, got {rendered}"
    );
}

#[test]
fn should_invent_no_frame_between_two_unsupported_states() {
    // §18.5 and §45.1. The frame carries the two instants the gap record states and the one it
    // derives from them, and no third instant of its own; and none of the words §45.1 forbids.
    let record = gap_record("12:40:18", "12:44:30", "source_disconnected", None);
    let lines = gap_frame(&record, 60, &RenderOptions::default());
    let rendered = lines.join("\n");
    let lower = rendered.to_ascii_lowercase();

    for forbidden in ["decrypting", "scrub", "tape", "rewinding"] {
        assert!(
            !lower.contains(forbidden),
            "§45.1: `{forbidden}` is theatre rather than evidence, got {rendered}"
        );
    }
    let clocks: Vec<&str> = rendered
        .split(|c: char| !c.is_ascii_digit() && c != ':')
        .filter(|word| word.len() == 8 && word.matches(':').count() == 2)
        .collect();
    assert!(
        clocks
            .iter()
            .all(|clock| *clock == "12:40:18" || *clock == "12:44:30"),
        "§18.5: the frame shows only instants the record states, got {clocks:?} in {rendered}"
    );
}

#[test]
fn should_render_the_same_frame_twice_when_nothing_changed() {
    // §45.1 forbids random visual noise in historical mode. A pure function has none to add.
    let record = gap_record("12:40:18", "12:44:30", "not_recorded", None);
    assert_eq!(
        gap_frame(&record, 60, &RenderOptions::default()),
        gap_frame(&record, 60, &RenderOptions::default()),
        "the frame is a pure function of the gap and the width"
    );
    for width in [40usize, 80] {
        for line in gap_frame(&record, width, &RenderOptions::default()) {
            assert!(
                line.chars().count() <= width,
                "§39.3: nothing is drawn past column {width}, got {line:?}"
            );
        }
    }
}
