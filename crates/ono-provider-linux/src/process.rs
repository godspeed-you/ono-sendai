//! The `process` target, answered from `/proc` alone (spec §23.1, §28.1).
//!
//! Every field comes from a kernel file. Nothing here runs `ps` or reads its output, which spec
//! §50 forbids and which `/proc` makes unnecessary.
//!
//! Three behaviours are worth stating because they are easy to get wrong and expensive to get
//! wrong:
//!
//! - **CPU is a rate, and the first observation has none.** `/proc/<pid>/stat` counts ticks since
//!   boot. A percentage is the difference between two observations divided by the time between
//!   them, so the first `get process` reports `cpu` as `null` and the second reports the share of
//!   one logical CPU used since the first. That is what
//!   `docs/spec/schemas/process.v1.yaml` documents the field to mean.
//! - **Identity is `(pid, started)`.** A signal re-reads the start time immediately before
//!   delivering and refuses when it changed, so a recycled pid is never signalled
//!   (ADR-0015 T13).
//! - **A field this user may not read is an error, not a zero.** A process belonging to someone
//!   else routinely hides `cmdline`, `exe` and `cwd`; those fields then carry an
//!   [`ErrorValue`](ono_value::ErrorValue), which spec §10.5 keeps distinct from both `null` and
//!   a fabricated default.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectRef, Provider, Query, Risk, Selector,
};
use ono_value::{ByteSize, ErrorValue, RecordValue, Schema, Value, ValueRef};

use crate::accounts::{Accounts, NssAccounts};
use crate::common::{errno_error, group_ref, io_error, provenance, timestamp, user_ref};
use crate::procfs;
use crate::schemas;

/// The provider's stable id, as it appears in every record's provenance.
pub const PROVIDER_ID: &str = "linux.procfs";

/// The monotonic clock the CPU sampler measures its interval against.
///
/// Injectable because a rate is a quotient of two measurements, and a test that cannot state the
/// denominator cannot assert the quotient.
pub trait Clock: Send + Sync + std::fmt::Debug {
    /// Nanoseconds since an arbitrary but fixed point, never going backwards.
    fn now_nanos(&self) -> u128;
}

/// The machine's own monotonic clock.
#[derive(Debug)]
pub struct SystemClock {
    base: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemClock {
    /// A clock counting from now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now_nanos(&self) -> u128 {
        self.base.elapsed().as_nanos()
    }
}

/// Delivering a signal to a process.
///
/// Injectable so that the pid-reuse refusal of ADR-0015 T13 can be proven without a test run
/// ever being able to signal an unrelated process on the machine it runs on.
pub trait Signals: Send + Sync + std::fmt::Debug {
    /// Delivers `signal` to `pid`.
    ///
    /// # Errors
    ///
    /// Returns the structured form of the kernel's refusal.
    fn send(&self, pid: i32, signal: i32) -> Result<(), ErrorValue>;
}

/// `kill(2)`.
#[derive(Debug, Default)]
pub struct KernelSignals;

impl Signals for KernelSignals {
    fn send(&self, pid: i32, signal: i32) -> Result<(), ErrorValue> {
        let signal = nix::sys::signal::Signal::try_from(signal)
            .map_err(|_| unknown_signal(&signal.to_string()))?;
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), signal)
            .map_err(|errno| errno_error(errno, &PathBuf::from(format!("/proc/{pid}"))))
    }
}

/// One CPU observation, kept so the next one can be turned into a rate.
#[derive(Debug, Clone, Copy)]
struct Sample {
    ticks: u64,
    at_nanos: u128,
}

/// Everything reading one process needs, cheap enough to hand to a streaming task.
#[derive(Debug, Clone)]
struct Reader {
    proc_root: PathBuf,
    accounts: Arc<dyn Accounts>,
    clock: Arc<dyn Clock>,
    boot_seconds: Option<i64>,
    clock_ticks: u64,
    page_size: u128,
    samples: Arc<Mutex<HashMap<(i64, u64), Sample>>>,
}

/// Processes, from procfs.
///
/// ```no_run
/// use ono_provider_api::{Provider, Query};
/// use ono_provider_linux::ProcessProvider;
///
/// let provider = ProcessProvider::new();
/// let stream = provider.snapshot(&Query::target("process"))?;
/// assert!(stream.boundedness().is_bounded());
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Debug)]
pub struct ProcessProvider {
    reader: Reader,
    signals: Arc<dyn Signals>,
}

