//! The commands that describe the shell: `type`, `inspect`, `help`, `explain`, `get command`.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, providers, run};
use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

/// One entry of a map value.
fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    value
        .as_map()
        .unwrap_or_else(|_| panic!("expected a map, got {}", value.type_name()))
        .get(name)
        .unwrap_or_else(|| panic!("expected a `{name}` entry"))
}

// --- type --------------------------------------------------------------------------------------

#[tokio::test]
async fn should_report_the_schema_of_what_is_flowing_through() {
    let ran = run("get process | type", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    let described = ran.only();
    assert_eq!(field(described, "type"), &Value::string("record"));
    assert_eq!(field(described, "schema"), &Value::string("ono.widget/1"));

    let names: Vec<String> = field(described, "fields")
        .as_list()
        .expect("a field list")
        .iter()
        .map(|entry| field(entry, "name").to_string())
        .collect();
    assert_eq!(names, ["pid", "name", "size", "owner"]);
}

#[tokio::test]
async fn should_report_what_a_pipeline_would_produce_without_running_it() {
    // No provider at all: the answer comes from the contract, so nothing is enumerated.
    let ran = run(r#"type "get process""#, &fixture::no_providers())
        .await
        .expect("the pipeline runs");

    let described = ran.only();
    assert_eq!(
        field(described, "type"),
        &Value::string("stream<ono.process/1>")
    );
    assert_eq!(field(described, "schema"), &Value::string("ono.process/1"));
}

#[tokio::test]
async fn should_accept_a_subject_written_as_bare_words() {
    // The contract's own example is `type get socket` — no quotes. A subject of several words is
    // the pipeline they spell, exactly as if it had been quoted.
    let ran = run("type get process", &fixture::no_providers())
        .await
        .expect("the documented example runs");

    let described = ran.only();
    assert_eq!(
        field(described, "type"),
        &Value::string("stream<ono.process/1>")
    );
    assert_eq!(field(described, "schema"), &Value::string("ono.process/1"));
}

// --- inspect -----------------------------------------------------------------------------------

#[tokio::test]
async fn should_show_every_field_with_how_it_was_known_and_where_it_came_from() {
    let ran = run(
        "get process 3 | inspect",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let described = ran.only();
    assert_eq!(field(described, "schema"), &Value::string("ono.widget/1"));

    let accesses: Vec<(String, String)> = field(described, "fields")
        .as_list()
        .expect("a field list")
        .iter()
        .map(|entry| {
            (
                field(entry, "name").to_string(),
                field(entry, "access").to_string(),
            )
        })
        .collect();
    assert_eq!(
        accesses,
        [
            ("pid".to_owned(), "known".to_owned()),
            ("name".to_owned(), "known".to_owned()),
            // The widget whose size nobody could tell us: unknown, never absent and never zero.
            ("size".to_owned(), "unknown".to_owned()),
            ("owner".to_owned(), "known".to_owned()),
        ],
        "spec §10.5: the three absences stay three things"
    );

    let provenance = field(described, "provenance");
    assert_eq!(
        field(provenance, "provider"),
        &Value::string("test.fixture")
    );
    assert_eq!(field(provenance, "source"), &Value::string("memory"));
    assert_eq!(field(provenance, "link"), &Value::string("local"));
}

#[tokio::test]
async fn should_show_the_causal_chain_of_an_error() {
    let cause = ErrorValue::new(ErrorCode::IoPermissionDenied, "/proc/1/environ is root's");
    let error = ErrorValue::new(
        ErrorCode::ProviderUnavailable,
        "the process could not be read",
    )
    .with_source(cause)
    .with_help("run it as the owner")
    .into_value();

    let table = fixture::table();
    let registry = fixture::registry();
    let parsed = ono_parser::parse("inspect");
    let stage = &parsed.program().statements[0]
        .as_pipeline()
        .expect("a pipeline")
        .head
        .stages[0];
    let resolved = registry
        .resolve("inspect", &stage.arguments)
        .expect("resolves");
    let bound = resolved.contract.bind(resolved.arguments).expect("binds");
    let providers = fixture::no_providers();
    let mut invocation = ono_command::Invocation::new(resolved.contract, &bound, &providers)
        .with_input(ono_pipeline::ValueStream::from_values([error]));

    let ono_command::Outcome::Values(stream) = table
        .run(resolved.contract.id(), &mut invocation)
        .await
        .expect("`inspect` runs")
    else {
        panic!("`inspect` produces values");
    };
    let collected = stream.collect().await;
    let described = &collected.values()[0];

    let reported = field(described, "error");
    assert_eq!(
        field(reported, "name"),
        &Value::string("provider.unavailable")
    );
    assert_eq!(
        field(reported, "help"),
        &Value::string("run it as the owner")
    );
    let chain = field(reported, "chain").as_list().expect("a chain");
    assert_eq!(
        chain.len(),
        1,
        "spec §16.2: the whole causal chain, not only the top"
    );
    assert_eq!(
        field(&chain[0], "code"),
        &Value::string("io.permission_denied")
    );
}

// --- help and explain ----------------------------------------------------------------------------

#[tokio::test]
async fn should_answer_help_with_the_page_the_registry_generates() {
    let ran = run(r#"help "get process""#, &fixture::no_providers())
        .await
        .expect("the pipeline runs");

    let synopsis = field(ran.only(), "synopsis")
        .as_str()
        .expect("a synopsis")
        .to_owned();
    assert!(
        synopsis.contains("get process"),
        "spec §15.2: the page derives from metadata — {synopsis}"
    );
}

#[tokio::test]
async fn should_answer_explain_with_a_plan_and_run_nothing() {
    let ran = run(
        r#"explain "get process | to json""#,
        &fixture::no_providers(),
    )
    .await
    .expect("the pipeline runs");

    let plan = ran.only();
    let stages = field(plan, "stages").as_list().expect("a stage list");
    assert_eq!(stages.len(), 2);
    assert_eq!(
        field(&stages[0], "command"),
        &Value::string("ono.process.get")
    );
    assert_eq!(field(plan, "mutating"), &Value::Bool(false));
}

// --- get command / find command -------------------------------------------------------------------

#[tokio::test]
async fn should_answer_get_command_from_the_registry_itself() {
    let ran = run("get command --verb where", &fixture::no_providers())
        .await
        .expect("the pipeline runs");

    let record = ran.only().as_record().expect("a command is an object");
    assert_eq!(record.schema_id().name(), "ono.command");
    assert_eq!(record.get("id"), Some(&Value::string("ono.data.where")));
    assert_eq!(record.get("stability"), Some(&Value::string("stable")));
}

#[tokio::test]
async fn should_say_a_core_command_was_contributed_by_the_core() {
    let ran = run("get command --verb where", &fixture::no_providers())
        .await
        .expect("the pipeline runs");

    let record = ran.only().as_record().expect("a command is an object");
    assert_eq!(
        record.get("origin"),
        Some(&Value::string("core")),
        "spec §31.64: every registry entry records where it came from"
    );
}

#[tokio::test]
async fn should_find_a_command_by_what_it_does_rather_than_by_its_name() {
    let ran = run(r#"find command "listening""#, &fixture::no_providers())
        .await
        .expect("the pipeline runs");

    assert!(
        !ran.values().is_empty(),
        "spec §15.4: discovery searches summaries, not only names"
    );
}

#[tokio::test]
async fn should_let_the_registry_be_filtered_like_any_other_stream() {
    let ran = run(
        "get command | where stability == \"stable\" | count",
        &fixture::no_providers(),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(
        ran.only(),
        &Value::Int(
            fixture::registry()
                .with_stability(ono_command::Stability::Stable)
                .len() as i128
        )
    );
}
