//! What the full-screen map does, asserted without a terminal (spec v0.4 §8.3, §23.3, §23.4,
//! §39.1, §39.3).
//!
//! The PTY suite in `ono-cli` proves the view works on a real screen. These are the outcomes that
//! screen cannot show cheaply: that moving the cursor asks the shell for nothing, that a rebound
//! key reaches the same normative action, that the cursor is legible with colour switched off,
//! and that nothing is ever drawn past the right edge.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_spatial_render::{Action, Charset, Effect, Key, Keymap, MapView};
use ono_value::{FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, Value};

/// A `SpatialMap`-shaped record: a centre, three nodes below it, and the hierarchy edges that
/// say so. Built by hand rather than through `ono-spatial-query`, because this crate must not
/// depend on the crate that ranks — it draws what it is handed (§45.4).
fn map_record() -> RecordValue {
    let node = |id: &str, label: &str| {
        Value::Record(std::sync::Arc::new(
            RecordValue::builder(node_schema(), Provenance::local("test", node_id()))
                .set("id", Value::string(id))
                .expect("id")
                .set("label", Value::string(label))
                .expect("label")
                .build(),
        ))
    };
    let edge = |source: &str, target: &str| {
        Value::Record(std::sync::Arc::new(
            RecordValue::builder(edge_schema(), Provenance::local("test", edge_id()))
                .set("source", Value::string(source))
                .expect("source")
                .set("target", Value::string(target))
                .expect("target")
                .set("kind", Value::string("hierarchy"))
                .expect("kind")
                .build(),
        ))
    };
    RecordValue::builder(map_schema(), Provenance::local("test", map_id()))
        .set("center", Value::string("root"))
        .expect("center")
        .set("zoom_level", Value::Int(1))
        .expect("zoom")
        .set("completeness", Value::string("bounded"))
        .expect("completeness")
        .set(
            "nodes",
            Value::list(vec![
                node("root", "SYSTEM"),
                node("compute", "COMPUTE"),
                node("network", "NETWORK"),
                node("storage", "STORAGE"),
            ]),
        )
        .expect("nodes")
        .set(
            "edges",
            Value::list(vec![
                edge("root", "compute"),
                edge("root", "network"),
                edge("root", "storage"),
            ]),
        )
        .expect("edges")
        .build()
}

fn map_id() -> SchemaId {
    SchemaId::new("test.spatial-map", 1)
}
fn node_id() -> SchemaId {
    SchemaId::new("test.map-node", 1)
}
fn edge_id() -> SchemaId {
    SchemaId::new("test.map-edge", 1)
}

fn node_schema() -> std::sync::Arc<Schema> {
    std::sync::Arc::new(
        Schema::builder(node_id(), "MapNode")
            .field(FieldDef::new("id", FieldType::String))
            .field(FieldDef::new("label", FieldType::String))
            .build()
            .expect("a well-formed schema"),
    )
}

fn edge_schema() -> std::sync::Arc<Schema> {
    std::sync::Arc::new(
        Schema::builder(edge_id(), "MapEdge")
            .field(FieldDef::new("source", FieldType::String))
            .field(FieldDef::new("target", FieldType::String))
            .field(FieldDef::new("kind", FieldType::String))
            .build()
            .expect("a well-formed schema"),
    )
}

fn map_schema() -> std::sync::Arc<Schema> {
    std::sync::Arc::new(
        Schema::builder(map_id(), "SpatialMap")
            .field(FieldDef::new("center", FieldType::String))
            .field(FieldDef::new("zoom_level", FieldType::Int))
            .field(FieldDef::new("completeness", FieldType::String))
            .field(FieldDef::new("nodes", FieldType::list(FieldType::Any)))
            .field(FieldDef::new("edges", FieldType::list(FieldType::Any)))
            .build()
            .expect("a well-formed schema"),
    )
}

fn view() -> MapView {
    MapView::new(
        &map_record(),
        60,
        12,
        Charset::Ascii,
        Keymap::default_bindings(),
    )
}

#[test]
fn should_leave_the_shell_alone_when_the_cursor_moves_among_the_drawn_nodes() {
    // §23.4: "Moving focus inside a map MUST NOT change the shell's current place." The view
    // says so by answering with nothing for the shell to do.
    let mut view = view();
    for key in [Key::Down, Key::Down, Key::Tab, Key::Up, Key::BackTab] {
        assert_eq!(
            view.apply(key),
            Effect::Stay,
            "moving focus with {key:?} asked the shell to do something"
        );
    }
}

#[test]
fn should_ask_the_shell_to_enter_the_focused_node_when_enter_is_pressed() {
    // §23.4: "Only `Enter` or explicit navigation action changes place."
    let mut view = view();
    view.apply(Key::Down);
    assert_eq!(
        view.apply(Key::Enter),
        Effect::Enter("compute".to_owned()),
        "Enter enters the node the cursor is on"
    );
}