impl Default for ProcessProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessProvider {
    /// Processes of the machine this shell runs on.
    #[must_use]
    pub fn new() -> Self {
        Self::rooted("/")
    }

    /// Processes of the `proc` filesystem mounted under `root`.
    ///
    /// `root` locates the kernel interface — `<root>/proc` — and nothing else: the paths inside
    /// a record are the ones the kernel itself reported.
    #[must_use]
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        let proc_root = root.as_ref().join("proc");
        Self {
            reader: Reader {
                boot_seconds: procfs::boot_time_seconds(&proc_root),
                proc_root,
                accounts: Arc::new(NssAccounts::new()),
                clock: Arc::new(SystemClock::new()),
                clock_ticks: procfs::clock_ticks(),
                page_size: procfs::page_size(),
                samples: Arc::new(Mutex::new(HashMap::new())),
            },
            signals: Arc::new(KernelSignals),
        }
    }

    /// Resolves user and group references through `accounts` instead of through the system's NSS.
    #[must_use]
    pub fn with_accounts(mut self, accounts: Arc<dyn Accounts>) -> Self {
        self.reader.accounts = accounts;
        self
    }

    /// Measures the CPU sampling interval against `clock`.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.reader.clock = clock;
        self
    }

    /// Delivers signals through `signals`.
    #[must_use]
    pub fn with_signals(mut self, signals: Arc<dyn Signals>) -> Self {
        self.signals = signals;
        self
    }

    /// Treats the system as having booted `seconds` after the epoch, for a fixture whose
    /// `/proc/stat` is not the running kernel's.
    #[must_use]
    pub fn with_boot_time(mut self, seconds: i64) -> Self {
        self.reader.boot_seconds = Some(seconds);
        self
    }

    /// The pid a selector pins the query to, when one does.
    fn pinned_pid(query: &Query) -> Option<i64> {
        query
            .selectors()
            .iter()
            .find_map(|selector| match selector {
                Selector::Field { name, value } if name == "pid" => {
                    value.as_int().ok().and_then(|pid| i64::try_from(pid).ok())
                }
                Selector::Identity(id) if id.schema() == &schemas::process_id() => id
                    .values()
                    .first()
                    .and_then(|value| value.as_int().ok())
                    .and_then(|pid| i64::try_from(pid).ok()),
                _ => None,
            })
    }
}

impl Reader {
    /// The process ids `/proc` currently holds, in ascending order.
    fn pids(&self) -> Result<Vec<i64>, ErrorValue> {
        let entries = fs::read_dir(&self.proc_root).map_err(|error| {
            io_error(&error, &self.proc_root).with_help(
                "`process` needs a mounted procfs; without one the shell cannot see processes at \
                 all, which is not the same as there being none",
            )
        })?;
        let mut pids: Vec<i64> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
            .collect();
        pids.sort_unstable();
        Ok(pids)
    }

    /// The start time of `stat`, as the wall-clock instant spec §23.1 makes half the identity.
    fn started(&self, stat: &procfs::ProcStat) -> Option<Timestamp> {
        let boot = self.boot_seconds?;
        let ticks = self.clock_ticks;
        let seconds = i64::try_from(stat.starttime / ticks).ok()?;
        let nanoseconds = i64::try_from(stat.starttime % ticks * 1_000_000_000 / ticks).ok()?;
        timestamp(boot.checked_add(seconds)?, nanoseconds)
    }

    /// The share of one logical CPU used since the previous observation of the same process.
    fn cpu(&self, pid: i64, stat: &procfs::ProcStat) -> Option<f64> {
        let now = self.clock.now_nanos();
        let sample = Sample {
            ticks: stat.cpu_ticks(),
            at_nanos: now,
        };
        let previous = self
            .samples
            .lock()
            .ok()
            .and_then(|mut samples| samples.insert((pid, stat.starttime), sample))?;
        let elapsed = now.checked_sub(previous.at_nanos)?;
        if elapsed == 0 {
            return None;
        }
        let ticks = stat.cpu_ticks().checked_sub(previous.ticks)?;
        let seconds = elapsed as f64 / 1e9;
        Some(ticks as f64 / self.clock_ticks as f64 / seconds * 100.0)
    }

