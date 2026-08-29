//! `watch` (spec §18.2): a finite query becomes a live stream of updates.
//!
//! ADR-0024 fixes the semantics. A subscription always begins with the current state as
//! `snapshot` events, so no consumer reconstructs the starting point; sameness is the record's
//! declared identity and nothing else, so a recycled pid is a new object rather than the old one
//! changing; and where the provider has no event source the runtime polls — with the interval on
//! every event as `source: poll`, because spec §18.2 requires polling to be explicit rather than
//! a cost that is invisible until someone profiles it.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{EventKind, EventStream, ProviderRegistry, Query, Selector};
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value};

use crate::invoke::{CommandImpl, Invocation, Outcome};

/// How often a poll-driven watch compares snapshots when `--every` is not written.
///
/// Two seconds is fast enough that a live table feels alive and slow enough that watching every
/// process on a loaded machine costs nothing anyone notices (spec §34).
const DEFAULT_INTERVAL: Duration = Duration::from_secs(2);

/// The `watch <target>` implementation, one instance per contract.
#[derive(Debug)]
pub(crate) struct WatchCommand {
    id: String,
}

impl WatchCommand {
    pub(crate) fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }
}

impl CommandImpl for WatchCommand {
    fn id(&self) -> &str {
        &self.id
    }

    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        // A watch narrows inside a context exactly as `get` does, and by the same seam: the
        // command table amended the arguments before this ran (ADR-0076 §1), and
        // `contract.query` carries them — declared parameters and ambient selectors alike — to
        // the provider.
        let mut query = ctx.contract().query(ctx.arguments())?;

        if ctx.contract().target() == Some("file") {
            query = file_tree_query(&query);
        }

        let event_schema = event_schema_for(ctx.contract())?;
        let interval = ctx
            .arguments()
            .option("every")
            .and_then(|value| match value {
                Value::Duration(every) => u64::try_from(every.nanoseconds())
                    .ok()
                    .map(Duration::from_nanos),
                _ => None,
            })
            .unwrap_or(DEFAULT_INTERVAL);

        // Spec §18.2: a provider that can be told about changes is subscribed to; one that
        // cannot is polled, and every event says which it was (ADR-0235).
        let object_field = ctx.contract().target().unwrap_or("object").to_owned();
        Ok(Outcome::Values(watch_stream(
            ctx.providers().clone(),
            query,
            event_schema,
            object_field,
            interval,
        )))
    }
}

/// The query behind `watch file <path>`: what lies beneath a directory, not the one entry.
///
/// ADR-0078: a watch of a directory reports the files created, changed and removed under it —
/// the provider's directory listing, hidden entries included, one level deep or the whole tree
/// with `--recursive` — rather than the directory's own record whose mtime moves. A path that
/// is not a directory is watched as the one entry it is.
fn file_tree_query(query: &Query) -> Query {
    let is_directory = query.selectors().iter().any(|selector| match selector {
        Selector::Field { name, value } if name == "path" => match value {
            Value::Path(path) => path.is_dir(),
            Value::String(text) => std::path::Path::new(text.as_ref()).is_dir(),
            _ => false,
        },
        _ => false,
    });
    if !is_directory {
        return query.clone();
    }
    let mut listing = Query::target("dir");
    for selector in query.selectors() {
        listing = listing.with(selector.clone());
    }
    for (name, value) in query.options() {
        listing = listing.option(name.clone(), value.clone());
    }
    listing = listing.option("all", Value::Bool(true));
    if let Some(max) = query.max() {
        listing = listing.limit(max);
    }
    listing
}

/// The event schema this watch emits, resolved from the contract's declared output.
fn event_schema_for(contract: &crate::CommandContract) -> Result<Arc<Schema>, ErrorValue> {
    let declared = contract
        .output()
        .schema_references()
        .first()
        .map(|reference| reference.to_string());
    declared
        .as_deref()
        .and_then(|reference| reference.parse::<SchemaId>().ok())
        .and_then(|id| ono_value::builtin_schemas().get(&id))
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!(
                    "`{}` declares {}, which this build does not carry yet",
                    contract.spelling(),
                    declared.unwrap_or_else(|| "no event schema".to_owned()),
                ),
            )
            .with_help(
                "the schema arrives with its phase; `docs/spec/schemas/deferred.yaml` \
                        says which",
            )
        })
}

