//! The four things the change layer asks of the world (spec v0.6 §4.3, §4.7, §7.3, §23).
//!
//! §50.1 keeps providers out of the core and §54.5 requires every one of these to be replaceable
//! by a scripted answer, which is why `ono-change-executor` takes them as closures. This module
//! is the shell's implementation of those closures, and it is the only place in the change tree
//! that touches an `ono-provider-api` provider:
//!
//! - **freezing** turns a selector into the concrete object identity §7.1 asks for;
//! - **executing** runs one [`PlanAction`] through the provider that owns its target (§4.7);
//! - **observing** asks the world whether a [`VerificationContract`] holds (§23.3);
//! - **revalidating** rechecks an action's preconditions against the world (§7.3).
//!
//! Every one of them is synchronous, because the executor's seams are, and the provider registry
//! is asynchronous. [`block_on`] is the bridge, and it is `block_in_place` on a multi-threaded
//! runtime rather than a nested runtime: the shell builds one runtime per process and a second
//! one here would run a provider's I/O on a reactor nothing else knows about.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_change_core::VerificationStatus;
use ono_change_core::{
    DriftFinding, Execution, FrozenTarget, PlanAction, VerificationContract, error,
};
use ono_change_executor::{ExecutionOutcome, Observation};
use ono_change_plan::{FileTarget, ServiceTarget};
use ono_change_protection::MountTable;
use ono_provider_api::{Action, ObjectId, ProviderRegistry, Query, Selector};
use ono_value::{ErrorValue, RecordValue, SchemaId, Value};

use super::actions::TargetShape;

/// The schema a frozen package target carries.
const PACKAGE_SCHEMA: &str = "ono.package/1";

/// Runs `future` to completion from a synchronous seam.
///
/// The runtime handle comes from the caller because §39.2's discipline applies to a reactor as
/// much as to a clock: a function that reached for the ambient runtime could not be run from a
/// test that has none.
pub fn block_on<T>(handle: &tokio::runtime::Handle, future: impl Future<Output = T>) -> T {
    match tokio::runtime::Handle::try_current() {
        // Inside the runtime already: `block_in_place` moves this task off the worker so the
        // provider's own I/O keeps a thread to run on.
        Ok(current) => tokio::task::block_in_place(|| current.block_on(future)),
        Err(_) => handle.block_on(future),
    }
}

/// Every record `target` answers for, drained from the provider that owns it.
///
/// # Errors
///
/// Whatever the provider refused with. A provider that answered partially leaves its failures on
/// the stream's error channel, and those are reported rather than swallowed (§16.5).
pub async fn objects(
    providers: &ProviderRegistry,
    target: &str,
    selector: Option<Selector>,
) -> Result<Vec<RecordValue>, ErrorValue> {
    let mut query = Query::target(target).for_verb("get");
    if let Some(selector) = selector {
        query = query.with(selector);
    }
    let mut stream = providers.snapshot(&query)?;
    let mut records = Vec::new();
    while let Some(event) = stream.recv().await {
        if let ono_pipeline::StreamEvent::Value(value) = event
            && let Ok(record) = value.as_record()
        {
            records.push(RecordValue::clone(record));
        }
    }
    Ok(records)
}

