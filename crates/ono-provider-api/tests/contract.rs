//! The provider contract, exercised through a fixture provider.
//!
//! Spec §31's preamble requires that "command metadata, value schemas, object identity, provider
//! capabilities, rendering and execution plans SHOULD already be shaped so that KUANG/11 can
//! consume them without special cases". These tests hold that line: everything a plugin will be
//! given later is what a core provider is asked for now.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_provider_api::{
    Action, Capability, EventKind, ObjectId, Provider, ProviderRegistry, Query, Risk, Selector,
};
use ono_value::{Provenance, RecordValue, Schema, SchemaId, Value};

mod fixture;
use fixture::{FixtureProvider, fixture_schema};

fn registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(FixtureProvider::new()));
    registry
}

#[tokio::test]
async fn should_answer_a_snapshot_with_the_objects_it_can_see() {
    let registry = registry();
    let stream = registry
        .snapshot(&Query::target("widget"))
        .expect("the fixture provider answers `widget`");
    let collected = stream.collect().await;
    assert_eq!(collected.values().len(), 3, "got {:?}", collected.values());
    assert!(collected.errors().is_empty());
}

#[tokio::test]
async fn should_report_which_provider_and_when_on_every_object_it_produces() {
    // Spec §25.2: provenance is what makes `inspect` and `explain` trustworthy. A record with no
    // provenance is a record nobody can check.
    let collected = registry()
        .snapshot(&Query::target("widget"))
        .expect("a stream")
        .collect()
        .await;
    for value in collected.values() {
        let record = value.as_record().expect("a record");
        assert_eq!(record.provenance().provider(), "test.fixture");
        assert!(
            record
                .provenance()
                .source()
                .is_some_and(|source| !source.is_empty())
        );
    }
}

#[tokio::test]
async fn should_refuse_a_target_no_provider_answers() {
    let error = registry()
        .snapshot(&Query::target("nothing-answers-this"))
        .expect_err("an unanswered target must be reported");
    assert_eq!(error.code(), ErrorCode::ResolveTargetNotFound);
}

#[tokio::test]
async fn should_say_why_it_cannot_answer_rather_than_returning_nothing() {
    // Spec §35.3 and ADR-0015: a provider that is not available on this machine must say so.
    // Returning an empty result would be indistinguishable from "there are none".
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(FixtureProvider::unavailable(
        "no such subsystem here",
    )));
    let error = registry
        .snapshot(&Query::target("widget"))
        .expect_err("an unavailable provider must be reported");
    assert_eq!(error.code(), ErrorCode::ProviderUnavailable);
    assert!(
        error.message().contains("no such subsystem"),
        "{}",
        error.message()
    );
}

#[tokio::test]
async fn should_declare_the_capabilities_a_command_needs_before_it_runs() {
    let provider = FixtureProvider::new();
    let capabilities = provider.capabilities();
    let ids: Vec<&str> = capabilities.iter().map(Capability::id).collect();
    assert!(ids.contains(&"widget.list"));
    assert!(ids.contains(&"widget.remove"));

    let find = |id: &str| {
        capabilities
            .iter()
            .find(|capability| capability.id() == id)
            .expect("declared")
    };
    assert_eq!(find("widget.list").risk(), Risk::Read);
    assert!(!find("widget.list").needs_elevation());
    assert_eq!(find("widget.remove").risk(), Risk::Destructive);
}

#[tokio::test]
async fn should_refuse_an_action_whose_capability_the_provider_does_not_declare() {
    let error = registry()
        .act(&Action::new(
            "widget",
            "levitate",
            ObjectId::new(SchemaId::new("ono.widget", 1), [Value::Int(1)]),
        ))
        .await
        .expect_err("an undeclared operation must be refused");
    assert_eq!(error.code(), ErrorCode::ProviderUnsupported);
}

#[tokio::test]
async fn should_report_what_it_did_to_each_target_rather_than_one_boolean() {
    // Spec §11.5 and §16.5: a mutation answers with an ActionResult, never with a status alone.
    let result = registry()
        .act(&Action::new(
            "widget",
            "remove",
            ObjectId::new(SchemaId::new("ono.widget", 1), [Value::Int(1)]),
        ))
        .await
        .expect("the fixture removes widget 1");
    assert_eq!(result.operation(), "remove");
    assert!(result.changed());
    assert!(result.is_success());
}

#[tokio::test]
async fn should_report_a_failed_action_as_a_result_rather_than_as_an_error() {
    let result = registry()
        .act(&Action::new(
            "widget",
            "remove",
            ObjectId::new(SchemaId::new("ono.widget", 1), [Value::Int(99)]),
        ))
        .await
        .expect("a refusal is still a result");
    assert!(!result.is_success());
    assert!(!result.changed());
    assert!(
        result.error().is_some(),
        "a failed result carries its reason"
    );
}

