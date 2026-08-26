//! A provider that exists only so the command implementations can be exercised without depending
//! on the machine the tests run on.
//!
//! It answers about `process`, because that is a target the command registry declares and these
//! tests drive the registry's own contracts. What it answers *with* is `ono.widget/1`, a shape of
//! its own: three objects whose sizes are chosen so the three-valued semantics of ADR-0014 are
//! observable — one below a kibibyte, one above it, and one whose size is unknown.
//!
//! It honours the [`Query`] rather than ignoring it, so a test can tell whether a selector reached
//! the provider or was filtered afterwards (spec §27.1).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    dead_code,
    reason = "a fixture states its preconditions the way a test does, and not every helper is \
              used by every test binary (AGENTS.md section 16)"
)]

use std::sync::{Arc, OnceLock};

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectRef, Provider, ProviderRegistry, Query,
    Risk, Selector,
};
use ono_value::{
    ByteSize, ErrorValue, FieldDef, FieldType, Provenance, RecordBuilder, RecordValue, Schema,
    SchemaId, Value,
};

/// The schema the fixture advertises, `ono.widget/1`.
pub fn widget_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            Arc::new(
                Schema::builder(SchemaId::new("ono.widget", 1), "Widget")
                    .field(FieldDef::new("pid", FieldType::Int).required())
                    .field(FieldDef::new("name", FieldType::String).required())
                    .field(FieldDef::new("size", FieldType::ByteSize).nullable())
                    .field(FieldDef::new("owner", FieldType::String).nullable())
                    .identity(["pid"])
                    .default_view(["pid", "name", "size"])
                    .build()
                    .expect("the fixture schema is valid"),
            )
        })
        .clone()
}

/// One widget. A `size` of `None` is the unknown of spec §10.5, never a fabricated zero.
pub fn widget(pid: i128, name: &str, size: Option<u128>, owner: &str) -> RecordValue {
    let schema = widget_schema();
    // No observed timestamp: the fixture must render identically on every run, which is what
    // makes an assertion over a serialisation possible at all.
    let provenance = Provenance::local("test.fixture", schema.id().clone()).from_source("memory");
    RecordValue::builder(schema, provenance)
        .set("pid", Value::Int(pid))
        .and_then(|builder| builder.set("name", Value::string(name)))
        .and_then(|builder| {
            builder.set(
                "size",
                size.map_or(Value::Null, |bytes| {
                    Value::ByteSize(ByteSize::from_bytes(bytes))
                }),
            )
        })
        .and_then(|builder| builder.set("owner", Value::string(owner)))
        .map(RecordBuilder::build)
        .expect("the fixture record is valid")
}

/// The three widgets the fixture holds.
pub fn widgets() -> Vec<RecordValue> {
    vec![
        widget(1, "alpha", Some(512), "root"),
        widget(2, "beta", Some(2048), "ono"),
        widget(3, "gamma", None, "root"),
    ]
}

/// The widgets as pipeline values.
pub fn widget_values() -> Vec<Value> {
    widgets().into_iter().map(RecordValue::into_value).collect()
}

/// A provider over the three widgets.
#[derive(Debug)]
pub struct FixtureProvider {
    availability: Availability,
    failing: Vec<i128>,
    endless: bool,
}

impl Default for FixtureProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FixtureProvider {
    /// A provider that answers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            availability: Availability::Available,
            failing: Vec::new(),
            endless: false,
        }
    }

    /// A provider that cannot answer here, and says why (spec §35.3, ADR-0015).
    #[must_use]
    pub fn unavailable(reason: &str) -> Self {
        Self {
            availability: Availability::unavailable(reason),
            failing: Vec::new(),
            endless: false,
        }
    }

    /// Makes the mutation of one object fail, so a bulk mutation can be observed not to collapse.
    #[must_use]
    pub fn failing_on(mut self, pid: i128) -> Self {
        self.failing.push(pid);
        self
    }

    /// Makes the snapshot a stream that never ends, so the rule of spec §11.1 is testable.
    #[must_use]
    pub fn endless(mut self) -> Self {
        self.endless = true;
        self
    }
}

#[async_trait::async_trait]
impl Provider for FixtureProvider {
    fn id(&self) -> &str {
        "test.fixture"
    }

    fn targets(&self) -> &[&str] {
        &["process"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![widget_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("process.list", Risk::Read),
            Capability::new("process.signal", Risk::Mutate),
        ]
    }

    fn availability(&self) -> Availability {
        self.availability.clone()
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        // The provider narrows itself, which is the push-down of spec §27.1: a test asking for one
        // object can then tell that the selector reached the provider.
        let matching: Vec<Value> = widgets()
            .into_iter()
            .filter(|widget| query.matches(widget))
            .map(RecordValue::into_value)
            .collect();
        let boundedness = if self.endless {
            Boundedness::Unbounded
        } else {
            Boundedness::Bounded
        };
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            boundedness,
            move |sink| async move {
                loop {
                    for value in &matching {
                        if sink.send(value.clone()).await.is_err() {
                            return;
                        }
                    }
                    if boundedness == Boundedness::Bounded {
                        return;
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        Ok(widgets()
            .into_iter()
            .filter(|widget| selector.matches(widget))
            .filter_map(|widget| ObjectRef::of(&widget))
            .collect())
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        if !matches!(action.operation(), "stop" | "kill" | "start") {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("the fixture has no operation `{}`", action.operation()),
            ));
        }
        let pid = action
            .target()
            .values()
            .first()
            .and_then(|value| value.as_int().ok())
            .unwrap_or(-1);
        if self.failing.contains(&pid) {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(ErrorCode::IoPermissionDenied, "this one is not yours"),
            ));
        }
        Ok(ActionOutcome::succeeded(action, true))
    }
}

/// A registry holding one fixture provider.
#[must_use]
pub fn providers(provider: FixtureProvider) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(provider));
    registry
}

/// A registry with no providers at all.
#[must_use]
pub fn no_providers() -> ProviderRegistry {
    ProviderRegistry::new()
}
