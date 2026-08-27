//! A provider that exists only to exercise the contract.
//!
//! It is deliberately a *complete* provider — snapshot, subscription, resolution and action — so
//! that the contract tests exercise every path a real provider must implement, and so that a
//! later provider author has one worked example to read.

#![allow(
    clippy::panic,
    clippy::expect_used,
    dead_code,
    reason = "a fixture states its preconditions the way a test does; not every helper is used by every test file"
)]

use std::sync::Arc;
use std::sync::OnceLock;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, EventStream, ObjectEvent, ObjectRef, Provider,
    Query, Risk, Selector,
};
use ono_value::{
    ErrorValue, FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, Value,
};

/// The schema the fixture advertises.
pub fn fixture_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            Arc::new(
                Schema::builder(SchemaId::new("ono.widget", 1), "Widget")
                    .field(FieldDef::new("id", FieldType::Int).required())
                    .field(FieldDef::new("name", FieldType::String).required())
                    .field(FieldDef::new("size", FieldType::ByteSize).nullable())
                    .identity(["id"])
                    .default_view(["id", "name", "size"])
                    .build()
                    .expect("the fixture schema is valid"),
            )
        })
        .clone()
}

/// A provider over three widgets held in memory.
#[derive(Debug)]
pub struct FixtureProvider {
    availability: Availability,
    subscribes: bool,
    violate: bool,
}

impl FixtureProvider {
    pub fn new() -> Self {
        Self {
            availability: Availability::Available,
            subscribes: true,
            violate: false,
        }
    }

    pub fn unavailable(reason: &str) -> Self {
        Self {
            availability: Availability::unavailable(reason),
            subscribes: true,
            violate: false,
        }
    }

    pub fn without_subscription(mut self) -> Self {
        self.subscribes = false;
        self
    }

    pub fn emitting_a_violation(mut self) -> Self {
        self.violate = true;
        self
    }

    fn widget(&self, id: i128, name: &str) -> RecordValue {
        let schema = fixture_schema();
        let provenance =
            Provenance::local("test.fixture", schema.id().clone()).from_source("memory");
        let builder = RecordValue::builder(schema, provenance).set("id", Value::Int(id));
        let builder = builder.expect("a valid field");
        let builder = if self.violate {
            // A required string field holding a number is exactly the drift spec §35.3 asks the
            // conformance suite to catch.
            builder.set("name", Value::Int(0))
        } else {
            builder.set("name", Value::String(name.into()))
        };
        builder.unwrap_or_else(|error| panic!("{error}")).build()
    }

    fn widgets(&self) -> Vec<RecordValue> {
        vec![
            self.widget(1, "alpha"),
            self.widget(2, "beta"),
            self.widget(3, "gamma"),
        ]
    }
}

#[async_trait::async_trait]
impl Provider for FixtureProvider {
    fn id(&self) -> &str {
        "test.fixture"
    }

    fn targets(&self) -> &[&str] {
        &["widget"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![fixture_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("widget.list", Risk::Read),
            Capability::new("widget.watch", Risk::Observe),
            Capability::new("widget.remove", Risk::Destructive),
        ]
    }

    fn availability(&self) -> Availability {
        self.availability.clone()
    }

    fn snapshot(&self, _query: &Query) -> Result<ValueStream, ErrorValue> {
        let widgets = self.widgets();
        let violate = self.violate;
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                for widget in widgets {
                    if violate {
                        let _ = sink
                            .fail(ErrorValue::new(
                                ErrorCode::ProviderSchemaViolation,
                                "the fixture was asked to emit a violation",
                            ))
                            .await;
                        continue;
                    }
                    if sink.send(Value::Record(Arc::new(widget))).await.is_err() {
                        break;
                    }
                }
            },
        ))
    }

    fn subscribe(&self, _query: &Query) -> Result<EventStream, ErrorValue> {
        if !self.subscribes {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                "the fixture was built without subscription support",
            ));
        }
        let widgets = self.widgets();
        Ok(EventStream::spawn(
            PipelineConfig::new(),
            move |sink| async move {
                for widget in &widgets {
                    let event = ObjectEvent::snapshot(widget);
                    if sink.send(event).await.is_err() {
                        return;
                    }
                }
                if let Some(widget) = widgets.first() {
                    let changed = ObjectEvent::changed(widget, ["name"]);
                    let _ = sink.send(changed).await;
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        Ok(self
            .widgets()
            .into_iter()
            .filter(|widget| selector.matches(widget))
            .filter_map(|widget| ObjectRef::of(&widget))
            .collect())
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        if action.operation() != "remove" {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("the fixture has no operation `{}`", action.operation()),
            ));
        }
        let exists = action
            .target()
            .values()
            .first()
            .and_then(|value| value.as_int().ok())
            .is_some_and(|id| (1..=3).contains(&id));

        if exists {
            Ok(ActionOutcome::succeeded(action, true))
        } else {
            Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(ErrorCode::IoNotFound, "no such widget"),
            ))
        }
    }
}
