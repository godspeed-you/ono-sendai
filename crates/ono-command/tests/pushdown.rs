//! ADR-0980: which comparisons of a `where` a producer hands the provider behind it.
//!
//! The provider here is the widget fixture with a declaration added, so the rule is exercised
//! with no real source behind it: what the provider is asked is the outcome, read off the
//! [`Query`] it receives. The journal's own translation is tested where `journalctl` is, in
//! `ono-cli/tests/predicate_pushdown.rs`.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use std::sync::{Arc, Mutex};

use fixture::{FixtureProvider, registry, table, widget_schema};
use ono_command::{Invocation, Outcome, pushable_comparisons};
use ono_pipeline::ValueStream;
use ono_provider_api::{
    Capability, Comparison, ComparisonOperator, LiteralType, ObjectRef, Provider, ProviderRegistry,
    PushdownCapability, Query, Selector,
};
use ono_value::{ErrorValue, Schema, Value};

/// The comparisons each query carried, in the order the queries arrived.
type Asked = Arc<Mutex<Vec<Vec<Comparison>>>>;

/// The widget fixture, declaring what a test gives it and remembering what it is asked.
///
/// It evaluates none of the comparisons itself: the declaration says what it *may* be handed,
/// and the `where` after it decides every widget either way.
#[derive(Debug)]
struct Declaring {
    widgets: FixtureProvider,
    declared: Vec<PushdownCapability>,
    asked: Asked,
}

impl Declaring {
    fn new(declared: Vec<PushdownCapability>) -> (Self, Asked) {
        let asked = Asked::default();
        let provider = Self {
            widgets: FixtureProvider::new(),
            declared,
            asked: Arc::clone(&asked),
        };
        (provider, asked)
    }
}

#[async_trait::async_trait]
impl Provider for Declaring {
    fn id(&self) -> &str {
        "test.declaring"
    }

