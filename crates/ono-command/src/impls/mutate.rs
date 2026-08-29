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
use ono_pipeline::{StreamEvent, ValueStream};
use ono_provider_api::{Action, ActionOutcome, ObjectId, Selector};
use ono_value::{ErrorValue, SchemaId, Value};

use crate::contract::{Confirmation, DeclaredType};
use crate::invoke::{CommandImpl, Invocation, Outcome, OutcomeFuture, must_be_awaited};

/// The bulk threshold of spec §11.6 when `safety.confirm.bulk_threshold` is not configured.
const DEFAULT_BULK_THRESHOLD: usize = 10;

/// One object to act on: its identity, where it was observed (ADR-0082 §4), how a person knows
/// it, and whether anything resolved it.
#[derive(Debug)]
struct Target {
    id: ObjectId,
    source: Option<String>,
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
        let mut arguments: Vec<(String, Value)> = contract
            .options()
            .iter()
            .filter_map(|option| {
                ctx.arguments()
                    .option(option.name())
                    .map(|value| (option.name().to_owned(), value.clone()))
            })
            .collect();

        // `set <target> <name>` with no property is a request to change nothing, and the
        // provider would only be able to say so per target. It is a usage error before anything
        // is resolved, naming the properties the contract declares (ADR-0084).
        if contract.verb() == "set" && arguments.iter().all(|(name, _)| name == "dry-run") {
            let properties: Vec<String> = contract
                .options()
                .iter()
                .filter(|option| !matches!(option.name(), "dry-run" | "confirm" | "provider"))
                .map(|option| format!("--{}", option.name()))
                .collect();
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{spelling}` needs a property to set, and none was given"),
            )
            .with_help(format!(
                "name what should change: {}",
                properties.join(", ")
            )));
        }

        // A command whose input is content rather than objects — `write file` takes
        // `bytes | string` — consumes the pipeline as the `content` argument, and its targets
        // are the selector's (ADR-0082 §3). Content is what the contract says it accepts: a
        // mutation that declares no content input takes the objects arriving on the pipeline as
        // its targets, which is the object-in spelling of spec §11.5 (ADR-0216).
        let takes_content = contract.input().admits_bytes() || contract.input().admits_text();
        if ctx.has_input()
            && takes_content
            && !contract.input().is_stream()
            && let Some(input) = ctx.take_input()
        {
            arguments.push((
                "content".to_owned(),
                collect_content(input, &spelling).await?,
            ));
        }

        let mut failures = Vec::new();
        let targets = self.targets(ctx, target, &spelling, &mut failures).await?;
        let mut arguments = arguments;
        // A creating verb's selectors describe the object, and the provider needs them by name
        // — `mount filesystem <source> <target>` is a source and a target, not two anonymous
        // identity values (ADR-0098 §1).
        if creates(contract.verb()) {
            arguments.extend(named_selectors(ctx));
        }
        // The selectors that did not supply the targets — a `destination` — travel with the
        // action under their own names (ADR-0082 §2).
        let supplied = self.target_selector(ctx);
        for spec in contract.selectors() {
            if supplied.as_deref() != Some(spec.name())
                && let Some(value) = ctx.arguments().selector(spec.name())
                && !arguments.iter().any(|(name, _)| name == spec.name())
            {
                arguments.push((spec.name().to_owned(), value.clone()));
            }
        }

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
        // action, so a refused bulk never half-ran. The threshold is the session's
        // `safety.confirm.bulk_threshold` (spec §30), which reaches the invocation as a
        // `config.*` binding of its scope (ADR-0010, ADR-0082 §5).
        let threshold = bulk_threshold(ctx);
        if contract.option("confirm").is_some() && targets.len() > threshold && !confirmed {
            return Err(ErrorValue::new(
                ErrorCode::SafetyConfirmationRequired,
                format!(
                    "`{spelling}` would act on {} objects, which is more than the \
                     bulk threshold of {threshold}",
                    targets.len(),
                ),
            )
            .with_help(format!(
                "nothing was changed. Write `--confirm` to act on all {} (spec §17.4), \
                 or narrow the selection",
                targets.len(),
            )));
        }

        let providers = ctx.providers();
        let mut outcomes = Vec::with_capacity(targets.len() + failures.len());
        outcomes.append(&mut failures);
        let dry_run = ctx.arguments().option("dry-run") == Some(&Value::Bool(true));
        for object in targets {
            let mut action = Action::new(target, &operation, object.id);
            if let Some(source) = object.source {
                action = action.with_source(source);
            }
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

    /// The selector that supplies the targets when nothing arrives on the pipeline: the first
    /// one the user wrote, in the contract's order.
    fn target_selector(&self, ctx: &Invocation<'_>) -> Option<String> {
        ctx.contract()
            .selectors()
            .iter()
            .find(|spec| ctx.arguments().selector(spec.name()).is_some())
            .map(|spec| spec.name().to_owned())
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
                            source: record.provenance().source().map(str::to_owned),
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

        // A verb that creates what it names has nothing to resolve: the object does not exist
        // yet, and asking the provider for it would answer "not found" to every request. Its
        // identity is the selectors as written, in contract order (ADR-0098 §1).
        if creates(ctx.contract().verb()) {
            let named = named_selectors(ctx);
            if named.is_empty() {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{spelling}` needs something to create, and none was given"),
                )
                .with_help(format!("name it, as in `{spelling} <selector>`")));
            }
            let label = named
                .iter()
                .map(|(_, value)| value.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            return Ok(vec![Target {
                id: ObjectId::new(
                    schema_of(ctx, target),
                    named.into_iter().map(|(_, value)| value),
                ),
                source: None,
                label: Some(label),
                unresolved: None,
            }]);
        }

        let (spec, value) = ctx
            .contract()
            .selectors()
            .iter()
            .find_map(|spec| {
                ctx.arguments()
                    .selector(spec.name())
                    .map(|value| (spec, value.clone()))
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

        let name = spec.name();
        // A `path` selector is acted on, not resolved: every filesystem call takes a path, and
        // a file `write` is about to create has nothing to resolve. "It is not there" is then
        // the outcome of the act (ADR-0082 §1). That holds for a command acting on the
        // target's own objects; one whose input names another schema — `unmount filesystem`
        // acts on `ono.mount/1` — has the provider resolve the path to that object
        // (ADR-0116 §2).
        if matches!(spec.declared_type(), DeclaredType::Path) && acts_on_own_objects(ctx, target) {
            let schema = schema_of(ctx, target);
            return Ok(paths_of(&value)
                .into_iter()
                .map(|path| Target {
                    source: Some(path.to_string_lossy().into_owned()),
                    label: Some(path.to_string_lossy().into_owned()),
                    unresolved: None,
                    id: ObjectId::new(schema.clone(), [Value::Path(path.into())]),
                })
                .collect());
        }

        let mut objects: Vec<Target> = ctx
            .providers()
            .resolve(target, &Selector::field(name, value.clone()))
            .await?
            .iter()
            .map(|reference| Target {
                id: reference.id().clone(),
                source: reference.provenance().source().map(str::to_owned),
                label: Some(reference.label().to_owned()),
                unresolved: None,
            })
            .collect();
        // A name can answer for more than one kind of object — `root` is a user and a group,
        // and one provider resolves both. When that happens, the kind the command acts on is
        // the one its input type declares; a provider that answered with one kind is never
        // second-guessed (ADR-0102).
        let kinds = objects
            .iter()
            .map(|object| object.id.schema().to_string())
            .collect::<std::collections::BTreeSet<_>>();
        if kinds.len() > 1
            && let Some(accepted) = ctx.contract().input().element_schema()
        {
            objects.retain(|object| object.id.schema().to_string() == accepted);
        }
        // A selector that names nothing is not an empty selection: the user asked to act on
        // one particular thing, and that thing is still the target (spec §16.5, ADR-0068 §2).
        // Whether it exists is the provider's to say — a creation names what does not exist
        // yet, and a kernel refuses an unprivileged caller before it looks — so the provider is
        // asked, on the object as the user named it (ADR-0088 §2). An empty stream would be the
        // answer to a filter that matched nothing.
        if objects.is_empty() {
            objects.push(Target {
                id: ObjectId::new(schema_of(ctx, target), [value.clone()]),
                source: None,
                label: None,
                unresolved: Some((name.to_owned(), value.clone())),
            });
        }
        Ok(objects)
    }
}

