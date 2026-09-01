//! The renderer snapshots spec v0.4 §43.5 asks for: the same map drawn at 40, 80, 120 and 200
//! columns.
//!
//! §43.5 is explicit that these are **presentation tests and MUST NOT become semantic
//! contracts**. Nothing here asserts what a place *is*; the data contract lives in
//! `crates/ono-spatial-query` and in the `spatial_map` suite. What these snapshots hold
//! is that the drawing stays inside the terminal it was given, that the same nodes are drawn at
//! every width (§2.19: the user's place survives rendering changes), and that a change of layout
//! is visible in a diff rather than silent.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_spatial_render::{Charset, Key, Keymap, MapView};
use ono_value::{FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, Value};

/// The four widths §43.5 names.
const WIDTHS: [usize; 4] = [40, 80, 120, 200];

fn schema(id: &str, fields: &[(&str, FieldType)]) -> std::sync::Arc<Schema> {
    let mut builder = Schema::builder(SchemaId::new(id, 1), id);
    for (name, kind) in fields {
        builder = builder.field(FieldDef::new(name, kind.clone()));
    }
    std::sync::Arc::new(builder.build().expect("a well-formed schema"))
}

/// A map with labels long enough that 40 columns must do something about them and 200 need not.
fn map_record() -> RecordValue {
    let node_schema = schema(
        "ono.map-node",
        &[
            ("id", FieldType::String),
            ("label", FieldType::String),
            ("type", FieldType::String),
            ("state", FieldType::String),
        ],
    );
    let edge_schema = schema(
        "ono.map-edge",
        &[
            ("source", FieldType::String),
            ("target", FieldType::String),
            ("kind", FieldType::String),
            ("relation", FieldType::String),
        ],
    );
    let map_schema = schema(
        "ono.spatial-map",
        &[
            ("center", FieldType::String),
            ("zoom_level", FieldType::Int),
            ("completeness", FieldType::String),
            ("nodes", FieldType::list(FieldType::Any)),
            ("edges", FieldType::list(FieldType::Any)),
        ],
    );
    let node = |id: &str, label: &str, kind: &str, state: &str| {
        Value::Record(std::sync::Arc::new(
            RecordValue::builder(
                node_schema.clone(),
                Provenance::local("test", SchemaId::new("ono.map-node", 1)),
            )
            .set("id", Value::string(id))
            .expect("id")
            .set("label", Value::string(label))
            .expect("label")
            .set("type", Value::string(kind))
            .expect("type")
            .set("state", Value::string(state))
            .expect("state")
            .build(),
        ))
    };
    let edge = |source: &str, target: &str, relation: &str| {
        Value::Record(std::sync::Arc::new(
            RecordValue::builder(
                edge_schema.clone(),
                Provenance::local("test", SchemaId::new("ono.map-edge", 1)),
            )
            .set("source", Value::string(source))
            .expect("source")
            .set("target", Value::string(target))
            .expect("target")
            .set("kind", Value::string("hierarchy"))
            .expect("kind")
            .set("relation", Value::string(relation))
            .expect("relation")
            .build(),
        ))
    };
    RecordValue::builder(
        map_schema,
        Provenance::local("test", SchemaId::new("ono.spatial-map", 1)),
    )
    .set("center", Value::string("root"))
    .expect("center")
    .set("zoom_level", Value::Int(1))
    .expect("zoom")
    .set("completeness", Value::string("bounded"))
    .expect("completeness")
    .set(
        "nodes",
        Value::list(vec![
            node("root", "SYSTEM", "System", "local"),
            node("compute", "COMPUTE", "Domain", "available"),
            node("network", "NETWORK", "Domain", "available"),
            node(
                "unit",
                "a-long-unit-name-that-a-narrow-terminal-cannot-hold.service",
                "Service",
                "failed",
            ),
        ]),
    )
    .expect("nodes")
    .set(
        "edges",
        Value::list(vec![
            edge("root", "compute", "contains"),
            edge("root", "network", "contains"),
            edge("compute", "unit", "runs"),
        ]),
    )
    .expect("edges")
    .build()
}

