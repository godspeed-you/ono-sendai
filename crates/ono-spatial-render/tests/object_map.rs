//! What a map of an *object* draws — spec v0.4 §23.5, §11.4.
//!
//! §23.5: "Edges MUST show relation labels", and §11.4 makes a relationship explainable — the
//! reader must be able to say why a neighbour is there. A map centred on a process reaches its
//! neighbours by relationship rather than by containment, so almost all of them land outside the
//! hierarchy tree; a list of bare display names is not a view a neighbour can be chosen from.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_spatial_render::{Charset, map_lines};
use ono_value::{FieldType, MapValue, Provenance, RecordValue, SchemaId, Value};

mod support;
use support::schema;

/// A map centred on a process whose neighbours are reached by relationship only: two of them
/// share a display name, as four `containerd-shim` processes do on a real host.
fn object_map() -> RecordValue {
    let node_schema = schema(
        "ono.map-node",
        &[
            ("id", FieldType::String),
            ("object_ref", FieldType::Any),
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
            ("source_label", FieldType::String),
            ("target_label", FieldType::String),
            ("kind", FieldType::String),
            ("relation", FieldType::String),
            ("confidence", FieldType::String),
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
    let node = |id: &str, label: &str, pid: i128| {
        let mut reference = MapValue::new();
        reference.insert(
            std::sync::Arc::from("schema"),
            Value::string("ono.process/1"),
        );
        reference.insert(std::sync::Arc::from("pid"), Value::Int(pid));
        Value::Record(std::sync::Arc::new(
            RecordValue::builder(
                node_schema.clone(),
                Provenance::local("test", SchemaId::new("ono.map-node", 1)),
            )
            .set("id", Value::string(id))
            .expect("id")
            .set("object_ref", Value::Map(std::sync::Arc::new(reference)))
            .expect("object_ref")
            .set("label", Value::string(label))
            .expect("label")
            .set("type", Value::string("Process"))
            .expect("type")
            .set("state", Value::string("sleeping"))
            .expect("state")
            .build(),
        ))
    };
    let edge = |target: &str, target_label: &str, relation: &str| {
        Value::Record(std::sync::Arc::new(
            RecordValue::builder(
                edge_schema.clone(),
                Provenance::local("test", SchemaId::new("ono.map-edge", 1)),
            )
            .set("source", Value::string("here"))
            .expect("source")
            .set("target", Value::string(target))
            .expect("target")
            .set("source_label", Value::string("systemd"))
            .expect("source_label")
            .set("target_label", Value::string(target_label))
            .expect("target_label")
            .set("kind", Value::string("relationship"))
            .expect("kind")
            .set("relation", Value::string(relation))
            .expect("relation")
            .set("confidence", Value::string("exact"))
            .expect("confidence")
            .build(),
        ))
    };
    RecordValue::builder(
        map_schema,
        Provenance::local("test", SchemaId::new("ono.spatial-map", 1)),
    )
    .set("center", Value::string("here"))
    .expect("center")
    .set("zoom_level", Value::Int(2))
    .expect("zoom")
    .set("completeness", Value::string("complete"))
    .expect("completeness")
    .set(
        "nodes",
        Value::list([
            node("here", "systemd", 1),
            node("shim-a", "containerd-shim", 4711),
            node("shim-b", "containerd-shim", 4712),
            node("owner", "docker.service", 900),
        ]),
    )
    .expect("nodes")
    .set(
        "edges",
        Value::list([
            edge("shim-a", "containerd-shim", "process.parent_of"),
            edge("shim-b", "containerd-shim", "process.parent_of"),
            edge("owner", "docker.service", "service.controls_process"),
        ]),
    )
    .expect("edges")
    .build()
}

/// The rows of the group that holds what the hierarchy did not reach.
fn neighbour_rows(lines: &[ono_spatial_render::MapLine]) -> Vec<String> {
    let start = lines
        .iter()
        .position(|line| line.text().trim() == "also here")
        .expect("§23.5: the neighbours a map draws are on it");
    lines[start + 1..]
        .iter()
        .take_while(|line| !line.text().trim().is_empty())
        .map(|line| line.text().trim().to_owned())
        .collect()
}

#[test]
fn should_name_the_relation_every_neighbour_of_an_object_stands_in() {
    // §23.5: "Edges MUST show relation labels." A row that says only `containerd-shim` cannot be
    // used to choose a neighbour, which is the whole purpose of the view (§11.4).
    let lines = map_lines(&object_map(), 100, Charset::Ascii);
    let rows = neighbour_rows(&lines);
    assert_eq!(rows.len(), 3, "three neighbours, got {rows:?}");
    for row in &rows {
        assert!(
            row.contains("process.parent_of") || row.contains("service.controls_process"),
            "every row names the relation it stands in, got {row:?}"
        );
    }
}

#[test]
fn should_tell_two_neighbours_sharing_a_display_name_apart() {
    // §11.4 and §2.17: four `containerd-shim` rows with nothing to tell them apart is a view that
    // cannot answer the question it was drawn for. The identity is already on the node —
    // `map-node.v1.yaml`'s `object_ref` carries the values of the schema's identity fields.
    let lines = map_lines(&object_map(), 100, Charset::Ascii);
    let rows = neighbour_rows(&lines);
    let shims: Vec<&String> = rows
        .iter()
        .filter(|row| row.contains("containerd-shim"))
        .collect();
    assert_eq!(shims.len(), 2, "got {rows:?}");
    assert_ne!(shims[0], shims[1], "the two rows are distinguishable");
    assert!(
        shims.iter().any(|row| row.contains("4711"))
            && shims.iter().any(|row| row.contains("4712")),
        "the identity that tells them apart is the one the node carries, got {shims:?}"
    );
}
