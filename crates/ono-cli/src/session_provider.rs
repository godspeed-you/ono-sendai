//! The shell's own tables, answered as objects (spec §18.4, ADR-0090).
//!
//! `get job` returns structured job objects, and the job table lives nowhere a provider crate can
//! see: half of it is the executor's process groups (spec §18.1) and half is the session's
//! detached native pipelines (ADR-0024). So the session *publishes* what it knows — a plain row
//! per job, refreshed before every native pipeline runs — and this provider answers from the
//! published rows like any other provider answers from its system.
//!
//! The same seam carries the KUANG/11 tables (ADR-0107): the packages of spec §31.8 are the
//! plugin home on disk overlaid with the runtime instances this session started, and both live
//! in the [`Host`](crate::kuang_host::Host) the tables hold. The remote links of spec §21 and the
//! hosts they reach are session state in exactly the same way, and each becomes one more target
//! here with one more table in [`SessionTables`].

use std::sync::{Arc, Mutex};

use ono_core::{ErrorCode, ExitStatus};
use ono_pipeline::ValueStream;
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectRef, Provider, Query, Risk, Selector,
};
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value};

/// The provider's stable id, as it appears in every record's provenance.
pub const PROVIDER_ID: &str = "ono.shell";

/// One job as the session publishes it — the fields of `ono.job/1`, before they are a record.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    /// The job number, the `%1` of `fg %1`.
    pub number: u32,
    /// `external` for a process group, `native` for a detached pipeline.
    pub kind: &'static str,
    /// One of the `ono.job/1` states.
    pub state: &'static str,
    /// The pipeline as typed.
    pub command: String,
    /// The process group id; `None` for a native job.
    pub process_group: Option<u32>,
    /// The process ids in the job; `None` for a native job.
    pub pids: Option<Vec<u32>>,
    /// When the job was detached.
    pub started: Value,
    /// The status once the job finished; `None` while it runs or when a signal ended it.
    pub exit_status: Option<ExitStatus>,
}

/// What the session has published for the provider to answer from.
#[derive(Debug, Default)]
pub struct SessionTables {
    jobs: Vec<JobRow>,
    /// The KUANG/11 host: where packages are, and which of them run (ADR-0107).
    pub kuang: crate::kuang_host::Host,
}

impl SessionTables {
    /// Replaces the job table with what is true now.
    pub fn publish_jobs(&mut self, jobs: Vec<JobRow>) {
        self.jobs = jobs;
    }
}

/// The shell's tables, as a provider.
#[derive(Debug)]
pub struct SessionProvider {
    tables: Arc<Mutex<SessionTables>>,
}

impl SessionProvider {
    /// A provider answering from `tables`, which the session keeps current.
    #[must_use]
    pub fn new(tables: Arc<Mutex<SessionTables>>) -> Self {
        Self { tables }
    }