/// The watch runtime of v0.2 §18.2, as a stream any caller can subscribe to (ADR-0024).
///
/// `watch <target>` is one caller; v0.4 §25.1's live map is another, and it must be the same
/// runtime rather than a second one beside it — v0.4 §2.16 forbids the spatial layer from
/// becoming an undocumented second source of system truth, and two loops asking the same
/// providers would disagree about when something happened.
///
/// The events carry the envelope of §31.14: `kind` in snapshot|added|changed|removed, `at`, the
/// object under the target's own field name, and `source` in subscription|poll. The stream is
/// unbounded and ends when the consumer stops listening.
///
/// # Errors
///
/// `resolve.target_not_found` where the build carries no `ono.<target>-event/1` contract, which
/// is how a caller learns that a target cannot be watched at all.
pub fn watch_events(
    providers: &ProviderRegistry,
    target: &str,
    query: Query,
    interval: Duration,
) -> Result<ValueStream, ErrorValue> {
    let id = SchemaId::new(&format!("ono.{target}-event"), 1);
    let schema = ono_value::builtin_schemas().get(&id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            format!("`{target}` has no event contract in this build, so it cannot be watched"),
        )
        .with_help("`ono.<target>-event/1` is what a watchable target declares (spec §18.2)")
    })?;
    Ok(watch_stream(
        providers.clone(),
        query,
        schema,
        target.to_owned(),
        interval,
    ))
}

/// Whether this build can watch `target` at all (spec §18.2).
#[must_use]
pub fn is_watchable(target: &str) -> bool {
    ono_value::builtin_schemas()
        .get(&SchemaId::new(&format!("ono.{target}-event"), 1))
        .is_some()
}

/// The watch runtime: a provider subscription where there is one, and the poll loop where
/// there is not (spec §18.2, ADR-0034, ADR-0235).
///
/// The subscription is opened *before* the first snapshot is taken, so a change that happens
/// while the snapshot is being read is queued rather than lost; the reconciliation below then
/// folds it into the state the snapshot established.
fn watch_stream(
    providers: ProviderRegistry,
    query: Query,
    event_schema: Arc<Schema>,
    object_field: String,
    interval: Duration,
) -> ValueStream {
    ValueStream::spawn(
        PipelineConfig::new(),
        Boundedness::Unbounded,
        move |sink| async move {
            match providers.subscribe(&query) {
                Ok(events) => {
                    subscribed(
                        &sink,
                        &providers,
                        &query,
                        events,
                        &event_schema,
                        &object_field,
                    )
                    .await;
                }
                // A provider that cannot be told about changes is asked instead. Its refusal is
                // not an error the user needs: `watch` works either way, and `source` says which
                // way it worked (spec §18.2).
                Err(_) => {
                    polled(
                        &sink,
                        &providers,
                        &query,
                        &event_schema,
                        &object_field,
                        interval,
                    )
                    .await;
                }
            }
        },
    )
}

/// Drives a provider subscription: the current state, then every change the provider reports.
///
/// A provider subscription reports what changed and nothing else — the snapshot is the runtime's,
/// so that `watch x | take 1` means the same thing whichever source is behind it (ADR-0024). The
/// state the snapshot established is kept, so an `added` for an object the snapshot already
/// carried is the change it really is, and a `removed` for one it never carried is dropped.
async fn subscribed(
    sink: &ono_pipeline::StreamSink,
    providers: &ProviderRegistry,
    query: &Query,
    mut events: EventStream,
    event_schema: &Arc<Schema>,
    object_field: &str,
) {
    let mut known: BTreeMap<String, Arc<RecordValue>> = BTreeMap::new();
    let collected = match providers.snapshot(query) {
        Ok(stream) => stream.collect().await,
        Err(error) => {
            let _ = sink.fail(error).await;
            events.cancel();
            return;
        }
    };
    for error in collected.errors() {
        if sink.fail(error.clone()).await.is_err() {
            events.cancel();
            return;
        }
    }
    for value in collected.into_values() {
        let Value::Record(record) = value else {
            continue;
        };
        known.insert(identity_key(&record), record);
    }
    if known.is_empty() {
        if sink
            .send(empty_snapshot(event_schema, SUBSCRIPTION))
            .await
            .is_err()
        {
            events.cancel();
            return;
        }
    } else {
        for record in known.values() {
            if sink
                .send(event(
                    "snapshot",
                    record,
                    None,
                    event_schema,
                    object_field,
                    SUBSCRIPTION,
                ))
                .await
                .is_err()
            {
                events.cancel();
                return;
            }
        }
    }

    while let Some(observed) = events.recv().await {
        let Some(record) = observed.value().cloned() else {
            continue;
        };
        let key = identity_key(&record);
        let emitted = match observed.kind() {
            // A provider subscription does not send the snapshot; if one arrives it is state,
            // not a change, and it belongs in `known` without being announced twice.
            EventKind::Snapshot => {
                known.insert(key, record);
                continue;
            }
            EventKind::Added | EventKind::Changed => match known.insert(key, Arc::clone(&record)) {
                Some(previous) if previous == record => continue,
                Some(previous) => event(
                    "changed",
                    &record,
                    Some(changed_fields(&previous, &record)),
                    event_schema,
                    object_field,
                    SUBSCRIPTION,
                ),
                None => event(
                    "added",
                    &record,
                    None,
                    event_schema,
                    object_field,
                    SUBSCRIPTION,
                ),
            },
            EventKind::Removed => {
                if known.remove(&key).is_none() {
                    continue;
                }
                event(
                    "removed",
                    &record,
                    None,
                    event_schema,
                    object_field,
                    SUBSCRIPTION,
                )
            }
        };
        if sink.send(emitted).await.is_err() {
            break;
        }
    }
    events.cancel();
}