/// The concrete object `subject` names, frozen as §4.3 and §7.1 require.
///
/// A service and a package are resolved against the provider that serves them, because §7.1 makes
/// the provider namespace part of the identity: two providers may both offer `nginx.service`, and
/// a plan resolved against one must not revalidate against the other. A path is resolved against
/// the mount table instead, because §7.1's file identity is a canonical path *and* the persistence
/// domain that actually holds its state (Appendix B).
///
/// # Errors
///
/// `change.target_unresolved` where the selector matched nothing: §4.3 freezes the set of objects
/// that match now, and a selector that matched none produced no plan.
pub async fn freeze(
    providers: &ProviderRegistry,
    mounts: &MountTable,
    shape: TargetShape,
    subject: &str,
    field: &str,
    creates: bool,
) -> Result<FrozenTarget, ErrorValue> {
    // §4.3 keeps the selector a target came from so `explain` can show it, and that is the text
    // the operator wrote rather than the name of the field it was matched against.
    let selector = subject;
    match shape {
        TargetShape::Service => {
            let records = objects(
                providers,
                "service",
                Some(Selector::field("name", Value::string(subject))),
            )
            .await?;
            let record = named(&records, subject).ok_or_else(|| {
                error::target_unresolved(
                    subject,
                    "§4.3 freezes the objects that match now, and no service manager on this host \
                     serves a unit by that name.",
                )
            })?;
            let namespace = text(&record, "provider").unwrap_or_else(|| "service".to_owned());
            let unit = text(&record, "name").unwrap_or_else(|| subject.to_owned());
            let generation = generation_of(&record);
            let mut frozen = ServiceTarget::new(&namespace, &unit).resolved_from(selector);
            if let Some(generation) = generation.as_deref() {
                frozen = frozen.with_generation(generation);
            }
            frozen.freeze()
        }
        TargetShape::File => {
            let path = canonical(Path::new(subject))?;
            let domain = mounts.resolve(&path);
            let boundary = domain
                .boundary()
                .map(str::to_owned)
                .unwrap_or_else(|| domain.mount().source().to_owned());
            // §7.1 asks a file target to record the persistence domain that holds its state, and
            // the frozen target does — as a field. It is deliberately kept out of the *identity*:
            // a canonical path already names one object on one host, and every layer that has to
            // recognise the object again from the identity alone — the executor's wave scheduler,
            // a recovery provider matching an archive entry against what the plan touched — reads
            // it as a path. A domain that moved is what §7.2's `persistence-domain` precondition
            // is for, and drift is where that difference belongs.
            // §4.3's selector is recorded only where something matched. A plan that *creates*
            // its object froze an identity rather than resolving one, and §7.2's existence
            // precondition — "the object still exists and is still this object" — has nothing to
            // say about an object that was not there. `selector()` is what tells the two apart.
            let mut frozen = FileTarget::at(&path).freeze()?.in_domain(boundary);
            if path.exists() {
                frozen = frozen.resolved_from(selector);
            }
            Ok(frozen)
        }
        TargetShape::Package => {
            let record =
                resolved_object(providers, "package", subject, "name", "package manager").await?;
            let namespace = text(&record, "provider").unwrap_or_else(|| "package".to_owned());
            let name = text(&record, "name").unwrap_or_else(|| subject.to_owned());
            let mut identity = format!("{namespace}:{name}");
            if let Some(version) = text(&record, "version") {
                identity.push('#');
                identity.push_str(&version);
            }
            Ok(FrozenTarget::new(PACKAGE_SCHEMA, identity, name).resolved_from(selector))
        }
        // §7.1: every other target is an object a provider knows by name, and freezing it is
        // asking that provider for it and recording what came back. The identity is the
        // provider's namespace and the object's name, with the generation §7.2 will revalidate
        // against where the provider offers one — the same three facts a service target carries,
        // built the same way, because a route, a mount and a container are not special.
        TargetShape::Named(word) => {
            // §4.3 turns a selector into an identity, and an operation that *creates* its object
            // has no set to match: `add user alice` names `alice`, and no provider can answer
            // for her until the plan runs. The identity is the name the operator gave, and
            // §7.2's precondition is that it is still not taken.
            let Some(record) = maybe_object(providers, word, subject, field).await? else {
                if creates {
                    // No `resolved_from`: nothing matched, so §7.2 has no existence precondition
                    // to state and `fragment_for` reads the absence as exactly that.
                    return Ok(FrozenTarget::new(
                        format!("ono.{word}/1"),
                        format!("{word}:{subject}"),
                        subject,
                    ));
                }
                return Err(error::target_unresolved(
                    subject,
                    &format!(
                        "§4.3 freezes the objects that match now, and no provider on this host \
                         knows a `{word}` by that name."
                    ),
                ));
            };
            let namespace = text(&record, "provider").unwrap_or_else(|| word.to_owned());
            let name = text(&record, "name").unwrap_or_else(|| subject.to_owned());
            // §7.1: the identity is the value that selected the object, not its display name.
            // Two `sleep` processes have one name and two pids, and a plan that froze the name
            // would be a plan about whichever of them answered first.
            let key = text(&record, field).unwrap_or_else(|| subject.to_owned());
            let mut identity = format!("{namespace}:{key}");
            if let Some(generation) = generation_of(&record) {
                identity.push_str("#generation=");
                identity.push_str(&generation);
            }
            Ok(FrozenTarget::new(format!("ono.{word}/1"), identity, name).resolved_from(selector))
        }
    }
}

