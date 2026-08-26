//! The mutating commands: `kill process`, `stop`/`start`/`restart service`, and every other
//! command whose verb changes the world.
//!
//! Spec §11.5 and §16.5 fix what these answer with: **one [`ActionOutcome`] per target**, never a
//! count and never a collapsed status. `97 succeeded, 3 failed` has to stay three named failures,
//! so nothing here aggregates and nothing here stops at the first failure.
//!
//! The targets come from one of two places, and the difference matters:
//!
//! - from the **pipeline**, where each record carries its own identity, so
//!   `get process | where name == "foo" | stop process` signals exactly the processes that were
//!   enumerated;
//! - from the **selectors**, where the provider resolves them first. Resolving is what makes the
//!   identity complete — a process is `(pid, started)`, not a pid — which is what keeps a signal
//!   from reaching a recycled pid (spec §27.3, ADR-0015 T13).

use ono_core::ErrorCode;
use ono_pipeline::StreamEvent;
use ono_provider_api::{Action, ActionOutcome, ObjectId, Selector};
use ono_value::{ErrorValue, Value};

use crate::invoke::{CommandImpl, Invocation, Outcome, OutcomeFuture, must_be_awaited};

/// A mutation over whichever provider owns the contract's target.
#[derive(Debug)]
pub(crate) struct ProviderMutation {
    id: String,
}

impl ProviderMutation {
    pub(crate) fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }

    async fn run(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        let contract = ctx.contract();
        let spelling = contract.spelling();
        let target = contract.target().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`{spelling}` names no target to act on"),
            )
        })?;
        // The operation is the verb the user typed, which is the vocabulary every provider's
        // `act` already speaks: `kill`, `stop`, `start`, `restart`.
        let operation = contract.verb().to_owned();
        let arguments: Vec<(String, Value)> = contract
            .options()
            .iter()
            .filter_map(|option| {
                ctx.arguments()
                    .option(option.name())
                    .map(|value| (option.name().to_owned(), value.clone()))
            })
            .collect();

        let mut failures = Vec::new();
        let targets = self.targets(ctx, target, &spelling, &mut failures).await?;

        let providers = ctx.providers();
        let mut outcomes = Vec::with_capacity(targets.len() + failures.len());
        outcomes.append(&mut failures);
        for object in targets {
            let mut action = Action::new(target, &operation, object);
            for (name, value) in &arguments {
                action = action.with(name, value.clone());
            }
            match providers.act(&action).await {
                Ok(outcome) => outcomes.push(outcome),
                // The provider could not attempt it at all. That is still this target's outcome,
                // not the pipeline's: the other targets keep going (spec §16.5).
                Err(error) => outcomes.push(ActionOutcome::failed(&action, error)),
            }
        }
        Ok(Outcome::Actions(outcomes))
    }

    /// The objects to act on: the ones that arrived, or the ones the selectors name.
    async fn targets(
        &self,
        ctx: &mut Invocation<'_>,
        target: &str,
        spelling: &str,
        failures: &mut Vec<ActionOutcome>,
    ) -> Result<Vec<ObjectId>, ErrorValue> {
        if let Some(mut input) = ctx.take_input() {
            let mut objects = Vec::new();
            while let Some(event) = input.recv().await {
                match event {
                    StreamEvent::Value(Value::Record(record)) => match ObjectId::of(&record) {
                        Some(id) => objects.push(id),
                        None => {
                            return Err(ErrorValue::new(
                                ErrorCode::TypeMismatch,
                                format!(
                                    "`{spelling}` needs objects with an identity, and \
                                     `{}` declares none",
                                    record.schema_id()
                                ),
                            )
                            .with_help(
                                "a projection is a value, not an object; act on the objects \
                                 themselves",
                            ));
                        }
                    },
                    StreamEvent::Value(other) => {
                        return Err(ErrorValue::new(
                            ErrorCode::TypeMismatch,
                            format!(
                                "`{spelling}` acts on objects, and a {} is not one",
                                other.type_name()
                            ),
                        ));
                    }
                    // A failure upstream concerns one object; it is neither swallowed nor allowed
                    // to become an action that never happened.
                    StreamEvent::Failure(error) => {
                        failures.push(upstream_failure(target, ctx.contract().verb(), error))
                    }
                }
            }
            return Ok(objects);
        }

        let selector = ctx
            .contract()
            .selectors()
            .iter()
            .find_map(|spec| {
                ctx.arguments()
                    .selector(spec.name())
                    .map(|value| Selector::field(spec.name(), value.clone()))
            })
            .ok_or_else(|| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{spelling}` needs something to act on, and none was given"),
                )
                .with_help(format!(
                    "name it, as in `{spelling} <selector>`, or pipe the objects in"
                ))
            })?;

        Ok(ctx
            .providers()
            .resolve(target, &selector)
            .await?
            .iter()
            .map(|reference| reference.id().clone())
            .collect())
    }
}

/// An upstream failure, carried through as the outcome of a target that was never acted on.
fn upstream_failure(target: &str, operation: &str, error: ErrorValue) -> ActionOutcome {
    let object = ObjectId::new(
        ono_value::SchemaId::new(&format!("ono.{target}"), 1),
        [Value::Null],
    );
    let action = Action::new(target, operation, object);
    ActionOutcome::failed(&action, error)
}

impl CommandImpl for ProviderMutation {
    fn id(&self) -> &str {
        &self.id
    }

    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(must_be_awaited(&ctx.contract().spelling()))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move { self.run(ctx).await })
    }
}
