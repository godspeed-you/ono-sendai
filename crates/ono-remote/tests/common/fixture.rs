//! The fixture provider both ends of the remote suites share.
//!
//! It stands in for `linux.procfs` on a remote machine: a small, deterministic set of
//! process-like records, an endless target for cancellation suites, a flaky target for spec
//! §16.5, and an unavailable provider so negotiation visibility (spec §21.3, §35.3) is
//! assertable. The same file is compiled into the `ono-remote-fixture-agent` binary (via
//! `#[path]`), so the child-process suite serves exactly the objects the in-process suites do.

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a shared test fixture states preconditions the same way a #[test] body does, and \
              each consumer uses a different subset of the helpers"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, EventStream, ObjectEvent, ObjectRef, Provider,
    ProviderRegistry, Query, Risk, Selector,
};
use ono_value::{
    ErrorValue, FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, SchemaRegistry,
    Value,
};

/// The id of the schema the fixture provider produces.
pub fn fixture_schema_id() -> SchemaId {
    SchemaId::new("ono.test.remote-fixture", 1)
}

/// The schema the fixture provider produces.
pub fn fixture_schema() -> Schema {
    Schema::builder(fixture_schema_id(), "FixtureProcess")
        .field(FieldDef::new("pid", FieldType::Int).required())
        .field(FieldDef::new("name", FieldType::String).nullable())
        .identity(["pid"])
        .default_view(["pid", "name"])
        .build()
        .expect("the fixture schema is well formed")
}

/// A registry holding the fixture schema, for the client end of a link.
pub fn fixture_schemas() -> Arc<SchemaRegistry> {
    let mut registry = SchemaRegistry::new();
    registry
        .register(fixture_schema())
        .expect("the fixture schema registers");
    Arc::new(registry)
}

/// One record as the fixture provider observes it: locally, on the "remote" machine.
pub fn fixture_record(pid: i128, name: &str) -> RecordValue {
    let schema = Arc::new(fixture_schema());
    let provenance = Provenance::local("fixture.demo", schema.id().clone())
        .from_source(&format!("fixture://process/{pid}"));
    RecordValue::builder(schema, provenance)
        .set("pid", Value::Int(pid))
        .expect("pid is a field of the fixture schema")
        .set("name", Value::String(name.into()))
        .expect("name is a field of the fixture schema")
        .build()
}

/// Every record the fixture's `process` target answers with, in order.
pub fn fixture_records() -> Vec<RecordValue> {
    vec![
        fixture_record(1, "init"),
        fixture_record(2, "portd"),
        fixture_record(3, "nginx"),
    ]
}

/// What the fixture provider observed while it ran.
#[derive(Debug, Default)]
pub struct FixtureObserved {
    /// How many `tick` values were sent successfully.
    pub ticks_sent: AtomicUsize,
    /// Whether the endless `tick` producer noticed that its consumer was gone.
    pub tick_cancelled: AtomicBool,
}

impl FixtureObserved {
    /// How many `tick` values were sent so far.
    pub fn ticks_sent(&self) -> usize {
        self.ticks_sent.load(Ordering::SeqCst)
    }

    /// Whether the endless producer observed cancellation.
    pub fn tick_cancelled(&self) -> bool {
        self.tick_cancelled.load(Ordering::SeqCst)
    }
}

/// The provider the fixture agent serves: deterministic, offline, self-describing.
#[derive(Debug, Default)]
pub struct FixtureProvider {
    observed: Arc<FixtureObserved>,
}

impl FixtureProvider {
    /// A provider reporting what it did into `observed`.
    pub fn new(observed: Arc<FixtureObserved>) -> Self {
        Self { observed }
    }
}

#[async_trait::async_trait]
impl Provider for FixtureProvider {
    fn id(&self) -> &str {
        "fixture.demo"
    }