fn drawn(width: usize) -> Vec<String> {
    let mut view = MapView::new(
        &map_record(),
        width,
        12,
        Charset::Ascii,
        Keymap::default_bindings(),
    );
    view.apply(Key::Down);
    view.frame()
}

#[test]
fn should_draw_the_same_map_inside_every_width_the_spec_names() {
    for width in WIDTHS {
        let frame = drawn(width);
        assert_eq!(
            frame.len(),
            12,
            "the frame at {width} columns is as tall as the terminal"
        );
        for line in &frame {
            assert!(
                line.chars().count() <= width,
                "at {width} columns `{line}` is drawn past the right edge"
            );
            assert!(
                !line.contains('\u{1b}'),
                "at {width} columns the drawing carries an escape sequence: {line:?}"
            );
        }
    }
}

#[test]
fn should_show_every_node_at_every_width_so_a_narrow_terminal_hides_nothing() {
    // §2.19 and §39.3: a narrower terminal may collapse the layout; it may not remove a place.
    for width in WIDTHS {
        let frame = drawn(width).join("\n");
        for label in ["SYSTEM", "COMPUTE", "NETWORK"] {
            assert!(
                frame.contains(label),
                "at {width} columns the map does not show {label}:\n{frame}"
            );
        }
        assert!(
            frame.contains("a-long-unit-name"),
            "at {width} columns the map does not show the long-named service:\n{frame}"
        );
    }
}

#[test]
fn should_mark_exactly_one_focused_line_at_every_width() {
    // §39.1: the focused item is legible without colour, whatever the terminal is.
    for width in WIDTHS {
        let frame = drawn(width);
        let marked: Vec<&String> = frame.iter().filter(|line| line.starts_with('>')).collect();
        assert_eq!(
            marked.len(),
            1,
            "at {width} columns exactly one line carries the cursor, got {frame:?}"
        );
    }
}

#[test]
fn should_render_the_snapshot_the_spec_asks_to_be_kept_at_each_width() {
    // §43.5: the snapshot itself. It exists so that a layout change is visible in a diff; it is
    // a presentation test, and no other test may depend on its text.
    let snapshot: String = WIDTHS
        .iter()
        .map(|width| {
            format!(
                "== {width} columns ==\n{}\n",
                drawn(*width)
                    .iter()
                    .map(|line| format!("|{}|", line.trim_end()))
                    .collect::<Vec<String>>()
                    .join("\n")
            )
        })
        .collect();
    assert_eq!(snapshot, EXPECTED, "the map's layout changed:\n{snapshot}");
}

const EXPECTED: &str = r"== 40 columns ==
| map SYSTEM  L1  4 nodes  bounded|
||
|    SYSTEM  local|
|>   +- COMPUTE  available|
|    |  `- a-long-unit-name-that-a-narrow|
|    `- NETWORK  available|
||
||
||
||
||
| Enter enter  b back  u up  Esc close  ?|
== 80 columns ==
| map SYSTEM  L1  4 nodes  bounded|
||
|    SYSTEM  local|
|>   +- COMPUTE  available|
|    |  `- a-long-unit-name-that-a-narrow-terminal-cannot-hold.service  failed|
|    `- NETWORK  available|
||
||
||
||
||
| Enter enter  b back  u up  Esc close  ? help|
== 120 columns ==
| map SYSTEM  L1  4 nodes  bounded|
||
|    SYSTEM  local|
|>   +- COMPUTE  available|
|    |  `- a-long-unit-name-that-a-narrow-terminal-cannot-hold.service  failed|
|    `- NETWORK  available|
||
||
||
||
||
| Enter enter  b back  u up  Esc close  ? help|
== 200 columns ==
| map SYSTEM  L1  4 nodes  bounded|
||
|    SYSTEM  local|
|>   +- COMPUTE  available|
|    |  `- a-long-unit-name-that-a-narrow-terminal-cannot-hold.service  failed|
|    `- NETWORK  available|
||
||
||
||
||
| Enter enter  b back  u up  Esc close  ? help|
";