#[tokio::test]
async fn should_resolve_a_selector_to_the_objects_it_names() {
    let found = registry()
        .resolve(
            "widget",
            &Selector::field("name", Value::String("beta".into())),
        )
        .await
        .expect("the fixture resolves by name");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id().schema(), &SchemaId::new("ono.widget", 1));
}

#[tokio::test]
async fn should_identify_the_same_object_across_two_observations() {
    // Spec §18.3 and ADR-0015 T13: a stable identity is what lets a live view update a row rather
    // than print a new one, and what stops a signal reaching a recycled pid.
    let first = registry()
        .snapshot(&Query::target("widget"))
        .expect("a stream")
        .collect()
        .await;
    let second = registry()
        .snapshot(&Query::target("widget"))
        .expect("a stream")
        .collect()
        .await;

    let ids = |collected: &ono_pipeline::Collected| -> Vec<ObjectId> {
        collected
            .values()
            .iter()
            .filter_map(|value| value.as_record().ok())
            .map(|record| ObjectId::of(record).expect("identified"))
            .collect()
    };
    assert_eq!(ids(&first), ids(&second));
}

#[tokio::test]
async fn should_deliver_a_snapshot_event_before_any_change_when_subscribed() {
    // Spec §31.14: the three primitives are snapshot, subscribe and watch, and a subscription
    // that skipped the initial state would make every consumer reconstruct it by hand.
    let mut events = registry()
        .subscribe(&Query::target("widget"))
        .expect("the fixture supports subscription");

    let first = events.recv().await.expect("at least one event");
    assert_eq!(first.kind(), EventKind::Snapshot);
    assert!(first.value().is_some());
    assert_eq!(first.schema(), &SchemaId::new("ono.widget", 1));
    assert!(first.provenance().provider() == "test.fixture");
}

#[tokio::test]
async fn should_say_a_change_is_a_change_and_which_fields_moved() {
    let mut events = registry()
        .subscribe(&Query::target("widget"))
        .expect("a subscription");
    let mut seen_change = false;
    for _ in 0..16 {
        let Some(event) = events.recv().await else {
            break;
        };
        if event.kind() == EventKind::Changed {
            assert!(
                event
                    .changed_fields()
                    .is_some_and(|fields| !fields.is_empty()),
                "a change must say what changed"
            );
            seen_change = true;
            break;
        }
    }
    assert!(seen_change, "the fixture emits a change");
}

#[tokio::test]
async fn should_refuse_a_subscription_a_provider_cannot_offer() {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(FixtureProvider::new().without_subscription()));
    let error = registry
        .subscribe(&Query::target("widget"))
        .expect_err("a provider that cannot watch must say so, not poll silently");
    assert_eq!(error.code(), ErrorCode::ProviderUnsupported);
}

#[tokio::test]
async fn should_validate_what_a_provider_emits_against_the_schema_it_advertises() {
    // Spec §35.3 and §36.5: a provider whose output leaves its advertised schema is a contract
    // violation, and the gate must be able to see it.
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(FixtureProvider::new().emitting_a_violation()));
    let collected = registry
        .snapshot(&Query::target("widget"))
        .expect("a stream")
        .collect()
        .await;
    assert!(
        collected
            .errors()
            .iter()
            .any(|error| error.code() == ErrorCode::ProviderSchemaViolation),
        "got {:?}",
        collected.errors()
    );
}

#[test]
fn should_describe_an_object_by_its_schema_and_identity_values() {
    let schema: Arc<Schema> = fixture_schema();
    let record = RecordValue::builder(
        schema.clone(),
        Provenance::local("test.fixture", schema.id().clone()).from_source("memory"),
    )
    .set("id", Value::Int(7))
    .expect("a valid field")
    .set("name", Value::String("gamma".into()))
    .expect("a valid field")
    .build();

    let id = ObjectId::of(&record).expect("the schema declares an identity");
    assert_eq!(id.schema(), schema.id());
    assert_eq!(id.values(), &[Value::Int(7)]);
    assert_eq!(id.to_string(), "ono.widget/1[7]");
}

#[test]
fn should_refuse_to_identify_a_record_whose_schema_declares_no_identity() {
    let anonymous = Schema::builder(SchemaId::new("ono.anonymous", 1), "Anonymous")
        .field(ono_value::FieldDef::new("value", ono_value::FieldType::Int).required())
        .build()
        .expect("a valid schema");
    let id = anonymous.id().clone();
    let record = RecordValue::builder(
        Arc::new(anonymous),
        Provenance::local("test.fixture", id).from_source("memory"),
    )
    .set("value", Value::Int(1))
    .expect("a valid field")
    .build();

    assert!(
        ObjectId::of(&record).is_none(),
        "a record with no declared identity has no object identity to speak of"
    );
}
