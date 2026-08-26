//! One pipeline, end to end: a provider, a predicate, a projection and a serialization.
//!
//! ```text
//! get process | where size > 1KiB | select name | to json
//! ```
//!
//! Everything the crate exists for is in that line — a contract resolved and bound, an expression
//! compiled against a record's schema, a stream that is never materialised until the serializer
//! has to, and the three-valued semantics of ADR-0014 deciding which rows survive.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, providers, run};

#[tokio::test]
async fn should_carry_a_whole_pipeline_from_a_provider_to_json() {
    let ran = run(
        "get process | where size > 1KiB | select name | to json",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let json: serde_json::Value =
        serde_json::from_str(&ran.text()).expect("`to json` writes a JSON document");

    assert_eq!(
        json,
        serde_json::json!([{ "name": "beta" }]),
        "one row survives the filter: `alpha` is under the threshold and `gamma`'s size is \
         unknown, which ADR-0014 says is not the same as being over it — and spec §33.5 writes \
         it as the data alone, with no Ono envelope for an external tool to trip over"
    );
    assert!(
        ran.failures().is_empty(),
        "nothing failed along the way: {:?}",
        ran.failures()
    );
}

#[tokio::test]
async fn should_report_the_typo_and_run_nothing_at_all() {
    // The other half of the same story: spec §11.3 catches `cpy` from the declared output schema
    // of `get process`, before a single object is enumerated.
    let error = fixture::check("get process | where cpy > 20 | select name | to json")
        .expect_err("`cpy` is not a field of `ono.process/1`");

    assert_eq!(error.code(), ono_core::ErrorCode::TypeUnknownField);
    assert_eq!(error.help(), Some("perhaps: cpu"));
}
