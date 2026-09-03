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

/// A schema whose identity is one nullable field, as `ono.socket/1`'s `inode` is.
fn remnant_schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.remnant", 1), "Remnant")
            .field(ono_value::FieldDef::new("inode", ono_value::FieldType::Int).nullable())
            .field(ono_value::FieldDef::new("state", ono_value::FieldType::String).required())
            .identity(["inode"])
            .default_view(["inode", "state"])
            .build()
            .expect("a valid schema"),
    )
}

fn remnant(state: &str) -> RecordValue {
    let schema = remnant_schema();
    let id = schema.id().clone();
    RecordValue::builder(
        schema,
        Provenance::local("test.fixture", id).from_source("memory"),
    )
    .set("inode", Value::Null)
    .expect("a valid field")
    .set("state", Value::String(state.into()))
    .expect("a valid field")
    .build()
}

#[test]
fn should_refuse_to_identify_a_record_whose_every_identity_component_is_null() {
    // Spec §2.17 and §35.3: identity is what the identity fields say, and a null is the absence
    // of a value rather than a value. A record that supplies none of them says nothing about
    // which object it is.
    let record = remnant("time-wait");

    assert!(
        ObjectId::of(&record).is_none(),
        "every identity component is null, so the record carries no identity: giving it one \
         would make every such record the same object"
    );
    assert!(
        ono_provider_api::ObjectRef::of(&record).is_none(),
        "a reference names an object, and there is no object here to name"
    );
}

#[test]
fn should_not_make_two_records_the_same_object_because_both_have_no_identity() {
    // Two sockets in TIME_WAIT have no inode each. They are two remnants of two connections,
    // and an identity built from two nulls made them one place on every map (§42.1).
    let left = remnant("time-wait");
    let right = remnant("close");

    assert_eq!(ObjectId::of(&left), None);
    assert_eq!(ObjectId::of(&right), None);
}

#[test]
fn should_keep_identifying_a_record_whose_identity_is_only_partly_null() {
    // `ono.route/1` identifies by (table, family, destination, gateway, interface), and the
    // default route has no destination. That record is still an object.
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.partial", 1), "Partial")
            .field(ono_value::FieldDef::new("table", ono_value::FieldType::String).required())
            .field(ono_value::FieldDef::new("destination", ono_value::FieldType::String).nullable())
            .identity(["table", "destination"])
            .default_view(["table", "destination"])
            .build()
            .expect("a valid schema"),
    );
    let id = schema.id().clone();
    let record = RecordValue::builder(
        schema,
        Provenance::local("test.fixture", id).from_source("memory"),
    )
    .set("table", Value::String("main".into()))
    .expect("a valid field")
    .set("destination", Value::Null)
    .expect("a valid field")
    .build();

    let identity = ObjectId::of(&record).expect("one identity component is present");
    assert_eq!(
        identity.values(),
        &[Value::String("main".into()), Value::Null]
    );
}

// --- a record is acted on by the provider its identity names (issue #16, ADR-0559) ------------

/// A target two providers claim, whose objects say which of the two made them.
///
/// This is `ono.package/1` in miniature: `provider + name`, one record shape, two databases a
/// machine can carry at once. Debian with `rpm` installed is the real case.
mod two_databases {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
    use ono_provider_api::{
        Action, ActionOutcome, Availability, Capability, Provider, Query, Risk, Selector,
    };
    use ono_value::{
        ErrorValue, FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, Value,
    };

    pub fn schema() -> Arc<Schema> {
        Arc::new(
            Schema::builder(SchemaId::new("ono.parcel", 1), "Parcel")
                .field(FieldDef::new("provider", FieldType::String).required())
                .field(FieldDef::new("name", FieldType::String).required())
                .identity(["provider", "name"])
                .default_view(["provider", "name"])
                .build()
                .expect("the parcel schema is valid"),
        )
    }

    pub fn record(database: &str, name: &str) -> RecordValue {
        let schema = schema();
        let provenance = Provenance::local("test.parcels", schema.id().clone());
        RecordValue::builder(schema, provenance)
            .set("provider", Value::string(database))
            .expect("declared")
            .set("name", Value::string(name))
            .expect("declared")
            .build()
    }

    /// A provider over one database, counting the actions it was asked to perform.
    #[derive(Debug)]
    pub struct Database {
        id: &'static str,
        database: &'static str,
        pub acted: Arc<AtomicUsize>,
    }

