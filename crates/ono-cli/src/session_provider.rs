//! The shell's own tables, answered as objects (spec §18.4, §21; ADR-0090, ADR-0103).
//!
//! `get job` returns structured job objects, and the job table lives nowhere a provider crate can
//! see: half of it is the executor's process groups (spec §18.1) and half is the session's
//! detached native pipelines (ADR-0024). So the session *publishes* what it knows — a plain row
//! per job, refreshed before every native pipeline runs — and this provider answers from the
//! published rows like any other provider answers from its system.
//!
//! The same seam carries the session's other tables: the remote links of spec §21 are session
//! state in exactly the same way, published as one row per link, and the hosts of spec §9.1 are
//! the hosts those links reach together with what the configured host sources list
//! (`crate::hosts`), read when asked.

use std::sync::{Arc, Mutex};

use ono_core::{ErrorCode, ExitStatus};
use ono_pipeline::ValueStream;
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectRef, Provider, Query, Risk, Selector,
};
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value};

use crate::hosts::{HostEntry, HostSources};

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

/// One link as the session publishes it — the fields of `ono.link/1`, before they are a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRow {
    /// The link's name, as the user gave it.
    pub name: String,
    /// The host the link points at.
    pub host: String,
    /// `ssh` or `local`.
    pub transport: String,
    /// Whether the agentless fallback of spec §21.3 was asked for.
    pub agentless: bool,
    /// One of the `ono.link/1` states.
    pub state: &'static str,
    /// The targets the remote negotiated; empty until it did.
    pub targets: Vec<String>,
    /// The link protocol version the handshake settled on.
    pub protocol: Option<u16>,
    /// The ids of the providers the remote offers.
    pub providers: Option<Vec<String>>,
}

/// What the session has published for the provider to answer from.
#[derive(Debug, Default)]
pub struct SessionTables {
    jobs: Vec<JobRow>,
    links: Vec<LinkRow>,
}

impl SessionTables {
    /// Replaces the job table with what is true now.
    pub fn publish_jobs(&mut self, jobs: Vec<JobRow>) {
        self.jobs = jobs;
    }

    /// Replaces the link table with what is true now.
    pub fn publish_links(&mut self, links: Vec<LinkRow>) {
        self.links = links;
    }
}

/// The shell's tables, as a provider.
#[derive(Debug)]
pub struct SessionProvider {
    tables: Arc<Mutex<SessionTables>>,
    sources: HostSources,
}

impl SessionProvider {
    /// A provider answering from `tables`, which the session keeps current, and from the host
    /// sources of `sources`, read when asked.
    #[must_use]
    pub fn new(tables: Arc<Mutex<SessionTables>>, sources: HostSources) -> Self {
        Self { tables, sources }
    }