/// The schema named after `target` among the acting provider's, at the version it advertises,
/// or the conventional `ono.<target>/1`.
fn schema_of(ctx: &Invocation<'_>, target: &str) -> SchemaId {
    let name = format!("ono.{target}");
    ctx.providers()
        .for_target(target)
        .iter()
        .flat_map(|provider| provider.schemas())
        .map(|schema| schema.id().clone())
        .find(|id| id.name() == name)
        .unwrap_or_else(|| SchemaId::new(&name, 1))
}

/// Whether the command acts on objects of its target's own schema: its input names no stream of
/// another schema. `remove file` takes `stream<ono.file/1>`; `unmount filesystem` takes
/// `stream<ono.mount/1>` and acts on mounts (ADR-0116 §2).
fn acts_on_own_objects(ctx: &Invocation<'_>, target: &str) -> bool {
    let own = format!("ono.{target}/");
    ctx.contract()
        .input()
        .schema_references()
        .iter()
        .all(|schema| schema.starts_with(&own))
}

/// The paths a `path` selector's value names: one, or each of the list a glob resolved to.
fn paths_of(value: &Value) -> Vec<std::path::PathBuf> {
    match value {
        Value::Path(path) => vec![path.to_path_buf()],
        Value::String(text) => vec![std::path::PathBuf::from(text.as_ref())],
        Value::List(items) => items.iter().flat_map(paths_of).collect(),
        _ => Vec::new(),
    }
}

