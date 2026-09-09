//! What the default timeline text says (spec v0.5 §11.5, §11.6, §11.7, §39.3).
//!
//! §43.5 of v0.4 makes renderer output a presentation test and never a semantic contract, so
//! nothing here asserts what an event *is*. What it does assert is the two things §11 makes
//! MUST: an event carries a reference a person can type back, and a coverage gap inside the
//! window is drawn even though events sit on both sides of it. §55.5 names a silent gap as a
//! trust-destroying failure, which is why the gap has a test of its own.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_temporal_render::{RenderOptions, timeline};
use ono_value::Value;

mod support;
use support::{event, field_change, gap, provenance, subject};

/// §11.5's own example: a config file changed, an action was requested, the unit followed.
fn nginx_window() -> Vec<Value> {
    vec![
        event(
            "e17510000000000000000001",
            "object.changed",
            "12:17:51.203",
            "config/nginx.conf",
            &[("subject", subject("config/nginx.conf", "file"))],
        ),
        event(
            "e18020000000000000000002",
            "action.requested",
            "12:18:02.011",
            "nginx.service",
            &[(
                "payload",
                support::map(&[("operation", Value::string("reload"))]),
            )],
        ),
        event(
            "e18034000000000000000003",
            "object.changed",
            "12:18:03.401",
            "nginx.service",
            &[
                (
                    "changed_fields",
                    Value::list(vec![field_change(
                        "state",
                        Value::string("activating"),
                        Value::string("active"),
                        "observed",
                    )]),
                ),
                ("provenance", provenance("linux.systemd-dbus")),
            ],
        ),
    ]
}

#[test]
fn should_write_the_clock_the_subject_and_what_happened_when_a_timeline_is_rendered() {
    // §11.5's three columns. The order of the columns is what a reader scans by, so the time
    // starts the row and the subject stands between it and the change.
    let view = support::timeline_record(nginx_window(), Vec::new());
    let lines = timeline(&view, 100, &RenderOptions::default());
    let rendered = lines.join("\n");

    let row = lines
        .iter()
        .find(|line| line.contains("config/nginx.conf"))
        .unwrap_or_else(|| panic!("the first event is drawn, got {rendered}"));
    assert!(
        row.trim_start().starts_with("12:17:51.203"),
        "§11.5: the row opens with the wall clock of the event, got {row:?}"
    );
    assert!(
        row.contains("changed"),
        "§11.5: the row says what happened, got {row:?}"
    );
    assert!(
        rendered.contains("reload requested"),
        "§11.5: an action row names the operation that was asked for, got {rendered}"
    );
    assert!(
        rendered.contains("state") && rendered.contains("active"),
        "§6.2: a field change names the field and the value it took, got {rendered}"
    );
}

#[test]
fn should_abbreviate_the_source_tag_when_the_setting_leaves_it_on() {
    // §11.5: "Source tags SHOULD be abbreviated but inspectable." `linux.systemd-dbus` is the
    // §7.1 class; `[systemd]` is what it reads as in a row.
    let view = support::timeline_record(nginx_window(), Vec::new());
    let rendered = timeline(&view, 100, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("[systemd]"),
        "§11.5: the systemd row carries its abbreviated source tag, got {rendered}"
    );
    assert!(
        rendered.contains("[ono]"),
        "§11.5: an event the shell itself observed is tagged `ono`, got {rendered}"
    );
}

#[test]
fn should_leave_every_source_tag_out_when_the_setting_turns_them_off() {
    // §33's `temporal.ui.show_source_tags` decides whether the tags appear at all.
    let view = support::timeline_record(nginx_window(), Vec::new());
    let options = RenderOptions {
        show_source_tags: false,
        ..RenderOptions::default()
    };
    let rendered = timeline(&view, 100, &options).join("\n");
    assert!(
        !rendered.contains("[systemd]") && !rendered.contains("[ono]"),
        "`temporal.ui.show_source_tags` is off, so no row carries a tag, got {rendered}"
    );
    assert!(
        rendered.contains("nginx.service"),
        "the rows themselves are unchanged, got {rendered}"
    );
}

#[test]
fn should_carry_a_reference_that_can_be_typed_back_when_an_event_is_drawn() {
    // §11.6: "Rendered events MUST expose stable references usable in subsequent commands" —
    // `inspect event @e42`, `at event @e42`, `why event @e42`.
    let view = support::timeline_record(nginx_window(), Vec::new());
    let lines = timeline(&view, 100, &RenderOptions::default());
    let rendered = lines.join("\n");
    let row = lines
        .iter()
        .find(|line| line.contains("config/nginx.conf"))
        .unwrap_or_else(|| panic!("the first event is drawn, got {rendered}"));
    let reference = row
        .split_whitespace()
        .find(|word| word.starts_with("@e"))
        .unwrap_or_else(|| panic!("§11.6: the row carries an `@e…` reference, got {row:?}"));
    let digits = reference
        .trim_start_matches('@')
        .strip_prefix('e')
        .unwrap_or_default();
    assert!(
        !digits.is_empty() && "17510000000000000000001".starts_with(digits),
        "§11.6: the reference identifies this event and nothing else, got {reference:?}"
    );
}

