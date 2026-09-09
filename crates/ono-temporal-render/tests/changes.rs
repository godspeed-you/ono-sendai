//! `changes` as text (spec v0.5 §13.2, §13.3, §13.4).
//!
//! §13.3 fixes the shape — a section per change class, an object per line, and a changed
//! object's fields beneath it. §13.4 fixes the rule that matters: "If one side lacks enough
//! evidence, the field MUST be reported as unknown rather than fabricated", with the coverage
//! that explains the ignorance beside it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_temporal_render::{RenderOptions, changes};
use ono_value::{RecordValue, Value};

mod support;
use support::{change, field_change};

/// §13.3's own window: two objects appeared, one went, and two changed.
fn window() -> Vec<RecordValue> {
    vec![
        change("added", "process/7128", "process", Vec::new()),
        change("removed", "process/6902", "process", Vec::new()),
        change(
            "changed",
            "service/backup",
            "service",
            vec![field_change(
                "state",
                Value::string("running"),
                Value::string("failed"),
                "observed",
            )],
        ),
        change(
            "changed",
            "filesystem/data",
            "filesystem",
            vec![field_change(
                "used",
                Value::string("81.2%"),
                Value::string("94.1%"),
                "observed",
            )],
        ),
    ]
}

#[test]
fn should_write_a_section_per_class_and_an_object_per_line_when_changes_are_rendered() {
    let lines = changes(&window(), 100, &RenderOptions::default());
    let rendered = lines.join("\n");

    for heading in ["ADDED", "REMOVED", "CHANGED"] {
        assert!(
            lines.iter().any(|line| line.trim() == heading),
            "§13.3: `{heading}` is a section of its own, got {rendered}"
        );
    }
    for object in [
        "process/7128",
        "process/6902",
        "service/backup",
        "filesystem/data",
    ] {
        assert!(
            rendered.contains(object),
            "§13.3: `{object}` gets a line, got {rendered}"
        );
    }
    let added = lines
        .iter()
        .position(|line| line.trim() == "ADDED")
        .expect("the added section");
    let removed = lines
        .iter()
        .position(|line| line.trim() == "REMOVED")
        .expect("the removed section");
    let changed = lines
        .iter()
        .position(|line| line.trim() == "CHANGED")
        .expect("the changed section");
    assert!(
        added < removed && removed < changed,
        "§13.3: the sections come in that order, got {rendered}"
    );
}

#[test]
fn should_write_a_changed_field_as_before_to_after_beneath_its_object() {
    let lines = changes(&window(), 100, &RenderOptions::default());
    let rendered = lines.join("\n");
    let object = lines
        .iter()
        .position(|line| line.contains("service/backup"))
        .expect("the changed service");
    let field = lines
        .iter()
        .position(|line| line.contains("running -> failed"))
        .unwrap_or_else(|| panic!("§13.3: the field reads `before -> after`, got {rendered}"));
    assert!(
        field > object,
        "§13.3: the field sits beneath its object, got {rendered}"
    );
    assert!(
        lines[field].starts_with("    "),
        "§13.3: and is indented under it, got {:?}",
        lines[field]
    );
    assert!(
        lines[field].contains("state"),
        "§13.3: the field is named, got {:?}",
        lines[field]
    );
}

#[test]
fn should_report_a_side_with_no_evidence_as_unknown_with_the_coverage_that_explains_it() {
    // §13.4, in full. `from` is unknown because nothing observed the field before the window
    // opened, and the coverage line is what turns an absence into an explanation.
    let unknown = change(
        "changed",
        "filesystem/data",
        "filesystem",
        vec![field_change(
            "used",
            Value::Null,
            Value::string("94.1%"),
            "unknown",
        )],
    );
    let lines = changes(&[unknown], 100, &RenderOptions::default());
    let rendered = lines.join("\n");

    assert!(
        lines
            .iter()
            .any(|line| line.contains("from") && line.contains("unknown")),
        "§13.4: the side with no evidence reads `unknown`, got {rendered}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("to") && line.contains("94.1%")),
        "§13.4: the side that was observed still reads its value, got {rendered}"
    );
    let coverage = lines
        .iter()
        .find(|line| line.contains("coverage"))
        .unwrap_or_else(|| panic!("§13.4: the coverage explains the unknown, got {rendered}"));
    assert!(
        coverage.contains("complete")
            || coverage.contains("partial")
            || coverage.contains("uncertain"),
        "§13.4: the coverage line names the composed headline, got {coverage:?}"
    );
    assert!(
        coverage.contains("before 12:17"),
        "§13.4: and the boundary it holds before, got {coverage:?}"
    );
    assert!(
        !rendered.contains("-> 0") && !rendered.contains("0 ->"),
        "§13.4: an unobserved side is never a zero, got {rendered}"
    );
    assert!(
        !rendered.contains(" ->  ") && !rendered.contains("  -> "),
        "§13.4: and never an empty string, got {rendered}"
    );
}

#[test]
fn should_render_nothing_when_the_window_holds_no_change() {
    assert!(changes(&[], 80, &RenderOptions::default()).is_empty());
}

#[test]
fn should_name_both_ends_when_a_relation_changed() {
    // §13.2 keeps relation changes apart from object changes, and §6.4 makes an edge two ends.
    let record = support::relation_change("relation_added", "backup", "nas01:22", "connects_to");
    let rendered = changes(&[record], 100, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("RELATION ADDED"),
        "§13.2: a relation change is its own class, got {rendered}"
    );
    assert!(
        rendered.contains("backup -> nas01:22"),
        "§6.4: both ends of the edge are named, got {rendered}"
    );
    assert!(
        rendered.contains("connects_to"),
        "§6.4: and the relation type, got {rendered}"
    );
}

#[test]
fn should_stay_inside_the_terminal_at_every_supported_width() {
    for width in [40, 60, 80, 100, 120] {
        for line in changes(&window(), width, &RenderOptions::default()) {
            assert!(
                line.chars().count() <= width,
                "a line overflowed {width} columns: {line:?}"
            );
        }
    }
}
