//! The `service` provider itself.

use std::collections::BTreeMap;
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, EventSink, EventStream, ObjectEvent, ObjectId,
    ObjectRef, Provider, Query, Risk, Selector, TemporalCapabilities,
};
use ono_value::{ErrorValue, RecordValue, SchemaId, Value};

use crate::record::{already_in_state, service_schema, unit_name_candidates, unit_record};
use crate::{BusError, JobKind, JobRef, SystemdBus, UnitProperties, UnitSignal, unit_object_path};

/// The id this provider signs its records with, and the value of their `provider` field.
///
/// How many unit reads are in flight at once while enumerating (§33.2).
///
/// Bounded rather than unbounded: D-Bus multiplexes, but a machine with thousands of units should
/// not open thousands of concurrent calls, and §28.1 keeps every queue in this shell bounded. The
/// window is wide enough that the round-trip latency of one unit is hidden by the others and
/// narrow enough that the bus is never the thing under load.
const UNITS_IN_FLIGHT: usize = 32;

/// `ono.service/1` is identified by `provider + name` (spec §28.3), because a machine can run
/// more than one service manager and a unit name alone does not say which one answered. Spec
/// §33.4 names this one `systemd` in a link's provider list, so that is what it is called here.
pub const PROVIDER_ID: &str = "systemd";

/// Whether a service manager could be reached, and what to ask if it could.
#[derive(Debug)]
enum Backing {
    Ready(Arc<dyn SystemdBus>),
    Missing(String),
}

/// The systemd service provider: `ono.service/1` records read over D-Bus.
///
/// It never runs `systemctl` and never parses its output (spec §23.3, §50). Where no service
/// manager answers — a container, a WSL session, a machine using another init — it reports
/// [`Availability::Unavailable`] with the reason, because an empty result would be
/// indistinguishable from a machine that genuinely has no services (spec §10.5, §35.3).
///
/// ```
/// use ono_provider_api::Provider;
///
/// let runtime = tokio::runtime::Builder::new_current_thread()
///     .enable_all()
///     .build()
///     .unwrap();
/// runtime.block_on(async {
///     let provider = ono_provider_systemd::SystemdProvider::connect().await;
///     // On a machine with no service manager the provider says so, and says why.
///     if let Some(reason) = provider.availability().reason() {
///         assert!(reason.contains("D-Bus"));
///     }
/// });
/// ```
#[derive(Debug)]
pub struct SystemdProvider {
    backing: Backing,
}

impl SystemdProvider {
    /// Connects to the D-Bus system bus and probes `org.freedesktop.systemd1.Manager`.
    ///
    /// Never fails: being unable to reach a service manager is a *state* of this provider, not
    /// an error of construction, and it is reported through [`Provider::availability`].
    ///
    /// Detection is the socket plus a successful `Manager` property read. It is deliberately not
    /// the presence of a `systemctl` binary, which says nothing about whether systemd is pid 1
    /// here — the exact mistake that makes a provider return an empty list inside a container.
    pub async fn connect() -> Self {
        match crate::dbus::SystemBus::connect().await {
            Ok(bus) => Self::over(Arc::new(bus)).await,
            Err(error) => Self {
                backing: Backing::Missing(error.message().to_owned()),
            },
        }
    }

    /// A provider over any implementation of the systemd D-Bus surface, probed the same way.
    pub async fn over(bus: Arc<dyn SystemdBus>) -> Self {
        let backing = match bus.manager_version().await {
            Ok(_) => Backing::Ready(bus),
            Err(error) => Backing::Missing(error.message().to_owned()),
        };
        Self { backing }
    }

    fn bus(&self) -> Result<Arc<dyn SystemdBus>, ErrorValue> {
        match &self.backing {
            Backing::Ready(bus) => Ok(Arc::clone(bus)),
            Backing::Missing(reason) => Err(ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("no systemd service manager answers here — {reason}"),
            )
            .with_help(
                "`service` needs a running service manager. Having none is not the same as \
                 having no services, so this is a refusal to answer rather than an empty answer.",
            )),
        }
    }
}

/// How a query is answered: by asking systemd for named units, or by enumerating them.
struct Plan {
    /// The units to ask for by name, or `None` to list every unit.
    named: Option<String>,
    /// The selectors still to apply to each record, once it has been read.
    remaining: Vec<Selector>,
}