    impl Database {
        pub fn new(id: &'static str, database: &'static str) -> Self {
            Self {
                id,
                database,
                acted: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl Provider for Database {
        fn id(&self) -> &str {
            self.id
        }

        fn targets(&self) -> &[&str] {
            &["parcel"]
        }

        fn identity_token(&self) -> Option<&str> {
            Some(self.database)
        }

        fn schemas(&self) -> Vec<Arc<Schema>> {
            vec![schema()]
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::new("parcel.manage", Risk::Mutate)]
        }

        fn availability(&self) -> Availability {
            Availability::Available
        }

        fn snapshot(&self, _query: &Query) -> Result<ValueStream, ErrorValue> {
            let value = Value::Record(Arc::new(record(self.database, "curl")));
            Ok(ValueStream::spawn(
                PipelineConfig::new(),
                Boundedness::Bounded,
                move |sink| async move {
                    let _ = sink.send(value).await;
                },
            ))
        }

        async fn resolve(
            &self,
            _selector: &Selector,
        ) -> Result<Vec<ono_provider_api::ObjectRef>, ErrorValue> {
            Ok(Vec::new())
        }

        async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
            self.acted.fetch_add(1, Ordering::SeqCst);
            Ok(ActionOutcome::succeeded(action, true))
        }
    }
}

#[tokio::test]
async fn should_act_through_the_provider_the_records_identity_names() {
    let first = Arc::new(two_databases::Database::new("test.first", "alpha"));
    let second = Arc::new(two_databases::Database::new("test.second", "beta"));
    let (acted_first, acted_second) = (Arc::clone(&first.acted), Arc::clone(&second.acted));
    let mut registry = ProviderRegistry::new();
    registry.register(first);
    registry.register(second);

    // A record the *second* database made, handed down a pipeline to a mutation.
    let object = ObjectId::of(&two_databases::record("beta", "curl")).expect("an identity");
    let outcome = registry
        .act(&Action::new("parcel", "remove", object))
        .await
        .expect("the action is performed");

    assert_eq!(outcome.status(), ono_value::ActionStatus::Success);
    assert_eq!(
        (
            acted_first.load(std::sync::atomic::Ordering::SeqCst),
            acted_second.load(std::sync::atomic::Ordering::SeqCst)
        ),
        (0, 1),
        "the record names `beta`, so `beta` acts on it — not whichever provider registered first"
    );
}

#[tokio::test]
async fn should_refuse_by_name_when_the_provider_a_record_names_is_not_here() {
    let only = Arc::new(two_databases::Database::new("test.first", "alpha"));
    let acted = Arc::clone(&only.acted);
    let mut registry = ProviderRegistry::new();
    registry.register(only);

    let object = ObjectId::of(&two_databases::record("beta", "curl")).expect("an identity");
    let error = registry
        .act(&Action::new("parcel", "remove", object))
        .await
        .expect_err("a record made elsewhere is not this database's to act on");

    assert_eq!(error.code(), ErrorCode::ProviderUnavailable);
    assert!(
        error.message().contains("beta"),
        "the refusal names the provider the record named, got {}",
        error.message()
    );
    assert_eq!(
        acted.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "nothing else may act on it in its place"
    );
}

#[tokio::test]
async fn should_act_through_the_first_available_provider_when_a_selector_named_the_object() {
    // `add package curl` builds `ono.package/1[curl]` — the name a user typed, not the identity
    // a record carries. A name says nothing about which of two databases it belongs to, so the
    // question the routing answers is not being asked and the ordinary resolution stands
    // (ADR-0559).
    let first = Arc::new(two_databases::Database::new("test.first", "alpha"));
    let second = Arc::new(two_databases::Database::new("test.second", "beta"));
    let (acted_first, acted_second) = (Arc::clone(&first.acted), Arc::clone(&second.acted));
    let mut registry = ProviderRegistry::new();
    registry.register(first);
    registry.register(second);

    let named = ObjectId::new(
        ono_value::SchemaId::new("ono.parcel", 1),
        [Value::string("curl")],
    );
    registry
        .act(&Action::new("parcel", "remove", named))
        .await
        .expect("a package named by hand is acted on by the provider that answers");

    assert_eq!(
        (
            acted_first.load(std::sync::atomic::Ordering::SeqCst),
            acted_second.load(std::sync::atomic::Ordering::SeqCst)
        ),
        (1, 0)
    );
}
