//! Binding a native implementation to a registry id, and the check spec §27.2 asks CI to run.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared helper in a test binary states its preconditions the same way a #[test] \
              body does (AGENTS.md section 16)"
)]

use std::sync::Arc;

use ono_command::{CommandImpl, CommandRegistry, CommandTable, Invocation, Outcome, Stability};
use ono_pipeline::{CancelToken, ValueStream};
use ono_provider_api::{Action, ActionOutcome, ObjectId, ProviderRegistry};
use ono_value::{ErrorValue, SchemaId, Value};

fn registry() -> &'static CommandRegistry {
    CommandRegistry::embedded().expect("the embedded command contracts must parse")
}

/// An implementation that answers with the selector it was given, so a test can observe that the
/// bound arguments reached it.
#[derive(Debug)]
struct EchoPid;

impl CommandImpl for EchoPid {
    fn id(&self) -> &str {
        "ono.process.get"
    }

    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        let pid = ctx.arguments().require_selector("pid")?.clone();
        Ok(Outcome::Values(ValueStream::from_values([pid])))
    }
}

fn bound(
    source: &str,
) -> (
    &'static ono_command::CommandContract,
    ono_command::BoundArguments,
) {
    let parsed = ono_parser::parse(source);
    let stage = parsed
        .program()
        .statements
        .first()
        .and_then(ono_parser::Statement::as_pipeline)
        .and_then(|pipeline| pipeline.head.stages.first())
        .expect("the source is a pipeline with one stage");
    let head = stage.head.name().expect("a command head");
    let resolved = registry()
        .resolve(head, &stage.arguments)
        .expect("resolves");
    let arguments = resolved.contract.bind(resolved.arguments).expect("binds");
    (resolved.contract, arguments)
}

#[tokio::test]
async fn should_hand_the_bound_arguments_and_the_input_to_the_implementation() {
    let (contract, arguments) = bound("get process 4419");
    let providers = ProviderRegistry::new();
    let mut table = CommandTable::new();
    table.register(Arc::new(EchoPid));

    let implementation = table
        .get(contract.id())
        .expect("`ono.process.get` was registered");
    let mut invocation = Invocation::new(contract, &arguments, &providers);
    assert!(!invocation.has_input(), "a producer stage has no input");

    let outcome = implementation
        .invoke(&mut invocation)
        .expect("the implementation runs");
    let Outcome::Values(stream) = outcome else {
        panic!("`get process` yields values, not action outcomes");
    };

    assert_eq!(stream.collect().await.values(), [Value::Int(4419)]);
}

#[tokio::test]
async fn should_expose_the_input_stream_exactly_once() {
    let (contract, arguments) = bound("stop process");
    let providers = ProviderRegistry::new();
    let mut invocation = Invocation::new(contract, &arguments, &providers)
        .with_input(ValueStream::from_values([Value::Int(1), Value::Int(2)]));

    assert!(invocation.has_input());
    let taken = invocation
        .take_input()
        .expect("the input is available once");
    assert!(
        !invocation.has_input(),
        "the stream is moved to the implementation, not cloned"
    );
    assert_eq!(
        taken.collect().await.values(),
        [Value::Int(1), Value::Int(2)]
    );
}

#[test]
fn should_report_every_stable_command_that_has_no_implementation() {
    let mut table = CommandTable::new();
    table.register(Arc::new(EchoPid));

    let unbound = ono_command::unbound_stable_commands(registry(), &table);

    assert!(
        !unbound.contains(&"ono.process.get"),
        "a registered id is bound"
    );
    assert!(
        unbound.contains(&"ono.process.kill"),
        "spec §27.2: CI must see the stable commands nothing implements"
    );
    assert!(
        unbound.iter().all(|id| registry()
            .get(id)
            .is_some_and(|command| command.stability() == Stability::Stable)),
        "only stable commands are a compatibility promise, so only they are reported"
    );
    assert_eq!(
        unbound.len(),
        registry().with_stability(Stability::Stable).len() - 1
    );
}

/// A mutating implementation, which answers with per-target outcomes rather than a value stream
/// (spec §11.5, §16.5).
#[derive(Debug)]
struct KillProcess;

impl CommandImpl for KillProcess {
    fn id(&self) -> &str {
        "ono.process.kill"
    }

    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        let pid = ctx.arguments().require_selector("pid")?.clone();
        let signal = ctx
            .arguments()
            .option("signal")
            .cloned()
            .unwrap_or_default();
        let object = ObjectId::new(SchemaId::new("ono.process", 1), [pid]);
        let action = Action::new("process", "signal", object).with("signal", signal);
        Ok(Outcome::Actions(vec![ActionOutcome::succeeded(
            &action, true,
        )]))
    }
}

#[test]
fn should_yield_per_target_action_outcomes_for_a_mutating_command() {
    let (contract, arguments) = bound("kill process 4419 --signal=SIGTERM");
    assert_eq!(contract.id(), "ono.process.kill");
    assert_eq!(
        arguments.option("signal"),
        Some(&Value::String("SIGTERM".into()))
    );

    let providers = ProviderRegistry::new();
    let mut invocation = Invocation::new(contract, &arguments, &providers);
    let outcome = KillProcess
        .invoke(&mut invocation)
        .expect("the implementation runs");

    let Outcome::Actions(outcomes) = outcome else {
        panic!("`kill process` yields action outcomes, not a value stream");
    };
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].is_success());
    assert_eq!(outcomes[0].operation(), "signal");
}

#[test]
fn should_carry_a_cancellation_token_into_the_implementation() {
    let (contract, arguments) = bound("get process");
    let providers = ProviderRegistry::new();
    let token = CancelToken::new();
    let invocation = Invocation::new(contract, &arguments, &providers).with_cancel(token.clone());

    assert!(!invocation.cancel_token().is_cancelled());
    token.cancel();
    assert!(
        invocation.cancel_token().is_cancelled(),
        "spec §18.5: one token stops every stage of the pipeline"
    );
}