#[test]
fn should_mark_the_focused_line_without_colour_when_the_view_is_drawn() {
    // §39.1: colour MUST NOT be required to distinguish the focused item.
    let mut view = view();
    view.apply(Key::Down);
    let frame = view.frame();
    let marked: Vec<&String> = frame.iter().filter(|line| line.starts_with('>')).collect();
    assert_eq!(
        marked.len(),
        1,
        "exactly one line carries the cursor, got {frame:?}"
    );
    assert!(
        marked[0].contains("COMPUTE"),
        "the cursor is on the focused node, got {:?}",
        marked[0]
    );
    assert!(
        !frame.iter().any(|line| line.contains('\u{1b}')),
        "the drawing carries no escape sequences at all, got {frame:?}"
    );
}

#[test]
fn should_fit_every_line_inside_the_terminal_when_the_view_is_narrow() {
    // §39.3: at narrow widths the projection may collapse, but nothing is drawn past the edge.
    let view = MapView::new(
        &map_record(),
        30,
        10,
        Charset::Ascii,
        Keymap::default_bindings(),
    );
    let frame = view.frame();
    assert_eq!(frame.len(), 10, "the frame is exactly as tall as asked");
    for line in &frame {
        assert!(
            line.chars().count() <= 30,
            "`{line}` is wider than the terminal"
        );
    }
}

#[test]
fn should_reach_the_same_action_when_a_key_is_rebound() {
    // §23.3: "Key bindings MUST be configurable. Semantic actions are normative; exact
    // single-key choices MAY be remapped."
    let mut keymap = Keymap::default_bindings();
    keymap
        .apply_overrides("close=q, enter=Space")
        .expect("a well-formed override");
    let mut view = MapView::new(&map_record(), 60, 12, Charset::Ascii, keymap);
    assert_eq!(view.apply(Key::Char('q')), Effect::Close, "`q` now closes");
    view.apply(Key::Down);
    assert_eq!(
        view.apply(Key::Char(' ')),
        Effect::Enter("compute".to_owned()),
        "Space now enters"
    );
    assert_eq!(
        view.apply(Key::Esc),
        Effect::Close,
        "Esc keeps the meaning it had: a partial rebinding leaves no action unreachable"
    );
}

#[test]
fn should_refuse_an_override_that_names_no_action_rather_than_silently_ignoring_it() {
    let mut keymap = Keymap::default_bindings();
    let problem = keymap
        .apply_overrides("teleport=t")
        .expect_err("an unknown action is reported");
    assert!(problem.contains("teleport"), "got {problem}");
}

#[test]
fn should_offer_every_normative_action_a_key_when_nothing_is_configured() {
    // §23.3's table is normative in its actions; every one of them must be reachable.
    let keymap = Keymap::default_bindings();
    for action in Action::ALL {
        assert!(
            !keymap.keys_for(action).is_empty(),
            "`{}` has no key bound to it",
            action.name()
        );
    }
}

#[test]
fn should_show_the_key_table_when_help_is_asked_for() {
    // §23.3's `?`. The table is generated from the bindings in force, so a rebound key is what
    // the help says it is.
    let mut keymap = Keymap::default_bindings();
    keymap.apply_overrides("close=q").expect("an override");
    let mut view = MapView::new(&map_record(), 60, 24, Charset::Ascii, keymap);
    assert_eq!(view.apply(Key::Char('?')), Effect::Stay);
    let shown = view.frame().join("\n");
    assert!(
        shown.contains("close, keeping the place"),
        "the help names the semantic action, got:\n{shown}"
    );
    assert!(
        shown.contains('q'),
        "the help names the key actually bound, got:\n{shown}"
    );
}

#[test]
fn should_keep_the_cursor_on_the_same_node_when_the_view_is_drawn_at_a_new_size() {
    // §43.4: "terminal resize preserves current place and focus where possible."
    let mut view = view();
    view.apply(Key::Down);
    view.apply(Key::Down);
    let before = view.focused_node().map(str::to_owned);
    view.resize(&map_record(), 40, 8, Charset::Ascii);
    assert_eq!(
        view.focused_node().map(str::to_owned),
        before,
        "the resize moved the cursor"
    );
}

#[test]
fn should_move_the_cursor_to_the_match_when_the_map_is_searched() {
    // §23.3's `/`: search the visible map. Searching is a view action and moves nothing else.
    let mut view = view();
    for key in [
        Key::Char('/'),
        Key::Char('s'),
        Key::Char('t'),
        Key::Char('o'),
    ] {
        assert_eq!(view.apply(key), Effect::Stay);
    }
    assert_eq!(
        view.apply(Key::Enter),
        Effect::Stay,
        "Enter applies the query"
    );
    assert_eq!(
        view.focused_node(),
        Some("storage"),
        "the cursor is on the node that matched"
    );
}
