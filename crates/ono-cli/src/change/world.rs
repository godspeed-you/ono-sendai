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
pub const PACKAGE_SCHEMA: &str = "ono.package/1";

/// The precondition field §43.5's path identity is frozen under.
pub const LSTAT_FIELD: &str = "lstat";

/// What the object at `path` is, read without following a symlink (§43.5, §7.1).
///
/// The file type, the device and the inode: the three facts that change when the object at a
/// path is replaced — by another file, or by a link to one — and that no content check can see,
/// because reading content follows the link.
///
/// # Errors
///
/// Whatever `lstat(2)` refused with.
pub fn lstat_identity(path: &Path) -> std::io::Result<String> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let metadata = path.symlink_metadata()?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        "symlink"
    } else if file_type.is_dir() {
        "dir"
    } else if file_type.is_file() {
        "file"
    } else if file_type.is_block_device() {
        "block-device"
    } else if file_type.is_char_device() {
        "char-device"
    } else if file_type.is_fifo() {
        "fifo"
    } else if file_type.is_socket() {
        "socket"
    } else {
        "other"
    };
    Ok(format!(
        "{kind} dev={} ino={}",
        metadata.dev(),
        metadata.ino()
    ))
}

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
/// The object also carries its v0.4 place where the spatial layer has one for it (§9.3): impact
/// is walked from that place, and a target without one is a target the walk cannot leave.
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
    let (frozen, record) = resolved(
        providers,
        mounts,
        shape.target_word(),
        subject,
        field,
        creates,
    )
    .await?;
    Ok(placed(providers, frozen, record.as_ref()).await)
}

