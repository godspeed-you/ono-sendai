//! `to`, `from` and `format`: the explicit boundary between values and bytes (spec §12.3, §12.4).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, providers, run};
use ono_core::ErrorCode;
use ono_value::Value;

#[tokio::test]
async fn should_serialize_a_stream_as_one_json_document() {
    let ran = run(
        "get process | select name | to json",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let json: serde_json::Value =
        serde_json::from_str(&ran.text()).expect("`to json` writes a JSON document");
    let rows = json.as_array().expect("a stream serializes as an array");
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0]["$record"]["fields"]["name"],
        serde_json::json!("alpha"),
        "a record keeps its schema and its fields (ADR-0016 item 6)"
    );
}

#[tokio::test]
async fn should_indent_when_asked_to() {
    let ran = run(
        "get process | select name | to json --pretty",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert!(
        ran.text().contains("\n  "),
        "`--pretty` indents for a reader"
    );
}

#[tokio::test]
async fn should_keep_a_semantic_scalar_canonical_unless_a_human_form_was_asked_for() {
    let canonical = run(
        "get process 2 | each size | to json",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs")
    .text();
    let human = run(
        "get process 2 | each size | to json --human",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs")
    .text();

    assert!(
        canonical.contains("$bytesize"),
        "spec §33.5: canonical unless a human format is explicitly requested — {canonical}"
    );
    assert!(
        human.contains("2.00 KiB"),
        "`--human` shows the display form — {human}"
    );
}

#[tokio::test]
async fn should_round_trip_a_value_through_json() {
    let ran = run(
        "get process | each name | to json | from json",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(
        ran.values(),
        [
            Value::string("alpha"),
            Value::string("beta"),
            Value::string("gamma")
        ],
        "spec §46: what `to json` wrote, `from json` reads back"
    );
}

#[tokio::test]
async fn should_round_trip_a_value_through_yaml() {
    let ran = run(
        "get process | each name | to yaml | from yaml",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.values().len(), 3);
    assert_eq!(ran.values()[1], Value::string("beta"));
}

#[tokio::test]
async fn should_write_a_csv_with_one_header_row() {
    let ran = run(
        "get process | select name owner | to csv",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.text(), "name,owner\nalpha,root\nbeta,ono\ngamma,root\n");
}

#[tokio::test]
async fn should_read_a_csv_back_as_one_row_per_line() {
    let ran = run(
        "get process | select name owner | to csv | from csv",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.values().len(), 3);
    assert_eq!(
        ran.values()[0]
            .as_map()
            .expect("a row is a map")
            .get("name"),
        Some(&Value::string("alpha"))
    );
}

#[tokio::test]
async fn should_refuse_to_write_a_csv_for_a_shape_it_cannot_carry() {
    // CSV has one type and no nesting; a stream of bare scalars has no columns to head.
    let error = run(
        "get process | each name | to csv",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs")
    .failures()
    .first()
    .cloned()
    .expect("a string is not a CSV row (spec §12.3)");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[tokio::test]
async fn should_write_one_line_per_value_as_text() {
    let ran = run(
        "get process | each name | to text",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.text(), "alpha\nbeta\ngamma\n");
}

#[tokio::test]
async fn should_refuse_to_put_a_record_on_one_line() {
    let error = run("get process | to text", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs")
        .failures()
        .first()
        .cloned()
        .expect("a record does not fit on one line (spec §12.3)");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(
        error.help().is_some_and(|help| help.contains("to json")),
        "the error names what to use instead"
    );
}

#[tokio::test]
async fn should_write_raw_bytes_for_a_byte_sink() {
    let ran = run(
        "get process | each name | to bytes",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(
        ran.only().as_bytes().expect("bytes").as_ref(),
        b"alphabetagamma"
    );
}

#[tokio::test]
async fn should_refuse_a_format_it_does_not_have_before_reading_anything() {
    let error = run("get process | to xml", &providers(FixtureProvider::new()))
        .await
        .expect_err("there is no `to xml`");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(
        error.help().is_some_and(|help| help.contains("json")),
        "the error names the formats there are: {error:?}"
    );
}

// --- format -----------------------------------------------------------------------------------

#[tokio::test]
async fn should_render_a_table_with_a_column_per_field() {
    let ran = run(
        "get process | select name owner | format table",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let text = ran.text();
    let mut lines = text.lines();
    assert!(
        lines
            .next()
            .is_some_and(|header| header.contains("NAME") && header.contains("OWNER")),
        "{text}"
    );
    assert!(
        lines.next().is_some_and(|row| row.contains("alpha")),
        "{text}"
    );
}

#[tokio::test]
async fn should_render_only_the_columns_that_were_asked_for() {
    let ran = run(
        "get process | format table --columns [name]",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let text = ran.text();
    assert!(text.contains("NAME"), "{text}");
    assert!(!text.contains("OWNER"), "{text}");
}

#[tokio::test]
async fn should_truncate_visibly_when_a_row_limit_was_given() {
    let ran = run(
        "get process | format table --max-rows 1",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let text = ran.text();
    assert!(text.contains("alpha"), "{text}");
    assert!(
        text.contains("more"),
        "spec §13.3: truncation is visible, never silent — {text}"
    );
}

#[tokio::test]
async fn should_render_the_other_built_in_views() {
    for view in ["list", "tree", "raw"] {
        let ran = run(
            &format!("get process | each name | format {view}"),
            &providers(FixtureProvider::new()),
        )
        .await
        .unwrap_or_else(|error| panic!("`format {view}` runs: {error:?}"));

        assert!(
            ran.text().contains("alpha"),
            "`format {view}` shows the values: {}",
            ran.text()
        );
    }
}

#[tokio::test]
async fn should_refuse_a_renderer_it_does_not_have() {
    let error = run(
        "get process | format origami",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect_err("there is no `format origami`");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}
