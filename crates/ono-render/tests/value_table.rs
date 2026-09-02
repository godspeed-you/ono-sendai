//! Turning a stream of values into the table of spec §13.2, with the columns spec §27.3 puts on
//! the schema and the heterogeneous behaviour spec §11.4 asks to be explicit about.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the way a test does"
)]

use std::sync::Arc;

use jiff::tz::TimeZone;
use ono_render::{Cell, Layout, Presentation, Renderer, Theme, View};
use ono_value::{
    ByteSize, FieldDef, FieldType, Percent, Provenance, RecordValue, Schema, SchemaId, Value,
};

mod support;
use support::{map, strip};

fn schema(default_view: bool) -> Arc<Schema> {
    let mut builder = Schema::builder(SchemaId::new("ono.demo", 1), "Demo")
        .field(FieldDef::new("pid", FieldType::Int).required())
        .field(FieldDef::new("name", FieldType::String).required())
        .field(FieldDef::new("cpu", FieldType::Float).nullable())
        .field(FieldDef::new("memory", FieldType::ByteSize).nullable())
        .field(FieldDef::new("command", FieldType::String).nullable())
        .identity(["pid"]);
    if default_view {
        builder = builder.default_view(["pid", "name", "memory"]);
    }
    Arc::new(builder.build().unwrap())
}

fn process(schema: &Arc<Schema>, pid: i128, name: &str, memory: Option<u128>) -> Value {
    RecordValue::builder(
        Arc::clone(schema),
        Provenance::local("demo", SchemaId::new("ono.demo", 1)),
    )
    .set("pid", Value::Int(pid))
    .unwrap()
    .set("name", Value::string(name))
    .unwrap()
    .set("cpu", Value::Percent(Percent::new(24.8)))
    .unwrap()
    .set(
        "memory",
        memory.map_or(Value::Null, |bytes| {
            Value::ByteSize(ByteSize::from_bytes(bytes))
        }),
    )
    .unwrap()
    .set(
        "command",
        Value::string("/usr/lib/postgresql/16/bin/postgres -D /var/lib/postgresql/16/main"),
    )
    .unwrap()
    .build()
    .into_value()
}

fn renderer() -> Renderer {
    Renderer::in_zone(TimeZone::UTC)
}

#[test]
fn should_take_the_columns_from_the_schema_default_view() {
    let schema = schema(true);
    let table = renderer().table(&[process(&schema, 812, "postgres", Some(1_288_490_188))]);
    let headers: Vec<&str> = table
        .columns()
        .iter()
        .map(ono_render::Column::header)
        .collect();
    assert_eq!(headers, ["PID", "NAME", "MEMORY"]);
}

#[test]
fn should_fall_back_to_the_field_order_when_the_schema_declares_no_default_view() {
    let schema = schema(false);
    let table = renderer().table(&[process(&schema, 812, "postgres", Some(1024))]);
    let headers: Vec<&str> = table
        .columns()
        .iter()
        .map(ono_render::Column::header)
        .collect();
    assert_eq!(headers, ["PID", "NAME", "CPU", "MEMORY", "COMMAND"]);
}

#[test]
fn should_render_one_row_per_record_with_the_human_form_of_each_field() {
    let schema = schema(true);
    let table = renderer().table(&[
        process(&schema, 812, "postgres", Some(1_288_490_188)),
        process(&schema, 4419, "nginx", Some(19_083_264)),
    ]);
    let lines = Layout::new(80).render(&table);
    assert_eq!(lines.len(), 3, "header plus two rows, got {lines:#?}");
    assert!(lines[1].contains("postgres"), "got {:?}", lines[1]);
    assert!(lines[1].contains("1.20 GiB"), "got {:?}", lines[1]);
    assert!(lines[2].contains("18.20 MiB"), "got {:?}", lines[2]);
}

#[test]
fn should_show_null_as_null_rather_than_an_empty_cell() {
    let schema = schema(true);
    let table = renderer().table(&[process(&schema, 812, "postgres", None)]);
    assert_eq!(table.row(0)[2], Cell::new("null"));
    let lines = Layout::new(80).render(&table);
    assert!(lines[1].contains("null"), "got {:?}", lines[1]);
}

#[test]
fn should_show_a_type_column_when_the_stream_is_heterogeneous() {
    let schema = schema(true);
    let table = renderer().table(&[
        process(&schema, 812, "postgres", Some(1024)),
        map(&[("name", Value::string("nginx"))]),
    ]);
    let headers: Vec<&str> = table
        .columns()
        .iter()
        .map(ono_render::Column::header)
        .collect();
    assert_eq!(
        headers,
        ["TYPE", "VALUE"],
        "spec §11.4: a heterogeneous stream is allowed but must be explicit, never a union of \
         columns in which a missing field looks like an unknown one"
    );
    assert_eq!(table.row(0)[0], Cell::new("ono.demo/1"));
    assert_eq!(table.row(1)[0], Cell::new("map"));
}

