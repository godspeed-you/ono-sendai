//! `explain` produces the plan of spec §42 without executing anything (spec §15.3).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared helper in a test binary states its preconditions the same way a #[test] \
              body does (AGENTS.md section 16)"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ono_command::{CommandRegistry, ExecutionPlan, Resolution};
use ono_pipeline::ValueStream;
use ono_provider_api::{
    Action, ActionOutcome, Capability, ObjectRef, Provider, ProviderRegistry, Query, Risk, Selector,
};
use ono_value::{ErrorValue, Schema, SchemaId};

/// A provider that records every time anybody asks it to do real work.
#[derive(Debug, Default)]
struct CountingProvider {
    touched: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl Provider for CountingProvider {
    fn id(&self) -> &str {
        "linux.procfs"
    }

    fn targets(&self) -> &[&str] {
        &["process"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        Vec::new()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("process.list", Risk::Read)]
    }

    fn snapshot(&self, _query: &Query) -> Result<ValueStream, ErrorValue> {
        self.touched.fetch_add(1, Ordering::SeqCst);
        Ok(ValueStream::from_values([]))
    }

    async fn resolve(&self, _selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        self.touched.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        self.touched.fetch_add(1, Ordering::SeqCst);
        Ok(ActionOutcome::succeeded(action, true))
    }
}

fn registry() -> &'static CommandRegistry {
    CommandRegistry::embedded().expect("the embedded command contracts must parse")
}

fn plan_of(source: &str, providers: Option<&ProviderRegistry>) -> ExecutionPlan {
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
        .expect("the source is a pipeline");
    ono_command::plan(registry(), providers, pipeline, source)
}

#[test]
fn should_resolve_every_stage_of_a_read_pipeline() {
    let source = "get process | where cpu > 20 | to json";
    let plan = plan_of(source, None);

    assert_eq!(plan.stages().len(), 3);

    let first = &plan.stages()[0];
    assert_eq!(first.source(), "get process");
    assert_eq!(
        first.resolution(),
        &Resolution::Native {
            id: "ono.process.get".to_owned()
        }
    );
    assert_eq!(first.capability(), Some("process.list"));
    assert_eq!(first.input(), "null");
    assert_eq!(first.output(), "stream<ono.process/1>");
    assert!(first.is_streaming());

    let second = &plan.stages()[1];
    assert_eq!(second.source(), "where cpu > 20");
    assert_eq!(
        second.resolution(),
        &Resolution::Native {
            id: "ono.data.where".to_owned()
        }
    );
    assert_eq!(
        second.input(),
        "stream<ono.process/1>",
        "a stage's input is the previous stage's output, as spec §42.1 shows"
    );
    assert_eq!(
        second.output(),
        "stream<ono.process/1>",
        "`where` declares `stream<any>` and carries the upstream element type through"
    );
    assert_eq!(second.fields(), ["cpu"]);

    let third = &plan.stages()[2];
    assert_eq!(third.source(), "to json");
    assert_eq!(
        third.resolution(),
        &Resolution::Native {
            id: "ono.data.to".to_owned()
        }
    );
    assert_eq!(third.output(), "string | bytes");
}

#[test]
fn should_render_a_plan_in_the_shape_of_spec_42_1() {
    let source = "get process | where cpu > 20 | to json";
    let text = plan_of(source, None).render();

    assert!(text.starts_with("PIPELINE"), "rendered:\n{text}");
    for required in [
        "1. get process",
        "command      ono.process.get",
        "output       stream<ono.process/1>",
        "streaming    yes",
        "privilege    none",
        "2. where cpu > 20",
        "3. to json",
    ] {
        assert!(
            text.contains(required),
            "the plan must contain `{required}`, rendered:\n{text}"
        );
    }
}

#[test]
fn should_name_the_provider_that_would_answer_without_asking_it_anything() {
    let touched = Arc::new(AtomicUsize::new(0));
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(CountingProvider {
        touched: Arc::clone(&touched),
    }));

    let plan = plan_of("get process | where cpu > 20 | to json", Some(&providers));

    assert_eq!(plan.stages()[0].provider(), Some("linux.procfs"));
    assert_eq!(
        touched.load(Ordering::SeqCst),
        0,
        "spec §15.3: `explain` plans without executing anything"
    );
}

#[test]
fn should_report_the_risk_and_privilege_of_a_mutating_stage() {
    let plan = plan_of("get process | stop process", None);
    let stage = &plan.stages()[1];

    assert_eq!(
        stage.resolution(),
        &Resolution::Native {
            id: "ono.process.stop".to_owned()
        }
    );
    assert_eq!(stage.risk(), Some(Risk::Mutate));
    assert!(
        stage
            .privilege()
            .is_some_and(|p| p.as_str() == "conditional"),
        "spec §42.2 shows privilege on the mutating stage"
    );

    let text = plan.render();
    assert!(text.contains("risk"), "rendered:\n{text}");
}

#[test]
fn should_say_when_a_head_is_not_a_native_command() {
    let plan = plan_of("ls -la", None);
    let stage = &plan.stages()[0];

    assert_eq!(
        stage.resolution(),
        &Resolution::External {
            head: "ls".to_owned()
        }
    );
    assert!(
        plan.render().contains("ls"),
        "the plan still reports a stage it cannot resolve natively"
    );
}

#[test]
fn should_offer_the_plan_as_structured_data() {
    let plan = plan_of("get process | to json", None);
    let value = plan.to_value();
    let map = value.as_map().expect("a plan is a map of fields");
    let stages = map
        .get("stages")
        .and_then(|stages| stages.as_list().ok())
        .expect("a plan carries its stages as a list");

    assert_eq!(stages.len(), 2);
    let first = stages[0].as_map().expect("a stage is a map of fields");
    assert_eq!(
        first.get("command").and_then(|id| id.as_str().ok()),
        Some("ono.process.get")
    );
}

#[test]
fn should_verify_a_schema_id_is_carried_through_the_plan() {
    let plan = plan_of("get process | take 10", None);
    let last = plan.stages().last().expect("two stages");

    let schema: SchemaId = last
        .element_schema()
        .expect("the element type survives a transform that declares `stream<any>`")
        .parse()
        .expect("the carried type is a schema id");
    assert_eq!(schema.name(), "ono.process");
}