/// The poll loop: snapshot, diff by identity, emit, sleep, repeat — until nobody listens.
async fn polled(
    sink: &ono_pipeline::StreamSink,
    providers: &ProviderRegistry,
    query: &Query,
    event_schema: &Arc<Schema>,
    object_field: &str,
    interval: Duration,
) {
    let mut known: BTreeMap<String, Arc<RecordValue>> = BTreeMap::new();
    let mut first = true;

    loop {
        let collected = match providers.snapshot(query) {
            Ok(stream) => stream.collect().await,
            Err(error) => {
                let _ = sink.fail(error).await;
                return;
            }
        };
        for error in collected.errors() {
            if sink.fail(error.clone()).await.is_err() {
                return;
            }
        }

        let mut seen: BTreeMap<String, Arc<RecordValue>> = BTreeMap::new();
        for value in collected.into_values() {
            let Value::Record(record) = value else {
                continue;
            };
            seen.insert(identity_key(&record), record);
        }

        // ADR-0024: the stream begins with the current state — and when that state is
        // "nothing", the snapshot is still an event, or `watch x | take 1` never returns.
        if first && seen.is_empty() && sink.send(empty_snapshot(event_schema, POLL)).await.is_err()
        {
            return;
        }

        // What changed, in identity order — deterministic however the provider answered.
        for (key, record) in &seen {
            let event = match known.get(key) {
                _ if first => event("snapshot", record, None, event_schema, object_field, POLL),
                None => event("added", record, None, event_schema, object_field, POLL),
                Some(previous) if previous != record => {
                    let moved = changed_fields(previous, record);
                    event(
                        "changed",
                        record,
                        Some(moved),
                        event_schema,
                        object_field,
                        POLL,
                    )
                }
                Some(_) => continue,
            };
            if sink.send(event).await.is_err() {
                return;
            }
        }
        for (key, record) in &known {
            if !seen.contains_key(key)
                && sink
                    .send(event(
                        "removed",
                        record,
                        None,
                        event_schema,
                        object_field,
                        POLL,
                    ))
                    .await
                    .is_err()
            {
                return;
            }
        }

        known = seen;
        first = false;
        tokio::time::sleep(interval).await;
        if sink.is_cancelled() {
            return;
        }
    }
}

/// One record's identity, rendered — the sameness rule of ADR-0024.
///
/// A schema with no declared identity keys every record by its whole rendered self, so nothing
/// is ever "the same object changing": values are appended, never updated in place.
fn identity_key(record: &Arc<RecordValue>) -> String {
    let identity = record.identity();
    if identity.is_empty() {
        format!("{record:?}")
    } else {
        identity.to_string()
    }
}

/// The field names whose values moved between two observations of one object.
fn changed_fields(previous: &Arc<RecordValue>, current: &Arc<RecordValue>) -> Vec<Value> {
    current
        .schema()
        .fields()
        .iter()
        .filter(|field| previous.get(field.name()) != current.get(field.name()))
        .map(|field| Value::string(field.name()))
        .collect()
}

/// The `source` of an event the runtime produced by asking again (spec §18.2).
const POLL: &str = "poll";

/// The `source` of an event a provider reported because the system told it (spec §18.2).
const SUBSCRIPTION: &str = "subscription";

/// The snapshot of a listing with nothing in it: `kind: snapshot` carrying no object.
fn empty_snapshot(schema: &Arc<Schema>, source: &str) -> Value {
    let provenance = Provenance::local("ono.runtime", schema.id().clone()).from_source("watch");
    let built = RecordValue::builder(Arc::clone(schema), provenance)
        .set("kind", Value::string("snapshot"))
        .and_then(|builder| builder.set("at", Value::Timestamp(jiff::Timestamp::now())))
        .and_then(|builder| builder.set("source", Value::string(source)));
    match built {
        Ok(builder) => Value::Record(Arc::new(builder.build())),
        Err(error) => error.into_value(),
    }
}

/// One event, in the envelope the event schema declares (spec §31.14).
fn event(
    kind: &str,
    record: &Arc<RecordValue>,
    changed: Option<Vec<Value>>,
    schema: &Arc<Schema>,
    object_field: &str,
    source: &str,
) -> Value {
    let provenance = record.provenance().clone();
    let built = RecordValue::builder(Arc::clone(schema), provenance)
        .set("kind", Value::string(kind))
        .and_then(|builder| builder.set("at", Value::Timestamp(jiff::Timestamp::now())))
        .and_then(|builder| builder.set(object_field, Value::Record(Arc::clone(record))))
        .and_then(|builder| builder.set("changed", changed.map_or(Value::Null, Value::list)))
        .and_then(|builder| builder.set("source", Value::string(source)));
    match built {
        Ok(builder) => Value::Record(Arc::new(builder.build())),
        Err(error) => error.into_value(),
    }
}
