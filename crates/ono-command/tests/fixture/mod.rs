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
    live: Option<LiveWidgets>,
}

/// A shared, mutable widget population, so a test can change the world between two polls.
#[derive(Clone, Debug)]
pub struct LiveWidgets {
    state: Arc<std::sync::Mutex<Vec<RecordValue>>>,
}

impl LiveWidgets {
    fn new() -> Self {
        Self {
            state: Arc::new(std::sync::Mutex::new(widgets())),
        }
    }

    fn current(&self) -> Vec<RecordValue> {
        self.state.lock().expect("the fixture lock").clone()
    }

    /// Replaces widget `pid`'s size.
    pub fn set_size(&self, pid: i128, size: u128) {
        let mut state = self.state.lock().expect("the fixture lock");
        if let Some(position) = state
            .iter()
            .position(|record| record.get("pid") == Some(&Value::Int(pid)))
        {
            let name = state[position]
                .get("name")
                .and_then(|value| value.as_str().ok().map(str::to_owned))
                .unwrap_or_default();
            let owner = state[position]
                .get("owner")
                .and_then(|value| value.as_str().ok().map(str::to_owned))
                .unwrap_or_default();
            state[position] = widget(pid, &name, Some(size), &owner);
        }
    }

    /// Adds a widget.
    pub fn add(&self, pid: i128, name: &str, size: Option<u128>, owner: &str) {
        self.state
            .lock()
            .expect("the fixture lock")
            .push(widget(pid, name, size, owner));
    }

    /// Removes widget `pid`.
    pub fn remove(&self, pid: i128) {
        self.state
            .lock()
            .expect("the fixture lock")
            .retain(|record| record.get("pid") != Some(&Value::Int(pid)));
    }
}

impl Default for FixtureProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FixtureProvider {
    /// A provider whose population a test can mutate between polls.
    #[must_use]
    pub fn live() -> Self {
        let mut provider = Self::new();
        provider.live = Some(LiveWidgets::new());
        provider
    }

    /// The handle a test mutates the live population through.
    ///
    /// # Panics
    ///
    /// Panics when the provider was not built with [`FixtureProvider::live`].
    #[must_use]
    pub fn handle(&self) -> LiveWidgets {
        self.live.clone().expect("a live fixture")
    }