    fn schema(name: &str) -> Result<Arc<Schema>, ErrorValue> {
        let id = SchemaId::new(name, 1);
        ono_value::builtin_schemas().get(&id).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("{PROVIDER_ID} advertises {id} but no contract defines it"),
            )
        })
    }

    /// The job records as of the last publication, oldest first.
    fn jobs(&self) -> Result<Vec<RecordValue>, ErrorValue> {
        let schema = Self::schema("ono.job")?;
        let tables = self.lock();
        // The current job is the one an unqualified reference means: the most recent (spec
        // §18.1, as `fg` without an argument reads it).
        let current = tables.jobs.iter().map(|job| job.number).max();
        tables
            .jobs
            .iter()
            .map(|job| job_record(job, current == Some(job.number), &schema))
            .collect()
    }

    /// The link records as of the last publication, oldest first.
    fn links(&self) -> Result<Vec<RecordValue>, ErrorValue> {
        let schema = Self::schema("ono.link")?;
        self.lock()
            .links
            .iter()
            .map(|link| link_record(link, &schema))
            .collect()
    }

    /// The host records: every source's entries, one record per name, in the order the
    /// sources are consulted — the shell's own file, the OpenSSH configuration, the links held.
    /// A source that cannot be read is reported on the stream's failure channel and the other
    /// sources still answer (spec §16.5).
    fn hosts(
        &self,
        only_source: Option<&str>,
    ) -> Result<(Vec<RecordValue>, Vec<ErrorValue>), ErrorValue> {
        let schema = Self::schema("ono.host")?;
        let links = self.lock().links.clone();
        let mut failures = Vec::new();
        let mut entries: Vec<(HostEntry, &'static str)> = Vec::new();
        let mut consult = |source: &'static str, read: Result<Vec<HostEntry>, ErrorValue>| {
            if only_source.is_some_and(|wanted| wanted != source) {
                return;
            }
            match read {
                Ok(found) => entries.extend(found.into_iter().map(|entry| (entry, source))),
                Err(error) => failures.push(error),
            }
        };
        consult("ono", self.sources.own_hosts());
        consult("ssh-config", self.sources.ssh_hosts());
        consult(
            "link",
            Ok(links
                .iter()
                .map(|link| HostEntry::named(&link.host))
                .collect()),
        );

        let mut records: Vec<RecordValue> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for (entry, source) in entries {
            if seen.contains(&entry.name) {
                continue;
            }
            seen.push(entry.name.clone());
            let held = links.iter().find(|link| link.host == entry.name);
            records.push(host_record(&entry, source, held, &schema)?);
        }
        Ok((records, failures))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SessionTables> {
        self.tables
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn job_record(
    job: &JobRow,
    current: bool,
    schema: &Arc<Schema>,
) -> Result<RecordValue, ErrorValue> {
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

/// The `ono.link/1` record of one published row, for a command that answers with the link it
/// just made (`connect host`, ADR-0104).
pub fn link_value(link: &LinkRow) -> Result<Value, ErrorValue> {
    let schema = SessionProvider::schema("ono.link")?;
    link_record(link, &schema).map(RecordValue::into_value)
}

fn link_record(link: &LinkRow, schema: &Arc<Schema>) -> Result<RecordValue, ErrorValue> {
    let strings = |items: &[String]| Value::list(items.iter().map(|item| Value::string(item)));
    Ok(RecordValue::builder(
        Arc::clone(schema),
        Provenance::local(PROVIDER_ID, schema.id().clone()),
    )
    .set("name", Value::string(&link.name))?
    .set("host", Value::string(&link.host))?
    .set("transport", Value::string(&link.transport))?
    .set(
        "mode",
        Value::string(if link.agentless { "agentless" } else { "agent" }),
    )?
    .set("state", Value::string(link.state))?
    .set("targets", strings(&link.targets))?
    .set(
        "protocol",
        link.protocol
            .map_or(Value::Null, |version| Value::Int(i128::from(version))),
    )?
    .set(
        "providers",
        link.providers.as_deref().map_or(Value::Null, strings),
    )?
    .build())
}

fn host_record(
    entry: &HostEntry,
    source: &str,
    held: Option<&LinkRow>,
    schema: &Arc<Schema>,
) -> Result<RecordValue, ErrorValue> {
    let text = |value: Option<&String>| value.map_or(Value::Null, |text| Value::string(text));
    Ok(RecordValue::builder(
        Arc::clone(schema),
        Provenance::local(PROVIDER_ID, schema.id().clone()),
    )
    .set("name", Value::string(&entry.name))?
    .set("address", text(entry.address.as_ref()))?
    .set("port", entry.port.map_or(Value::Null, Value::Port))?
    .set("user", text(entry.user.as_ref()))?
    .set("source", Value::string(source))?
    .set(
        "link",
        held.map_or(Value::Null, |link| Value::string(&link.name)),
    )?
    .set(
        "transport",
        held.map_or(Value::Null, |link| Value::string(&link.transport)),
    )?
    .build())
}

/// The name a `host` action names, from the selector the user wrote or the object's identity.
fn host_name(action: &Action) -> Option<String> {
    action
        .argument("name")
        .or_else(|| action.target().values().first())
        .and_then(|value| value.as_str().ok())
        .map(str::to_owned)
}

#[async_trait::async_trait]
impl Provider for SessionProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["job", "link", "host"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        ["ono.job", "ono.link", "ono.host"]
            .into_iter()
            .filter_map(|name| Self::schema(name).ok())
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("job.list", Risk::Read),
            Capability::new("link.list", Risk::Read),
            Capability::new("host.list", Risk::Read),
        ]
    }

    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let limit = query.max().unwrap_or(usize::MAX);
        let (records, failures) = match query.target_name() {
            "job" => (self.jobs()?, Vec::new()),
            "link" => (self.links()?, Vec::new()),
            "host" => {
                let source = query
                    .option_value("source")
                    .and_then(|value| value.as_str().ok().map(str::to_owned));
                self.hosts(source.as_deref())?
            }
            other => {
                return Err(ErrorValue::new(
                    ErrorCode::ProviderUnsupported,
                    format!("{PROVIDER_ID} answers no target named `{other}`"),
                ));
            }
        };
        let values: Vec<Value> = records
            .into_iter()
            .filter(|record| query.matches(record))
            .take(limit)
            .map(RecordValue::into_value)
            .chain(failures.into_iter().map(ErrorValue::into_value))
            .collect();
        Ok(ValueStream::from_values(values))
    }

    /// A job by `id`, a host by `name`. Links are never resolved here: their mutations are the
    /// shell's own (ADR-0103), and a `name` naming both a host and a link would otherwise make
    /// one `set host` act twice.
    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let records = match selector.field_name() {
            Some("id") => self.jobs()?,
            Some("name") => self.hosts(None)?.0,
            _ => return Ok(Vec::new()),
        };
        Ok(records
            .iter()
            .filter(|record| selector.matches(record))
            .filter_map(ObjectRef::of)
            .collect())
    }

    /// `add`, `set` and `remove` of a host, against the shell's own host file (ADR-0103 §2,
    /// ADR-0104). The OpenSSH configuration is never written: a host it lists cannot be changed
    /// from here.
    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        if action.target_name() != "host" {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!(
                    "{PROVIDER_ID} does not implement `{}` for `{}`",
                    action.operation(),
                    action.target_name()
                ),
            ));
        }
        let Some(name) = host_name(action) else {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(ErrorCode::TypeMismatch, "a host is named by its `name`"),
            ));
        };
        let mut hosts = match self.sources.own_hosts() {
            Ok(hosts) => hosts,
            Err(error) => return Ok(ActionOutcome::failed(action, error)),
        };
        let address = action
            .argument("address")
            .and_then(|value| ono_value::canonical_text(value).ok());
        let position = hosts.iter().position(|host| host.name == name);
        let changed = match action.operation() {
            "add" => {
                if position.is_some() {
                    return Ok(ActionOutcome::failed(
                        action,
                        ErrorValue::new(
                            ErrorCode::IoAlreadyExists,
                            format!("the host file already records `{name}`"),
                        )
                        .with_help(format!("`set host {name} --address …` changes it")),
                    ));
                }
                hosts.push(HostEntry {
                    name: name.clone(),
                    address,
                    port: None,
                    user: None,
                });
                true
            }
            "set" => {
                let Some(index) = position else {
                    return Ok(ActionOutcome::failed(
                        action,
                        ErrorValue::new(
                            ErrorCode::IoNotFound,
                            format!("the host file does not record `{name}`"),
                        )
                        .with_help(
                            "only hosts in the shell's own file can be changed; the OpenSSH \
                             configuration is read, never written",
                        ),
                    ));
                };
                let Some(address) = address else {
                    return Ok(ActionOutcome::failed(
                        action,
                        ErrorValue::new(
                            ErrorCode::TypeMismatch,
                            "`set host` needs a property to set, and none was given",
                        )
                        .with_help("name what should change: --address"),
                    ));
                };
                let before = hosts[index].address.replace(address);
                before != hosts[index].address
            }
            "remove" => {
                let Some(index) = position else {
                    return Ok(ActionOutcome::failed(
                        action,
                        ErrorValue::new(
                            ErrorCode::IoNotFound,
                            format!("the host file does not record `{name}`"),
                        )
                        .with_help("`get host` shows every source; only the `ono` one is written"),
                    ));
                };
                hosts.remove(index);
                true
            }
            other => {
                return Err(ErrorValue::new(
                    ErrorCode::ProviderUnsupported,
                    format!("{PROVIDER_ID} does not implement `{other}` for a host"),
                ));
            }
        };
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!(
                    "dry run: would {} `{name}` in the host file",
                    action.operation()
                ),
            ));
        }
        if changed && let Err(error) = self.sources.write_own(hosts) {
            return Ok(ActionOutcome::failed(action, error));
        }
        Ok(ActionOutcome::succeeded(action, changed))
    }
}