    fn targets(&self) -> &[&str] {
        &["process", "tick", "flaky"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![Arc::new(fixture_schema())]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("process.list", Risk::Read),
            Capability::new("process.signal", Risk::Mutate).needing_elevation(),
        ]
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let observed = Arc::clone(&self.observed);
        match query.target_name() {
            "process" => {
                let limit = query.max().unwrap_or(usize::MAX);
                let min_pid = match query.option_value("min-pid") {
                    Some(Value::Int(min)) => *min,
                    _ => i128::MIN,
                };
                let query = query.clone();
                Ok(ValueStream::spawn(
                    PipelineConfig::new(),
                    Boundedness::Bounded,
                    move |sink| async move {
                        let mut sent = 0;
                        for record in fixture_records() {
                            if sent >= limit || !query.matches(&record) {
                                continue;
                            }
                            if !matches!(record.get("pid"), Some(Value::Int(pid)) if *pid >= min_pid)
                            {
                                continue;
                            }
                            if sink.send(record.into_value()).await.is_err() {
                                return;
                            }
                            sent += 1;
                        }
                    },
                ))
            }
            "tick" => Ok(ValueStream::spawn(
                PipelineConfig::new(),
                Boundedness::Bounded,
                move |sink| async move {
                    let mut pid = 0i128;
                    loop {
                        let record = fixture_record(pid, "tick");
                        if sink.send(record.into_value()).await.is_err() {
                            observed.tick_cancelled.store(true, Ordering::SeqCst);
                            return;
                        }
                        observed.ticks_sent.fetch_add(1, Ordering::SeqCst);
                        pid += 1;
                    }
                },
            )),
            "flaky" => Ok(ValueStream::spawn(
                PipelineConfig::new(),
                Boundedness::Bounded,
                move |sink| async move {
                    let _ = sink.send(fixture_record(1, "init").into_value()).await;
                    let _ = sink
                        .fail(ErrorValue::new(
                            ErrorCode::IoPermissionDenied,
                            "one object could not be read",
                        ))
                        .await;
                    let _ = sink.send(fixture_record(2, "portd").into_value()).await;
                },
            )),
            other => Err(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("the fixture has no target `{other}`"),
            )),
        }
    }

    fn subscribe(&self, query: &Query) -> Result<EventStream, ErrorValue> {
        if query.target_name() != "process" {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                "only the fixture's `process` target can be watched",
            ));
        }
        Ok(EventStream::spawn(
            PipelineConfig::new(),
            |sink| async move {
                let record = fixture_record(3, "nginx");
                let _ = sink
                    .send(ObjectEvent::snapshot(&record).with_sequence(1))
                    .await;
                let _ = sink
                    .send(ObjectEvent::removed(&record).with_sequence(2))
                    .await;
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        Ok(fixture_records()
            .iter()
            .filter(|record| selector.matches(record))
            .filter_map(ObjectRef::of)
            .collect())
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        if action.operation() != "stop" {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("the fixture cannot `{}`", action.operation()),
            ));
        }
        let Some(Value::String(signal)) = action.argument("signal") else {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(
                    ErrorCode::ProviderUnsupported,
                    "stop needs a `signal` argument",
                ),
            ));
        };
        if action.target().values().first() == Some(&Value::Int(1)) {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(
                    ErrorCode::SafetyPolicyDenied,
                    "pid 1 is protected in this fixture",
                ),
            ));
        }
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!("dry run: would send {signal}"),
            ));
        }
        Ok(ActionOutcome::succeeded(action, true))
    }
}

/// A provider that exists on the remote machine but cannot answer there (spec §35.3).
#[derive(Debug, Default)]
pub struct AbsentProvider;

#[async_trait::async_trait]
impl Provider for AbsentProvider {
    fn id(&self) -> &str {
        "fixture.absent"
    }

    fn targets(&self) -> &[&str] {
        &["service"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        Vec::new()
    }

    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }

    fn availability(&self) -> Availability {
        Availability::unavailable("no service manager in this fixture")
    }