/// The one object a provider answers with for `subject`, where there is one.
async fn maybe_object(
    providers: &ProviderRegistry,
    target: &str,
    subject: &str,
    field: &str,
) -> Result<Option<RecordValue>, ErrorValue> {
    let wanted = subject
        .parse::<i128>()
        .map_or_else(|_| Value::string(subject), Value::Int);
    let records = objects(providers, target, Some(Selector::field(field, wanted))).await?;
    Ok(matching(&records, field, subject))
}

/// The one object a provider answers with for `subject`, or §4.3's refusal.
async fn resolved_object(
    providers: &ProviderRegistry,
    target: &str,
    subject: &str,
    field: &str,
    noun: &str,
) -> Result<RecordValue, ErrorValue> {
    // §4.3 asks the provider for the object, and the provider holds the field in its own type.
    // `pid` is an integer, and a string `4211` matches no row of it.
    let wanted = subject
        .parse::<i128>()
        .map_or_else(|_| Value::string(subject), Value::Int);
    let records = objects(providers, target, Some(Selector::field(field, wanted))).await?;
    matching(&records, field, subject).ok_or_else(|| {
        error::target_unresolved(
            subject,
            &format!(
                "§4.3 freezes the objects that match now, and no {noun} on this host knows a \
                 `{target}` by that name."
            ),
        )
    })
}

/// The record whose `name` is `subject`, or the only one where the provider answered with one.
fn named(records: &[RecordValue], subject: &str) -> Option<RecordValue> {
    matching(records, "name", subject)
}

/// The record whose `field` is `subject`, or the only one the provider answered with.
///
/// §4.3 freezes what matched now. A provider that filtered on the selector itself answers with
/// one record and the first is it; one that answered with several is asked which of them the
/// operator meant, by the field they named it with.
fn matching(records: &[RecordValue], field: &str, subject: &str) -> Option<RecordValue> {
    records
        .iter()
        .find(|record| text(record, field).as_deref() == Some(subject))
        .or_else(|| records.first())
        .cloned()
}

/// §7.2's service generation: the fact that changes when the unit is restarted.
///
/// `since` is the honest one where the provider has it — a restart moves it — and the state word
/// is the fallback. A unit whose provider offers neither has no generation, and the frozen target
/// says so by carrying none rather than by inventing a counter.
fn generation_of(record: &RecordValue) -> Option<String> {
    if let Some(Value::Timestamp(since)) = record.get("since") {
        return Some(since.to_string());
    }
    text(record, "substate").or_else(|| text(record, "state"))
}

/// A record's string field, where it holds one.
fn text(record: &RecordValue, field: &str) -> Option<String> {
    match record.get(field) {
        Some(Value::String(text)) => Some(text.to_string()),
        Some(other) => ono_value::canonical_text(other).ok(),
        None => None,
    }
}

/// `path` made canonical, resolving a path whose last component does not exist yet.
///
/// A plan may write a file that is not there — `copy file ./nginx.conf to /etc/nginx/nginx.conf`
/// on a host that has neither — and §7.1 still wants a canonical path, because an unresolved one
/// is a selector wearing the name of an identity. So the parent is canonicalised and the name is
/// joined onto it.
///
/// # Errors
///
/// `change.target_unresolved` where not even the parent directory exists, because then the path
/// names nowhere and the persistence domain behind it cannot be established either.
fn canonical(path: &Path) -> Result<PathBuf, ErrorValue> {
    if let Ok(resolved) = path.canonicalize() {
        return Ok(resolved);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|failure| {
                error::target_unresolved(
                    &path.display().to_string(),
                    &format!("the working directory could not be read: {failure}"),
                )
            })?
            .join(path)
    };
    let (parent, name) = match (absolute.parent(), absolute.file_name()) {
        (Some(parent), Some(name)) => (parent.to_path_buf(), name.to_owned()),
        _ => {
            return Err(error::target_unresolved(
                &path.display().to_string(),
                "§7.1 asks for a canonical path, and this one names no file.",
            ));
        }
    };
    let parent = parent.canonicalize().map_err(|failure| {
        error::target_unresolved(
            &path.display().to_string(),
            &format!(
                "§7.1 asks for a canonical path, and `{}` could not be resolved: {failure}",
                parent.display()
            ),
        )
    })?;
    Ok(parent.join(name))
}