    /// Reads one process, or reports why it could not be read.
    async fn read(&self, pid: i64, schema: &Arc<Schema>) -> Result<RecordValue, ErrorValue> {
        let dir = self.proc_root.join(pid.to_string());
        let stat_path = dir.join("stat");
        let stat_text = fs::read_to_string(&stat_path).map_err(|error| {
            io_error(&error, &stat_path)
                .with_target(ValueRef::name(&format!("process {pid}")))
                .with_help("the process was listed and then read; it exited in between")
        })?;
        let stat = procfs::parse_stat(&stat_text).ok_or_else(|| malformed(&stat_path))?;

        let mut sources = vec![stat_path.display().to_string()];
        let started = self.started(&stat);
        let identity = self.identity_fields(&dir, &mut sources).await;
        let command = self.command(&dir, &mut sources);
        let executable = link(&dir.join("exe"), &mut sources);
        let cwd = link(&dir.join("cwd"), &mut sources);
        let service = self.service(&dir, &mut sources);
        let memory = u128::try_from(stat.rss_pages).map_or(Value::Null, |pages| {
            Value::ByteSize(ByteSize::from_bytes(pages.saturating_mul(self.page_size)))
        });

        let record = RecordValue::builder(
            Arc::clone(schema),
            provenance(PROVIDER_ID, schema.id(), &sources.join(" + ")),
        )
        .set("pid", Value::Int(i128::from(pid)))?
        .set("ppid", parent(stat.ppid))?
        .set("name", Value::string(&stat.comm))?
        .set("command", command)?
        .set("executable", executable)?
        .set("user", identity.user)?
        .set("group", identity.group)?
        .set("state", Value::string(procfs::state_name(stat.state)))?
        .set(
            "cpu",
            self.cpu(pid, &stat).map_or(Value::Null, Value::Float),
        )?
        .set("memory", memory)?
        .set(
            "virtual_mem",
            Value::ByteSize(ByteSize::from_bytes(u128::from(stat.vsize))),
        )?
        .set("threads", Value::Int(i128::from(stat.threads)))?
        .set("started", started.map_or(Value::Null, Value::Timestamp))?
        .set("cwd", cwd)?
        .set("service", service)?
        // No container provider claims this process; unknown is null, never a guess (spec §35.3).
        .set("container", Value::Null)?
        .build();
        Ok(record)
    }

    /// The `user` and `group` references, keeping a numeric id whichever way the read went.
    async fn identity_fields(&self, dir: &Path, sources: &mut Vec<String>) -> Identity {
        let status_path = dir.join("status");
        let (uid, gid) = match fs::read_to_string(&status_path) {
            Ok(text) => {
                sources.push(status_path.display().to_string());
                procfs::parse_status_ids(&text)
            }
            Err(error) => match fs::metadata(dir) {
                // `hidepid` can withhold `status` while the numeric owner of the process
                // directory stays readable. Spec §23.6 wants the numeric identity kept; falling
                // back to the directory's owner is how.
                Ok(metadata) => {
                    use std::os::unix::fs::MetadataExt as _;
                    sources.push(dir.display().to_string());
                    (Some(metadata.uid()), Some(metadata.gid()))
                }
                Err(_) => {
                    let failure = io_error(&error, &status_path).into_value();
                    return Identity {
                        user: failure.clone(),
                        group: failure,
                    };
                }
            },
        };

        let user = match uid {
            Some(uid) => {
                let name = self.accounts.user(uid).await.map(|account| account.name);
                schemas::require(&schemas::user_id())
                    .map_or_else(ErrorValue::into_value, |schema| {
                        user_ref(&schema, uid, name.as_deref())
                    })
            }
            None => Value::Null,
        };
        let group = match gid {
            Some(gid) => {
                let name = self.accounts.group(gid).await.map(|account| account.name);
                schemas::require(&schemas::group_id())
                    .map_or_else(ErrorValue::into_value, |schema| {
                        group_ref(&schema, gid, name.as_deref())
                    })
            }
            None => Value::Null,
        };
        Identity { user, group }
    }

