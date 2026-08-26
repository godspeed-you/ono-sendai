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
use ono_provider_api::{ProviderRegistry, Query};
use ono_value::{ErrorValue, RecordValue, Schema, SchemaId, Value};

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
        let mut query = ctx.contract().query(ctx.arguments())?;
        for frame in ctx.context() {
            // A watch narrows inside a context exactly as `get` does (spec §14.3).
            if frame.kind() == crate::FrameKind::Object {
                query = query.with(super::producer::ambient_selector(
                    ctx.contract(),
                    ctx.providers(),
                    frame,
                )?);
            }
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

        // Spec §18.2: providers MAY support event-driven updates; this build's providers do not
        // yet, so the runtime polls and says so on every event. When a provider grows
        // `subscribe`, this is where the stream switches source.
        Ok(Outcome::Values(poll(
            ctx.providers().clone(),
            query,
            event_schema,
            interval,
        )))
    }
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

/// The poll loop: snapshot, diff by identity, emit, sleep, repeat — until nobody listens.
fn poll(
    providers: ProviderRegistry,
    query: Query,
    event_schema: Arc<Schema>,
    interval: Duration,
) -> ValueStream {
    ValueStream::spawn(
        PipelineConfig::new(),
        Boundedness::Unbounded,
        move |sink| async move {
            let mut known: BTreeMap<String, Arc<RecordValue>> = BTreeMap::new();
            let mut first = true;

            loop {
                let collected = match providers.snapshot(&query) {
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

                // What changed, in identity order — deterministic however the provider answered.
                for (key, record) in &seen {
                    let event = match known.get(key) {
                        _ if first => event("snapshot", record, None, &event_schema),
                        None => event("added", record, None, &event_schema),
                        Some(previous) if previous != record => {
                            let moved = changed_fields(previous, record);
                            event("changed", record, Some(moved), &event_schema)
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
                            .send(event("removed", record, None, &event_schema))
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
        },
    )
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

/// One event, in the envelope the event schema declares (spec §31.14).
fn event(
    kind: &str,
    record: &Arc<RecordValue>,
    changed: Option<Vec<Value>>,
    schema: &Arc<Schema>,
) -> Value {
    let provenance = record.provenance().clone();
    let built = RecordValue::builder(Arc::clone(schema), provenance)
        .set("kind", Value::string(kind))
        .and_then(|builder| builder.set("at", Value::Timestamp(jiff::Timestamp::now())))
        .and_then(|builder| builder.set("process", Value::Record(Arc::clone(record))))
        .and_then(|builder| builder.set("changed", changed.map_or(Value::Null, Value::list)))
        .and_then(|builder| builder.set("source", Value::string("poll")));
    match built {
        Ok(builder) => Value::Record(Arc::new(builder.build())),
        Err(error) => error.into_value(),
    }
}
