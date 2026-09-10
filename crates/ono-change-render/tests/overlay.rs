//! `map --plan`'s overlay on the drawn map: §21.2, §21.3 and Appendix E.7.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::{Charset, plan_overlay};
use ono_value::Value;

mod support;
use support::{contains, list, map, s};

/// The map as `ono-spatial-render` drew it: each line, and the node it draws where it draws one.
fn drawn() -> Vec<(&'static str, Option<&'static str>)> {
    vec![
        ("MAP host-1 / 3 nodes", None),
        ("", None),
        ("  host-1", Some("host:host-1")),
        ("  +-- nginx.service  running", Some("service:nginx")),
        ("  |   +-- process/1842", Some("process:1842")),
        ("  +-- sshd.service  running", Some("service:sshd")),
    ]
}

/// The overlay `map --plan` attaches: one drawn object covered, one drawn object uncovered, one
/// object the map did not draw.
fn overlay() -> Value {
    map(&[
        ("plan", s("a82f1c0d9e4b7a63")),
        ("state", s("sealed")),
        ("protection", s("partially-protected")),
        (
            "objects",
            Value::list([
                map(&[
                    ("object", s("nginx.service")),
                    ("identity", s("nginx.service")),
                    ("place", s("service:nginx")),
                    ("covered", Value::Bool(true)),
                    ("expectation", s("replacement expected")),
                ]),
                map(&[
                    ("object", s("sshd.service")),
                    ("identity", s("sshd.service")),
                    ("place", s("service:sshd")),
                    ("covered", Value::Bool(false)),
                    ("expectation", s("modified")),
                ]),
                map(&[
                    ("object", s("/etc/nginx/nginx.conf")),
                    ("identity", s("/etc/nginx/nginx.conf")),
                    ("place", Value::Null),
                    ("covered", Value::Bool(true)),
                    ("expectation", s("modified")),
                    ("covered_by", list(&["zfs:rpool/ROOT@ono-a82f"])),
                ]),
            ]),
        ),
    ])
}

fn line_of<'a>(lines: &'a [String], needle: &str) -> &'a str {
    lines
        .iter()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("`{needle}` is drawn, got {lines:?}"))
}

#[test]
fn should_name_the_plan_and_its_protection_above_the_map() {
    let lines = plan_overlay(&drawn(), &overlay(), 80, Charset::Ascii);
    let first = lines.first().expect("a heading");
    assert!(
        first.contains("a82f") && first.contains("PARTIALLY_PROTECTED"),
        "§21.1: the overlay says which plan it projects and how protected it is. Got {first:?}"
    );
}

#[test]
fn should_mark_a_drawn_object_the_plan_touches_and_its_coverage() {
    let lines = plan_overlay(&drawn(), &overlay(), 80, Charset::Ascii);
    let nginx = line_of(&lines, "nginx.service");
    assert!(
        nginx.contains("~ replacement expected"),
        "§21.2: a proposed effect is an overlay on the object it touches. Got {nginx:?}"
    );
    assert!(
        nginx.contains("<->"),
        "§21.3 and Appendix E.7: an object a recovery asset covers shows coverage. Got {nginx:?}"
    );
}

#[test]
fn should_leave_a_drawn_object_the_plan_does_not_touch_as_it_was() {
    let lines = plan_overlay(&drawn(), &overlay(), 80, Charset::Ascii);
    assert_eq!(
        line_of(&lines, "process/1842"),
        "  |   +-- process/1842",
        "§21.2: the current world is the base layer, and §21.4 invents nothing about it"
    );
}

#[test]
fn should_say_a_touched_object_is_not_covered_rather_than_leaving_the_mark_out() {
    let lines = plan_overlay(&drawn(), &overlay(), 80, Charset::Ascii);
    let sshd = line_of(&lines, "sshd.service");
    assert!(
        sshd.contains("~ modified") && sshd.contains("not covered") && !sshd.contains("<->"),
        "Appendix E.8: coverage summaries show what they exclude. Got {sshd:?}"
    );
}

#[test]
fn should_list_a_plan_object_the_map_did_not_draw_with_the_asset_that_covers_it() {
    let lines = plan_overlay(&drawn(), &overlay(), 80, Charset::Ascii);
    let heading = lines
        .iter()
        .position(|line| line == "not on this map")
        .expect("§21.2 draws over the map, so the rest of the plan is listed below it");
    let conf = &lines[heading + 1];
    assert!(
        conf.contains("/etc/nginx/nginx.conf")
            && conf.contains("~")
            && conf.contains("<-> zfs:rpool/ROOT@ono-a82f"),
        "§21.3's example: `nginx.conf  ~  <-> zfs:rpool/ROOT@ono-a82f`. Got {conf:?}"
    );
}

#[test]
fn should_draw_the_coverage_mark_in_the_terminal_alphabet() {
    let lines = plan_overlay(&drawn(), &overlay(), 80, Charset::Unicode);
    assert!(
        contains(&lines, "\u{2194}"),
        "§20.3: better glyphs where the terminal is known to support them"
    );
}

#[test]
fn should_lay_the_overlay_out_at_the_width_it_was_given() {
    for width in [40usize, 80] {
        for line in plan_overlay(&drawn(), &overlay(), width, Charset::Ascii) {
            assert!(
                line.chars().count() <= width,
                "v0.4 §39.3: the map adapts to the width. Got {line:?}"
            );
        }
    }
}
