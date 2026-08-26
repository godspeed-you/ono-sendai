//! Nested values in the ASCII tree of spec §22.4, and the remaining views of spec §13.6.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the way a test does"
)]

use std::sync::Arc;

use bytes::Bytes;
use jiff::tz::TimeZone;
use ono_render::{Layout, Presentation, Renderer, Theme, View};
use ono_value::{
    ByteSize, FieldDef, FieldType, MapValue, Provenance, RecordValue, Schema, SchemaId, Value,
};

fn renderer() -> Renderer {
    Renderer::in_zone(TimeZone::UTC)
}

fn map(pairs: &[(&str, Value)]) -> Value {
    let mut map = MapValue::new();
    for (key, value) in pairs {
        map.insert((*key).into(), value.clone());
    }
    Value::Map(Arc::new(map))
}

fn record() -> Value {
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.demo", 1), "Demo")
            .field(FieldDef::new("pid", FieldType::Int).required())
            .field(FieldDef::new("owner", FieldType::Map).nullable())
            .field(FieldDef::new("memory", FieldType::ByteSize).nullable())
            .field(FieldDef::new("args", FieldType::list(FieldType::String)).nullable())
            .identity(["pid"])
            .build()
            .unwrap(),
    );
    RecordValue::builder(
        schema,
        Provenance::local("demo", SchemaId::new("ono.demo", 1)),
    )
    .set("pid", Value::Int(812))
    .unwrap()
    .set("owner", map(&[("name", Value::string("postgres"))]))
    .unwrap()
    .set("memory", Value::Null)
    .unwrap()
    .set(
        "args",
        Value::list([Value::string("-D"), Value::string("/var/lib")]),
    )
    .unwrap()
    .build()
    .into_value()
}

#[test]
fn should_render_a_nested_record_as_a_tree() {
    let lines = Layout::new(80).render_tree(&renderer().tree(&record()));
    assert_eq!(lines[0], "ono.demo/1");
    assert_eq!(lines[1], "+-- pid: 812");
    assert_eq!(lines[2], "+-- owner: map");
    assert_eq!(lines[3], "|   +-- name: postgres");
}

#[test]
fn should_show_null_as_null_in_a_tree() {
    let lines = Layout::new(80).render_tree(&renderer().tree(&record()));
    assert!(
        lines.iter().any(|line| line.ends_with("memory: null")),
        "spec §10.5: an unknown is visible, got {lines:#?}"
    );
}

#[test]
fn should_render_a_list_field_with_one_child_per_element() {
    let lines = Layout::new(80).render_tree(&renderer().tree(&record()));
    let at = lines
        .iter()
        .position(|line| line.contains("args:"))
        .unwrap_or_else(|| panic!("no args node in {lines:#?}"));
    assert_eq!(lines[at], "+-- args: list (2)");
    assert_eq!(lines[at + 1], "|   +-- -D");
    assert_eq!(lines[at + 2], "|   +-- /var/lib");
}

#[test]
fn should_render_a_bare_map_as_a_tree() {
    let value = map(&[
        ("name", Value::string("nginx")),
        (
            "memory",
            Value::ByteSize(ByteSize::from_bytes(1_288_490_188)),
        ),
    ]);
    let lines = Layout::new(80).render_tree(&renderer().tree(&value));
    assert_eq!(lines[0], "map");
    assert!(
        lines.iter().any(|line| line.ends_with("memory: 1.20 GiB")),
        "got {lines:#?}"
    );
}

#[test]
fn should_never_draw_a_tree_wider_than_the_terminal() {
    let lines = Layout::new(24).render_tree(&renderer().tree(&record()));
    for line in &lines {
        assert!(
            unicode_width::UnicodeWidthStr::width(line.as_str()) <= 24,
            "{line:?}"
        );
    }
}

#[test]
fn should_emit_no_escape_sequences_in_a_tree_when_the_destination_takes_no_colour() {
    let tree = renderer().tree(&record());
    for line in Layout::new(80).render_tree_styled(&tree, &Theme::default(), Presentation::Pipe) {
        assert!(!line.contains('\u{1b}'), "{line:?}");
    }
}

#[test]
fn should_paint_a_tree_on_a_terminal_without_changing_its_shape() {
    let tree = renderer().tree(&record());
    let plain = Layout::new(80).render_tree(&tree);
    let painted =
        Layout::new(80).render_tree_styled(&tree, &Theme::default(), Presentation::Terminal);
    assert!(painted.iter().any(|line| line.contains('\u{1b}')));
    let stripped: Vec<String> = painted.iter().map(|line| strip(line)).collect();
    assert_eq!(stripped, plain);
}

#[test]
fn should_render_the_canonical_form_in_the_raw_view() {
    let lines = Layout::new(80).render_view(
        &renderer(),
        &[
            Value::ByteSize(ByteSize::from_bytes(1_288_490_188)),
            Value::Null,
        ],
        View::Raw,
    );
    assert_eq!(
        lines,
        ["1288490188B", "null"],
        "spec §33.5: the raw view shows canonical values, not display forms"
    );
}

#[test]
fn should_render_a_hex_dump_in_the_hex_view() {
    let value = Value::Bytes(Bytes::from_static(b"ono\x00\xff"));
    let lines = Layout::new(80).render_view(&renderer(), &[value], View::Hex);
    assert_eq!(lines.len(), 1, "got {lines:#?}");
    assert!(
        lines[0].starts_with("00000000  6f 6e 6f 00 ff"),
        "got {:?}",
        lines[0]
    );
    assert!(
        lines[0].ends_with("|ono..|"),
        "a byte that is not printable shows as a full stop, got {:?}",
        lines[0]
    );
}

#[test]
fn should_start_a_new_hex_line_every_sixteen_bytes() {
    let value = Value::Bytes(Bytes::from(vec![0x41; 20]));
    let lines = Layout::new(80).render_view(&renderer(), &[value], View::Hex);
    assert_eq!(lines.len(), 2, "got {lines:#?}");
    assert!(
        lines[1].starts_with("00000010  41 41 41 41 "),
        "got {:?}",
        lines[1]
    );
    for line in &lines {
        assert!(
            unicode_width::UnicodeWidthStr::width(line.as_str()) <= 80,
            "{line:?}"
        );
    }
}

#[test]
fn should_report_a_value_with_no_byte_form_in_the_hex_view() {
    let lines = Layout::new(80).render_view(&renderer(), &[Value::Int(1)], View::Hex);
    assert!(
        lines[0].contains("no raw byte form"),
        "spec §12.3: refuse rather than invent an encoding, got {lines:#?}"
    );
}

#[test]
fn should_render_one_tree_per_value_in_the_tree_view() {
    let lines = Layout::new(80).render_view(&renderer(), &[record(), record()], View::Tree);
    assert_eq!(
        lines.iter().filter(|line| *line == "ono.demo/1").count(),
        2,
        "got {lines:#?}"
    );
}

/// Removes every ANSI escape sequence, so a painted line can be compared with a plain one.
fn strip(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            for escaped in chars.by_ref() {
                if escaped == 'm' {
                    break;
                }
            }
        } else {
            out.push(character);
        }
    }
    out
}