/// Carries out one action through the provider that owns its target (§4.7).
///
/// An opaque action runs its program with the argument vector it carries and no shell in between
/// (§2.17, §12.3): `argv` reaches `execve` as it stands, so a value containing a semicolon is a
/// value containing a semicolon.
///
/// A `RecoveryOperation` is carried out by the recovery provider that named itself in it, through
/// `recovery` — a recovery plan's RECOVER actions are ordinary plan actions (§24.2), and `apply`
/// on a recovery plan is how §5.8's second half happens.
#[must_use]
pub fn execute(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
    action: &PlanAction,
) -> ExecutionOutcome {
    execute_with(handle, providers, None, action)
}

/// [`execute`], with the change session a `RecoveryOperation` needs to reach its asset.
#[must_use]
pub fn execute_with(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
    recovery: Option<&super::session::ChangeState>,
    action: &PlanAction,
) -> ExecutionOutcome {
    match action.execution() {
        Execution::ProviderAction {
            operation,
            arguments,
            ..
        } => {
            let Some(target) = argument(arguments, "target") else {
                return ExecutionOutcome::Failed(error::action_not_plannable(
                    action.summary(),
                    "the action carries no target, so no provider owns it",
                ));
            };
            let verb = argument(arguments, "verb").unwrap_or_else(|| operation.to_string());
            let object = argument(arguments, "object").unwrap_or_default();
            let selector = argument(arguments, "object_selector").unwrap_or_default();
            let is_path = matches!(
                arguments
                    .iter()
                    .find(|(name, _)| name.as_ref() == "object_is_path")
                    .map(|(_, value)| value),
                Some(Value::Bool(true))
            );
            let mut request = match block_on(
                handle,
                identify(providers, &target, &selector, &object, is_path),
            ) {
                Ok(identified) => identified.into_action(&target, &verb),
                Err(refusal) => return ExecutionOutcome::Failed(refusal),
            };
            for (name, value) in arguments {
                if !matches!(
                    name.as_ref(),
                    "target" | "verb" | "object" | "object_selector" | "object_is_path"
                ) {
                    request = request.with(name.as_ref(), value.clone());
                }
            }
            match block_on(handle, providers.act(&request)) {
                Ok(outcome) if outcome.is_success() => ExecutionOutcome::Succeeded,
                Ok(outcome) => {
                    ExecutionOutcome::Failed(outcome.error().cloned().unwrap_or_else(|| {
                        error::tool_failed(
                            action.summary(),
                            outcome
                                .message()
                                .unwrap_or("the provider reported no reason"),
                        )
                    }))
                }
                Err(refusal) => ExecutionOutcome::Failed(refusal),
            }
        }
        Execution::Program { program, argv }
        | Execution::Opaque {
            program: Some(program),
            argv,
            ..
        } => run_program(program, argv),
        // §6.3's escape with no program to run is a description and nothing else. Appendix F.2's
        // uncertainty boundary is the honest answer: nothing was established.
        Execution::Opaque { program: None, .. } => {
            ExecutionOutcome::Unknown(error::remote_state_unknown("localhost", action.summary()))
        }
        Execution::RecoveryOperation {
            provider,
            capability,
            arguments,
        } => restore(recovery, action, provider, capability, arguments),
    }
}