impl Plan {
    /// Splits a query into what systemd can be asked directly and what has to be filtered.
    fn of(query: &Query) -> Self {
        let mut named = None;
        let mut remaining = Vec::new();
        for selector in query.selectors() {
            match selector {
                Selector::Field { name, value } if name == "name" && named.is_none() => {
                    match value.as_str() {
                        Ok(text) => named = Some(text.to_owned()),
                        Err(_) => remaining.push(selector.clone()),
                    }
                }
                Selector::Identity(id) if named.is_none() => match unit_name(id) {
                    Ok(name) => named = Some(name),
                    Err(_) => remaining.push(selector.clone()),
                },
                other => remaining.push(other.clone()),
            }
        }
        Self { named, remaining }
    }

    fn keeps(&self, record: &ono_value::RecordValue) -> bool {
        self.remaining
            .iter()
            .all(|selector| selector.matches(record))
    }
}

/// The unit name an identity refers to.
///
/// `ono.service/1` is identified by `provider + name`, so the name is the second value.
fn unit_name(id: &ObjectId) -> Result<String, ErrorValue> {
    let expected = SchemaId::new("ono.service", 1);
    let name = id
        .values()
        .get(1)
        .and_then(|value| value.as_str().ok())
        .filter(|_| id.schema() == &expected);
    name.map(ToOwned::to_owned).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            format!("`{id}` does not name a systemd unit"),
        )
        .with_help("a service action needs an `ono.service/1` identity of `provider` and `name`")
    })
}

