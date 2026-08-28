//! What the compact textual place view says, asserted without a terminal (spec v0.4 §24.1,
//! §24.2, §3.6, §39.3).
//!
//! §43.5 makes renderer output a presentation test and never a semantic contract, so nothing here
//! asserts what a place *is*. What it does assert is that a line the reader takes as a statement
//! about the list above it is a statement about the list above it: §24.2 forbids the renderer
//! from implying an exit that is not one, and a bare count printed under the exits implies
//! exactly that.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_value::{FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, Value};

fn schema(id: &str, fields: &[(&str, FieldType)]) -> std::sync::Arc<Schema> {
    let mut builder = Schema::builder(SchemaId::new(id, 1), id);
    for (name, kind) in fields {
        builder = builder.field(FieldDef::new(name, kind.clone()));
    }
    std::sync::Arc::new(builder.build().expect("a well-formed schema"))
}

/// The root place as `look` finds it on an ordinary host: six exits, and a neighbourhood whose
/// budget hid a great many of the places behind them.
fn root_view(hidden: i128) -> RecordValue {
    let group_schema = schema(
        "ono.place-group",
        &[
            ("label", FieldType::String),
            ("state", FieldType::String),
            ("count", FieldType::Int),
            ("detail", FieldType::String),
        ],
    );
    let neighborhood_schema = schema(
        "ono.spatial-neighborhood",
        &[("hidden_count", FieldType::Int)],
    );
    let view_schema = schema(
        "ono.place-view",
        &[
            ("label", FieldType::String),
            ("hostname", FieldType::String),
            ("groups", FieldType::list(FieldType::Any)),
            ("neighborhood", FieldType::Any),
        ],
    );

    let group = |label: &str, count: i128| {
        Value::Record(std::sync::Arc::new(
            RecordValue::builder(
                group_schema.clone(),
                Provenance::local("test", SchemaId::new("ono.place-group", 1)),
            )
            .set("label", Value::string(label))
            .expect("label")
            .set("state", Value::string("available"))
            .expect("state")
            .set("count", Value::Int(count))
            .expect("count")
            .set("detail", Value::Null)
            .expect("detail")
            .build(),
        ))
    };
    let neighborhood = Value::Record(std::sync::Arc::new(
        RecordValue::builder(
            neighborhood_schema,
            Provenance::local("test", SchemaId::new("ono.spatial-neighborhood", 1)),
        )
        .set("hidden_count", Value::Int(hidden))
        .expect("hidden_count")
        .build(),
    ));

    RecordValue::builder(
        view_schema,
        Provenance::local("test", SchemaId::new("ono.place-view", 1)),
    )
    .set("label", Value::string("SYSTEM"))
    .expect("label")
    .set("hostname", Value::string("testbox"))
    .expect("hostname")
    .set(
        "groups",
        Value::list(vec![
            group("COMPUTE", 4),
            group("NETWORK", 7),
            group("STORAGE", 4),
            group("CONTAINERS", 7),
            group("IDENTITY", 3),
            group("DEVICES", 215),
        ]),
    )
    .expect("groups")
    .set("neighborhood", neighborhood)
    .expect("neighborhood")
    .build()
}

#[test]
fn should_say_what_the_hidden_count_counts_when_the_view_bounded_the_neighborhood() {
    // §24.1 and §24.2 with §3.6. The hidden count belongs to the *neighbourhood* — the places
    // behind the exits that the view budget left out — and `look` prints it under the exits,
    // where a reader takes it for more of them. At the root that read as "this machine has 205
    // exits", which §24.2 forbids the renderer from implying.
    let lines = ono_spatial_render::place_view(&root_view(199), 100);
    let rendered = lines.join("\n");
    let hidden = lines
        .iter()
        .find(|line| line.contains("199"))
        .unwrap_or_else(|| panic!("the view discloses what it left out (§3.6), got {rendered}"));
    assert!(
        hidden.contains("neighbours"),
        "§24.2: the count says what it counts rather than reading as more exits, got {hidden:?} \
         in {rendered}"
    );
    assert_eq!(
        lines.iter().filter(|line| line.contains("COMPUTE")).count(),
        1,
        "the exits themselves are unchanged, got {rendered}"
    );
}

#[test]
fn should_leave_the_disclosure_out_when_the_view_hid_nothing() {
    // The other half: a place whose neighbourhood fitted says nothing about what it hid, because
    // it hid nothing. A line that always appears is not a disclosure.
    let lines = ono_spatial_render::place_view(&root_view(0), 100);
    let rendered = lines.join("\n");
    assert!(
        !rendered.contains("not shown"),
        "nothing was hidden, so nothing is disclosed, got {rendered}"
    );
}

#[test]
fn should_keep_the_disclosure_inside_a_narrow_terminal() {
    // §39.3: the view stays usable at 40 columns, so the line that says what was hidden is
    // written to fit rather than truncated into nonsense.
    for width in [40usize, 80, 120] {
        let lines = ono_spatial_render::place_view(&root_view(199), width);
        let hidden = lines
            .iter()
            .find(|line| line.contains("199"))
            .unwrap_or_else(|| panic!("the disclosure survives at {width} columns"));
        assert!(
            hidden.chars().count() <= width,
            "§39.3: nothing is drawn past the right edge at {width} columns, got {hidden:?}"
        );
        assert!(
            hidden.contains("neighbours"),
            "§24.2: the count says what it counts at every width, got {hidden:?}"
        );
    }
}
