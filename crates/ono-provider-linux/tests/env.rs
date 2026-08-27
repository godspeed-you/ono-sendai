//! What `get env` answers.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers shared between the cases below sit outside a `#[test]` function, where a \
              failed precondition should still abort loudly"
)]

mod common;

use common::{drain, find, records};
use ono_provider_api::{Provider, Query, Selector};
use ono_provider_linux::{EnvBinding, EnvProvider, EnvSource};
use ono_value::Value;

fn provider() -> EnvProvider {
    EnvProvider::new([
        EnvBinding::inherited("PATH", "/usr/bin:/bin"),
        EnvBinding::inherited("HOME", "/home/ada"),
        EnvBinding::shell("SCRATCH", "/tmp/work", false),
        EnvBinding {
            name: "ONO_THEME".to_owned(),
            value: "dark".to_owned(),
            exported: true,
            source: EnvSource::Config,
        },
    ])
}

#[tokio::test]
async fn should_report_every_declared_field_of_a_variable() {
    let collected = drain(
        provider()
            .snapshot(&Query::target("env"))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);

    let path = find(&records, "name", "PATH").expect("PATH is bound");
    assert_eq!(path.get("value"), Some(&Value::string("/usr/bin:/bin")));
    assert_eq!(path.get("exported"), Some(&Value::Bool(true)));
    assert_eq!(path.get("source"), Some(&Value::string("inherited")));

    let scratch = find(&records, "name", "SCRATCH").expect("SCRATCH is bound");
    assert_eq!(scratch.get("exported"), Some(&Value::Bool(false)));
    assert_eq!(
        scratch.get("source"),
        Some(&Value::string("shell")),
        "a binding made during the session says so, which is what `get env` is for"
    );

    let theme = find(&records, "name", "ONO_THEME").expect("ONO_THEME is bound");
    assert_eq!(theme.get("source"), Some(&Value::string("config")));
}

#[tokio::test]
async fn should_answer_in_the_same_order_every_time() {
    let names = |collected: &ono_pipeline::Collected| -> Vec<String> {
        records(collected)
            .iter()
            .filter_map(|record| {
                record
                    .get("name")
                    .and_then(|v| v.as_str().ok().map(str::to_owned))
            })
            .collect()
    };
    let first = drain(
        provider()
            .snapshot(&Query::target("env"))
            .expect("a snapshot"),
    )
    .await;
    let second = drain(
        provider()
            .snapshot(&Query::target("env"))
            .expect("a snapshot"),
    )
    .await;

    assert_eq!(names(&first), names(&second));
    assert_eq!(
        names(&first),
        ["HOME", "ONO_THEME", "PATH", "SCRATCH"],
        "redirected output has to be deterministic (spec §50)"
    );
}

#[tokio::test]
async fn should_resolve_one_variable_by_name() {
    let query = Query::target("env").with(Selector::field("name", Value::string("HOME")));
    let collected = drain(provider().snapshot(&query).expect("a snapshot")).await;
    let records = records(&collected);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].get("value"), Some(&Value::string("/home/ada")));
}

#[tokio::test]
async fn should_produce_no_record_at_all_for_a_variable_that_is_not_set() {
    let query = Query::target("env").with(Selector::field("name", Value::string("NOT_SET")));
    let collected = drain(provider().snapshot(&query).expect("a snapshot")).await;

    assert!(
        records(&collected).is_empty(),
        "an unset variable produces no record, which is what keeps `value` non-null"
    );
    assert!(collected.errors().is_empty());
}

#[tokio::test]
async fn should_restrict_to_the_exported_bindings_when_asked() {
    let query = Query::target("env").option("exported", Value::Bool(false));
    let collected = drain(provider().snapshot(&query).expect("a snapshot")).await;
    let records = records(&collected);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].get("name"), Some(&Value::string("SCRATCH")));
}

#[tokio::test]
async fn should_answer_for_the_session_it_was_given_rather_than_for_the_process_environment() {
    let provider = provider();
    let bound: Vec<&str> = provider
        .bindings()
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    // Something this process really has in its environment but the session was not given. A
    // provider that read its own environment would report it.
    let leaked = std::env::vars()
        .map(|(name, _)| name)
        .find(|name| !bound.contains(&name.as_str()))
        .expect("the test process has an environment of its own");

    let collected = drain(
        provider
            .snapshot(&Query::target("env"))
            .expect("a snapshot"),
    )
    .await;
    assert!(
        find(&records(&collected), "name", &leaked).is_none(),
        "`get env` answers for the scope the user is in, not for whatever execve handed the binary"
    );
}

#[test]
fn should_claim_only_the_read_capability() {
    let provider = EnvProvider::new([]);
    assert_eq!(provider.targets(), ["env"]);
    let ids: Vec<String> = provider
        .capabilities()
        .iter()
        .map(|capability| capability.id().to_owned())
        .collect();
    assert_eq!(
        ids,
        ["env.read"],
        "setting a variable changes the session's scope, which the evaluator owns"
    );
}
