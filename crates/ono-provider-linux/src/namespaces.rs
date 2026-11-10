//! The kernel namespaces the processes of this host live in, one `ono.namespace/1` record each
//! (spec §8.1's `namespace` target, v0.3 §1.35, v0.4 §7.3, ADR-0971).
//!
//! The kernel names a process's namespaces through the links in `/proc/<pid>/ns`, and that is
//! the only place it names them: a namespace is whatever at least one such link points at. So
//! the listing is an aggregation over the process table — the interface `lsns` reads — and it
//! covers the processes the caller may look into, which the provider contract states.

use std::collections::BTreeMap;
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

use crate::accounts::{Accounts, NssAccounts};
use crate::common::{io_error, provenance};
use crate::{procfs, schemas};

pub const PROVIDER_ID: &str = "linux.namespaces";

/// The namespace kinds a process is *in*. The `pid_for_children` and `time_for_children` links
/// name the namespace its children will enter, which no process lives in, so they are left out —
/// as `lsns` leaves them out.
const KINDS: [&str; 8] = ["cgroup", "ipc", "mnt", "net", "pid", "time", "user", "uts"];

/// Provider for the `namespace` target.
#[derive(Debug, Clone)]
pub struct NamespaceProvider {
    root: PathBuf,
    accounts: Arc<dyn Accounts>,
}

impl Default for NamespaceProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// One namespace as the scan of `/proc` found it.
#[derive(Debug, Clone)]
struct Found {
    kind: &'static str,
    inode: u64,
    processes: i128,
    /// The lowest pid in the namespace: the scan runs in ascending pid order.
    pid: i64,
    parent_pid: Option<i64>,
    uid: Option<u32>,
    command: Option<String>,
}

/// What one scan of the process table produced.
#[derive(Debug, Default)]
struct Scan {
    found: Vec<Found>,
    /// The first link the kernel refused to disclose, when it refused any.
    refused: Option<ErrorValue>,
}

impl NamespaceProvider {
    /// The namespaces of the system this shell runs on.
    #[must_use]
    pub fn new() -> Self {
        Self::rooted("/")
    }

    /// The namespaces of the system whose `/proc` is mounted under `root`.
    #[must_use]
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            accounts: Arc::new(NssAccounts::new()),
        }
    }

    /// Resolves user names through `accounts` rather than through NSS.
    #[must_use]
    pub fn with_accounts(mut self, accounts: Arc<dyn Accounts>) -> Self {
        self.accounts = accounts;
        self
    }

    fn proc_root(&self) -> PathBuf {
        self.root.join("proc")
    }

    /// Reads every process's namespace links and groups them by kind and inode.
    fn scan(&self) -> Result<Scan, ErrorValue> {
        let proc_root = self.proc_root();
        let entries = fs::read_dir(&proc_root).map_err(|error| {
            io_error(&error, &proc_root).with_help(
                "`namespace` reads the links under /proc/<pid>/ns; without a mounted procfs \
                 there is nothing to read them from",
            )
        })?;
        let mut pids: Vec<i64> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
            .collect();
        pids.sort_unstable();

        let mut by_identity: BTreeMap<(&'static str, u64), Found> = BTreeMap::new();
        let mut refused = None;
        for pid in pids {
            let dir = proc_root.join(pid.to_string());
            for kind in KINDS {
                let link = dir.join("ns").join(kind);
                let inode = match read_inode(&link, kind) {
                    Ok(Some(inode)) => inode,
                    Ok(None) => continue,
                    Err(error) => {
                        refused.get_or_insert_with(|| io_error(&error, &link));
                        continue;
                    }
                };
                by_identity
                    .entry((kind, inode))
                    .and_modify(|found| found.processes += 1)
                    .or_insert_with(|| Found {
                        kind,
                        inode,
                        processes: 1,
                        pid,
                        parent_pid: None,
                        uid: None,
                        command: None,
                    });
            }
        }

        // What `lsns` shows beside a namespace: the lowest process in it. One process is usually
        // the lowest of several namespaces, so each is read once.
        let mut described: BTreeMap<i64, (Option<i64>, Option<u32>, Option<String>)> =
            BTreeMap::new();
        let mut found: Vec<Found> = by_identity.into_values().collect();
        for namespace in &mut found {
            let (parent_pid, uid, command) = described
                .entry(namespace.pid)
                .or_insert_with(|| describe(&proc_root.join(namespace.pid.to_string())))
                .clone();
            namespace.parent_pid = parent_pid;
            namespace.uid = uid;
            namespace.command = command;
        }
        Ok(Scan { found, refused })
    }

    /// The scan as records, or the refusal where nothing could be read and something was refused.
    async fn records(&self, schema: &Arc<Schema>) -> Result<Vec<RecordValue>, ErrorValue> {
        let provider = self.clone();
        let scan = tokio::task::spawn_blocking(move || provider.scan())
            .await
            .map_err(|joined| {
                ErrorValue::new(
                    ErrorCode::ProviderUnavailable,
                    format!("the namespace reader stopped before answering: {joined}"),
                )
            })??;
        if scan.found.is_empty()
            && let Some(refusal) = scan.refused
        {
            // Namespaces exist and none may be read: an empty list would say there are none
            // (v0.4 §42.4).
            return Err(refusal.with_help(
                "the namespaces of another user's processes are readable by root, or with \
                 CAP_SYS_PTRACE",
            ));
        }
        let proc_root = self.proc_root();
        let mut records = Vec::with_capacity(scan.found.len());
        for namespace in scan.found {
            let user = match namespace.uid {
                Some(uid) => self.accounts.user(uid).await.map(|account| account.name),
                None => None,
            };
            let int = |value: Option<i64>| value.map_or(Value::Null, |n| Value::Int(i128::from(n)));
            let text = |value: Option<String>| value.map_or(Value::Null, |t| Value::string(&t));
            records.push(
                RecordValue::builder(
                    Arc::clone(schema),
                    provenance(
                        PROVIDER_ID,
                        schema.id(),
                        &proc_root
                            .join(format!("<pid>/ns/{}", namespace.kind))
                            .to_string_lossy(),
                    ),
                )
                .set("id", Value::Int(i128::from(namespace.inode)))?
                .set("type", Value::string(namespace.kind))?
                .set("processes", Value::Int(namespace.processes))?
                .set("pid", Value::Int(i128::from(namespace.pid)))?
                .set("parent_pid", int(namespace.parent_pid))?
                .set("user", text(user))?
                .set(
                    "uid",
                    namespace
                        .uid
                        .map_or(Value::Null, |uid| Value::Int(i128::from(uid))),
                )?
                .set("command", text(namespace.command))?
                .build(),
            );
        }
        Ok(records)
    }
}

