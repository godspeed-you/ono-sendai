//! `trace` (spec §22): known relationships become a graph value that travels the pipeline.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, providers, run};
use ono_value::Value;

#[tokio::test]
async fn should_answer_a_trace_with_a_graph_rooted_at_the_named_object() {
    let ran = run("trace process 2", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    let record = ran.only().as_record().expect("one ono.graph/1 record");
    assert_eq!(record.schema_id().to_string(), "ono.graph/1");

    let root = record.get("root").expect("a root reference");
    let label = root
        .as_map()
        .expect("a reference map")
        .get("label")
        .and_then(|label| label.as_str().ok())
        .expect("a label");
    assert!(
        label.contains("beta") || label.contains('2'),
        "the root names the traced object, got {label:?}"
    );

    let nodes = record
        .get("nodes")
        .expect("nodes")
        .as_list()
        .expect("a list");
    assert!(
        !nodes.is_empty(),
        "the graph holds at least the object itself (spec §22.1)"
    );
}

#[tokio::test]
async fn should_report_a_trace_of_nothing_rather_than_an_empty_graph() {
    let error = run("trace process 99", &providers(FixtureProvider::new()))
        .await
        .expect_err("process 99 does not exist in the fixture");
    assert_eq!(error.code(), ono_core::ErrorCode::ResolveTargetNotFound);
    assert!(
        error.message().contains("99"),
        "the refusal names what was asked for: {}",
        error.message()
    );
    let _ = Value::Null;
}