    /// A provider that answers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            availability: Availability::Available,
            failing: Vec::new(),
            endless: false,
            live: None,
        }
    }

    /// A provider that cannot answer here, and says why (spec §35.3, ADR-0015).
    #[must_use]
    pub fn unavailable(reason: &str) -> Self {
        Self {
            availability: Availability::unavailable(reason),
            failing: Vec::new(),
            endless: false,
            live: None,
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
        let population = match &self.live {
            Some(live) => live.current(),
            None => widgets(),
        };
        let matching: Vec<Value> = population
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

// --- driving the command table ------------------------------------------------------------------

use ono_command::{
    CommandRegistry, CommandTable, Invocation, Outcome, Scope, builtin_commands, check_pipeline,
};
use ono_pipeline::Collected;

/// The embedded registry.
pub fn registry() -> &'static CommandRegistry {
    CommandRegistry::embedded().expect("the embedded command contracts must parse")
}

/// The table this build delivers.
pub fn table() -> CommandTable {
    builtin_commands(registry())
}

/// What a whole pipeline produced.
#[derive(Debug)]
pub enum Ran {
    /// The values the last stage emitted, with the partial failures beside them.
    Values(Collected),
    /// One outcome per target, from a mutating last stage (spec §11.5).
    Actions(Vec<ono_provider_api::ActionOutcome>),
}

impl Ran {
    /// The values, when the pipeline produced values.
    pub fn values(&self) -> &[Value] {
        match self {
            Ran::Values(collected) => collected.values(),
            Ran::Actions(_) => panic!("this pipeline ended in a mutation"),
        }
    }

    /// The partial failures, when the pipeline produced values.
    pub fn failures(&self) -> &[ErrorValue] {
        match self {
            Ran::Values(collected) => collected.errors(),
            Ran::Actions(_) => panic!("this pipeline ended in a mutation"),
        }
    }

    /// The per-target outcomes, when the pipeline ended in a mutation.
    pub fn actions(&self) -> &[ono_provider_api::ActionOutcome] {
        match self {
            Ran::Actions(outcomes) => outcomes,
            Ran::Values(_) => panic!("this pipeline produced values"),
        }
    }

    /// The single value the pipeline produced.
    pub fn only(&self) -> &Value {
        match self.values() {
            [value] => value,
            other => panic!("expected exactly one value, got {}", other.len()),
        }
    }

    /// The single value as text.
    pub fn text(&self) -> String {
        self.only()
            .as_str()
            .expect("the pipeline produced text")
            .to_owned()
    }
}

/// Runs `source` against `providers`, the way the evaluator will.
pub async fn run(source: &str, providers: &ProviderRegistry) -> Result<Ran, ErrorValue> {
    run_full(source, providers, Scope::new(), Vec::new()).await
}

/// Runs `source` inside the given context frames (spec §14.3).
pub async fn run_with_context(
    source: &str,
    providers: &ProviderRegistry,
    context: Vec<ono_command::ContextFrame>,
) -> Result<Ran, ErrorValue> {
    run_full(source, providers, Scope::new(), context).await
}

/// Runs `source` with a scope of shell bindings.
pub async fn run_in(
    source: &str,
    providers: &ProviderRegistry,
    scope: Scope,
) -> Result<Ran, ErrorValue> {
    run_full(source, providers, scope, Vec::new()).await
}

/// Runs `source` with shell bindings and context frames.
pub async fn run_full(
    source: &str,
    providers: &ProviderRegistry,
    scope: Scope,
    context: Vec<ono_command::ContextFrame>,
) -> Result<Ran, ErrorValue> {
    let table = table();
    let scope = Arc::new(scope);
    let parsed = ono_parser::parse(source);
    assert!(
        parsed.diagnostics().is_empty(),
        "`{source}` must parse cleanly, but produced {:?}",
        parsed.diagnostics()
    );
    let pipeline = parsed
        .program()
        .statements
        .first()
        .and_then(ono_parser::Statement::as_pipeline)
        .expect("the source is a pipeline")
        .clone();

    let mut stream: Option<ono_pipeline::ValueStream> = None;
    let mut actions = None;
    let lists =
        std::iter::once(&pipeline.head).chain(pipeline.tail.iter().map(|chained| &chained.list));
    for list in lists {
        for stage in &list.stages {
            let head = stage.head.name().expect("a command head");
            let resolved = registry().resolve(head, &stage.arguments)?;
            let bound = resolved.contract.bind(resolved.arguments)?;
            let mut invocation = Invocation::new(resolved.contract, &bound, providers)
                .with_scope(Arc::clone(&scope))
                .with_context(context.clone());
            if let Some(input) = stream.take() {
                invocation = invocation.with_input(input);
            }
            match table.run(resolved.contract.id(), &mut invocation).await? {
                Outcome::Values(values) => stream = Some(values),
                Outcome::Actions(outcomes) => actions = Some(outcomes),
            }
        }
    }

    match (stream, actions) {
        (_, Some(outcomes)) => Ok(Ran::Actions(outcomes)),
        (Some(values), None) => Ok(Ran::Values(values.collect().await)),
        (None, None) => panic!("`{source}` produced nothing at all"),
    }
}

/// Checks `source` the way spec §11.3 asks, before anything runs.
pub fn check(source: &str) -> Result<(), ErrorValue> {
    let schemas: Vec<Arc<Schema>> = ono_value::builtin_schemas().schemas().cloned().collect();
    let parsed = ono_parser::parse(source);
    let pipeline = parsed
        .program()
        .statements
        .first()
        .and_then(ono_parser::Statement::as_pipeline)
        .expect("the source is a pipeline");
    check_pipeline(registry(), &schemas, pipeline)
}
