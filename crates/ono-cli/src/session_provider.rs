//! The shell's own tables, answered as objects (spec §18.4, §21, §31; ADR-0090, ADR-0103,
//! ADR-0107).
//!
//! `get job` returns structured job objects, and the job table lives nowhere a provider crate can
//! see: half of it is the executor's process groups (spec §18.1) and half is the session's
//! detached native pipelines (ADR-0024). So the session *publishes* what it knows — a plain row
//! per job, refreshed before every native pipeline runs — and this provider answers from the
//! published rows like any other provider answers from its system.
//!
//! The same seam carries the session's other tables. The remote links of spec §21 are session
//! state in exactly the same way, published as one row per link, and the hosts of spec §9.1 are
//! the hosts those links reach together with what the configured host sources list
//! (`crate::hosts`), read when asked. The KUANG/11 tables (ADR-0107) — the packages of spec §31.8
//! as the plugin home on disk overlaid with the runtime instances this session started, their
//! capability grants and audit trail — live in the [`Host`](crate::kuang_host::Host) the tables
//! hold.
use std::sync::{Arc, Mutex};

use ono_core::{ErrorCode, ExitStatus};
use ono_pipeline::ValueStream;
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectRef, Provider, Query, Risk, Selector,
};
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value};

use crate::hosts::{HostEntry, HostSources};

/// The severities `ono.finding/1` carries, weakest first (spec §31.24's closed set).
const SEVERITIES: &[&str] = &["info", "low", "medium", "high", "critical"];

/// Where `severity` sits in that order, or `None` when it is not one of them.
fn severity_rank(severity: &str) -> Option<usize> {
    SEVERITIES.iter().position(|known| *known == severity)
}

/// The provider's stable id, as it appears in every record's provenance.
pub const PROVIDER_ID: &str = "ono.shell";

/// Everything `ono.shell` answers for on the machine it runs on.
const ALL_TARGETS: &[&str] = &[
    "job",
    "link",
    "host",
    "host-key",
    "plugin",
    "capability",
    "audit",
    "assistant",
    "model",
    "finding",
];

/// The subset that describes the session rather than the machine (spec §14.4, ADR-0269).
///
/// A package loaded on the far side is a fact about the far side and stays remote; the links
/// this session holds, the jobs it started and the hosts it knows are not.
/// The targets the shell answers about itself: session facts, not observations of a machine.
pub const SESSION_TARGETS: &[&str] = &["job", "link", "host", "host-key"];

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
    /// The KUANG/11 host: where packages are, and which of them run (ADR-0107).
    pub kuang: crate::kuang_host::Host,
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
    /// Which of `ono.shell`'s targets this instance answers for (spec §14.4, ADR-0269).
    targets: &'static [&'static str],
}

impl SessionProvider {
    /// A provider answering from `tables`, which the session keeps current, and from the host
    /// sources of `sources`, read when asked.
    #[must_use]
    pub fn new(tables: Arc<Mutex<SessionTables>>, sources: HostSources) -> Self {
        Self {
            tables,
            sources,
            targets: ALL_TARGETS,
        }
    }