#[test]
fn should_show_one_value_column_when_the_stream_is_scalars() {
    let table = renderer().table(&[Value::Int(1), Value::Int(2)]);
    let headers: Vec<&str> = table
        .columns()
        .iter()
        .map(ono_render::Column::header)
        .collect();
    assert_eq!(headers, ["VALUE"]);
    assert_eq!(table.row(1)[0], Cell::new("2"));
}

#[test]
fn should_use_the_map_keys_as_columns_when_every_row_is_the_same_shape() {
    let table = renderer().table(&[
        map(&[("name", Value::string("nginx")), ("port", Value::Port(80))]),
        map(&[("name", Value::string("sshd")), ("port", Value::Port(22))]),
    ]);
    let headers: Vec<&str> = table
        .columns()
        .iter()
        .map(ono_render::Column::header)
        .collect();
    assert_eq!(headers, ["NAME", "PORT"]);
}

#[test]
fn should_show_the_no_results_line_for_an_empty_stream() {
    let lines = Layout::new(80).render(&renderer().table(&[]));
    assert_eq!(lines, ["(no results)"]);
}

#[test]
fn should_stay_within_the_terminal_at_eighty_and_at_two_hundred_columns() {
    let schema = schema(false);
    let table = renderer().table(&[
        process(&schema, 812, "postgres", Some(1_288_490_188)),
        process(&schema, 4419, "nginx", None),
    ]);
    for width in [80, 200] {
        for line in Layout::new(width).render(&table) {
            assert!(
                unicode_width::UnicodeWidthStr::width(line.as_str()) <= width,
                "at {width} columns: {line:?}"
            );
        }
    }
}

#[test]
fn should_keep_the_full_value_when_the_view_truncates_it() {
    let schema = schema(false);
    let table = renderer().table(&[process(&schema, 812, "postgres", Some(1024))]);
    let lines = Layout::new(60).render(&table);
    assert!(
        lines.iter().any(|line| line.contains("...")),
        "the long command should be visibly shortened, got {lines:#?}"
    );
    assert_eq!(
        table.row(0)[4].text(),
        "/usr/lib/postgresql/16/bin/postgres -D /var/lib/postgresql/16/main",
        "spec §13.3: copy, export and serialization must retain the full value"
    );
}

#[test]
fn should_emit_no_escape_sequences_when_the_destination_takes_no_colour() {
    let schema = schema(true);
    let table = renderer().table(&[process(&schema, 812, "postgres", None)]);
    let theme = Theme::default();
    for presentation in [
        Presentation::Pipe,
        Presentation::Plain,
        Presentation::Redirect,
    ] {
        for line in Layout::new(80).render_styled(&table, &theme, presentation) {
            assert!(!line.contains('\u{1b}'), "for {presentation:?}: {line:?}");
        }
    }
}

#[test]
fn should_paint_the_cells_when_the_destination_is_a_terminal() {
    let schema = schema(true);
    let table = renderer().table(&[process(&schema, 812, "postgres", None)]);
    let lines = Layout::new(80).render_styled(&table, &Theme::default(), Presentation::Terminal);
    assert!(
        lines[1].contains('\u{1b}'),
        "a terminal gets colour, got {:?}",
        lines[1]
    );
    assert!(lines[1].contains("null"), "got {:?}", lines[1]);
}

#[test]
fn should_keep_a_styled_line_within_the_terminal_width() {
    let schema = schema(false);
    let table = renderer().table(&[process(&schema, 812, "postgres", Some(1024))]);
    let plain = Layout::new(80).render(&table);
    let painted = Layout::new(80).render_styled(&table, &Theme::default(), Presentation::Terminal);
    let stripped: Vec<String> = painted.iter().map(|line| strip(line)).collect();
    assert_eq!(stripped, plain, "colour must not change the layout");
}

#[test]
fn should_make_an_escape_sequence_inert_all_the_way_through_the_table() {
    let table = renderer().table(&[Value::string("nginx\u{1b}[2Joops")]);
    for presentation in [Presentation::Pipe, Presentation::Terminal] {
        let lines = Layout::new(80).render_styled(&table, &Theme::default(), presentation);
        let payload = strip(&lines[1]);
        assert!(
            !payload.contains('\u{1b}'),
            "spec §49 / ADR-0015 T1: for {presentation:?} got {payload:?}"
        );
    }
}

#[test]
fn should_render_the_list_view_as_one_labelled_block_per_record() {
    let schema = schema(true);
    let lines = Layout::new(80).render_view(
        &renderer(),
        &[process(&schema, 812, "postgres", None)],
        View::Table,
    );
    let list = Layout::new(80).render_view(
        &renderer(),
        &[process(&schema, 812, "postgres", None)],
        View::List,
    );
    assert!(list.len() > lines.len(), "a list stacks, got {list:#?}");
    assert!(
        list.iter().any(|line| line.starts_with("pid")),
        "got {list:#?}"
    );
    assert!(
        list.iter().any(|line| line.contains("null")),
        "got {list:#?}"
    );
}
