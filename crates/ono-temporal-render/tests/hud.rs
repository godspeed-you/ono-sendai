//! The temporal HUD, the paused marker, the return-to-now summary and the full-screen timeline
//! (spec v0.5 §4.6, §8.6, §18.2, §18.7, §19.2, §19.3, §45.2).
//!
//! §4.6: "Historical context MUST be visually obvious", and "the distinction MUST remain visible
//! in monochrome and plain text". §8.6 keeps `[PAST]` and `[PAST?]` apart on coverage rather than
//! on whether an event happens to exist near the coordinate.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_temporal_render::{
    RenderOptions, paused_marker, return_to_now, temporal_hud, timeline_view,
};
use ono_value::Value;

mod support;
use support::{at, change, context, coverage, event, field_change, gap, subject};

#[test]
fn should_mark_the_past_in_words_when_the_context_is_historical() {
    // §4.6's minimum: the coordinate and the marker, both in text.
    let historical = context(
        "historical",
        Some("12:07:14"),
        Some("[PAST]"),
        coverage("complete", &[("service.state", "complete")], Vec::new()),
    );
    let lines = temporal_hud(&historical, 80, &RenderOptions::default());
    let rendered = lines.join("\n");
    assert!(
        rendered.contains("@12:07:14"),
        "§4.6: the HUD names the coordinate, got {rendered}"
    );
    assert!(
        rendered.contains("[PAST]"),
        "§4.6: the marker is a word, got {rendered}"
    );
    assert!(
        rendered.contains("-10m"),
        "§4.2: the HUD keeps what was asked for beside what it resolved to, got {rendered}"
    );
}

#[test]
fn should_say_why_the_reconstruction_is_uncertain_when_the_marker_is_qualified() {
    // §8.6: `[PAST?]` is a claim about coverage, so the HUD says what the coverage was.
    let uncertain = context(
        "historical",
        Some("12:07:14"),
        Some("[PAST?]"),
        coverage(
            "partial",
            &[("process.existence", "partial")],
            vec![gap("12:05:00", "12:06:00", "not_recorded", None)],
        ),
    );
    let rendered = temporal_hud(&uncertain, 80, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("[PAST?]"),
        "§8.6: partial coverage carries the qualified marker, got {rendered}"
    );
    assert!(
        rendered.contains("partial"),
        "§8.6: the HUD says what the coverage was, got {rendered}"
    );
    assert!(
        rendered.contains("gap"),
        "§11.7: a gap behind the coordinate is never silent, got {rendered}"
    );
}

#[test]
fn should_say_nothing_temporal_when_the_context_is_the_present() {
    // §4.1: the present is the default, and §4.6 makes the marker the sign of the past. A marker
    // that always appeared would mark nothing.
    let present = context("present", None, None, Value::Null);
    assert!(
        temporal_hud(&present, 80, &RenderOptions::default()).is_empty(),
        "the present carries no historical marker"
    );
}

#[test]
fn should_name_the_frozen_instant_when_the_view_is_paused() {
    // §18.2's HUD line. Pausing freezes the view's cursor and nothing else, so the marker states
    // the instant the view is showing.
    let marker = paused_marker(&at("14:03:12.410"), &RenderOptions::default());
    assert_eq!(
        marker, "PAUSED @14:03:12.410",
        "§18.2: the paused HUD states the frozen coordinate"
    );
}

#[test]
fn should_summarise_what_changed_when_the_cursor_returns_to_now() {
    // §18.7's summary, built from the canonical `changes` records.
    let changes = vec![
        change("added", "process/2741", "process", Vec::new()),
        change("added", "process/2742", "process", Vec::new()),
        change("added", "process/2743", "process", Vec::new()),
        change("removed", "connection/8080", "connection", Vec::new()),
        change(
            "changed",
            "nginx.service",
            "service",
            vec![field_change(
                "state",
                Value::string("active"),
                Value::string("failed"),
                "observed",
            )],
        ),
    ];
    let lines = return_to_now(&changes, 80);
    let rendered = lines.join("\n");

    assert!(
        lines
            .first()
            .is_some_and(|line| line.contains("returned to now")),
        "§18.7: the summary says where the cursor went, got {rendered}"
    );
    assert!(
        rendered.contains("+3 processes"),
        "§18.7: appearances are counted by type, got {rendered}"
    );
    assert!(
        rendered.contains("-1 connection"),
        "§18.7: disappearances are counted by type, got {rendered}"
    );
    assert!(
        rendered.contains("nginx.service") && rendered.contains("active -> failed"),
        "§18.7: a state change is named rather than counted, got {rendered}"
    );
}