    /// The same provider, narrowed to the targets that describe *this session* rather than a
    /// machine (spec §14.4, ADR-0269).
    ///
    /// A link, a job and the context stack are facts about the shell that is running, not
    /// observations of a host, so they answer here even when a link frame has swapped every
    /// other provider for the far side's.
    #[must_use]
    pub fn session_facts(tables: Arc<Mutex<SessionTables>>, sources: HostSources) -> Self {
        Self {
            tables,
            sources,
            targets: SESSION_TARGETS,
        }
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

    /// The records of `target` as of now, and the per-object failures beside them.
    fn table(&self, target: &str) -> Result<(Vec<RecordValue>, Vec<ErrorValue>), ErrorValue> {
        match target {
            "job" => Ok((self.jobs()?, Vec::new())),
            "link" => Ok((self.links()?, Vec::new())),
            "host" => self.hosts(None),
            "host-key" => Ok((self.host_keys()?, Vec::new())),
            "plugin" => {
                let (records, mut failures) = self.lock().kuang.plugin_records()?;
                // A declaration the shell refused to register must not be a command that is
                // quietly missing from `get command` (spec §31.65, ADR-0282).
                failures.extend(crate::plugin_registry::refusals());
                Ok((records, failures))
            }
            "capability" => Ok((self.lock().kuang.capability_records(None)?, Vec::new())),
            "audit" => Ok((self.lock().kuang.audit_records()?, Vec::new())),
            // No assistant package is loaded, no model provider is configured, no analysis has
            // run: the typed, empty answer (ADR-0111 §3).
            "assistant" | "model" | "finding" => Ok((Vec::new(), Vec::new())),
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
        let (package, management, instance, trust) = {
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
            let trust = tables.kuang.trust().clone();
            (package, management, instance, trust)
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
                    &trust,
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

    /// `revoke capability` (spec §31.18: every grant is revocable): the grant is marked revoked
    /// and the running instance's broker evaluates the new policy at its next call.
    async fn revoke_capability(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let Some(Value::Uuid(id)) = action.target().values().first() else {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("{} is not a grant identity", action.target()),
            ));
        };
        let only = action
            .argument("plugin")
            .and_then(|value| value.as_str().ok())
            .map(str::to_owned);
        let (grant, instance, policy) = {
            let mut tables = self.lock();
            let Some(grant) = tables
                .kuang
                .grants()
                .iter()
                .find(|grant| grant.id == *id)
                .cloned()
            else {
                return Ok(ActionOutcome::failed(
                    action,
                    ErrorValue::new(ErrorCode::IoNotFound, "no such grant"),
                ));
            };
            if only.as_deref().is_some_and(|plugin| plugin != grant.plugin) {
                return Ok(ActionOutcome::skipped(
                    action,
                    format!("the grant belongs to `{}`", grant.plugin),
                ));
            }
            if grant.revoked_at.is_some() {
                return Ok(ActionOutcome::skipped(action, "already revoked"));
            }
            if action.is_dry_run() {
                return Ok(ActionOutcome::skipped(
                    action,
                    format!("would revoke {} from `{}`", grant.capability, grant.plugin),
                ));
            }
            tables.kuang.revoke(*id);
            let instance = tables.kuang.plugin(&grant.plugin);
            let policy = tables.kuang.policy_for(&grant.plugin);
            (grant, instance, policy)
        };
        if let Some(instance) = instance {
            instance.update_policy(policy).await;
        }
        let _ = grant;
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
        let keep_grants = action.argument("keep-grants") == Some(&Value::Bool(true));
        {
            let mut tables = self.lock();
            tables.kuang.remove_package(&package, keep_state)?;
            if !keep_grants {
                // The package is gone, so the permissions it held are gone with it: a package
                // that comes back must ask again (spec §31.18, §31.81, ADR-0233).
                tables.kuang.revoke_grants_of(&id);
            }
        }
        Ok(ActionOutcome::succeeded(action, true))
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

    /// The pinned host keys, in the order the trust store's file records them (spec §21.5).
    fn host_keys(&self) -> Result<Vec<RecordValue>, ErrorValue> {
        let schema = Self::schema("ono.host-key")?;
        crate::trust::rows(&self.sources)?
            .iter()
            .map(|row| host_key_record(row, &schema))
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

    /// `add`, `set` and `remove` of a host, against the shell's own host file (ADR-0103 §2,
    /// ADR-0104). The OpenSSH configuration is never written: a host it lists cannot be changed
    /// from here.
    async fn act_host(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
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

/// One pinned host key as an `ono.host-key/1` record (ADR-0355).
///
/// # Errors
///
/// `provider.schema_violation` when the contract that defines the schema is missing.
pub fn host_key_value(row: &crate::trust::KeyRow) -> Result<Value, ErrorValue> {
    let schema = SessionProvider::schema("ono.host-key")?;
    host_key_record(row, &schema).map(RecordValue::into_value)
}

fn host_key_record(
    row: &crate::trust::KeyRow,
    schema: &Arc<Schema>,
) -> Result<RecordValue, ErrorValue> {
    Ok(RecordValue::builder(
        Arc::clone(schema),
        Provenance::local(PROVIDER_ID, schema.id().clone()),
    )
    .set("host", Value::string(&row.host))?
    .set("algorithm", Value::string(&row.algorithm))?
    .set("fingerprint", Value::string(&row.fingerprint))?
    .set(
        "path",
        row.path
            .as_ref()
            .map_or(Value::Null, |path| Value::Path(path.as_path().into())),
    )?
    .build())
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

#[async_trait::async_trait]
impl Provider for SessionProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        self.targets
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        ["ono.job", "ono.link", "ono.host", "ono.host-key"]
            .into_iter()
            .filter_map(|name| Self::schema(name).ok())
            .chain(
                [
                    "ono.plugin",
                    "ono.plugin-package",
                    "ono.plugin-inspection",
                    "ono.capability-grant",
                    "ono.plugin-audit-event",
                    "ono.assistant",
                    "ono.model-provider",
                    "ono.finding",
                ]
                .into_iter()
                .filter_map(|name| crate::kuang_host::schema(name).ok()),
            )
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("job.list", Risk::Read),
            Capability::new("link.list", Risk::Read),
            Capability::new("host.list", Risk::Read),
            // Reading, recording and forgetting a pin: `get host-key` is a read of session
            // state, and the three mutations are the shell's own (ADR-0355).
            Capability::new("host.trust", Risk::Mutate),
            Capability::new("plugin.list", Risk::Read),
            Capability::new("plugin.search", Risk::Read),
            Capability::new("plugin.inspect", Risk::Read),
            Capability::new("plugin.remove", Risk::Destructive),
            Capability::new("plugin.unload", Risk::Mutate),
            Capability::new("plugin.set", Risk::Mutate),
            Capability::new("capability.list", Risk::Read),
            Capability::new("capability.revoke", Risk::Mutate),
            Capability::new("audit.list", Risk::Read),
            Capability::new("assistant.list", Risk::Read),
            Capability::new("model.list", Risk::Read),
            Capability::new("finding.list", Risk::Read),
        ]
    }

    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let limit = query.max().unwrap_or(usize::MAX);
        match query.target_name() {
            // The session's own rows: jobs and links as published, hosts as the sources list
            // them (ADR-0090, ADR-0103).
            "job" | "link" | "host" => {
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
            // The KUANG/11 tables (ADR-0107, ADR-0108, ADR-0111).
            _ => {
                if query.target_name() == "plugin" {
                    // `find plugin <term>`: the search selector answers packages as their sources
                    // describe them, not installed rows (ADR-0108 §4).
                    let term = query
                        .selectors()
                        .iter()
                        .find_map(|selector| match selector {
                            Selector::Field { name, value } if name == "query" => {
                                value.as_str().ok()
                            }
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
                let plugin = query
                    .option_value("plugin")
                    .and_then(|value| value.as_str().ok())
                    .map(str::to_owned);
                let (records, failures) = match (query.target_name(), plugin.as_deref()) {
                    // `get capability --plugin <id>`: that package's requests and grants, no definitions.
                    ("capability", Some(plugin)) => (
                        self.lock().kuang.capability_records(Some(plugin))?,
                        Vec::new(),
                    ),
                    (target, _) => self.table(target)?,
                };
                // `--plugin`, `--capability` and `--since` restrict the audit trail (kuang.yaml).
                let capability = query
                    .option_value("capability")
                    .and_then(|value| value.as_str().ok())
                    .map(str::to_owned);
                let since = query.option_value("since").cloned();
                let records: Vec<RecordValue> = if query.target_name() == "audit" {
                    records
                        .into_iter()
                        .filter(|record| {
                            let field =
                                |name: &str| record.get(name).and_then(|value| value.as_str().ok());
                            plugin
                                .as_deref()
                                .is_none_or(|wanted| field("plugin") == Some(wanted))
                                && capability
                                    .as_deref()
                                    .is_none_or(|wanted| field("capability") == Some(wanted))
                                && since.as_ref().is_none_or(|since| {
                                    record
                                        .get("at")
                                        .and_then(|at| at.as_timestamp().ok())
                                        .zip(since.as_timestamp().ok())
                                        .is_none_or(|(at, since)| at >= since)
                                })
                        })
                        .collect()
                } else {
                    records
                };
                // `get plugin --state loaded`: the option is a filter on the state column (kuang.yaml).
                let state = query
                    .option_value("state")
                    .and_then(|value| value.as_str().ok())
                    .map(str::to_owned);
                // `get finding --severity high`: a *minimum*, over the closed set of
                // `ono.finding/1` (spec §31.24). A level outside that set is refused rather than
                // ignored, because a filter nobody applied answers with everything (ADR-0233).
                let floor = match query.option_value("severity") {
                    Some(value) => {
                        let wanted = value.as_str()?;
                        Some(severity_rank(wanted).ok_or_else(|| {
                            ErrorValue::new(
                                ErrorCode::TypeMismatch,
                                format!("`{wanted}` is not a severity `ono.finding/1` carries"),
                            )
                            .with_help(format!(
                                "spec §31.24 closes the set at {}",
                                SEVERITIES.join(", ")
                            ))
                        })?)
                    }
                    None => None,
                };
                let values: Vec<Value> = records
                    .into_iter()
                    .filter(|record| query.matches(record))
                    .filter(|record| {
                        state.as_deref().is_none_or(|wanted| {
                            record.get("state").and_then(|value| value.as_str().ok())
                                == Some(wanted)
                        })
                    })
                    .filter(|record| {
                        floor.is_none_or(|floor| {
                            record
                                .get("severity")
                                .and_then(|value| value.as_str().ok())
                                .and_then(severity_rank)
                                .is_some_and(|rank| rank >= floor)
                        })
                    })
                    .take(limit)
                    .map(RecordValue::into_value)
                    .collect();
                Ok(stream_of(values, failures))
            }
        }
    }

    /// A grant by `selector` (kuang.yaml), a host by `name`, and anything else by every table
    /// whose identity is typed the way the selector is — a job's number, a package's id, a
    /// grant's uuid. Links are never resolved here: their mutations are the shell's own
    /// (ADR-0103), and a `name` naming both a host and a link would otherwise make one
    /// `set host` act twice.
    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        // `revoke capability <selector>`: the grant to revoke, by its capability or its id
        // (kuang.yaml); definitions are not grants and are never resolved.
        if let Selector::Field { name, value } = selector
            && name == "selector"
        {
            let text = value.as_str().ok().map(str::to_owned);
            let wanted = |record: &RecordValue| {
                let matches = |field: &str| {
                    record
                        .get(field)
                        .and_then(|held| ono_value::canonical_text(held).ok())
                        .is_some_and(|held| Some(held) == text)
                };
                matches("capability") || matches("id")
            };
            let (grants, _) = self.table("capability")?;
            return Ok(grants
                .iter()
                .filter(|record| {
                    record.get("plugin").is_some_and(|plugin| !plugin.is_null())
                        && record
                            .get("revoked_at")
                            .is_none_or(ono_value::Value::is_null)
                        && record.get("decision").and_then(|d| d.as_str().ok()) == Some("allow")
                })
                .filter(|record| wanted(record))
                .filter_map(ObjectRef::of)
                .collect());
        }
        if selector.field_name() == Some("name") {
            return Ok(self
                .hosts(None)?
                .0
                .iter()
                .filter(|record| selector.matches(record))
                .filter_map(ObjectRef::of)
                .collect());
        }
        // A selector carries no target; every table is asked, which is unambiguous because the
        // tables' identity fields are typed differently (a job's `id` is a number, a grant's a
        // uuid).
        let mut found = Vec::new();
        for target in ["job", "plugin", "capability"] {
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

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        match (action.target_name(), action.operation()) {
            ("host", _) => self.act_host(action).await,
            ("plugin", "remove") => self.remove_plugin(action).await,
            ("plugin", "unload") => self.unload_plugin(action).await,
            ("plugin", "set") => self.set_plugin(action).await,
            ("capability", "revoke") => self.revoke_capability(action).await,
            (target, operation) => Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{PROVIDER_ID} does not `{operation}` a {target}"),
            )),
        }
    }
}