    /// The argument vector: a list, `null` for a kernel thread, an error when it is hidden.
    fn command(&self, dir: &Path, sources: &mut Vec<String>) -> Value {
        let path = dir.join("cmdline");
        match fs::read(&path) {
            Ok(bytes) => {
                sources.push(path.display().to_string());
                let arguments = procfs::parse_cmdline(&bytes);
                if arguments.is_empty() {
                    // A kernel thread has no argument vector at all. That is an absence the
                    // kernel states, not a read that failed.
                    Value::Null
                } else {
                    Value::list(arguments.iter().map(|argument| Value::string(argument)))
                }
            }
            Err(error) => io_error(&error, &path).into_value(),
        }
    }

    /// The service unit claiming the process, where one does.
    fn service(&self, dir: &Path, sources: &mut Vec<String>) -> Value {
        let path = dir.join("cgroup");
        match fs::read_to_string(&path) {
            Ok(text) => {
                sources.push(path.display().to_string());
                procfs::service_unit(&text).map_or(Value::Null, |unit| Value::string(&unit))
            }
            // A kernel without cgroups has no unit to report: unknown, not unreadable.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
            Err(error) => io_error(&error, &path).into_value(),
        }
    }

    /// Re-reads a process's identity and refuses when it changed (ADR-0015 T13).
    fn confirm(&self, pid: i64, expected: Option<Timestamp>) -> Result<(), ErrorValue> {
        let stat_path = self.proc_root.join(pid.to_string()).join("stat");
        let stat_text = fs::read_to_string(&stat_path).map_err(|error| {
            io_error(&error, &stat_path)
                .with_target(ValueRef::name(&format!("process {pid}")))
                .with_help("nothing was signalled: the process is gone")
        })?;
        let stat = procfs::parse_stat(&stat_text).ok_or_else(|| malformed(&stat_path))?;
        let Some(expected) = expected else {
            // No recorded start time: the identity is the pid alone, which the schema documents
            // as vulnerable to reuse. There is nothing to confirm against.
            return Ok(());
        };
        if self.started(&stat) == Some(expected) {
            return Ok(());
        }
        Err(ErrorValue::new(
            ErrorCode::IoNotFound,
            format!("pid {pid} is no longer the process that was selected"),
        )
        .with_target(ValueRef::name(&format!("process {pid}")))
        .with_help(
            "the pid was reused between selection and signal, so nothing was signalled; run the \
             query again to select the process you meant",
        ))
    }
}

/// The pair of ownership references a process record carries.
struct Identity {
    user: Value,
    group: Value,
}

fn malformed(path: &Path) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderUnavailable,
        format!("{} is not in the layout proc(5) documents", path.display()),
    )
}

/// The parent id, or `null` for the process that has none.
fn parent(ppid: i64) -> Value {
    if ppid <= 0 {
        Value::Null
    } else {
        Value::Int(i128::from(ppid))
    }
}

/// A `/proc` magic link: a path, `null` when the kernel has none, an error when it is hidden.
fn link(path: &Path, sources: &mut Vec<String>) -> Value {
    match fs::read_link(path) {
        Ok(target) => {
            sources.push(path.display().to_string());
            Value::Path(Arc::from(target))
        }
        // A kernel thread has no executable and no working directory; the link is absent rather
        // than unreadable.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(error) => io_error(&error, path).into_value(),
    }
}

/// The signal an action names, defaulting to what the verb means.
fn requested_signal(action: &Action) -> Result<i32, ErrorValue> {
    match action.argument("signal") {
        Some(Value::Int(number)) => {
            i32::try_from(*number).map_err(|_| unknown_signal(&number.to_string()))
        }
        Some(Value::String(name)) => signal_number(name).ok_or_else(|| unknown_signal(name)),
        Some(other) => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("a signal is a name or a number, not {}", other.type_name()),
        )),
        // `docs/spec/commands/process.yaml`: `kill` defaults to SIGKILL, and that default is what
        // distinguishes it from `stop`, which asks for graceful termination.
        None if action.operation() == "kill" => Ok(9),
        None => Ok(15),
    }
}

fn unknown_signal(name: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderUnsupported,
        format!("`{name}` is not a signal this kernel defines"),
    )
    .with_help("signals are named as in signal(7), for example SIGTERM, SIGKILL or SIGHUP")
}

