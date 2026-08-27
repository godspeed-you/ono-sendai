//! The shell's own tables, answered as objects (spec §18.4, ADR-0090).
//!
//! `get job` returns structured job objects, and the job table lives nowhere a provider crate can
//! see: half of it is the executor's process groups (spec §18.1) and half is the session's
//! detached native pipelines (ADR-0024). So the session *publishes* what it knows — a plain row
//! per job, refreshed before every native pipeline runs — and this provider answers from the
//! published rows like any other provider answers from its system.
//!
//! The same seam is meant to carry the session's other tables later: the remote links of spec
//! §21 and the hosts they reach are session state in exactly the same way, and each becomes one
//! more target of this provider with one more table in [`SessionTables`].

use std::sync::{Arc, Mutex};

use ono_core::{ErrorCode, ExitStatus};
use ono_pipeline::ValueStream;
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
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
        &["job"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        Self::schema().into_iter().collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("job.list", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let limit = query.max().unwrap_or(usize::MAX);
        let values: Vec<Value> = self
            .jobs()?
            .into_iter()
            .filter(|job| query.matches(job))
            .take(limit)
            .map(RecordValue::into_value)
            .collect();
        Ok(ValueStream::from_values(values))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        Ok(self
            .jobs()?
            .iter()
            .filter(|job| selector.matches(job))
            .filter_map(ObjectRef::of)
            .collect())
    }
}