/// The inode a namespace link names: `Ok(None)` where there is no such namespace to name, an
/// error where the kernel will not say.
fn read_inode(link: &Path, kind: &str) -> io::Result<Option<u64>> {
    match fs::read_link(link) {
        // Some kernels let the `readlink` of a link the caller may not look through succeed and
        // disclose nothing; that is the same refusal as `EACCES` (ADR-0784).
        Ok(target) if target.as_os_str().is_empty() => {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
        Ok(target) => Ok(target
            .to_string_lossy()
            .strip_prefix(kind)
            .and_then(|rest| rest.strip_prefix(":["))
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|digits| digits.parse().ok())),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Err(error),
        // The process left, or this kernel has no namespace of that kind.
        Err(_) => Ok(None),
    }
}

/// The parent, the owner and the command line of the process in `dir`, each where it is readable.
fn describe(dir: &Path) -> (Option<i64>, Option<u32>, Option<String>) {
    let parent_pid = fs::read_to_string(dir.join("stat"))
        .ok()
        .and_then(|text| procfs::parse_stat(&text))
        .map(|stat| stat.ppid);
    let uid = fs::read_to_string(dir.join("status"))
        .ok()
        .and_then(|text| procfs::parse_status_ids(&text).0);
    let command = fs::read(dir.join("cmdline"))
        .ok()
        .map(|bytes| procfs::parse_cmdline(&bytes).join(" "))
        .filter(|command| !command.is_empty());
    (parent_pid, uid, command)
}

#[async_trait::async_trait]
impl Provider for NamespaceProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["namespace"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        schemas::require(&schemas::namespace_id())
            .into_iter()
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("namespace.read", Risk::Read)]
    }

    fn temporal(&self) -> TemporalCapabilities {
        // The namespaces that have a process in them now. One that emptied between two listings
        // left nothing here to ask about.
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
        let own = self.proc_root().join("self/ns");
        if self.proc_root().is_dir() {
            Availability::Available
        } else {
            Availability::unavailable(format!("{} is not readable", own.display()))
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let schema = schemas::require(&schemas::namespace_id())?;
        let provider = self.clone();
        let query = query.clone();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let records = match provider.records(&schema).await {
                    Ok(records) => records,
                    Err(error) => {
                        let _ = sink.fail(error).await;
                        return;
                    }
                };
                // v0.4.1 §34.4: the scan has aggregated every namespace before the bound cuts the
                // answer, so a query that selects nothing states how many there are (ADR-0576).
                if query.selectors().is_empty() {
                    sink.diagnostics()
                        .record_population(records.len().try_into().unwrap_or(u64::MAX));
                }
                let limit = query.max().unwrap_or(usize::MAX);
                for record in records
                    .into_iter()
                    .filter(|record| query.matches(record))
                    .take(limit)
                {
                    if sink.send(record.into_value()).await.is_err() {
                        return;
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let schema = schemas::require(&schemas::namespace_id())?;
        Ok(self
            .records(&schema)
            .await?
            .iter()
            .filter(|record| selector.matches(record))
            .filter_map(ObjectRef::of)
            .collect())
    }
}
