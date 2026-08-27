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

use crate::contract::Confirmation;
use crate::invoke::{CommandImpl, Invocation, Outcome, OutcomeFuture, must_be_awaited};

/// One object to act on: its identity, how a person knows it, and whether anything resolved it.
#[derive(Debug)]
struct Target {
    id: ObjectId,
    label: Option<String>,
    /// The selector the user wrote, when nothing answered to it. The provider is then the one
    /// to say whether the object exists — the kernel refuses a caller without privilege before
    /// it looks, and that refusal outranks "not found" (ADR-0088 §2).
    unresolved: Option<(String, Value)>,
}

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
        // The table bound this command because a provider advertised the capability; the
        // provider that will act now may be another one — a link frame mounts a remote's
        // (ADR-0036) — so the same question is asked of it, before anything is resolved. A
        // refusal here is the E0101 an unbound command answers, never a half-run.
        if let Some(capability) = contract.provider_capability()
            && let Ok(provider) = ctx.providers().provider_for(target)
            && !provider
                .capabilities()
                .iter()
                .any(|advertised| advertised.id() == capability)
        {
            return Err(ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!(
                    "`{}` is declared but the provider for `{target}` here ({}) does not \
                     implement `{capability}`",
                    contract.id(),
                    provider.id()
                ),
            )
            .with_help("`help` lists what this shell can do; the rest is scheduled, not hidden"));
        }
        // The operation is the verb the user typed, which is the vocabulary every provider's
        // `act` already speaks: `kill`, `stop`, `start`, `restart`, `set`.
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

        // A command whose single action is destructive needs `--confirm` every time (spec
        // §17.4): a script never waits for a prompt, so without the flag it acts on nothing
        // and says so (ADR-0088 §3).
        let confirmed = ctx.arguments().option("confirm") == Some(&Value::Bool(true));
        if contract.confirmation() == Confirmation::Always && !confirmed {
            return Err(ErrorValue::new(
                ErrorCode::SafetyConfirmationRequired,
                format!("`{spelling}` is destructive and was not confirmed"),
            )
            .with_help(format!(
                "nothing was changed. Write `{spelling} --confirm` to act (spec §17.4)"
            )));
        }

        // The bulk guard of spec §11.6 and §17.4: a selection above the threshold mutates
        // nothing unless the confirmation was written. The refusal names the scope — the count
        // is exactly what the user needed shown before acting — and it comes before the first
        // action, so a refused bulk never half-ran. The threshold is deliberately a constant
        // until configuration reaches invocations; a documented guard that exists strictly is
        // better than a configurable one that does not.
        const BULK_THRESHOLD: usize = 10;
        if contract.option("confirm").is_some() && targets.len() > BULK_THRESHOLD && !confirmed {
            return Err(ErrorValue::new(
                ErrorCode::SafetyConfirmationRequired,
                format!(
                    "`{spelling}` would act on {} objects, which is more than the bulk                      threshold of {BULK_THRESHOLD}",
                    targets.len(),
                ),
            )
            .with_help(format!(
                "nothing was changed. Write `--confirm` to act on all {} (spec §17.4), or                  narrow the selection",
                targets.len(),
            )));
        }

        let providers = ctx.providers();
        let mut outcomes = Vec::with_capacity(targets.len() + failures.len());
        outcomes.append(&mut failures);
        let dry_run = ctx.arguments().option("dry-run") == Some(&Value::Bool(true));
        for object in targets {
            let mut action = Action::new(target, &operation, object.id);
            if let Some(label) = object.label {
                action = action.labelled(label);
            }
            // A declared `--dry-run` is the ask-without-obeying of spec §11.6, not an ordinary
            // argument a provider might ignore: it travels in the action's own field, and a
            // provider that honours it answers `skipped` with what would have happened.
            if dry_run {
                action = action.as_dry_run();
            }
            for (name, value) in &arguments {
                if name != "dry-run" {
                    action = action.with(name, value.clone());
                }
            }
            if let Some((name, value)) = &object.unresolved {
                action = action.with(name, value.clone());
            }
            match providers.act(&action).await {
                Ok(outcome) => outcomes.push(outcome),
                // The provider could not attempt it at all. That is still this target's outcome,
                // not the pipeline's: the other targets keep going (spec §16.5). For an object
                // nothing resolved, "could not attempt" means it is not there to be acted on
                // (ADR-0068 §2).
                Err(error) => outcomes.push(match &object.unresolved {
                    Some((name, value)) => ActionOutcome::failed(
                        &action,
                        ErrorValue::new(
                            ErrorCode::IoNotFound,
                            format!("no {target} answers to {name} {value}"),
                        )
                        .with_help(format!("`get {target}` lists what is there"))
                        .with_source(error),
                    ),
                    None => ActionOutcome::failed(&action, error),
                }),
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
    ) -> Result<Vec<Target>, ErrorValue> {
        if let Some(mut input) = ctx.take_input() {
            let mut objects = Vec::new();
            while let Some(event) = input.recv().await {
                match event {
                    StreamEvent::Value(Value::Record(record)) => match ObjectId::of(&record) {
                        Some(id) => objects.push(Target {
                            id,
                            label: Some(ono_graph::label_of(&record)),
                            unresolved: None,
                        }),
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

        let (name, value) = ctx
            .contract()
            .selectors()
            .iter()
            .find_map(|spec| {
                ctx.arguments()
                    .selector(spec.name())
                    .map(|value| (spec.name(), value.clone()))
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

        let mut objects: Vec<Target> = ctx
            .providers()
            .resolve(target, &Selector::field(name, value.clone()))
            .await?
            .iter()
            .map(|reference| Target {
                id: reference.id().clone(),
                label: Some(reference.label().to_owned()),
                unresolved: None,
            })
            .collect();
        // A selector that names nothing is not an empty selection: the user asked to act on
        // one particular thing, and that thing is still the target (spec §16.5, ADR-0068 §2).
        // Whether it exists is the provider's to say — a creation names what does not exist
        // yet, and a kernel refuses an unprivileged caller before it looks — so the provider is
        // asked, on the object as the user named it (ADR-0088 §2). An empty stream would be the
        // answer to a filter that matched nothing.
        if objects.is_empty() {
            objects.push(Target {
                id: ObjectId::new(
                    ono_value::SchemaId::new(&format!("ono.{target}"), 1),
                    [value.clone()],
                ),
                label: None,
                unresolved: Some((name.to_owned(), value.clone())),
            });
        }
        Ok(objects)
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