/// Carries out one `recovery.restore` through the provider that owns the asset (§12.2, §24.2).
///
/// Everything the operation needs travels in the action: which provider, which capability and
/// which asset. Nothing is inferred — a provider that is not registered here is a refusal rather
/// than a substitution, because §11.4's validation was made against *that* provider's asset and
/// another one's answer would be a different claim.
fn restore(
    recovery: Option<&super::session::ChangeState>,
    action: &PlanAction,
    provider: &str,
    capability: &str,
    arguments: &[(Arc<str>, Value)],
) -> ExecutionOutcome {
    if capability != ono_change_core::RecoveryCapability::Restore.as_str() {
        // PREPARE and CLEANUP are driven by the executor and the retention pass, which hold the
        // asset. An action asking for one of them here would be asking the wrong thing to run it.
        return ExecutionOutcome::Failed(error::action_not_plannable(
            action.summary(),
            &format!(
                "`{capability}` is not carried out as a plan action; §4.5 runs preparation and                  §37 runs cleanup, each holding the asset it is about"
            ),
        ));
    }
    let Some(state) = recovery else {
        return ExecutionOutcome::Failed(error::provider_unavailable(
            provider,
            "this command was not given the change session, so no asset can be read back",
        ));
    };
    let Some(owner) = state.providers().get(provider) else {
        return ExecutionOutcome::Failed(error::provider_unavailable(
            provider,
            "no such recovery provider is registered here, and §11.4's validation was made              against that provider's asset",
        ));
    };
    let Some(reference) = argument(arguments, "asset") else {
        return ExecutionOutcome::Failed(error::action_not_plannable(
            action.summary(),
            "the recovery action names no asset to restore from",
        ));
    };
    let asset = match state
        .store()
        .resolve_asset(&reference)
        .and_then(|id| state.store().get_asset(&id))
    {
        Ok(asset) => asset,
        Err(refusal) => return ExecutionOutcome::Failed(refusal),
    };
    match owner.restore(action, &asset) {
        Ok(()) => ExecutionOutcome::Succeeded,
        Err(refusal) => ExecutionOutcome::Failed(refusal),
    }
}

/// Whether the bytes at `path` hash to `expected` (§25.1's persistent-state equivalence).
///
/// A file that cannot be read is `UNKNOWN` and not a failure: §2.4 forbids reading an
/// unestablished fact either way, and §23.3 has a word for a check that could not be answered.
fn digest_observation(path: &str, expected: &str) -> Observation {
    let Ok(bytes) = std::fs::read(path) else {
        return Observation::Unobservable(error::verification_unknown(
            path,
            "the file could not be read, so its digest could not be taken",
        ));
    };
    let found = ono_recovery_files::manifest::digest_of(&bytes);
    Observation::Answered {
        status: if found == expected {
            VerificationStatus::Passed
        } else {
            VerificationStatus::Failed
        },
        observed: Some(Value::string(&found)),
    }
}

/// Runs a resolved program with its argument vector and no shell (§2.17, §12.3, §43.6).
fn run_program(program: &str, argv: &[Arc<str>]) -> ExecutionOutcome {
    let mut command = std::process::Command::new(program);
    for argument in argv {
        command.arg(argument.as_ref());
    }
    match command.status() {
        Ok(status) if status.success() => ExecutionOutcome::Succeeded,
        Ok(status) => ExecutionOutcome::Failed(error::tool_failed(
            program,
            &format!("it exited {}", status.code().unwrap_or(-1)),
        )),
        // A program that could not be started did not run, which is a failure and not an
        // uncertainty: nothing of it reached the system.
        Err(failure) => ExecutionOutcome::Failed(error::tool_failed(program, &failure.to_string())),
    }
}

/// The object a provider action names, resolved the way an ordinary mutation resolves it.
///
/// ADR-0082 §1: a `path` selector is acted on rather than resolved — every filesystem call takes
/// a path, and a file a write is about to create has nothing to resolve. Everything else is asked
/// of the provider, and a selector that names nothing is still the target: whether the object
/// exists is the provider's to say, not this function's (ADR-0088 §2).
#[derive(Debug)]
struct Identified {
    id: ObjectId,
    label: String,
    source: Option<String>,
}

impl Identified {
    /// The provider action for `verb` on `target`.
    fn into_action(self, target: &str, verb: &str) -> Action {
        let mut action = Action::new(target, verb, self.id).labelled(self.label);
        if let Some(source) = self.source {
            action = action.with_source(source);
        }
        action
    }
}

