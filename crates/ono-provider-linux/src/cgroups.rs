//! The control groups of the unified hierarchy, one `ono.cgroup/1` record each (spec §8.1's
//! `cgroup` target, v0.4 §16.3, ADR-0971).
//!
//! The hierarchy is a directory tree the kernel mounts, and every directory in it is a group.
//! The path a record carries is the path `/proc/<pid>/cgroup` gives a process — `/` for the root,
//! `/system.slice/nginx.service` below it — so a group listed here and the group a process says
//! it is in are the same object.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Availability, Capability, ObjectRef, Provider, Query, Risk, Selector, TemporalCapabilities,
};
use ono_value::{ErrorValue, RecordValue, Schema, Value};

use crate::common::{io_error, provenance};
use crate::schemas;

pub const PROVIDER_ID: &str = "linux.cgroups";

/// Where the unified hierarchy is mounted, relative to the root, in the order to try them: the
/// mount of a cgroup v2 host, then the one a hybrid host keeps beside its v1 controllers.
const MOUNTS: [&str; 2] = ["sys/fs/cgroup", "sys/fs/cgroup/unified"];

/// Provider for the `cgroup` target.
#[derive(Debug, Clone)]
pub struct CgroupProvider {
    root: PathBuf,
}

impl Default for CgroupProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// What a walk of the hierarchy found: the groups, and the directories that refused a listing.
struct Walk {
    records: Vec<RecordValue>,
    errors: Vec<ErrorValue>,
}

impl CgroupProvider {
    /// The control groups of the system this shell runs on.
    #[must_use]
    pub fn new() -> Self {
        Self::rooted("/")
    }

    /// The control groups of the system whose `/sys` is mounted under `root`.
    #[must_use]
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// The directory the unified hierarchy is mounted on: the one that holds
    /// `cgroup.controllers`, which only the root of a cgroup v2 mount does.
    fn mount(&self) -> Option<PathBuf> {
        MOUNTS
            .iter()
            .map(|relative| self.root.join(relative))
            .find(|mount| mount.join("cgroup.controllers").is_file())
    }

    fn unavailable(&self) -> String {
        format!(
            "no unified control-group hierarchy is mounted at {}",
            self.root.join(MOUNTS[0]).display()
        )
    }

    /// Every group under the mount, in path order.
    fn walk(&self, schema: &Arc<Schema>) -> Result<Walk, ErrorValue> {
        let mount = self.mount().ok_or_else(|| {
            ErrorValue::new(ErrorCode::ProviderUnavailable, self.unavailable()).with_help(
                "`cgroup` lists the cgroup v2 hierarchy; a host that mounts only the v1 \
                 controllers has none",
            )
        })?;
        let mut walk = Walk {
            records: Vec::new(),
            errors: Vec::new(),
        };
        // Each entry is a group still to be listed: its directory and its path in the hierarchy.
        let mut pending = vec![(mount.clone(), String::from("/"))];
        while let Some((directory, path)) = pending.pop() {
            match children(&directory) {
                Ok(names) => {
                    for name in names {
                        let child = if path == "/" {
                            format!("/{name}")
                        } else {
                            format!("{path}/{name}")
                        };
                        pending.push((directory.join(&name), child));
                    }
                }
                // A group removed between its parent's listing and its own is not a group any
                // more, and saying nothing about it is the true answer.
                Err(error) if error.kind() == io::ErrorKind::NotFound && directory != mount => {
                    continue;
                }
                // The group is there; what is below it could not be listed, and the answer says so.
                Err(error) => walk.errors.push(io_error(&error, &directory)),
            }
            walk.records.push(record(schema, &mount, &path)?);
        }
        walk.records.sort_by_cached_key(path_text);
        Ok(walk)
    }
}