/// The number of a signal named as `signal(7)` names it, with or without the `SIG` prefix.
fn signal_number(name: &str) -> Option<i32> {
    if let Ok(number) = name.parse::<i32>() {
        return Some(number);
    }
    let bare = name
        .strip_prefix("SIG")
        .or_else(|| name.strip_prefix("sig"))
        .unwrap_or(name)
        .to_ascii_uppercase();
    nix::sys::signal::Signal::iterator()
        .find(|signal| signal.as_str().trim_start_matches("SIG") == bare)
        .map(|signal| signal as i32)
}

#[async_trait::async_trait]
impl Provider for ProcessProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["process"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        schemas::require(&schemas::process_id())
            .into_iter()
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("process.list", Risk::Read),
            Capability::new("process.inspect", Risk::Read),
            Capability::new("process.signal", Risk::Mutate),
        ]
    }

    fn availability(&self) -> Availability {
        if self.reader.proc_root.is_dir() {
            Availability::Available
        } else {
            Availability::unavailable(format!(
                "{} is not a mounted proc filesystem",
                self.reader.proc_root.display()
            ))
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let schema = schemas::require(&schemas::process_id())?;
        let reader = self.reader.clone();
        let pinned = Self::pinned_pid(query);
        let pids = match pinned {
            Some(pid) => vec![pid],
            None => reader.pids()?,
        };
        let enumerating = pinned.is_none();
        let limit = query.max().unwrap_or(usize::MAX);
        let query = query.clone();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let mut sent = 0;
                for pid in pids {
                    if sent >= limit {
                        break;
                    }
                    match reader.read(pid, &schema).await {
                        Ok(record) => {
                            if !understood_selectors_match(&query, &record) {
                                continue;
                            }
                            if sink.send(record.into_value()).await.is_err() {
                                return;
                            }
                            sent += 1;
                        }
                        // A process that exits between the listing and the detail read is not
                        // part of the answer: it no longer exists, and omitting something that
                        // is not there is not the same as hiding a failure (ADR-0029). Every
                        // other way a read can fail — a process that is there and cannot be
                        // read — still lands on the error channel with its identity, and so
                        // does a process the user named by pid, because that one is a target.
                        Err(error) => {
                            if enumerating && error.code() == ErrorCode::IoNotFound {
                                continue;
                            }
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
        let schema = schemas::require(&schemas::process_id())?;
        let pids =
            match ProcessProvider::pinned_pid(&Query::target("process").with(selector.clone())) {
                Some(pid) => vec![pid],
                None => self.reader.pids()?,
            };
        let mut found = Vec::new();
        for pid in pids {
            if let Ok(record) = self.reader.read(pid, &schema).await
                && selector.matches(&record)
                && let Some(reference) = ObjectRef::of(&record)
            {
                found.push(reference);
            }
        }
        Ok(found)
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let operation = action.operation();
        if !matches!(operation, "signal" | "kill" | "stop") {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{PROVIDER_ID} has no operation `{operation}`"),
            ));
        }
        let target = action.target();
        let Some(pid) = target
            .values()
            .first()
            .and_then(|value| value.as_int().ok())
            .and_then(|pid| i32::try_from(pid).ok())
        else {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                "a process is identified by its pid and its start time",
            ));
        };
        let expected = target
            .values()
            .get(1)
            .and_then(|value| value.as_timestamp().ok());

        let signal = requested_signal(action)?;
        if let Err(error) = self.reader.confirm(i64::from(pid), expected) {
            return Ok(ActionOutcome::failed(action, error));
        }
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!("would send signal {signal} to pid {pid}"),
            ));
        }
        match self.signals.send(pid, signal) {
            Ok(()) => Ok(ActionOutcome::succeeded(action, true)),
            Err(error) => Ok(ActionOutcome::failed(action, error)),
        }
    }
}

/// Whether the selectors this provider understands accept `record`.
///
/// Selectors it does not understand are left to the pipeline, which is what [`Query`] permits:
/// pushing a selector down is an optimisation, never a correctness condition.
fn understood_selectors_match(query: &Query, record: &RecordValue) -> bool {
    query.selectors().iter().all(|selector| match selector {
        Selector::Field { name, .. } if name == "pid" || name == "name" => selector.matches(record),
        Selector::Contains { name, .. } if name == "name" => selector.matches(record),
        Selector::Identity(id) if id.schema() == &schemas::process_id() => selector.matches(record),
        _ => true,
    })
}