/// Resolves the object `selector` names on `target`.
async fn identify(
    providers: &ProviderRegistry,
    target: &str,
    selector: &str,
    object: &str,
    is_path: bool,
) -> Result<Identified, ErrorValue> {
    let schema = schema_of(providers, target);
    if is_path {
        let path = PathBuf::from(object);
        return Ok(Identified {
            id: ObjectId::new(schema, [Value::Path(Arc::from(path))]),
            label: object.to_owned(),
            source: Some(object.to_owned()),
        });
    }
    let references = providers
        .resolve(target, &Selector::field(selector, Value::string(object)))
        .await?;
    match references.first() {
        Some(reference) => Ok(Identified {
            id: reference.id().clone(),
            label: reference.label().to_owned(),
            source: reference.provenance().source().map(str::to_owned),
        }),
        None => Ok(Identified {
            id: ObjectId::new(schema, [Value::string(object)]),
            label: object.to_owned(),
            source: None,
        }),
    }
}

/// The schema the objects of `target` carry, as the providers that serve it declare it.
fn schema_of(providers: &ProviderRegistry, target: &str) -> SchemaId {
    let name = format!("ono.{target}");
    providers
        .for_target(target)
        .iter()
        .flat_map(|provider| provider.schemas())
        .map(|schema| schema.id().clone())
        .find(|id| id.name() == name)
        .unwrap_or_else(|| SchemaId::new(&name, 1))
}

/// One argument of a structured execution, as text.
fn argument(arguments: &[(Arc<str>, Value)], name: &str) -> Option<String> {
    arguments
        .iter()
        .find(|(key, _)| key.as_ref() == name)
        .and_then(|(_, value)| match value {
            Value::String(text) => Some(text.to_string()),
            Value::Null => None,
            other => ono_value::canonical_text(other).ok(),
        })
}

/// Whether `contract` holds, asked of the world rather than of an exit status (§2.14, §23.3).
///
/// The subject is `<target> <identity>` — `service nginx`, `socket :443` — and the expression is
/// either `exists` or `<field> <operator> <value>`. A check whose target no provider serves is
/// [`Observation::Unobservable`], which §23.3 keeps distinct from a check that answered `UNKNOWN`:
/// the first did not run, the second ran and could not tell.
#[must_use]
pub fn observe(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
    contract: &VerificationContract,
) -> Observation {
    let subject = contract.subject();
    let (target, identity) = match subject.split_once(char::is_whitespace) {
        Some((target, identity)) => (target.trim(), identity.trim()),
        None => (subject.trim(), ""),
    };
    let expression = contract.expression().trim().to_owned();
    // `exists`, `exists == true` and `exists == false` are one question with two answers, and it
    // is the one question a provider cannot always be asked: a file the plan removed is gone, and
    // asking a filesystem provider to enumerate it would answer nothing rather than `absent`.
    let (asks_existence, wants_existence) = match expression.as_str() {
        "exists" | "exists == true" => (true, true),
        "exists == false" => (true, false),
        _ => (false, true),
    };
    // A path is observed directly. Enumerating a filesystem provider to find out whether one file
    // is there would read a directory tree to answer a question `stat` answers, and §52 budgets
    // verification rather than leaving it unbounded.
    if matches!(target, "file" | "dir") && identity.starts_with('/') {
        let exists = Path::new(identity).exists();
        if asks_existence {
            return Observation::Answered {
                status: if exists == wants_existence {
                    VerificationStatus::Passed
                } else {
                    VerificationStatus::Failed
                },
                observed: Some(Value::Bool(exists)),
            };
        }
        // §25.1: what a byte-consistent restore claims is that the bytes came back, so the
        // contract that establishes it asks for the digest of the bytes. No provider carries one
        // — a digest is not a property of a file, it is an answer to a question about it — so it
        // is read here, from the file, at the moment the question is asked.
        if let Some(expected) = expression.strip_prefix("sha256 ==") {
            return digest_observation(identity, expected.trim());
        }
    }
    let records = match block_on(handle, look_up(providers, target, identity)) {
        Ok(records) => records,
        Err(refusal) => return Observation::Unobservable(refusal),
    };
    let found = records.iter().find(|record| matches(record, identity));
    if asks_existence {
        return if found.is_some() == wants_existence {
            Observation::passed()
        } else {
            Observation::failed()
        };
    }
    let Some(record) = found else {
        return Observation::Answered {
            status: VerificationStatus::Failed,
            observed: None,
        };
    };
    let words: Vec<&str> = expression.split_whitespace().collect();
    let [field, operator, expected @ ..] = words.as_slice() else {
        return Observation::Unobservable(error::verification_unknown(
            subject,
            &format!(
                "`{expression}` is not a condition this build can observe: write `exists` or \
                 `<field> == <value>`"
            ),
        ));
    };
    let expected = expected.join(" ");
    let observed = record.get(field).cloned();
    let seen = observed
        .as_ref()
        .and_then(|value| match value {
            Value::String(text) => Some(text.to_string()),
            Value::Null => None,
            other => ono_value::canonical_text(other).ok(),
        })
        .unwrap_or_default();
    let holds = match *operator {
        "==" => seen == expected,
        "!=" => seen != expected,
        _ => {
            return Observation::Unobservable(error::verification_unknown(
                subject,
                &format!("`{operator}` is not an operator this build can observe"),
            ));
        }
    };
    Observation::Answered {
        status: if holds {
            VerificationStatus::Passed
        } else {
            VerificationStatus::Failed
        },
        observed,
    }
}