#[test]
fn should_claim_nothing_when_nothing_changed_while_the_cursor_was_away() {
    let rendered = return_to_now(&[], 80).join("\n");
    assert!(
        rendered.contains("returned to now"),
        "the cursor still moved, got {rendered}"
    );
    assert!(
        !rendered.contains('+') && !rendered.contains('-'),
        "an empty change set is counted as nothing, got {rendered}"
    );
}

#[test]
fn should_name_the_place_the_window_the_evidence_and_the_keys_in_the_full_screen_timeline() {
    // §19.2's information architecture, which is normative even though its border style is not:
    // the place and the window across the top, the events with a cursor, the evidence and
    // coverage line, and the keys of §19.3.
    let events = vec![
        event(
            "e14030600000000000000001",
            "object.changed",
            "12:18:01.000",
            "config/nginx.conf",
            &[("subject", subject("config/nginx.conf", "file"))],
        ),
        event(
            "e14031200000000000000002",
            "object.appeared",
            "12:18:02.000",
            "process/2741",
            &[("subject", subject("process/2741", "process"))],
        ),
    ];
    let view = support::timeline_record(events, Vec::new());
    let options = RenderOptions {
        cursor: Some("@e14031200000000000000002".to_owned()),
        ..RenderOptions::default()
    };
    let lines = timeline_view(&view, 100, &options);
    let rendered = lines.join("\n");

    assert!(
        lines
            .first()
            .is_some_and(|line| line.contains("local/service/nginx")),
        "§19.2: the place stands across the top, got {rendered}"
    );
    assert!(
        rendered.contains("12:17") && rendered.contains("12:25"),
        "§19.2: the window is drawn beside the place, got {rendered}"
    );
    let cursor = lines
        .iter()
        .find(|line| line.contains("process/2741"))
        .unwrap_or_else(|| panic!("the selected row is drawn, got {rendered}"));
    assert!(
        cursor.starts_with('>'),
        "§19.2: the cursor marks the selected event, got {cursor:?}"
    );
    assert!(
        rendered.contains("evidence") && rendered.contains("coverage"),
        "§19.2: the evidence and coverage line is part of the layout, got {rendered}"
    );
    for key in ["Enter", "W", "M", "A", "Esc"] {
        assert!(
            rendered.contains(key),
            "§19.3: the legend names the key `{key}`, got {rendered}"
        );
    }
}

#[test]
fn should_stay_inside_the_terminal_and_use_no_colour_at_every_width() {
    // §39.3 of v0.4 and §45.2 of v0.5, over every full-screen surface at once.
    let view = support::timeline_record(
        vec![event(
            "e14030600000000000000001",
            "object.changed",
            "12:18:01.000",
            "config/nginx.conf",
            &[],
        )],
        vec![gap("12:20:00", "12:24:12", "not_recorded", None)],
    );
    let historical = context(
        "historical",
        Some("12:07:14"),
        Some("[PAST?]"),
        coverage("partial", &[("service.state", "partial")], Vec::new()),
    );
    let changes = vec![change("added", "process/2741", "process", Vec::new())];
    for width in [40usize, 80, 120] {
        let mut lines = timeline_view(&view, width, &RenderOptions::default());
        lines.extend(temporal_hud(&historical, width, &RenderOptions::default()));
        lines.extend(return_to_now(&changes, width));
        for line in lines {
            assert!(
                line.chars().count() <= width,
                "§39.3: nothing is drawn past column {width}, got {line:?}"
            );
            assert!(
                !line.contains('\u{1b}'),
                "§45.2: meaning is carried by words, got {line:?}"
            );
        }
    }
}