    fn snapshot(&self, _query: &Query) -> Result<ValueStream, ErrorValue> {
        Err(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            "no service manager in this fixture",
        ))
    }

    async fn resolve(&self, _selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        Err(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            "no service manager in this fixture",
        ))
    }
}

/// The registry the fixture agent serves: one working provider, one visibly absent one.
pub fn fixture_registry(observed: Arc<FixtureObserved>) -> Arc<ProviderRegistry> {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(FixtureProvider::new(observed)));
    registry.register(Arc::new(AbsentProvider));
    Arc::new(registry)
}

// --- the temporal half of the fixture (v0.5 §21.1, §24) ---------------------------------------

/// The target the historical fixture provider answers about.
pub const HISTORICAL_TARGET: &str = "archived";

/// One record observed at a stated instant, so a suite can assert on source time.
pub fn fixture_record_observed(pid: i128, name: &str, at: jiff::Timestamp) -> RecordValue {
    let schema = Arc::new(fixture_schema());
    let provenance = Provenance::local("fixture.demo", schema.id().clone())
        .observed_at(at)
        .from_source(&format!("fixture://process/{pid}"));
    RecordValue::builder(schema, provenance)
        .set("pid", Value::Int(pid))
        .expect("pid is a field of the fixture schema")
        .set("name", Value::String(name.into()))
        .expect("name is a field of the fixture schema")
        .build()
}

/// A provider that keeps history and says so (spec v0.5 §21.1, §21.4).
///
/// It stands in for journald on the far side: it answers about the past, it declares that it
/// does, and it never claims `exhaustive_events`, which §21.5 forbids a source from advertising
/// merely because events usually arrive.
#[derive(Debug, Default)]
pub struct HistoricalProvider;

#[async_trait::async_trait]
impl Provider for HistoricalProvider {
    fn id(&self) -> &str {
        "fixture.archive"
    }

    fn targets(&self) -> &[&str] {
        &[HISTORICAL_TARGET]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![Arc::new(fixture_schema())]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("archive.read", Risk::Read)]
    }

    fn temporal(&self) -> ono_provider_api::TemporalCapabilities {
        ono_provider_api::TemporalCapabilities {
            current_snapshot: true,
            live_events: false,
            historical_query: true,
            exhaustive_events: false,
            causal_tokens: false,
            checkpointable: false,
            retained_history: Some(ono_value::Duration::from_nanoseconds(86_400_000_000_000)),
        }
    }

    fn snapshot(&self, _query: &Query) -> Result<ValueStream, ErrorValue> {
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            |sink| async move {
                let _ = sink.send(fixture_record(9, "archived").into_value()).await;
            },
        ))
    }

    fn history(
        &self,
        _query: &Query,
        window: &ono_provider_api::TimeWindow,
    ) -> Result<ValueStream, ErrorValue> {
        let window = *window;
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                for (pid, at) in [
                    (1_i128, "2026-08-31T11:55:00Z"),
                    (2, "2026-08-31T11:56:00Z"),
                    (3, "2026-08-31T10:00:00Z"),
                ] {
                    let at: jiff::Timestamp = match at.parse() {
                        Ok(at) => at,
                        Err(_) => continue,
                    };
                    if !window.contains(at) {
                        continue;
                    }
                    if sink
                        .send(fixture_record_observed(pid, "archived", at).into_value())
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            },
        ))
    }

    async fn resolve(&self, _selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        Ok(Vec::new())
    }
}

/// The fixture registry, plus a history-keeping provider when `historical` is set.
pub fn temporal_registry(
    observed: Arc<FixtureObserved>,
    historical: bool,
) -> Arc<ProviderRegistry> {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(FixtureProvider::new(observed)));
    registry.register(Arc::new(AbsentProvider));
    if historical {
        registry.register(Arc::new(HistoricalProvider));
    }
    Arc::new(registry)
}