/// The names of the directories directly under `directory`.
fn children(directory: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(directory)? {
        let Ok(entry) = entry else { continue };
        // `file_type` does not follow a link, and the kernel puts none here: a directory is a
        // group and everything else is one of the group's control files.
        if entry.file_type().is_ok_and(|kind| kind.is_dir())
            && let Some(name) = entry.file_name().to_str()
        {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

fn path_text(record: &RecordValue) -> String {
    match record.get("path") {
        Some(Value::Path(path)) => path.to_string_lossy().into_owned(),
        _ => String::new(),
    }
}

fn record(schema: &Arc<Schema>, mount: &Path, path: &str) -> Result<RecordValue, ErrorValue> {
    let (name, parent) = match path.rsplit_once('/') {
        Some((_, "")) | None => ("/", Value::Null),
        Some(("", name)) => (name, Value::Path(Arc::from(Path::new("/")))),
        Some((parent, name)) => (name, Value::Path(Arc::from(Path::new(parent)))),
    };
    Ok(RecordValue::builder(
        Arc::clone(schema),
        provenance(
            PROVIDER_ID,
            schema.id(),
            &mount.join(path.trim_start_matches('/')).to_string_lossy(),
        ),
    )
    .set("path", Value::Path(Arc::from(Path::new(path))))?
    .set("name", Value::string(name))?
    .set("parent", parent)?
    .build())
}

#[async_trait::async_trait]
impl Provider for CgroupProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["cgroup"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        schemas::require(&schemas::cgroup_id())
            .into_iter()
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("cgroup.read", Risk::Read)]
    }

    fn temporal(&self) -> TemporalCapabilities {
        // The hierarchy as it is now. A group that came and went between two listings left
        // nothing here to ask about.
        TemporalCapabilities {
            current_snapshot: true,
            live_events: false,
            historical_query: false,
            exhaustive_events: false,
            causal_tokens: false,
            checkpointable: true,
            retained_history: None,
        }
    }

    fn availability(&self) -> Availability {
        if self.mount().is_some() {
            Availability::Available
        } else {
            Availability::unavailable(self.unavailable())
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let schema = schemas::require(&schemas::cgroup_id())?;
        let provider = self.clone();
        let query = query.clone();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let walked = tokio::task::spawn_blocking(move || provider.walk(&schema)).await;
                let walk = match walked {
                    Ok(Ok(walk)) => walk,
                    Ok(Err(error)) => {
                        let _ = sink.fail(error).await;
                        return;
                    }
                    Err(joined) => {
                        let _ = sink
                            .fail(ErrorValue::new(
                                ErrorCode::ProviderUnavailable,
                                format!("the cgroup reader stopped before answering: {joined}"),
                            ))
                            .await;
                        return;
                    }
                };
                // v0.4.1 §34.4: an orientation bounds what it reads, and §2.17 wants the count it
                // then shows to be the real one. The walk has listed every group already, so the
                // population costs nothing — for a query that selects nothing (ADR-0576).
                if query.selectors().is_empty() {
                    sink.diagnostics()
                        .record_population(walk.records.len().try_into().unwrap_or(u64::MAX));
                }
                let limit = query.max().unwrap_or(usize::MAX);
                let mut sent = 0_usize;
                for record in walk.records {
                    if sent >= limit {
                        break;
                    }
                    if !query.matches(&record) {
                        continue;
                    }
                    if sink.send(record.into_value()).await.is_err() {
                        return;
                    }
                    sent += 1;
                }
                for error in walk.errors {
                    if sink.fail(error).await.is_err() {
                        return;
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let schema = schemas::require(&schemas::cgroup_id())?;
        let provider = self.clone();
        let walk = tokio::task::spawn_blocking(move || provider.walk(&schema))
            .await
            .map_err(|joined| {
                ErrorValue::new(
                    ErrorCode::ProviderUnavailable,
                    format!("the cgroup reader stopped before answering: {joined}"),
                )
            })??;
        Ok(walk
            .records
            .iter()
            .filter(|record| selector.matches(record))
            .filter_map(ObjectRef::of)
            .collect())
    }
}