    fn targets(&self) -> &[&str] {
        &["process"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![widget_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.widgets.capabilities()
    }

    fn pushdown(&self) -> Vec<PushdownCapability> {
        self.declared.clone()
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        self.asked
            .lock()
            .expect("the record of queries")
            .push(query.pushed().to_vec());
        self.widgets.snapshot(query)
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        self.widgets.resolve(selector).await
    }
}

/// `pid` compared with an integer, for the operators given.
fn pid_declared(operators: &[ComparisonOperator]) -> PushdownCapability {
    PushdownCapability::new(
        "process",
        "pid",
        LiteralType::Int,
        operators.iter().copied(),
    )
}

/// `name` compared with a string for equality.
fn name_declared() -> PushdownCapability {
    PushdownCapability::new(
        "process",
        "name",
        LiteralType::String,
        [ComparisonOperator::Equal],
    )
}

fn pid(operator: ComparisonOperator, bound: i128) -> Comparison {
    Comparison::new("pid", operator, Value::Int(bound))
}

/// What a pipeline produced and what its provider was asked on the way.
struct Ran {
    pids: Vec<i128>,
    asked: Vec<Vec<Comparison>>,
}

/// Runs `source` over a provider declaring `declared`, offering each stage the comparisons of
/// the stage after it — the evaluator's part of ADR-0980, done here by the same public function.
async fn run(source: &str, declared: Vec<PushdownCapability>) -> Ran {
    let (provider, asked) = Declaring::new(declared);
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(provider));

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
    let bound: Vec<_> = pipeline
        .head
        .stages
        .iter()
        .map(|stage| {
            let head = stage.head.name().expect("a command head");
            let resolved = registry()
                .resolve(head, &stage.arguments)
                .expect("the stage resolves");
            let arguments = resolved
                .contract
                .bind(resolved.arguments)
                .expect("the stage binds");
            (resolved.contract, arguments)
        })
        .collect();

    let table = table();
    let mut stream: Option<ValueStream> = None;
    for (position, (contract, arguments)) in bound.iter().enumerate() {
        let mut invocation = Invocation::new(contract, arguments, &providers);
        if let Some((next, following)) = bound.get(position + 1) {
            invocation = invocation.with_pushable(pushable_comparisons(next, following));
        }
        if let Some(input) = stream.take() {
            invocation = invocation.with_input(input);
        }
        match table
            .run(contract.id(), &mut invocation)
            .await
            .expect("the stage runs")
        {
            Outcome::Values(values) => stream = Some(values),
            Outcome::Actions(_) => panic!("`{source}` mutates nothing"),
        }
    }
    let collected = stream
        .expect("the pipeline produces values")
        .collect()
        .await;
    let pids = collected
        .values()
        .iter()
        .map(|value| {
            value
                .as_record()
                .expect("a widget")
                .get("pid")
                .and_then(|pid| pid.as_int().ok())
                .expect("a pid")
        })
        .collect();
    let asked = asked.lock().expect("the record of queries").clone();
    Ran { pids, asked }
}

/// The comparisons the stage `source` offers a producer before it.
fn offered(source: &str) -> Vec<Comparison> {
    let parsed = ono_parser::parse(source);
    let stage = &parsed
        .program()
        .statements
        .first()
        .and_then(ono_parser::Statement::as_pipeline)
        .expect("the source is a pipeline")
        .head
        .stages[0];
    let resolved = registry()
        .resolve(stage.head.name().expect("a command head"), &stage.arguments)
        .expect("the stage resolves");
    let arguments = resolved
        .contract
        .bind(resolved.arguments)
        .expect("the stage binds");
    pushable_comparisons(resolved.contract, &arguments)
}

// --- what a predicate offers --------------------------------------------------------------------

#[test]
fn should_offer_each_comparison_with_the_field_on_the_left_when_the_predicate_is_a_conjunction() {
    assert_eq!(
        offered(r#"where 3 >= pid and name == "beta" and (owner != "root") and true == alive"#),
        vec![
            pid(ComparisonOperator::LessOrEqual, 3),
            Comparison::new("name", ComparisonOperator::Equal, Value::string("beta")),
            Comparison::new("owner", ComparisonOperator::NotEqual, Value::string("root")),
            Comparison::new("alive", ComparisonOperator::Equal, Value::Bool(true)),
        ],
        "ADR-0980: a literal on the left mirrors the operator, parentheses only group"
    );
}

#[test]
fn should_offer_only_the_leading_comparisons_when_a_later_conjunct_is_something_else() {
    for (predicate, leading) in [
        ("where pid <= 2 and size > 1KiB and pid == 1", 1),
        (
            "where pid <= 2 and pid < 9 and pid + 1 == 2 and pid == 1",
            2,
        ),
        ("where pid <= 2 and (pid == 1 or pid == 2)", 1),
        ("where pid <= 2 and not (pid == 1)", 1),
        ("where pid <= 2 and pid <= $limit", 1),
        ("where pid <= 2 and pid <= size", 1),
        ("where pid <= 2 and pid <= 2.5", 1),
        (r#"where pid <= 2 and name == "beta-$suffix""#, 1),
        ("where pid <= 2 and pid in [1, 2]", 1),
    ] {
        assert_eq!(
            offered(predicate).len(),
            leading,
            "ADR-0980: `{predicate}` offers what it starts with and stops at the first conjunct \
             that is no field-against-literal comparison"
        );
    }
}

#[test]
fn should_offer_nothing_when_the_predicate_does_not_start_with_a_comparison() {
    for predicate in [
        "where pid == 1 or pid == 2",
        "where not (pid <= 2)",
        "where pid + 1 <= 2",
        "where pid <= -1",
        "where size > 1KiB and pid <= 2",
        "where pid <= $limit and pid <= 2",
        "where (pid == 1 or name == \"beta\") and pid <= 2",
        "where pid <= pid",
        "where 1 <= 2",
    ] {
        assert_eq!(
            offered(predicate),
            Vec::new(),
            "ADR-0980: `{predicate}` starts with something a source cannot be asked"
        );
    }
}

#[test]
fn should_offer_nothing_when_the_stage_is_not_a_filter() {
    for stage in ["take 2", "sort pid", "select pid name", "get process"] {
        assert_eq!(
            offered(stage),
            Vec::new(),
            "ADR-0980: `{stage}` decides nothing record by record that a source could decide"
        );
    }
}

// --- what a provider is handed ------------------------------------------------------------------

#[tokio::test]
async fn should_hand_the_provider_a_comparison_when_the_provider_declared_it() {
    let ran = run(
        "get process | where pid <= 2",
        vec![pid_declared(&[ComparisonOperator::LessOrEqual])],
    )
    .await;

    assert_eq!(
        ran.asked,
        vec![vec![pid(ComparisonOperator::LessOrEqual, 2)]],
        "ADR-0980: the declared comparison travels in the query"
    );
    assert_eq!(
        ran.pids,
        vec![1, 2],
        "the `where` decides every widget, whatever the provider did with the comparison"
    );
}

#[tokio::test]
async fn should_hand_the_provider_nothing_when_it_declares_nothing() {
    let ran = run("get process | where pid <= 2", Vec::new()).await;

    assert_eq!(
        ran.asked,
        vec![Vec::new()],
        "ADR-0980: a provider without a declaration is asked exactly what it was asked before"
    );
    assert_eq!(ran.pids, vec![1, 2]);
}

#[tokio::test]
async fn should_hand_the_provider_nothing_when_the_comparison_is_outside_its_declaration() {
    let declared = || {
        vec![
            pid_declared(&[ComparisonOperator::LessOrEqual, ComparisonOperator::Equal]),
            name_declared(),
        ]
    };
    for (pipeline, why) in [
        (
            "get process | where pid >= 2",
            "an operator it did not declare",
        ),
        (
            "get process | where pid < 3",
            "an operator it did not declare",
        ),
        (
            r#"get process | where owner == "root""#,
            "a field it did not declare",
        ),
        (
            r#"get process | where pid == "2""#,
            "a literal of another type",
        ),
        (
            r#"get process | where name != "beta""#,
            "an operator it did not declare for the field",
        ),
        ("get process | where name == 2", "a literal of another type"),
    ] {
        let ran = run(pipeline, declared()).await.asked;
        assert!(
            ran.iter().all(Vec::is_empty),
            "ADR-0980: `{pipeline}` is {why}, so nothing is handed over; got {ran:?}"
        );
    }
}

#[tokio::test]
async fn should_hand_the_provider_nothing_when_the_declaration_is_about_another_target() {
    let elsewhere = PushdownCapability::new(
        "service",
        "pid",
        LiteralType::Int,
        [ComparisonOperator::LessOrEqual],
    );

    let ran = run("get process | where pid <= 2", vec![elsewhere]).await;

    assert_eq!(
        ran.asked,
        vec![Vec::new()],
        "ADR-0980: a declaration covers the target it names and no other"
    );
}

#[tokio::test]
async fn should_hand_over_the_leading_declared_comparisons_when_the_conjunction_is_mixed() {
    let ran = run(
        r#"get process | where pid <= 2 and name == "beta" and owner == "ono" and pid == 2"#,
        vec![
            pid_declared(&[ComparisonOperator::LessOrEqual, ComparisonOperator::Equal]),
            name_declared(),
        ],
    )
    .await;

    assert_eq!(
        ran.asked,
        vec![vec![
            pid(ComparisonOperator::LessOrEqual, 2),
            Comparison::new("name", ComparisonOperator::Equal, Value::string("beta")),
        ]],
        "ADR-0980: the handover ends at the first comparison the provider did not declare, and \
         a declared one after it stays with the pipeline"
    );
    assert_eq!(ran.pids, vec![2], "the whole predicate is decided locally");
}

#[tokio::test]
async fn should_hand_the_provider_nothing_when_an_undeclared_comparison_comes_first() {
    let ran = run(
        r#"get process | where owner == "root" and pid <= 2"#,
        vec![pid_declared(&[ComparisonOperator::LessOrEqual])],
    )
    .await;

    assert_eq!(
        ran.asked,
        vec![Vec::new()],
        "ADR-0980: `owner == \"root\"` is decided first and on every widget, so nothing after \
         it may withhold one"
    );
    assert_eq!(ran.pids, vec![1]);
}

#[tokio::test]
async fn should_hand_the_provider_nothing_when_another_stage_stands_between() {
    for pipeline in [
        "get process | take 2 | where pid <= 2",
        "get process | sort pid | where pid <= 2",
        "get process | where size > 1KiB | where pid <= 2",
    ] {
        let ran = run(
            pipeline,
            vec![pid_declared(&[ComparisonOperator::LessOrEqual])],
        )
        .await;

        assert_eq!(
            ran.asked,
            vec![Vec::new()],
            "ADR-0980: in `{pipeline}` the stage after the producer sees every widget, so the \
             provider is asked for every widget"
        );
    }
}