/// The frozen target `subject` names on `word`, and the record it was frozen from where a provider
/// answered with one.
///
/// Keyed by the target word rather than by [`TargetShape`] so revalidation can re-derive an
/// identity from what a stored action carries (§7.3): the word is the `ono.<word>/1` a frozen
/// target's schema was built from, and [`TargetShape::of`] maps exactly these words.
async fn resolved(
    providers: &ProviderRegistry,
    mounts: &MountTable,
    word: &str,
    subject: &str,
    field: &str,
    creates: bool,
) -> Result<(FrozenTarget, Option<RecordValue>), ErrorValue> {
    // §4.3 keeps the selector a target came from so `explain` can show it, and that is the text
    // the operator wrote rather than the name of the field it was matched against.
    let selector = subject;
    match word {
        "service" => {
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
            Ok((frozen.freeze()?, Some(record)))
        }
        "file" | "dir" => {
            let path = canonical(Path::new(subject))?;
            let state = super::session::change_session().await?;
            let domain = file_domain(state.providers(), mounts, &path);
            // Appendix B.7: a tmpfs, procfs or network export is never a persistence domain, and
            // its mount source (`shm`, `proc`) is a label rather than one — so a refused domain
            // is recorded as none at all.
            let boundary = ono_change_protection::recorded_domain(&domain).map(str::to_owned);
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
            let mut frozen = FileTarget::at(&path).freeze()?;
            if let Some(boundary) = boundary {
                frozen = frozen.in_domain(boundary);
            }
            if path.exists() {
                frozen = frozen.resolved_from(selector);
            }
            Ok((frozen, None))
        }
        "package" => {
            let record =
                resolved_object(providers, "package", subject, "name", "package manager").await?;
            let namespace = text(&record, "provider").unwrap_or_else(|| "package".to_owned());
            let name = text(&record, "name").unwrap_or_else(|| subject.to_owned());
            let mut identity = format!("{namespace}:{name}");
            if let Some(version) = text(&record, "version") {
                identity.push('#');
                identity.push_str(&version);
            }
            let frozen = FrozenTarget::new(PACKAGE_SCHEMA, identity, name).resolved_from(selector);
            Ok((frozen, Some(record)))
        }
        // §7.1: every other target is an object a provider knows by name, and freezing it is
        // asking that provider for it and recording what came back. The identity is the
        // provider's namespace and the object's name, with the generation §7.2 will revalidate
        // against where the provider offers one — the same three facts a service target carries,
        // built the same way, because a route, a mount and a container are not special.
        word => {
            // §4.3 turns a selector into an identity, and an operation that *creates* its object
            // has no set to match: `add user alice` names `alice`, and no provider can answer
            // for her until the plan runs. The identity is the name the operator gave, and
            // §7.2's precondition is that it is still not taken.
            let Some(record) = maybe_object(providers, word, subject, field).await? else {
                if creates {
                    // No `resolved_from`: nothing matched, so §7.2 has no existence precondition
                    // to state and `fragment_for` reads the absence as exactly that.
                    let frozen = FrozenTarget::new(
                        format!("ono.{word}/1"),
                        format!("{word}:{subject}"),
                        subject,
                    );
                    return Ok((frozen, None));
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
            let frozen =
                FrozenTarget::new(format!("ono.{word}/1"), identity, name).resolved_from(selector);
            Ok((frozen, Some(record)))
        }
    }
}

/// `frozen`, carrying the v0.4 place it is where the spatial layer has one (§9.3, §9.6).
///
/// The record the provider answered with is registered in the session's index and its relations
/// are read, so the impact walk that follows has something to walk: a place with no observed
/// exits is a place the graph cannot leave, and one whose exits could not be read is recorded as
/// such by the index and becomes §9.6's boundary. A file is placed by its path, which is what
/// the filesystem is queried by (v0.4 §33.3). An object with no place — a package, a created
/// object nobody can answer for yet — is returned unplaced, and the walk says what it can.
///
/// Placing is best effort by design: §2.1 makes planning a read, and a relation provider that
/// could not answer is a gap in the impact graph rather than a reason to refuse the plan.
async fn placed(
    providers: &ProviderRegistry,
    frozen: FrozenTarget,
    record: Option<&RecordValue>,
) -> FrozenTarget {
    let now = jiff::Timestamp::now();
    let mut spatial = crate::spatial::spatial_session().await;
    let place = match record {
        Some(record) => {
            spatial.absorb(std::slice::from_ref(record), now);
            spatial.projection_of(record).ok()
        }
        None if frozen.schema() == ono_change_plan::freeze::FILE_SCHEMA => {
            let path = Path::new(frozen.label());
            crate::spatial::view::observe_path(providers, &mut spatial, path, now).await;
            [
                ono_spatial_core::SpatialType::File,
                ono_spatial_core::SpatialType::Directory,
            ]
            .into_iter()
            .find_map(|kind| spatial.reference(kind, frozen.label()))
        }
        None => None,
    };
    let Some(place) = place.filter(|place| spatial.index().contains(place)) else {
        return frozen;
    };
    let interest = crate::spatial::relations::Interest::here();
    let _ =
        crate::spatial::relations::observe(providers, &mut spatial, &place, &interest, now).await;
    frozen.at_place(place.as_str())
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

/// §7.2's generation: the fact that changes when the object is replaced by another of its name.
///
/// For a service, `since` is the honest one where the provider has it — a restart moves it — and
/// the state word is the fallback. A unit whose provider offers neither has no generation, and
/// the frozen target says so by carrying none rather than by inventing a counter.
///
/// A process is different: its state word is its run state, which moves between `sleeping` and
/// `running` without anything having happened to it, so it would be drift nobody caused. What
/// tells one process from another under the same pid is its start time — `ono.process/1`'s
/// identity is `pid` and `started` — and a process whose start time could not be read carries no
/// generation at all.
fn generation_of(record: &RecordValue) -> Option<String> {
    if matches!(
        record.schema().id().name(),
        "ono.process" | "ono.process-detail"
    ) {
        return match record.get("started") {
            Some(Value::Timestamp(started)) => Some(started.to_string()),
            _ => None,
        };
    }
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

/// The persistence domain of `path`, as the recovery provider that understands its filesystem
/// maps it (Appendix B.9), and the mount table's reading where none does (Appendix B.1).
///
/// One function for the freeze, the revalidation and `inspect plan --resolution`: a file in a
/// nested Btrfs subvolume records that subvolume, and the domain a plan recorded is checked at
/// `apply` against the same reading rather than against the mount's `subvol=` option.
#[must_use]
pub fn file_domain(
    recovery: &ono_change_protection::ProviderRegistry,
    mounts: &MountTable,
    path: &Path,
) -> ono_change_core::PersistenceDomain {
    let fallback = mounts.resolve(path);
    recovery.resolve_at(&path.display().to_string(), &fallback)
}

/// The host the innermost `enter link` frame stands on, where the session stands on one (§14.4).
#[must_use]
pub fn linked_host(context: &[ono_command::ContextFrame]) -> Option<String> {
    context
        .iter()
        .rev()
        .find(|frame| matches!(frame.kind(), ono_command::FrameKind::Link))
        .map(|frame| frame.identity().to_string())
}

/// The refusal for a link whose connection is not held (§29.3): nothing can reach its host, so
/// `command` changes nothing rather than answering from this machine under the host's name.
#[must_use]
pub fn link_down(host: &str, command: &str) -> ErrorValue {
    ErrorValue::new(
        ono_core::ErrorCode::RemoteUnreachable,
        format!("`{command}` needs the link to {host}, and it is not connected"),
    )
    .with_help(
        "v0.6 §29.1 and §29.3: a linked host's objects are reached through its link, and a link \
         that is down reaches nothing. `link host` connects it again. Nothing was changed"
            .to_owned(),
    )
    .with_metadata("host", Value::string(host))
}

/// Whether `plan` may run where the session stands (§29.1, §14.4, ADR-0848).
///
/// A plan whose targets carry a host runs only inside `enter link` to that host with the link
/// connected, because that frame is what routes provider calls there. A plan frozen on this
/// machine does not run inside a link: its actions would reach the linked host instead.
///
/// # Errors
///
/// `change.precondition_failed` naming the host the plan is about and where the session stands,
/// and `remote.unreachable` when the plan's own link is not connected.
pub fn route(
    context: &[ono_command::ContextFrame],
    plan: &ono_change_core::ChangePlan,
    command: &str,
) -> Result<(), ErrorValue> {
    let wanted = plan
        .targets()
        .iter()
        .find_map(|target| target.host().map(str::to_owned));
    let standing = linked_host(context);
    match (wanted, standing) {
        (None, None) => Ok(()),
        (Some(wanted), Some(standing)) if wanted == standing => {
            if crate::spatial::links::facts(&wanted).is_some_and(|facts| facts.connected) {
                Ok(())
            } else {
                Err(link_down(&wanted, command))
            }
        }
        (wanted, standing) => {
            let expected = wanted
                .as_deref()
                .map_or_else(|| "this machine".to_owned(), |host| format!("host {host}"));
            let found = standing.as_deref().map_or_else(
                || "this machine".to_owned(),
                |host| format!("host {host}, through `enter link {host}`"),
            );
            let remedy = wanted.as_deref().map_or_else(
                || "`leave` the link first.".to_owned(),
                |host| format!("`enter link {host}` first."),
            );
            Err(ErrorValue::new(
                ono_core::ErrorCode::ChangePreconditionFailed,
                format!(
                    "plan {} is about {expected}, and the session stands on {found}",
                    plan.id().short()
                ),
            )
            .with_help(format!(
                "v0.6 §29.1 and §14.4: provider calls go where the session stands, so `{command}` \
                 would reach {found} with actions frozen for {expected}. {remedy} Nothing was \
                 changed"
            ))
            .with_metadata("fact", Value::string("host"))
            .with_metadata("expected", Value::string(&expected))
            .with_metadata("found", Value::string(&found)))
        }
    }
}

/// `outcome`, read for an action on a linked host (§29.3, Appendix F.2, ADR-0848).
///
/// A link that failed under an action leaves the host's side of it unestablished: the request may
/// have reached the far side and run, or not. §29.3 forbids calling that a failure or a success
/// without evidence, so it settles `unknown`, and the executor's `remote-disconnect` keeps the
/// plan applying until the link is back and the host can be asked again. Every other outcome, and
/// every outcome on this machine, is what it was.
#[must_use]
pub fn over_link(
    plan: &ono_change_core::ChangePlan,
    action: &PlanAction,
    outcome: ExecutionOutcome,
) -> ExecutionOutcome {
    let host = action
        .target()
        .and_then(|identity| {
            plan.targets()
                .iter()
                .find(|target| target.identity() == identity)
        })
        .and_then(FrozenTarget::host);
    match (host, outcome) {
        (Some(host), ExecutionOutcome::Failed(refusal)) if lost_link(&refusal) => {
            ExecutionOutcome::Unknown(
                error::remote_state_unknown(host, action.summary())
                    .with_metadata("cause", Value::string(refusal.code().name())),
            )
        }
        (_, outcome) => outcome,
    }
}

/// Whether `refusal` says the link failed rather than that the far side answered no (§29.3).
fn lost_link(refusal: &ErrorValue) -> bool {
    matches!(
        refusal.code(),
        ono_core::ErrorCode::RemoteUnreachable
            | ono_core::ErrorCode::RemoteHandshakeTimeout
            | ono_core::ErrorCode::RemoteProtocolMismatch
    )
}

/// Carries out one action through the provider that owns its target (§4.7).
///
/// An opaque action runs its program with the argument vector it carries and no shell in between
/// (§2.17, §12.3): `argv` reaches `execve` as it stands, so a value containing a semicolon is a
/// value containing a semicolon.
#[must_use]
pub fn execute(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
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
        // A recovery operation is carried out by the provider that owns its asset, and only a
        // recovery plan's `apply` holds that asset and the operator's acceptance (§12.2, §24.5).
        // Running one from here would skip both, so it is refused by name.
        Execution::RecoveryOperation { .. } => {
            ExecutionOutcome::Failed(error::action_not_plannable(
                action.summary(),
                "a recovery operation runs through the provider that owns its asset, as an action \
             of a recovery plan (`recover`, then `apply`)",
            ))
        }
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
    // The provider holds the field in its own type, exactly as §4.3's freeze asked it: `pid` is
    // an integer, and a string `4211` resolves to nothing — which would leave the action naming
    // an object id no provider can act on.
    let wanted = object
        .parse::<i128>()
        .map_or_else(|_| Value::string(object), Value::Int);
    let references = providers
        .resolve(target, &Selector::field(selector, wanted))
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
    // §23.5: "Every verification check MUST have a timeout." A provider that never answers is
    // a check that timed out, and the contract — not this function — says what that means.
    let records = match block_on(
        handle,
        within(contract.timeout(), look_up(providers, target, identity)),
    ) {
        None => return Observation::TimedOut,
        Some(Ok(records)) => records,
        Some(Err(refusal)) => return Observation::Unobservable(refusal),
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
    // §2.4 and §35.3: a field the object does not carry, or carries as null, is unknown — and an
    // unknown compared with anything is neither equal nor different. Reading it as `""` would
    // make every `!=` pass and every `==` fail on a fact nobody observed.
    let observed = record.get(field).filter(|value| !value.is_null()).cloned();
    let seen = observed.as_ref().and_then(|value| match value {
        Value::String(text) => Some(text.to_string()),
        other => ono_value::canonical_text(other).ok(),
    });
    let (Some(observed), Some(seen)) = (observed, seen) else {
        return Observation::Unobservable(error::verification_unknown(
            subject,
            &format!(
                "the object carries no `{field}`, so whether it is `{operator} {expected}` is \
                 unknown (§2.4)"
            ),
        ));
    };
    let observed = Some(observed);
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

/// `future`, or `None` where it did not finish within `deadline` (§23.5).
async fn within<T>(deadline: std::time::Duration, future: impl Future<Output = T>) -> Option<T> {
    tokio::time::timeout(deadline, future).await.ok()
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
/// field, so `socket :443` matches a socket's port, `service nginx` a unit's name and
/// `process 4211` a process's pid — which is what a process plan froze it by (§7.1).
fn matches(record: &RecordValue, identity: &str) -> bool {
    if identity.is_empty() {
        return true;
    }
    let wanted = identity.trim_start_matches(':');
    ["name", "path", "local_port", "port", "unit", "pid", "id"]
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
    let subject = subject_of(action);
    for precondition in action.preconditions() {
        let observed = observe_subject(handle, providers, subject.as_ref(), precondition);
        let verdict = precondition.check(observed.as_ref());
        findings.push(DriftFinding::new(precondition, verdict, observed));
    }
    Ok(findings)
}

/// The target an action's frozen object belongs to, and the field it was selected by (§7.1).
///
/// Both travel in the action, because both were fixed when it was frozen: `target` is the word the
/// frozen schema `ono.<word>/1` was built from, and `object_selector` the field the provider was
/// asked by — `pid` for a process, `name` for a service. Revalidation asks the same question of
/// the same provider; asking any other would be revalidating a different object.
#[derive(Debug, Clone)]
struct Subject {
    target: String,
    field: String,
}

/// The [`Subject`] of `action`, where it is carried out through a provider.
fn subject_of(action: &PlanAction) -> Option<Subject> {
    let Execution::ProviderAction { arguments, .. } = action.execution() else {
        return None;
    };
    Some(Subject {
        target: argument(arguments, "target")?,
        field: argument(arguments, "object_selector").unwrap_or_else(|| "name".to_owned()),
    })
}

/// The subject a frozen identity names, for an action that did not carry one.
///
/// A plan stored before actions carried their target: its identity's namespace is all there is,
/// and the one namespace that names a target rather than a provider is the service manager's.
fn subject_from_namespace(namespace: &str) -> Subject {
    let target = match namespace {
        "systemd" | "service" => "service",
        _ => "package",
    };
    Subject {
        target: target.to_owned(),
        field: "name".to_owned(),
    }
}

/// The record of `target` whose `field` is `key` now, or `None` where the provider has none.
///
/// Matched strictly on the field: a provider that ignored the selector answers with every object
/// it serves, and the first of those is not the one the plan froze. A provider that refused the
/// narrow question — a process provider reading `/proc/<pid>` for a pid that has exited — is
/// asked the broad one, so an object that has gone is reported absent rather than unobservable.
async fn current_object(
    providers: &ProviderRegistry,
    target: &str,
    field: &str,
    key: &str,
) -> Result<Option<RecordValue>, ErrorValue> {
    let wanted = key
        .parse::<i128>()
        .map_or_else(|_| Value::string(key), Value::Int);
    let strict = |records: Vec<RecordValue>| {
        records
            .into_iter()
            .find(|record| text(record, field).as_deref() == Some(key))
    };
    if let Ok(records) = objects(providers, target, Some(Selector::field(field, wanted))).await
        && let Some(found) = strict(records)
    {
        return Ok(Some(found));
    }
    Ok(strict(objects(providers, target, None).await?))
}

/// Whether the object a frozen identity names still exists, as the `exists` precondition reads it.
fn observe_subject(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
    carried: Option<&Subject>,
    precondition: &ono_change_core::Precondition,
) -> Option<Value> {
    let subject = precondition.subject();
    // §7.2's `identity` field is the frozen identity re-derived from the world as it is now: a
    // file's inode and a unit's generation are in it, so an object replaced underneath the plan
    // answers with a different string and `check` calls it material drift.
    if precondition.field() == "identity" {
        return current_identity(handle, providers, carried, subject)
            .map(|found| Value::string(&found));
    }
    // A file subject is the canonical path as it stands. §7.1 keeps a file's persistence domain a
    // field beside its identity rather than a suffix of it, and a path may hold an `@` of its own
    // — a Btrfs subvolume is conventionally `@var` — so nothing is cut off it here.
    // §7.2 and Appendix B: the path is resolved through the mount table as it is now, so a
    // dataset that moved underneath the plan is drift rather than a protection that silently
    // covers something else.
    if precondition.field() == "persistence_domain" {
        let mounts = MountTable::from_proc().ok()?;
        let state = block_on(handle, super::session::change_session()).ok()?;
        let domain = file_domain(state.providers(), &mounts, Path::new(subject));
        // A path that now resolves to a refused domain has none, which is drift against the one
        // the plan froze rather than a match on the volatile filesystem's label.
        return Some(
            ono_change_protection::recorded_domain(&domain).map_or(Value::Null, Value::string),
        );
    }
    if precondition.field() == "sha256" {
        let path = subject;
        let bytes = std::fs::read(path).ok()?;
        return Some(Value::string(&ono_recovery_files::manifest::digest_of(
            &bytes,
        )));
    }
    // §43.5: what the path *is*, read without following it. A symlink swapped in over a frozen
    // file answers with another type and another inode, and a path that has gone answers
    // `absent` — both material, where a read that failed for any other reason is unknown.
    if precondition.field() == LSTAT_FIELD {
        let path = subject;
        return match lstat_identity(Path::new(path)) {
            Ok(found) => Some(Value::string(&found)),
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {
                Some(Value::string("absent"))
            }
            Err(_) => None,
        };
    }
    if subject.starts_with('/') {
        return Some(Value::Bool(Path::new(subject).exists()));
    }
    let (namespace, rest) = subject.split_once(':')?;
    let key = rest.split('#').next().unwrap_or(rest);
    let fallback = subject_from_namespace(namespace);
    let Subject { target, field } = carried.unwrap_or(&fallback);
    // §7.2: "package installed version still equals Z". A package that is no longer installed has
    // no version, and null against the frozen one is material rather than unknown.
    if precondition.field() == "version" {
        let found = block_on(handle, current_object(providers, target, "name", key)).ok()?;
        return Some(
            found
                .and_then(|record| text(&record, "version"))
                .map_or(Value::Null, |version| Value::string(&version)),
        );
    }
    // A service is looked up by the unit name the identity carries, and `restart service ssh`
    // froze `ssh.service`: the identity holds the provider's name, whatever the operator typed.
    let field = if target == "service" { "name" } else { field };
    let found = block_on(handle, current_object(providers, target, field, key)).ok()?;
    Some(Value::Bool(found.is_some()))
}

/// The identity the object at `frozen` carries now, or `None` where it could not be established.
///
/// For a path it is the canonical path with the inode facts §7.1 freezes beside it, re-read; for
/// anything else it is the provider's namespace, name and generation. A subject that cannot be
/// re-derived answers `None`, which §2.4 makes `UNKNOWN` and §7.3 blocks on rather than passes.
fn current_identity(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
    carried: Option<&Subject>,
    frozen: &str,
) -> Option<String> {
    if frozen.starts_with('/') {
        let mounts = MountTable::from_proc().unwrap_or_else(|_| MountTable::from_text(""));
        let resolved = canonical(Path::new(frozen)).ok()?;
        let state = block_on(handle, super::session::change_session()).ok()?;
        let domain = file_domain(state.providers(), &mounts, &resolved);
        let mut current = FileTarget::at(&resolved).freeze().ok()?;
        if let Some(boundary) = ono_change_protection::recorded_domain(&domain) {
            current = current.in_domain(boundary);
        }
        return Some(current.identity().to_owned());
    }
    let (namespace, rest) = frozen.split_once(':')?;
    let key = rest.split('#').next().unwrap_or(rest);
    let fallback = subject_from_namespace(namespace);
    let Subject { target, field } = carried.unwrap_or(&fallback);
    let field = if target == "service" { "name" } else { field };
    // The identity is re-derived by exactly the code that froze it, so an object nothing happened
    // to answers with the same string — and one replaced underneath the plan does not.
    let mounts = MountTable::from_text("");
    let (current, _) = block_on(
        handle,
        resolved(providers, &mounts, target, key, field, false),
    )
    .ok()?;
    Some(current.identity().to_owned())
}

/// The object and relation states v0.6 §22.2's pre-plan checkpoint is built from.
///
/// One [`ono_temporal_core::ObjectState`] per frozen target that has a place in `index`, carrying the record the
/// provider answers with *now* — the state about to change, asked of the provider by the same
/// lookup revalidation uses, so the checkpoint and §7.3's drift check read the same object. A
/// target with no place, or whose record cannot be read, is left out rather than filled in: a
/// checkpoint holding an invented record would be evidence of a state nobody observed (§2.4). The
/// relations are the index's edges touching those places, as they were last observed.
#[must_use]
pub fn checkpoint_states(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
    plan: &ono_change_core::ChangePlan,
    index: &ono_spatial_index::SpatialIndex,
    at: jiff::Timestamp,
) -> (
    Vec<ono_temporal_core::ObjectState>,
    Vec<ono_temporal_core::RelationState>,
) {
    let mut objects: Vec<ono_temporal_core::ObjectState> = Vec::new();
    for target in plan.targets() {
        let Some(id) = target
            .spatial_id()
            .and_then(ono_spatial_core::SpatialId::parse)
        else {
            continue;
        };
        let Some(entry) = index.get(&id) else {
            continue;
        };
        let Some(record) = current_record(handle, providers, plan, target) else {
            continue;
        };
        objects.push(ono_temporal_core::ObjectState {
            id,
            object_type: entry.object().object_type(),
            label: Arc::from(target.label()),
            record,
            observed_at: at,
            source: ono_temporal_core::EvidenceSource::session(),
        });
    }
    let mut relations: Vec<ono_temporal_core::RelationState> = Vec::new();
    for object in &objects {
        let Some(entry) = index.get(&object.id) else {
            continue;
        };
        for edge in entry.edges() {
            let relation = ono_temporal_core::RelationState {
                from: edge.source().clone(),
                to: edge.target().clone(),
                relation: Arc::from(edge.relation().to_string()),
                confidence: edge.confidence(),
                observed_at: edge.observed_at(),
                source: ono_temporal_core::EvidenceSource::session(),
            };
            // Both ends of an edge between two targets carry it; the checkpoint holds it once.
            if !relations.contains(&relation) {
                relations.push(relation);
            }
        }
    }
    (objects, relations)
}

/// The record `target` answers with now, asked the way revalidation asks it (§7.3).
fn current_record(
    handle: &tokio::runtime::Handle,
    providers: &ProviderRegistry,
    plan: &ono_change_core::ChangePlan,
    target: &FrozenTarget,
) -> Option<RecordValue> {
    let (word, field, key) = match target.schema() {
        ono_change_plan::freeze::FILE_SCHEMA => {
            ("file".to_owned(), "path".to_owned(), target.label())
        }
        ono_change_plan::freeze::SERVICE_SCHEMA => {
            ("service".to_owned(), "name".to_owned(), target.label())
        }
        PACKAGE_SCHEMA => ("package".to_owned(), "name".to_owned(), target.label()),
        _ => {
            let action = plan
                .actions()
                .iter()
                .find(|action| action.target() == Some(target.identity()))?;
            let Subject {
                target: word,
                field,
            } = subject_of(action)?;
            let (_, rest) = target.identity().split_once(':')?;
            (word, field, rest.split('#').next().unwrap_or(rest))
        }
    };
    block_on(handle, current_object(providers, &word, &field, key))
        .ok()
        .flatten()
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_give_up_on_a_check_that_does_not_answer_within_its_timeout() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a runtime with a clock");
        let answered = runtime.block_on(within(
            std::time::Duration::from_millis(20),
            std::future::pending::<()>(),
        ));
        assert!(
            answered.is_none(),
            "v0.6 §23.5: a check that never answers has timed out, and waiting forever is not an \
             answer"
        );
    }

    #[test]
    fn should_checkpoint_a_placed_file_target_with_the_record_it_has_now_and_skip_what_cannot_be_read()
     {
        let directory = std::env::temp_dir().join(format!("ono-checkpoint-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let file = directory.join("app.conf");
        std::fs::write(&file, b"before").expect("the file is written");
        let path = file.canonicalize().expect("the file resolves");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let handle = runtime.handle().clone();
        let providers = crate::providers::registry(std::env::vars());
        let now = jiff::Timestamp::now();
        let records = runtime
            .block_on(objects(
                &providers,
                "file",
                Some(Selector::field(
                    "path",
                    Value::string(&path.display().to_string()),
                )),
            ))
            .expect("the file provider answers");
        let record = records.first().expect("the file is described").clone();
        let mut spatial =
            crate::spatial::session::SpatialSessionState::new(crate::spatial::local_scope(), now);
        spatial.absorb(std::slice::from_ref(&record), now);
        let place = spatial.projection_of(&record).expect("a file has a place");
        assert!(spatial.index().contains(&place), "the place is indexed");
        let placed = FileTarget::at(&path)
            .freeze()
            .expect("a path freezes")
            .at_place(place.as_str());
        let unplaced = FileTarget::at(&directory.join("elsewhere"))
            .freeze()
            .expect("a path freezes");
        let plan = ono_change_core::ChangePlan::draft(
            ono_change_core::Intent::new("write the file", "plan write file"),
            "session",
            now,
        )
        .resolve(vec![placed, unplaced])
        .expect("a draft resolves");

        let (states, _) = checkpoint_states(&handle, &providers, &plan, spatial.index(), now);
        assert_eq!(
            states.len(),
            1,
            "v0.6 §22.2: the placed target is checkpointed, and the one with no place is not"
        );
        assert_eq!(states[0].id, place);
        assert_eq!(states[0].label.as_ref(), path.display().to_string());
        assert_eq!(
            text(&states[0].record, "path").as_deref(),
            Some(path.display().to_string().as_str()),
            "the record is the provider's own answer for the file"
        );
        assert_eq!(states[0].observed_at, now);

        std::fs::remove_file(&file).expect("the file is removed");
        let (states, relations) =
            checkpoint_states(&handle, &providers, &plan, spatial.index(), now);
        let _ = std::fs::remove_dir_all(&directory);
        assert!(
            states.is_empty() && relations.is_empty(),
            "§2.4: a target whose record cannot be read now is left out, never filled in"
        );
    }

    #[test]
    fn should_tell_a_symlink_from_the_file_it_points_at() {
        let directory = std::env::temp_dir().join(format!("ono-lstat-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let file = directory.join("file");
        let link = directory.join("link");
        std::fs::write(&file, b"x").expect("the file is written");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&file, &link).expect("the link is made");
        let of_file = lstat_identity(&file).expect("the file is there");
        let of_link = lstat_identity(&link).expect("the link is there");
        let _ = std::fs::remove_dir_all(&directory);
        assert!(
            of_file.starts_with("file "),
            "a regular file reads as one: {of_file}"
        );
        assert!(
            of_link.starts_with("symlink "),
            "a link reads as a link: {of_link}"
        );
        assert_ne!(
            of_file, of_link,
            "v0.6 §43.5: a link to a file is not the file, even though reading either gives the \
             same bytes"
        );
    }
}
