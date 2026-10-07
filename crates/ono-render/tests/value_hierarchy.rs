//! A stream of records that nest records of their own schema under `children` — `get process
//! --tree` — drawn as a tree of rows for a person to read, and as the table of its roots anywhere
//! else (issue #223, ADR-0948). Snapshot tests of the rendering only (AGENTS.md §11).

#![allow(
    clippy::unwrap_used,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

use std::sync::Arc;

use jiff::tz::TimeZone;
use ono_render::{Layout, Presentation, Renderer, Theme, View};
use ono_value::{FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, Value};

fn schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.demo", 1), "Demo")
            .field(FieldDef::new("pid", FieldType::Int).required())
            .field(FieldDef::new("name", FieldType::String).required())
            .field(FieldDef::new("user", FieldType::String).nullable())
            .identity(["pid"])
            .default_view(["pid", "name", "user"])
            .build()
            .unwrap(),
    )
}

fn node(schema: &Arc<Schema>, pid: i128, name: &str, children: Vec<Value>) -> Value {
    RecordValue::builder(
        Arc::clone(schema),
        Provenance::local("demo", SchemaId::new("ono.demo", 1)),
    )
    .set("pid", Value::Int(pid))
    .unwrap()
    .set("name", Value::string(name))
    .unwrap()
    .set("user", Value::string("root"))
    .unwrap()
    .set_extra("children", Value::list(children))
    .build()
    .into_value()
}

/// `init` with `sshd` (which has `bash`, which has `vim`) and `cron` under it; a second root.
fn forest() -> Vec<Value> {
    let schema = schema();
    let vim = node(&schema, 40, "vim", Vec::new());
    let bash = node(&schema, 30, "bash", vec![vim]);
    let sshd = node(&schema, 20, "sshd", vec![bash]);
    let cron = node(&schema, 21, "cron", Vec::new());
    vec![
        node(&schema, 1, "init", vec![sshd, cron]),
        node(&schema, 2, "kthreadd", Vec::new()),
    ]
}

fn lines(presentation: Presentation, width: usize) -> Vec<String> {
    Layout::new(width).render_view_styled(
        &Renderer::in_zone(TimeZone::UTC),
        &forest(),
        View::Table,
        &Theme::default(),
        presentation,
    )
}

#[test]
fn should_draw_the_hierarchy_with_tree_guides_when_a_person_reads_it() {
    let drawn: Vec<String> = lines(Presentation::Plain, 80)
        .into_iter()
        .map(|line| line.trim_end().to_owned())
        .collect();
    assert_eq!(
        drawn,
        [
            "PID  NAME             USER",
            "  1  init             root",
            " 20  +-- sshd         root",
            " 30  |   +-- bash     root",
            " 40  |       +-- vim  root",
            " 21  +-- cron         root",
            "  2  kthreadd         root",
        ],
        "every descendant is a row under its parent, indented with the guides of spec §22.4"
    );
}

#[test]
fn should_keep_the_table_of_the_roots_when_the_output_is_not_read_by_a_person() {
    for presentation in [
        Presentation::Pipe,
        Presentation::Redirect,
        Presentation::Script,
    ] {
        let drawn = lines(presentation, 80).join("\n");
        assert!(
            drawn.contains("init") && drawn.contains("kthreadd") && !drawn.contains("sshd"),
            "a pipe, a file and a script get the deterministic table of what the stream holds, \
             got {drawn:?}"
        );
    }
}

#[test]
fn should_keep_the_tree_within_the_width_of_the_terminal() {
    for line in lines(Presentation::Plain, 24) {
        assert!(
            unicode_width::UnicodeWidthStr::width(line.as_str()) <= 24,
            "a deep tree is shortened like any row, got {line:?}"
        );
    }
}

#[test]
fn should_neutralise_a_control_character_in_a_nested_name() {
    let schema = schema();
    let hostile = node(&schema, 9, "evil\u{1b}]0;pwn\u{7}", Vec::new());
    let root = node(&schema, 1, "init", vec![hostile]);
    let drawn = Layout::new(80)
        .render_view_styled(
            &Renderer::in_zone(TimeZone::UTC),
            &[root],
            View::Table,
            &Theme::default(),
            Presentation::Plain,
        )
        .join("\n");
    assert!(
        drawn.contains("+-- evil") && !drawn.contains('\u{1b}') && !drawn.contains('\u{7}'),
        "a nested name passes through the sanitiser like any cell, got {drawn:?}"
    );
}

/// One process with a chain of `depth` descendants, each the only child of the one before.
fn chain(depth: i128) -> Value {
    let schema = schema();
    let mut current = node(&schema, depth + 1, "leaf", Vec::new());
    for pid in (1..=depth).rev() {
        current = node(&schema, pid, "link", vec![current]);
    }
    current
}

#[test]
fn should_cap_the_guides_of_a_deep_chain_and_mark_what_it_elides() {
    let drawn = Layout::new(100_000).render_view_styled(
        &Renderer::in_zone(TimeZone::UTC),
        &[chain(2_000)],
        View::Table,
        &Theme::default(),
        Presentation::Plain,
    );
    let widest = drawn
        .iter()
        .map(|line| unicode_width::UnicodeWidthStr::width(line.as_str()))
        .max()
        .unwrap_or_default();
    assert!(
        widest < 400,
        "a chain two thousand deep draws a bounded guide per row, not one level per ancestor \
         (review R13); the widest line is {widest} columns"
    );
    let leaf = drawn
        .iter()
        .find(|line| line.contains("leaf"))
        .expect("the deepest row is drawn");
    assert!(
        leaf.contains("... +-- leaf"),
        "a row deeper than the drawn depth says levels were elided, got {leaf:?}"
    );
    assert_eq!(
        drawn.len(),
        2_002,
        "every row is still drawn — only the guides are capped"
    );
}