    fn schema() -> Result<Arc<Schema>, ErrorValue> {
        let id = SchemaId::new("ono.job", 1);
        ono_value::builtin_schemas().get(&id).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("{PROVIDER_ID} advertises {id} but no contract defines it"),
            )
        })
    }

    /// The schemas of every table this provider answers, by the target's name.
    fn schema_of(target: &str) -> Result<Arc<Schema>, ErrorValue> {
        match target {
            "job" => Self::schema(),
            "plugin" => crate::kuang_host::schema("ono.plugin"),
            other => Err(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("{PROVIDER_ID} has no table `{other}`"),
            )),
        }
    }

    /// The records of `target` as of now, and the per-object failures beside them.
    fn table(&self, target: &str) -> Result<(Vec<RecordValue>, Vec<ErrorValue>), ErrorValue> {
        match target {
            "job" => Ok((self.jobs()?, Vec::new())),
            "plugin" => self.lock().kuang.plugin_records(),
            other => Err(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("{PROVIDER_ID} has no table `{other}`"),
            )),
        }
    }

    /// The inspection of the one package the query selects (spec §31.33).
    ///
    /// What the record needs is gathered under the lock; the record itself is built by the
    /// stream's producer, because an unloaded package may have to be run through its
    /// handshake to learn what it contributes (ADR-0108 §3), and that is asynchronous.
    fn inspect(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let id = query
            .selectors()
            .iter()
            .find_map(|selector| match selector {
                Selector::Field { name, value } if name == "id" => value.as_str().ok(),
                _ => None,
            })
            .map(str::to_owned);
        let (package, management, instance) = {
            let tables = self.lock();
            let Some(package) = id
                .as_deref()
                .and_then(|id| tables.kuang.installed_package(id))
            else {
                return Ok(ValueStream::from_values([]));
            };
            let management = tables.kuang.management(&package.manifest.package.id);
            let instance = tables
                .kuang
                .instance(&package.manifest.package.id)
                .map(|instance| crate::kuang_host::Instance {
                    id: instance.id.clone(),
                    plugin: Arc::clone(&instance.plugin),
                    loaded_at: instance.loaded_at.clone(),
                });
            (package, management, instance)
        };
        Ok(ValueStream::spawn(
            ono_pipeline::PipelineConfig::new(),
            ono_pipeline::Boundedness::Bounded,
            move |sink| async move {
                use crate::kuang_host::{Contributions, discover, inspection_record};
                let declares_files = package.manifest.contributions.is_some();
                let (contributions, failure) = match &instance {
                    Some(instance) => (Contributions::of(&instance.plugin), None),
                    None if declares_files => (Contributions::default(), None),
                    None => match discover(&package).await {
                        Ok(contributions) => (contributions, None),
                        Err(error) => (Contributions::default(), Some(error)),
                    },
                };
                let record = inspection_record(
                    &package,
                    &management,
                    instance.as_ref(),
                    &contributions,
                    failure,
                );
                match record {
                    Ok(record) => {
                        let _ = sink.send(record.into_value()).await;
                    }
                    Err(error) => {
                        let _ = sink.fail(error).await;
                    }
                }
            },
        ))
    }

    /// The package id and version an action names.
    fn plugin_identity(action: &Action) -> Result<(String, String), ErrorValue> {
        let values = action.target().values();
        match (values.first(), values.get(1)) {
            (Some(Value::String(id)), Some(Value::String(version))) => {
                Ok((id.to_string(), version.to_string()))
            }
            _ => Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("{} is not a plugin identity", action.target()),
            )),
        }
    }

    /// Takes the instance of `id` out of the host and shuts it down (lifecycle.v1 `unload`).
    async fn unload_instance(&self, id: &str) -> bool {
        let instance = self.lock().kuang.remove_instance(id);
        match instance {
            Some(instance) => {
                instance
                    .plugin
                    .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
                    .await;
                true
            }
            None => false,
        }
    }

    /// `unload plugin` (lifecycle.v1 `unload`): the instance is shut down and its
    /// contributions are withdrawn; a package that is not loaded is left as it is.
    async fn unload_plugin(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let (id, _) = Self::plugin_identity(action)?;
        if self.lock().kuang.instance(&id).is_none() {
            return Ok(ActionOutcome::skipped(
                action,
                format!("`{id}` is not loaded"),
            ));
        }
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!("would unload `{id}`"),
            ));
        }
        Ok(ActionOutcome::succeeded(
            action,
            self.unload_instance(&id).await,
        ))
    }

    /// `set plugin --enabled … --background …` (spec §31.3, §31.38): management state on disk,
    /// with a disabled package unloaded first (lifecycle.v1 `disable`).
    async fn set_plugin(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let (id, _) = Self::plugin_identity(action)?;
        let mut management = self.lock().kuang.management(&id);
        let before = management.clone();
        if let Some(enabled) = action.argument("enabled") {
            management.enabled = enabled.as_bool()?;
        }
        if let Some(background) = action.argument("background") {
            management.background = background.as_bool()?;
        }
        if management == before {
            return Ok(ActionOutcome::skipped(
                action,
                format!("`{id}` already has these settings"),
            ));
        }
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!(
                    "would record enabled={} background={}",
                    management.enabled, management.background
                ),
            ));
        }
        if !management.enabled {
            self.unload_instance(&id).await;
        }
        self.lock().kuang.write_management(&id, &management)?;
        Ok(ActionOutcome::succeeded(action, true))
    }

    /// `remove plugin` (spec §31.81): a loaded instance is unloaded first, the directory is
    /// removed, and state is retained only when asked.
    async fn remove_plugin(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let (id, _) = Self::plugin_identity(action)?;
        let package = self.lock().kuang.installed_package(&id).ok_or_else(|| {
            ErrorValue::new(ErrorCode::IoNotFound, format!("`{id}` is not installed"))
        })?;
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!("would remove {}", package.directory.display()),
            ));
        }
        self.unload_instance(&id).await;
        let keep_state = action.argument("keep-state") == Some(&Value::Bool(true));
        self.lock().kuang.remove_package(&package, keep_state)?;
        Ok(ActionOutcome::succeeded(action, true))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SessionTables> {
        self.tables
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The job records as of the last publication, oldest first.
    fn jobs(&self) -> Result<Vec<RecordValue>, ErrorValue> {
        let schema = Self::schema()?;
        let tables = self
            .tables
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The current job is the one an unqualified reference means: the most recent (spec
        // §18.1, as `fg` without an argument reads it).
        let current = tables.jobs.iter().map(|job| job.number).max();
        tables
            .jobs
            .iter()
            .map(|job| record(job, current == Some(job.number), &schema))
            .collect()
    }
}

/// A bounded stream of `values`, with each per-object failure after them: a package that cannot
/// be read is one object's failure beside the others' records (spec §16.5).
fn stream_of(values: Vec<Value>, failures: Vec<ErrorValue>) -> ValueStream {
    if failures.is_empty() {
        return ValueStream::from_values(values);
    }
    ValueStream::spawn(
        ono_pipeline::PipelineConfig::new(),
        ono_pipeline::Boundedness::Bounded,
        move |sink| async move {
            for value in values {
                if sink.send(value).await.is_err() {
                    return;
                }
            }
            for failure in failures {
                if sink.fail(failure).await.is_err() {
                    return;
                }
            }
        },
    )
}

fn record(job: &JobRow, current: bool, schema: &Arc<Schema>) -> Result<RecordValue, ErrorValue> {
    let list_of = |pids: &[u32]| Value::list(pids.iter().map(|pid| Value::Int(i128::from(*pid))));
    Ok(RecordValue::builder(
        Arc::clone(schema),
        Provenance::local(PROVIDER_ID, schema.id().clone()),
    )
    .set("id", Value::Int(i128::from(job.number)))?
    .set("kind", Value::string(job.kind))?
    .set("state", Value::string(job.state))?
    .set("command", Value::string(&job.command))?
    .set("current", Value::Bool(current))?
    .set(
        "process_group",
        job.process_group
            .map_or(Value::Null, |group| Value::Int(i128::from(group))),
    )?
    .set("pids", job.pids.as_deref().map_or(Value::Null, list_of))?
    .set("started", job.started.clone())?
    .set(
        "exit_status",
        job.exit_status
            .map_or(Value::Null, |status| Value::Int(i128::from(status.code()))),
    )?
    .build())
}

#[async_trait::async_trait]
impl Provider for SessionProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["job", "plugin"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        Self::schema_of("job")
            .into_iter()
            .chain(
                ["ono.plugin", "ono.plugin-package", "ono.plugin-inspection"]
                    .into_iter()
                    .filter_map(|name| crate::kuang_host::schema(name).ok()),
            )
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("job.list", Risk::Read),
            Capability::new("plugin.list", Risk::Read),
            Capability::new("plugin.search", Risk::Read),
            Capability::new("plugin.inspect", Risk::Read),
            Capability::new("plugin.remove", Risk::Destructive),
            Capability::new("plugin.unload", Risk::Mutate),
            Capability::new("plugin.set", Risk::Mutate),
        ]
    }

    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let limit = query.max().unwrap_or(usize::MAX);
        if query.target_name() == "plugin" {
            // `find plugin <term>`: the search selector answers packages as their sources
            // describe them, not installed rows (ADR-0108 §4).
            let term = query
                .selectors()
                .iter()
                .find_map(|selector| match selector {
                    Selector::Field { name, value } if name == "query" => value.as_str().ok(),
                    _ => None,
                });
            if let Some(term) = term {
                let source = query
                    .option_value("source")
                    .and_then(|value| value.as_str().ok())
                    .map(str::to_owned);
                let (records, failures) =
                    self.lock().kuang.package_records(term, source.as_deref())?;
                return Ok(stream_of(
                    records
                        .into_iter()
                        .take(limit)
                        .map(RecordValue::into_value)
                        .collect(),
                    failures,
                ));
            }
            // `inspect plugin <id>`: the detail query answers the inspection record (ADR-0091,
            // ADR-0108 §3).
            if query.flag("detail") {
                return self.inspect(query);
            }
        }
        let (records, failures) = self.table(query.target_name())?;
        // `get plugin --state loaded`: the option is a filter on the state column (kuang.yaml).
        let state = query
            .option_value("state")
            .and_then(|value| value.as_str().ok())
            .map(str::to_owned);
        let values: Vec<Value> = records
            .into_iter()
            .filter(|record| query.matches(record))
            .filter(|record| {
                state.as_deref().is_none_or(|wanted| {
                    record.get("state").and_then(|value| value.as_str().ok()) == Some(wanted)
                })
            })
            .take(limit)
            .map(RecordValue::into_value)
            .collect();
        Ok(stream_of(values, failures))
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        match (action.target_name(), action.operation()) {
            ("plugin", "remove") => self.remove_plugin(action).await,
            ("plugin", "unload") => self.unload_plugin(action).await,
            ("plugin", "set") => self.set_plugin(action).await,
            (target, operation) => Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{PROVIDER_ID} does not `{operation}` a {target}"),
            )),
        }
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        // A selector carries no target; every table is asked, which is unambiguous because the
        // tables' identity fields are typed differently (a job's `id` is a number).
        let mut found = Vec::new();
        for target in ["job", "plugin"] {
            let (records, _) = self.table(target)?;
            found.extend(
                records
                    .iter()
                    .filter(|record| selector.matches(record))
                    .filter_map(ObjectRef::of),
            );
        }
        Ok(found)
    }
}