/// The objects `identity` could name on `target`, asked as narrowly as the provider permits.
///
/// A name selector first, because that is what a provider indexes on; a full enumeration only
/// where the name answered nothing, so `socket :443` — whose identity is a port rather than a
/// name — still finds its object.
async fn look_up(
    providers: &ProviderRegistry,
    target: &str,
    identity: &str,
) -> Result<Vec<RecordValue>, ErrorValue> {
    if !identity.is_empty() {
        let named = objects(
            providers,
            target,
            Some(Selector::field("name", Value::string(identity))),
        )
        .await?;
        if named.iter().any(|record| matches(record, identity)) {
            return Ok(named);
        }
    }
    objects(providers, target, None).await
}

/// Whether `record` is the object `identity` names.
///
/// The comparison is against the fields an identity is written with rather than against every
/// field, so `socket :443` matches a socket's port and `service nginx` matches a unit's name.
fn matches(record: &RecordValue, identity: &str) -> bool {
    if identity.is_empty() {
        return true;
    }
    let wanted = identity.trim_start_matches(':');
    ["name", "path", "local_port", "port", "unit"]
        .iter()
        .any(|field| text(record, field).as_deref() == Some(wanted))
}

/// Rechecks one action's preconditions against the world (§7.3).
///
/// A precondition that could not be observed is [`ono_change_core::DriftVerdict::Unknown`] rather
/// than a pass: §2.4 forbids promoting unknown, and §7.3 treats a check that could not be made as
/// drift.
///
/// # Errors
///
/// Never, in this build: every observation this shell can make answers, and one it cannot make
/// is `UNKNOWN` rather than a refusal. The signature matches the executor's seam, which is
/// allowed to fail for a provider that cannot be reached at all (§7.3).
pub fn revalidate(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
    action: &PlanAction,
) -> Result<Vec<DriftFinding>, ErrorValue> {
    let mut findings = Vec::new();
    for precondition in action.preconditions() {
        let observed = observe_subject(handle, providers, precondition);
        let verdict = precondition.check(observed.as_ref());
        findings.push(DriftFinding::new(precondition, verdict, observed));
    }
    Ok(findings)
}