/// The configured bulk threshold, from the session's `config.*` bindings in the scope.
fn bulk_threshold(ctx: &Invocation<'_>) -> usize {
    ctx.scope()
        .variable("config.safety.confirm.bulk_threshold")
        .and_then(|value| match value {
            Value::Int(count) => usize::try_from(*count).ok(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        })
        .unwrap_or(DEFAULT_BULK_THRESHOLD)
}

/// The pipeline as one `bytes` value: strings and bytes concatenated in order, byte for byte
/// (spec §12.1). Anything else has no byte form the shell may invent (spec §12.3).
async fn collect_content(mut input: ValueStream, spelling: &str) -> Result<Value, ErrorValue> {
    let mut content = Vec::new();
    while let Some(event) = input.recv().await {
        match event {
            StreamEvent::Value(Value::String(text)) => content.extend_from_slice(text.as_bytes()),
            StreamEvent::Value(Value::Bytes(raw)) => content.extend_from_slice(&raw),
            StreamEvent::Value(other) => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!(
                        "`{spelling}` writes bytes or text, and a {} is neither",
                        other.type_name()
                    ),
                )
                .with_help(format!(
                    "choose the representation: `… | to json | {spelling}` (spec §12.3)"
                )));
            }
            StreamEvent::Failure(error) => return Err(error),
        }
    }
    Ok(Value::Bytes(content.into()))
}

/// Whether a verb creates the object it names rather than acting on one that exists.
///
/// `docs/spec/verbs.yaml`: `add` is "Create a membership or association", `mount` is "Attach a
/// filesystem or resource" — both name something that is not there yet (ADR-0098 §1,
/// ADR-0102 §1).
fn creates(verb: &str) -> bool {
    matches!(verb, "add" | "mount")
}

/// The selectors that were written, by the names the contract gives them, in contract order.
fn named_selectors(ctx: &Invocation<'_>) -> Vec<(String, Value)> {
    ctx.contract()
        .selectors()
        .iter()
        .filter_map(|spec| {
            ctx.arguments()
                .selector(spec.name())
                .map(|value| (spec.name().to_owned(), value.clone()))
        })
        .collect()
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