/// Asks systemd for a unit, trying the suffix a user left off.
///
/// Only the last spelling's failure is the caller's answer. systemd rejects a name with no unit
/// suffix as `org.freedesktop.DBus.Error.InvalidArgs` rather than as "no such unit", so a
/// provider that treated the first candidate's failure as fatal could never answer
/// `get service nginx` at all.
async fn load_unit(
    bus: &Arc<dyn SystemdBus>,
    name: &str,
) -> Result<Option<UnitProperties>, BusError> {
    let candidates = unit_name_candidates(name);
    for (index, candidate) in candidates.iter().enumerate() {
        let is_last_spelling = index + 1 == candidates.len();
        match bus.unit_properties(candidate).await {
            // `LoadUnit` answers for a name it has never heard of with a stub whose `LoadState`
            // is `not-found`. Taking that at face value would report a service that is not
            // there — a fabricated object, which is worse than no answer (spec section 35.3).
            Ok(Some(properties)) if properties.load_state.as_deref() == Some("not-found") => {}
            Ok(Some(properties)) => return Ok(Some(properties)),
            // A unit systemd does not know is not an error while spellings remain.
            Ok(None) | Err(BusError::NoSuchUnit(_)) => {}
            Err(_) if !is_last_spelling => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

/// What a `service` action asks for.
#[derive(Debug, Clone, Copy)]
enum Operation {
    /// A job queued through the `Manager` interface.
    Job(JobKind),
    /// A change to the unit files, through `EnableUnitFiles` or `DisableUnitFiles`.
    SetEnabled(bool),
}

impl Operation {
    /// What `action` asks for, or the reason it asks for nothing this provider does.
    ///
    /// `set` is the property form of `service.yaml`'s `ono.service.set`: the property travels as
    /// an argument, and `--enabled` is the one persistent property a unit has (ADR-0084).
    fn of(action: &Action) -> Result<Self, ErrorValue> {
        let unsupported = |message: String, help: &str| {
            Err(ErrorValue::new(ErrorCode::ProviderUnsupported, message).with_help(help))
        };
        match action.operation() {
            "start" => Ok(Operation::Job(JobKind::Start)),
            "stop" => Ok(Operation::Job(JobKind::Stop)),
            "restart" => Ok(Operation::Job(JobKind::Restart)),
            "reload" => Ok(Operation::Job(JobKind::Reload)),
            "enable" => Ok(Operation::SetEnabled(true)),
            "disable" => Ok(Operation::SetEnabled(false)),
            "set" => match action.argument("enabled") {
                Some(Value::Bool(wanted)) => Ok(Operation::SetEnabled(*wanted)),
                Some(other) => unsupported(
                    format!(
                        "`enabled` is whether the unit starts at boot, a bool, not a {}",
                        other.type_name()
                    ),
                    "write `--enabled true` or `--enabled false`",
                ),
                None => unsupported(
                    "the systemd provider changes one persistent property, `enabled`, and \
                     `set` named none"
                        .to_owned(),
                    "write `--enabled true` or `--enabled false`",
                ),
            },
            other => unsupported(
                format!("the systemd provider has no operation `{other}`"),
                "it can start, stop, restart, reload, enable and disable a unit, and set \
                 `--enabled`",
            ),
        }
    }
}

#[async_trait::async_trait]
impl Provider for SystemdProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["service"]
    }

    fn schemas(&self) -> Vec<Arc<ono_value::Schema>> {
        vec![service_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("service.list", Risk::Read),
            // `docs/contracts/capabilities.yaml` gives `service.manage` elevation `required`: systemd
            // asks polkit before it changes a unit.
            Capability::new("service.manage", Risk::Mutate).needing_elevation(),
        ]
    }

    fn availability(&self) -> Availability {
        match &self.backing {
            Backing::Ready(_) => Availability::Available,
            Backing::Missing(reason) => Availability::unavailable(reason.clone()),
        }
    }

    /// `watch service`, through `Manager.Subscribe` and the signals that follow (§22.2).
    ///
    /// The subscription is opened before the baseline is read, so a transition that happens
    /// while the baseline is being read is queued rather than lost. Each signal names a unit;
    /// the unit is read again and compared, because systemd coalesces `PropertiesChanged` and
    /// sends property *names* rather than a complete state.
    fn subscribe(&self, query: &Query) -> Result<EventStream, ErrorValue> {
        let bus = self.bus()?;
        let plan = Plan::of(query);
        Ok(EventStream::spawn(PipelineConfig::new(), move |sink| {
            watch_units(bus, plan, sink)
        }))
    }

    fn temporal(&self) -> TemporalCapabilities {
        TemporalCapabilities {
            current_snapshot: true,
            // §21.3: the source pushes. `Manager.Subscribe` turns systemd's own `JobNew`,
            // `JobRemoved`, `UnitNew`, `UnitRemoved` and per-unit `PropertiesChanged` signals
            // into events, so a transition arrives because systemd said so rather than because
            // something asked again (§22.2).
            live_events: true,
            historical_query: false,
            // §21.5: systemd coalesces `PropertiesChanged` and sends property names rather than
            // values, so two transitions within one coalescing window arrive as one. A sequence
            // with that property cannot carry an absence claim.
            exhaustive_events: false,
            // A queued job answers with its object path, and a unit read while a job is in
            // flight names it. Both are transaction identities in the sense of §21.6, and they
            // are what §15.2 admits as evidence for `caused_by`.
            causal_tokens: true,
            checkpointable: true,
            retained_history: None,
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let bus = self.bus()?;
        let plan = Plan::of(query);
        let limit = query.max();

        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                // Each unit, and where it is when the listing already said: `ListUnits`
                // answers with the object path, so asking `LoadUnit` for it again is a round
                // trip per unit that buys nothing (ADR-0561).
                let units: Vec<(String, Option<String>)> = match &plan.named {
                    Some(name) => match load_unit(&bus, name).await {
                        Ok(Some(properties)) => vec![(properties.name.clone(), None)],
                        Ok(None) => Vec::new(),
                        Err(error) => {
                            let _ = sink.fail(error.into_error()).await;
                            return;
                        }
                    },
                    None => match bus.list_units().await {
                        Ok(listings) => listings
                            .into_iter()
                            // `ListUnits` enumerates a `not-found` stub for as long as some other
                            // unit references a name whose file is gone, and the loop below
                            // refuses one as a fabricated object. Dropping the stubs here rather
                            // than there is what makes the population the count of what would
                            // really be answered (§34.4, ADR-0576).
                            .filter(|unit| unit.load_state.as_deref() != Some("not-found"))
                            .map(|unit| (unit.name, unit.path))
                            .collect(),
                        Err(error) => {
                            let _ = sink.fail(error.into_error()).await;
                            return;
                        }
                    },
                };

                // §34.4 and §2.17: a bounded answer states how much it left out, so an orientation
                // that reads twenty units of six hundred still shows six hundred. `ListUnits` is
                // one round trip and has already happened, so the figure is free; the per-unit
                // property reads below are what the bound is for (ADR-0576).
                sink.diagnostics()
                    .record_population(units.len().try_into().unwrap_or(u64::MAX));

                // One unit costs three D-Bus round trips — `LoadUnit`, then `GetAll` for the
                // Unit and Service interfaces — and a machine has hundreds of units. Read
                // sequentially that is over a second of pure latency before the first record,
                // which is what made `look` inside COMPUTE cost 0.9 s whatever the host was
                // running (v0.4.1 §33.2). The reads are independent and D-Bus multiplexes them,
                // so they are issued in a bounded window and consumed in order: the emission
                // order is still `ListUnits` order, and a slow unit no longer delays the ones
                // behind it.
                use futures::StreamExt as _;
                let reads = futures::stream::iter(units.into_iter().map(|(name, path)| {
                    let bus = Arc::clone(&bus);
                    async move {
                        match path {
                            Some(path) => bus.unit_properties_at(&name, &path).await,
                            None => bus.unit_properties(&name).await,
                        }
                    }
                }))
                .buffered(UNITS_IN_FLIGHT);
                futures::pin_mut!(reads);

                let mut emitted = 0usize;
                while let Some(read) = reads.next().await {
                    if limit.is_some_and(|limit| emitted >= limit) {
                        return;
                    }
                    let properties = match read {
                        // `ListUnits` enumerates a `not-found` stub for as long as some other
                        // unit references a name whose file is gone. The by-name path refuses
                        // such stubs as fabricated objects, and the listing must agree with it
                        // — a unit the enumeration reports and a by-name query then denies is
                        // the disagreement that made the CI round trip flaky.
                        Ok(Some(properties))
                            if properties.load_state.as_deref() == Some("not-found") =>
                        {
                            continue;
                        }
                        Ok(Some(properties)) => properties,
                        // A unit that went away between the listing and the read is not a
                        // failure; it is what a snapshot of a moving system looks like.
                        Ok(None) => continue,
                        Err(error) => {
                            // One unreadable unit must not cost the others (spec §16.5).
                            if sink.fail(error.into_error()).await.is_err() {
                                return;
                            }
                            continue;
                        }
                    };
                    match unit_record(&properties) {
                        Ok(record) => {
                            if !plan.keeps(&record) {
                                continue;
                            }
                            emitted += 1;
                            if sink.send(record.into_value()).await.is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            if sink.fail(error).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let query = Query::target("service").with(selector.clone());
        let collected = self.snapshot(&query)?.collect().await;
        Ok(collected
            .values()
            .iter()
            .filter_map(|value| match value {
                Value::Record(record) => ObjectRef::of(record),
                _ => None,
            })
            .collect())
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let bus = self.bus()?;
        let operation = Operation::of(action)?;
        let name = unit_name(action.target())?;

        let properties = match load_unit(&bus, &name).await {
            Ok(Some(properties)) => properties,
            Ok(None) => {
                return Ok(ActionOutcome::failed(
                    action,
                    ErrorValue::new(
                        ErrorCode::IoNotFound,
                        format!("systemd knows no unit `{name}`"),
                    ),
                ));
            }
            Err(error) => return Ok(ActionOutcome::failed(action, error.into_error())),
        };

        match operation {
            Operation::Job(job) => {
                if let Some(why) = already_in_state(&properties, job) {
                    return Ok(ActionOutcome::skipped(action, why));
                }
                if action.is_dry_run() {
                    return Ok(ActionOutcome::skipped(
                        action,
                        format!("would queue `{job:?}` for `{}`", properties.name),
                    ));
                }
                match bus.queue_job(&properties.name, job).await {
                    // §17.3: the job path is the service manager's own transaction identity, and
                    // the outcome is the only thing this call produces, so it is where the
                    // mapping from Ono's `ActionId` to systemd's job has to be handed up. A
                    // refused job produces no job, and carries none.
                    Ok(queued) => Ok(job_outcome(action, &queued)),
                    Err(error) => Ok(ActionOutcome::failed(action, error.into_error())),
                }
            }
            Operation::SetEnabled(wanted) => {
                let target = if wanted { "enabled" } else { "disabled" };
                if properties.unit_file_state.as_deref() == Some(target) {
                    return Ok(ActionOutcome::skipped(
                        action,
                        format!("`{}` is already {target}", properties.name),
                    ));
                }
                if action.is_dry_run() {
                    return Ok(ActionOutcome::skipped(
                        action,
                        format!("would set `{}` to {target}", properties.name),
                    ));
                }
                match bus.set_unit_file_enabled(&properties.name, wanted).await {
                    Ok(true) => Ok(ActionOutcome::succeeded(action, true)),
                    Ok(false) => Ok(ActionOutcome::skipped(
                        action,
                        format!(
                            "systemd listed no unit-file change for `{}`",
                            properties.name
                        ),
                    )),
                    Err(error) => Ok(ActionOutcome::failed(action, error.into_error())),
                }
            }
        }
    }
}

/// The outcome of a queued job, carrying the identity systemd gave it.
///
/// `systemd.job` is the object path verbatim — the token spec §17.3's example writes as
/// `ActionId ono:a91f -> systemd job /org/freedesktop/systemd1/job/4821` — and `systemd.job_id`
/// is the number in it, so a consumer that correlates a `JobRemoved` signal by id does not have
/// to re-parse a path. Where systemd answered with a path that carries no number, the id is
/// absent rather than invented (spec §35.3).
fn job_outcome(action: &Action, queued: &JobRef) -> ActionOutcome {
    let outcome = ActionOutcome::succeeded(action, true)
        .with_metadata("systemd.job", Value::string(&queued.path));
    match queued.id {
        Some(id) => outcome.with_metadata("systemd.job_id", Value::Int(i128::from(id))),
        None => outcome,
    }
}

/// Drives one subscription: the manager's signals, as object events about units.
///
/// The shape is fixed by what systemd sends. `JobNew` and `JobRemoved` carry the job identity
/// §15.2 wants and no state, so they are remembered rather than emitted; `PropertiesChanged`
/// carries a unit path and the *names* of what moved, so the unit is read again and compared.
/// A transition observed while a job is in flight for that unit is attributed to the job — which
/// is exactly the evidence `ono.systemd-job-to-unit-state` joins on, and it is systemd's claim
/// rather than an inference from two things happening near each other.
async fn watch_units(bus: Arc<dyn SystemdBus>, plan: Plan, sink: EventSink) {
    use futures::StreamExt as _;

    // Subscribed first, so a transition during the baseline read is queued rather than lost.
    let Ok(mut signals) = bus.subscribe_units().await else {
        // §18.2: a provider that cannot be told about changes is polled instead, and the watch
        // runtime does that when this stream ends without having said anything.
        return;
    };
    // The one unit a narrowed subscription is about, under the spelling systemd knows it by:
    // `watch service nginx` is about `nginx.service`, and a signal for another unit is not an
    // answer to it. `None` watches every unit.
    let watched = match &plan.named {
        Some(name) => Some(match load_unit(&bus, name).await {
            Ok(Some(properties)) => properties.name,
            _ => name.clone(),
        }),
        None => None,
    };
    // §31.14 and the trait's own contract: a subscription begins with the current state, so a
    // consumer never has to reconstruct it and a change is a change against something real.
    let Some(mut known) = baseline(&bus, watched.as_deref(), &plan, &sink).await else {
        return;
    };
    // The jobs in flight, by unit. systemd tells us when one starts and when it ends, so the map
    // holds exactly the interval `ono.systemd-job-to-unit-state` bounds a transition by.
    let mut jobs: BTreeMap<String, JobRef> = BTreeMap::new();

    while let Some(signal) = signals.next().await {
        let unit = signal.unit().to_owned();
        if watched.as_deref().is_some_and(|watched| watched != unit) {
            continue;
        }
        match signal {
            UnitSignal::JobNew { ref job, .. } => {
                jobs.insert(unit, job.clone());
                continue;
            }
            // `UnitNew` and `UnitRemoved` are the manager's own memory management: systemd loads
            // a unit when something asks about it and garbage-collects it again when nothing
            // does. Neither says the service appeared or went away, and reading the unit back on
            // either loads it, which makes the manager announce it, which reads it again — a
            // reader turned into a writer, observed against a live manager on 2026-09-08. So
            // they are decoded, and they move nothing but the baseline: §6.3 forbids
            // manufacturing a disappearance, and a unit nobody was asking about is the clearest
            // case of one that has not disappeared.
            UnitSignal::UnitRemoved { .. } => {
                known.remove(&unit);
                continue;
            }
            UnitSignal::UnitNew { .. } => continue,
            UnitSignal::JobRemoved { .. } | UnitSignal::UnitChanged { .. } => {}
        }

        // Always read at the object path rather than through `LoadUnit`, for the same reason.
        // `JobRemoved` names no path, and the path a unit of that name has is systemd's own
        // encoding of it rather than something to ask for.
        let path = signal
            .path()
            .map_or_else(|| unit_object_path(&unit), str::to_owned);
        let read = bus.unit_properties_at(&unit, &path).await;
        // A unit that could not be read this time is not a change; the next signal asks again.
        let Ok(Some(properties)) = read else {
            continue;
        };
        if properties.load_state.as_deref() == Some("not-found") {
            continue;
        }
        let Ok(record) = unit_record(&properties) else {
            continue;
        };
        if !plan.keeps(&record) {
            continue;
        }

        // The job in flight for this unit, and — where the signal is the job's own removal —
        // the job the removal named. `JobRemoved` arrives after the transition it completes, so
        // the identity has to survive being taken out of the map.
        let cause = match &signal {
            UnitSignal::JobRemoved { job, .. } => {
                jobs.remove(&unit);
                Some(job.clone())
            }
            _ => jobs.get(&unit).cloned(),
        };

        let mut event = match known.get(&unit) {
            Some(previous) => {
                // Compared field by field rather than record by record: every read carries its
                // own observation instant in its provenance, so two identical states are two
                // unequal records and a coalesced signal would otherwise become a change.
                let fields = moved(previous, &record);
                if fields.is_empty() {
                    continue;
                }
                ObjectEvent::changed(&record, fields)
            }
            None => ObjectEvent::added(&record),
        };
        // §3.3: the source's own instant, where the source states one. `StateChangeTimestamp`
        // is when systemd recorded the unit moving; the read-back's clock is not.
        if let Some(at) = state_change_instant(&properties) {
            event = event.with_observed_at(at);
        }
        if let Some(job) = cause {
            event = event.with_cause(job.token());
        }
        known.insert(unit, record);
        if sink.send(event).await.is_err() {
            return;
        }
    }
}

/// The units the query keeps, as they are before the first signal.
///
/// A change is measured against something, and against nothing every unit looks new. The read is
/// bounded the way the enumeration is, and a unit that could not be read is simply not part of
/// the baseline: its first signal then reports it as appearing, which is the honest reading of
/// "this is the first state of it I could see".
async fn baseline(
    bus: &Arc<dyn SystemdBus>,
    watched: Option<&str>,
    plan: &Plan,
    sink: &EventSink,
) -> Option<BTreeMap<String, RecordValue>> {
    use futures::StreamExt as _;

    let units: Vec<(String, Option<String>)> = match watched {
        Some(name) => vec![(name.to_owned(), None)],
        None => match bus.list_units().await {
            Ok(listings) => listings
                .into_iter()
                .filter(|unit| unit.load_state.as_deref() != Some("not-found"))
                .map(|unit| (unit.name, unit.path))
                .collect(),
            Err(_) => Vec::new(),
        },
    };
    let reads = futures::stream::iter(units.into_iter().map(|(name, path)| {
        let bus = Arc::clone(bus);
        async move {
            let read = match path {
                Some(path) => bus.unit_properties_at(&name, &path).await,
                None => bus.unit_properties(&name).await,
            };
            (name, read)
        }
    }))
    .buffered(UNITS_IN_FLIGHT);
    futures::pin_mut!(reads);

    let mut known = BTreeMap::new();
    while let Some((name, read)) = reads.next().await {
        let Ok(Some(properties)) = read else {
            continue;
        };
        if properties.load_state.as_deref() == Some("not-found") {
            continue;
        }
        let Ok(record) = unit_record(&properties) else {
            continue;
        };
        if !plan.keeps(&record) {
            continue;
        }
        let mut event = ObjectEvent::snapshot(&record);
        if let Some(at) = state_change_instant(&properties) {
            event = event.with_observed_at(at);
        }
        if sink.send(event).await.is_err() {
            return None;
        }
        known.insert(name, record);
    }
    Some(known)
}

/// The fields whose values moved between two observations of one unit.
fn moved(previous: &RecordValue, current: &RecordValue) -> Vec<String> {
    current
        .schema()
        .fields()
        .iter()
        .filter(|field| previous.get(field.name()) != current.get(field.name()))
        .map(|field| field.name().to_owned())
        .collect()
}

/// `StateChangeTimestamp` as an instant: systemd's own record of when the unit last moved.
///
/// Zero is systemd's way of saying a unit has never changed state, and a microsecond count that
/// does not fit an instant is not one — both are absent rather than converted into the epoch
/// (spec §35.3).
fn state_change_instant(properties: &UnitProperties) -> Option<jiff::Timestamp> {
    let usec = properties.state_change_usec.filter(|usec| *usec > 0)?;
    jiff::Timestamp::from_microsecond(i64::try_from(usec).ok()?).ok()
}