/// Whether the object a frozen identity names still exists, as the `exists` precondition reads it.
fn observe_subject(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
    precondition: &ono_change_core::Precondition,
) -> Option<Value> {
    let subject = precondition.subject();
    // §7.2's `identity` field is the frozen identity re-derived from the world as it is now: a
    // file's inode and a unit's generation are in it, so an object replaced underneath the plan
    // answers with a different string and `check` calls it material drift.
    if precondition.field() == "identity" {
        return current_identity(handle, providers, subject).map(|found| Value::string(&found));
    }
    // §7.2 and Appendix B: the path is resolved through the mount table as it is now, so a
    // dataset that moved underneath the plan is drift rather than a protection that silently
    // covers something else.
    if precondition.field() == "persistence_domain" {
        let path = subject.split('@').next()?;
        let mounts = MountTable::from_proc().ok()?;
        let domain = mounts.resolve(Path::new(path));
        let boundary = domain
            .boundary()
            .map(str::to_owned)
            .unwrap_or_else(|| domain.mount().source().to_owned());
        return Some(Value::string(&boundary));
    }
    if precondition.field() == "sha256" {
        let path = subject.split('@').next()?;
        let bytes = std::fs::read(path).ok()?;
        return Some(Value::string(&ono_recovery_files::manifest::digest_of(
            &bytes,
        )));
    }
    if subject.starts_with('/') {
        let path = subject.split('@').next().unwrap_or(subject);
        return Some(Value::Bool(Path::new(path).exists()));
    }
    let (namespace, rest) = subject.split_once(':')?;
    let name = rest.split('#').next().unwrap_or(rest);
    let target = match namespace {
        "systemd" | "service" => "service",
        _ => "package",
    };
    let records = block_on(handle, objects(providers, target, None)).ok()?;
    Some(Value::Bool(
        records
            .iter()
            .any(|record| text(record, "name").as_deref() == Some(name)),
    ))
}

/// The identity the object at `frozen` carries now, or `None` where it could not be established.
///
/// For a path it is the canonical path with the inode facts §7.1 freezes beside it, re-read; for
/// anything else it is the provider's namespace, name and generation. A subject that cannot be
/// re-derived answers `None`, which §2.4 makes `UNKNOWN` and §7.3 blocks on rather than passes.
fn current_identity(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
    frozen: &str,
) -> Option<String> {
    if frozen.starts_with('/') {
        let path = frozen.split('@').next()?;
        let mounts = MountTable::from_proc().unwrap_or_else(|_| MountTable::from_text(""));
        let resolved = canonical(Path::new(path)).ok()?;
        let domain = mounts.resolve(&resolved);
        let boundary = domain
            .boundary()
            .map(str::to_owned)
            .unwrap_or_else(|| domain.mount().source().to_owned());
        return Some(
            FileTarget::at(&resolved)
                .freeze()
                .ok()?
                .in_domain(boundary)
                .identity()
                .to_owned(),
        );
    }
    let (namespace, rest) = frozen.split_once(':')?;
    let name = rest.split('#').next().unwrap_or(rest);
    let target = match namespace {
        "systemd" | "service" => "service",
        other => other,
    };
    let records = block_on(handle, objects(providers, target, None)).ok()?;
    let record = records
        .iter()
        .find(|record| text(record, "name").as_deref() == Some(name))?;
    let provider = text(record, "provider").unwrap_or_else(|| target.to_owned());
    let mut identity = format!("{provider}:{name}");
    if let Some(generation) = generation_of(record) {
        identity.push_str("#generation=");
        identity.push_str(&generation);
    }
    Some(identity)
}

/// The same object, resolved again against the world as it is now (§7.5).
///
/// `rebase` needs current identities rather than the ones the seal froze: a unit that has been
/// restarted since has a new generation, and that difference is the whole point of the new
/// revision. A target this build cannot re-freeze — a remote object, an opaque plan's host — is
/// returned unchanged rather than dropped, because a rebase that quietly shrank the target set
/// would be a different plan wearing the same intent.
///
/// # Errors
///
/// `change.target_unresolved` where the object has gone, which §7.5 reports as a plan with
/// nothing left to act on.
pub async fn refreeze(
    providers: &ProviderRegistry,
    mounts: &MountTable,
    target: &FrozenTarget,
) -> Result<FrozenTarget, ErrorValue> {
    let shape = match target.schema() {
        ono_change_plan::freeze::SERVICE_SCHEMA => TargetShape::Service,
        ono_change_plan::freeze::FILE_SCHEMA => TargetShape::File,
        PACKAGE_SCHEMA => TargetShape::Package,
        _ => return Ok(target.clone()),
    };
    let selector = target.selector().unwrap_or_else(|| target.label());
    // §7.5 refreezes what the plan already resolved, so the object is expected to be there:
    // `creates` is false, and a target that has gone is the refusal §7.5 reports.
    let _ = selector;
    freeze(providers, mounts, shape, target.label(), "name", false).await
}
