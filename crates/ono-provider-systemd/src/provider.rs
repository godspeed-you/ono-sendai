//! The `service` provider itself.

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectId, ObjectRef, Provider, Query, Risk,
    Selector,
};
use ono_value::{ErrorValue, SchemaId, Value};

use crate::record::{already_in_state, service_schema, unit_name_candidates, unit_record};
use crate::{BusError, JobKind, SystemdBus, UnitProperties};

/// The id this provider signs its records with, and the value of their `provider` field.
///
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
    fn parse(operation: &str) -> Option<Self> {
        match operation {
            "start" => Some(Operation::Job(JobKind::Start)),
            "stop" => Some(Operation::Job(JobKind::Stop)),
            "restart" => Some(Operation::Job(JobKind::Restart)),
            "reload" => Some(Operation::Job(JobKind::Reload)),
            "enable" => Some(Operation::SetEnabled(true)),
            "disable" => Some(Operation::SetEnabled(false)),
            _ => None,
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
            // `docs/spec/capabilities.yaml` gives `service.manage` elevation `required`: systemd
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

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let bus = self.bus()?;
        let plan = Plan::of(query);
        let limit = query.max();

        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let names = match &plan.named {
                    Some(name) => match load_unit(&bus, name).await {
                        Ok(Some(properties)) => vec![properties.name.clone()],
                        Ok(None) => Vec::new(),
                        Err(error) => {
                            let _ = sink.fail(error.into_error()).await;
                            return;
                        }
                    },
                    None => match bus.list_units().await {
                        Ok(listings) => listings.into_iter().map(|unit| unit.name).collect(),
                        Err(error) => {
                            let _ = sink.fail(error.into_error()).await;
                            return;
                        }
                    },
                };

                let mut emitted = 0usize;
                for name in names {
                    if limit.is_some_and(|limit| emitted >= limit) {
                        return;
                    }
                    let properties = match bus.unit_properties(&name).await {
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
        let Some(operation) = Operation::parse(action.operation()) else {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!(
                    "the systemd provider has no operation `{}`",
                    action.operation()
                ),
            )
            .with_help("it can start, stop, restart, reload, enable and disable a unit"));
        };
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
                    return Ok(ActionOutcome::succeeded(action, true));
                }
                match bus.queue_job(&properties.name, job).await {
                    Ok(()) => Ok(ActionOutcome::succeeded(action, true)),
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
                    return Ok(ActionOutcome::succeeded(action, true));
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