#[test]
fn should_draw_the_coverage_gap_when_events_exist_on_both_sides_of_it() {
    // §11.7: "A gap MUST not be hidden simply because events exist on both sides." §55.5 calls a
    // silent gap trust-destroying, so this is the test the crate exists for.
    let mut events = nginx_window();
    events.push(event(
        "e24120000000000000000004",
        "object.changed",
        "12:24:12.000",
        "nginx.service",
        &[],
    ));
    let view = support::timeline_record(
        events,
        vec![gap(
            "12:20:00",
            "12:24:12",
            "source_disconnected",
            Some("recorder offline"),
        )],
    );
    let lines = timeline(&view, 100, &RenderOptions::default());
    let rendered = lines.join("\n");

    let marker = lines
        .iter()
        .position(|line| line.contains("coverage gap"))
        .unwrap_or_else(|| panic!("§11.7: the gap is drawn, got {rendered}"));
    let before = lines
        .iter()
        .position(|line| line.contains("12:18:03.401"))
        .unwrap_or_else(|| panic!("the event before the gap is drawn, got {rendered}"));
    let after = lines
        .iter()
        .position(|line| line.contains("12:24:12"))
        .unwrap_or_else(|| panic!("the event after the gap is drawn, got {rendered}"));
    assert!(
        before < marker && marker <= after,
        "§11.7: the gap stands between the events on either side, got {rendered}"
    );
    assert!(
        lines[marker].contains("recorder offline"),
        "§7.5: the gap says why nothing is known there, got {:?}",
        lines[marker]
    );
    assert!(
        lines[marker].contains("4m 12s") || lines[marker].contains("4m12s"),
        "§11.7: the gap says how long it lasted, got {:?}",
        lines[marker]
    );
}

#[test]
fn should_say_the_list_was_cut_when_a_limit_truncated_the_window() {
    // §19.4: "A reader must be able to tell a quiet interval from a truncated one."
    let view = support::timeline_of(
        nginx_window(),
        Vec::new(),
        &[("truncated", Value::Bool(true))],
    );
    let rendered = timeline(&view, 100, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("truncated"),
        "§19.4: a cut list says it was cut, got {rendered}"
    );
}

#[test]
fn should_stay_inside_the_terminal_at_every_supported_width() {
    // v0.4 §39.3: 80 and 40 columns both work, so nothing is drawn past the right edge.
    let view = support::timeline_record(
        nginx_window(),
        vec![gap("12:20:00", "12:24:12", "not_recorded", None)],
    );
    for width in [40usize, 80, 120] {
        for line in timeline(&view, width, &RenderOptions::default()) {
            assert!(
                line.chars().count() <= width,
                "§39.3: nothing is drawn past column {width}, got {line:?}"
            );
        }
        let rendered = timeline(&view, width, &RenderOptions::default()).join("\n");
        assert!(
            rendered.contains("coverage gap"),
            "§11.7: the gap survives at {width} columns, got {rendered}"
        );
    }
}

#[test]
fn should_render_the_same_lines_twice_when_the_input_is_the_same() {
    // §50 of v0.2: behaviour is deterministic when output is redirected. A renderer that is a
    // pure function of its input and its width has nothing else to be.
    let view = support::timeline_record(nginx_window(), Vec::new());
    assert_eq!(
        timeline(&view, 80, &RenderOptions::default()),
        timeline(&view, 80, &RenderOptions::default()),
        "the renderer is a pure function of its input and its width"
    );
}

#[test]
fn should_print_the_session_reference_when_the_event_carries_one() {
    // ADR-0660 mints the shortest prefix of the digest that names one event inside a session, and
    // §20.4's completion offers the same string. A row that derived a second spelling would show
    // the reader two names for one event, so the row prints the one the session issued.
    let view = support::timeline_record(
        vec![event(
            "e17510000000000000000001",
            "object.changed",
            "12:17:51.203",
            "config/nginx.conf",
            &[("reference", Value::string("@e42"))],
        )],
        Vec::new(),
    );
    let rendered = timeline(&view, 100, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("@e42"),
        "§11.6: the row carries the reference the session issued, got {rendered}"
    );
    assert!(
        !rendered.contains("@e17510000"),
        "one event has one reference in a session, got {rendered}"
    );
}

#[test]
fn should_tag_a_row_from_its_evidence_source_class_when_the_event_declares_one() {
    // §11.5's `[systemd]` abbreviates the §7.1 evidence source class. `provenance.source` is a
    // different fact — what the observation was read from — and abbreviating it would print
    // `[Manager]` for a systemd row.
    let view = support::timeline_record(
        vec![support::event(
            "e18020000000000000000011",
            "object.changed",
            "12:18:02.044",
            "nginx.service",
            &[
                ("source", Value::string("linux.systemd-dbus")),
                (
                    "provenance",
                    support::map(&[
                        ("provider", Value::string("systemd")),
                        ("observed", Value::Null),
                        ("source", Value::string("org.freedesktop.systemd1.Manager")),
                        ("link", Value::string("local")),
                        ("schema", Value::string("ono.temporal-event/1")),
                    ]),
                ),
            ],
        )],
        Vec::new(),
    );
    let rendered = timeline(&view, 100, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("[systemd]"),
        "§11.5: the tag abbreviates the §7.1 class, got {rendered}"
    );
    assert!(
        !rendered.contains("[Manager]"),
        "`provenance.source` is what the observation was read from, not an evidence class, got {rendered}"
    );
}
