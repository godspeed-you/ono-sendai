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

/// Reading and setting a process's scheduling niceness.
///
/// Injectable for the same reason as [`Signals`]: a test that proves the refusal paths must not
/// be able to renice an unrelated process on the machine it runs on.
pub trait Priorities: Send + Sync + std::fmt::Debug {
    /// The niceness of `pid`.
    ///
    /// # Errors
    ///
    /// Returns the structured form of the kernel's refusal.
    fn get(&self, pid: i32) -> Result<i32, ErrorValue>;

    /// Sets the niceness of `pid`.
    ///
    /// # Errors
    ///
    /// Returns the structured form of the kernel's refusal — `io.permission_denied` when raising
    /// priority without `CAP_SYS_NICE`.
    fn set(&self, pid: i32, niceness: i32) -> Result<(), ErrorValue>;
}

/// `getpriority(2)` and `setpriority(2)`.
#[derive(Debug, Default)]
pub struct KernelPriorities;

impl KernelPriorities {
    fn pid(pid: i32) -> Result<rustix::process::Pid, ErrorValue> {
        rustix::process::Pid::from_raw(pid).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("{pid} is not a process id"),
            )
        })
    }

    fn failure(errno: rustix::io::Errno, pid: i32) -> ErrorValue {
        errno_error(
            nix::errno::Errno::from_raw(errno.raw_os_error()),
            &PathBuf::from(format!("/proc/{pid}")),
        )
    }
}

impl Priorities for KernelPriorities {
    fn get(&self, pid: i32) -> Result<i32, ErrorValue> {
        rustix::process::getpriority_process(Some(Self::pid(pid)?))
            .map_err(|errno| Self::failure(errno, pid))
    }

    fn set(&self, pid: i32, niceness: i32) -> Result<(), ErrorValue> {
        rustix::process::setpriority_process(Some(Self::pid(pid)?), niceness)
            .map_err(|errno| Self::failure(errno, pid))
    }
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
    /// How long the machine had been up when this reader was made, from `/proc/uptime`. It is
    /// read once per query rather than once per process: every process of one snapshot is
    /// measured against the same instant (ADR-0232).
    uptime_seconds: Option<f64>,
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
    priorities: Arc<dyn Priorities>,
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
                uptime_seconds: procfs::uptime_seconds(&proc_root),
                proc_root,
                accounts: Arc::new(NssAccounts::new()),
                clock: Arc::new(SystemClock::new()),
                clock_ticks: procfs::clock_ticks(),
                page_size: procfs::page_size(),
                samples: Arc::new(Mutex::new(HashMap::new())),
            },
            signals: Arc::new(KernelSignals),
            priorities: Arc::new(KernelPriorities),
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

    /// Reads and sets niceness through `priorities`.
    #[must_use]
    pub fn with_priorities(mut self, priorities: Arc<dyn Priorities>) -> Self {
        self.priorities = priorities;
        self
    }

    /// Treats the system as having booted `seconds` after the epoch, for a fixture whose
    /// `/proc/stat` is not the running kernel's.
    #[must_use]
    pub fn with_boot_time(mut self, seconds: i64) -> Self {
        self.reader.boot_seconds = Some(seconds);
        self
    }

    /// `set process --priority N` (ADR-0092): the niceness, through `setpriority(2)`.
    ///
    /// The identity is confirmed first, as for a signal, so a recycled pid is never reniced.
    /// A refusal — raising priority without `CAP_SYS_NICE` — is the target's failed outcome
    /// with the kernel's `io.permission_denied`, never an error that stops a bulk `set`.
    fn set_attributes(
        &self,
        action: &Action,
        pid: i32,
        expected: Option<Timestamp>,
    ) -> Result<ActionOutcome, ErrorValue> {
        let niceness = match action.argument("priority") {
            Some(Value::Int(niceness)) => i32::try_from(*niceness)
                .ok()
                .filter(|niceness| (-20..=19).contains(niceness))
                .ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("a niceness is -20 to 19, not {niceness}"),
                    )
                })?,
            Some(other) => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--priority` is a niceness, not {}", other.type_name()),
                ));
            }
            None => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "`set process` changes nothing without an attribute to set",
                )
                .with_help("`set process <pid> --priority N` sets the scheduling niceness"));
            }
        };
        if let Err(error) = self.reader.confirm(i64::from(pid), expected) {
            return Ok(ActionOutcome::failed(action, error));
        }
        let current = match self.priorities.get(pid) {
            Ok(current) => current,
            Err(error) => return Ok(ActionOutcome::failed(action, error)),
        };
        if current == niceness {
            return Ok(ActionOutcome::skipped(
                action,
                format!("pid {pid} already has niceness {niceness}"),
            ));
        }
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!("would set the niceness of pid {pid} from {current} to {niceness}"),
            ));
        }
        match self.priorities.set(pid, niceness) {
            Ok(()) => Ok(ActionOutcome::succeeded(action, true)),
            Err(error) => Ok(ActionOutcome::failed(action, error)),
        }
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
    /// The same reader, with the machine's uptime read again — once per query, so every process
    /// of one answer is measured over a window ending at the same instant (ADR-0232).
    fn refreshed(&self) -> Self {
        let mut reader = self.clone();
        reader.uptime_seconds = procfs::uptime_seconds(&self.proc_root);
        reader
    }

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

    /// The share of one logical CPU the process used, and the window it is the share over.
    ///
    /// A share is a rate, and a rate needs two readings. Where an earlier observation of the same
    /// process exists — a second `get process` in a session, a `watch`, or the extra reading
    /// `--sample` paid for — the window is the interval between them, and the answer is what the
    /// process is doing now. Where none does, the window is the process's own lifetime, which the
    /// kernel states in full: `starttime` against `/proc/uptime`. Both are shares of one logical
    /// CPU; `cpu_window` is what tells them apart (ADR-0232).
    ///
    /// `None` only when the kernel gave neither — no `/proc/uptime` and no earlier reading.
    fn cpu(&self, pid: i64, stat: &procfs::ProcStat) -> Option<(f64, ono_value::Duration)> {
        let now = self.clock.now_nanos();
        let sample = Sample {
            ticks: stat.cpu_ticks(),
            at_nanos: now,
        };
        let previous = self
            .samples
            .lock()
            .ok()
            .and_then(|mut samples| samples.insert((pid, stat.starttime), sample));
        if let Some(previous) = previous
            && let Some(elapsed) = now.checked_sub(previous.at_nanos)
            && elapsed > 0
            && let Some(ticks) = stat.cpu_ticks().checked_sub(previous.ticks)
        {
            let seconds = elapsed as f64 / 1e9;
            let window = i128::try_from(elapsed).ok()?;
            return Some((
                ticks as f64 / self.clock_ticks as f64 / seconds * 100.0,
                ono_value::Duration::from_nanoseconds(window),
            ));
        }
        self.lifetime_share(stat)
    }

    /// The share of one logical CPU the process has used since it started.
    ///
    /// The window is `uptime - starttime`, both measured from the same boot, so it needs no wall
    /// clock and no second reading. This is the `%CPU` of `ps(1)`.
    fn lifetime_share(&self, stat: &procfs::ProcStat) -> Option<(f64, ono_value::Duration)> {
        let uptime = self.uptime_seconds?;
        let started = stat.starttime as f64 / self.clock_ticks as f64;
        let lifetime = uptime - started;
        if lifetime <= 0.0 || !lifetime.is_finite() {
            return None;
        }
        let used = stat.cpu_ticks() as f64 / self.clock_ticks as f64;
        let window = ono_value::Duration::from_nanoseconds((lifetime * 1e9).round() as i128);
        Some((used / lifetime * 100.0, window))
    }

    /// Records the CPU counters of `pids` so the reads that follow can be rates against them
    /// (`--sample`, ADR-0232).
    fn sample_now(&self, pids: &[i64]) {
        let now = self.clock.now_nanos();
        let Ok(mut samples) = self.samples.lock() else {
            return;
        };
        for pid in pids {
            let path = self.proc_root.join(pid.to_string()).join("stat");
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Some(stat) = procfs::parse_stat(&text) else {
                continue;
            };
            samples.insert(
                (*pid, stat.starttime),
                Sample {
                    ticks: stat.cpu_ticks(),
                    at_nanos: now,
                },
            );
        }
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
        let share = self.cpu(pid, &stat);
        let identity = self.identity_fields(&dir, &mut sources).await;
        let command = self.command(&dir, &mut sources);
        let executable = link(&dir.join("exe"), &mut sources);
        let cwd = link(&dir.join("cwd"), &mut sources);
        let service = self.service(&dir, &mut sources);
        let pid_namespace = namespace_inode(&dir, "pid", &mut sources);
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
            share.map_or(Value::Null, |(share, _)| Value::Float(share)),
        )?
        .set(
            "cpu_window",
            share.map_or(Value::Null, |(_, window)| Value::Duration(window)),
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
        .set("pid_namespace", pid_namespace)?
        .build();
        Ok(record)
    }

    /// Reads the detail view of one process (spec §33.1): the record, plus what only a closer
    /// look answers — the parent by name, the cgroup, the open files and the sockets.
    async fn read_detail(
        &self,
        pid: i64,
        process: &Arc<Schema>,
        detail: &Arc<Schema>,
    ) -> Result<RecordValue, ErrorValue> {
        let record = self.read(pid, process).await?;
        let dir = self.proc_root.join(pid.to_string());
        let mut sources = vec![record.provenance().source().unwrap_or_default().to_owned()];

        let mut builder =
            RecordValue::builder(Arc::clone(detail), provenance(PROVIDER_ID, detail.id(), ""));
        for field in detail.fields() {
            if let Some(value) = record.get(field.name()) {
                builder = builder.set(field.name(), value.clone())?;
            }
        }
        let parent = match record.get("ppid") {
            Some(Value::Int(ppid)) => self.parent_ref(*ppid, process, &mut sources),
            _ => Value::Null,
        };
        let (open_files, sockets) = descriptors(&dir, &mut sources);
        let record = builder
            .set("parent", parent)?
            .set("cgroup", cgroup(&dir, &mut sources))?
            .set("open_files", open_files)?
            .set("sockets", sockets)?
            .provenance(provenance(PROVIDER_ID, detail.id(), &sources.join(" + ")))
            .build();
        Ok(record)
    }

    /// The parent as a reference: its pid, and — while the parent can still be read — its name
    /// and the start time that completes its identity.
    fn parent_ref(&self, ppid: i128, process: &Arc<Schema>, sources: &mut Vec<String>) -> Value {
        let stat_path = self.proc_root.join(ppid.to_string()).join("stat");
        let stat = fs::read_to_string(&stat_path)
            .ok()
            .and_then(|text| procfs::parse_stat(&text));
        if stat.is_some() {
            sources.push(stat_path.display().to_string());
        }
        let mut reference = RecordValue::builder(
            Arc::clone(process),
            provenance(PROVIDER_ID, process.id(), &stat_path.display().to_string()),
        );
        // The schema declares these fields; a name the schema declares cannot be unknown to it.
        if let Ok(with_pid) = reference.clone().set("pid", Value::Int(ppid)) {
            reference = with_pid;
        }
        if let Some(stat) = stat {
            if let Ok(with_name) = reference.clone().set("name", Value::string(&stat.comm)) {
                reference = with_name;
            }
            if let Some(started) = self.started(&stat)
                && let Ok(with_started) =
                    reference.clone().set("started", Value::Timestamp(started))
            {
                reference = with_started;
            }
        }
        Value::Record(Arc::new(reference.build()))
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

/// The unified-hierarchy control group path of `/proc/<pid>/cgroup`: a path, `null` on a kernel
/// without cgroups, an error when the file is hidden.
fn cgroup(dir: &Path, sources: &mut Vec<String>) -> Value {
    let path = dir.join("cgroup");
    match fs::read_to_string(&path) {
        Ok(text) => {
            sources.push(path.display().to_string());
            // `hierarchy:controllers:path`; the unified hierarchy is `0::<path>`, and on a v1
            // machine the first line's path is still the process's group.
            text.lines()
                .filter_map(|line| line.splitn(3, ':').nth(2))
                .find(|group| !group.is_empty())
                .map_or(Value::Null, |group| {
                    Value::Path(Arc::from(Path::new(group)))
                })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(error) => io_error(&error, &path).into_value(),
    }
}

/// What `/proc/<pid>/fd` holds: the open files as paths, and the sockets as inodes. Both carry
/// the read error when this user may not look into the descriptor table (spec §10.5).
fn descriptors(dir: &Path, sources: &mut Vec<String>) -> (Value, Value) {
    let path = dir.join("fd");
    let entries = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) => {
            let failure = io_error(&error, &path).into_value();
            return (failure.clone(), failure);
        }
    };
    sources.push(path.display().to_string());
    let mut numbered: Vec<(u32, String)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let fd = entry.file_name().to_str()?.parse::<u32>().ok()?;
            // A descriptor closed between the listing and the read is a process going about
            // its business, not a failure.
            let target = fs::read_link(entry.path()).ok()?;
            Some((fd, target.to_string_lossy().into_owned()))
        })
        .collect();
    numbered.sort_unstable();
    let mut files = Vec::new();
    let mut sockets = Vec::new();
    for (_, target) in numbered {
        if let Some(inode) = target
            .strip_prefix("socket:[")
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|inode| inode.parse::<i128>().ok())
        {
            sockets.push(Value::Int(inode));
        } else if target.starts_with('/') && !target.ends_with(" (deleted)") {
            files.push(Value::Path(Arc::from(Path::new(&target))));
        }
    }
    (Value::list(files), Value::list(sockets))
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

/// The inode of one of `/proc/<pid>/ns/<kind>`, which the kernel spells `pid:[4026531836]`.
///
/// v0.4 §10.2 makes the pid namespace part of a process's spatial identity: the same pid number
/// means different processes in different namespaces, so a container's pid 1 and the host's pid 1
/// must not reduce to one identity. A link nobody can read is null — never the root namespace,
/// which would be a guess (spec §35.3).
fn namespace_inode(dir: &Path, kind: &str, sources: &mut Vec<String>) -> Value {
    let path = dir.join("ns").join(kind);
    match fs::read_link(&path) {
        Ok(target) => {
            let text = target.to_string_lossy();
            let inode = text
                .strip_prefix(&format!("{kind}:["))
                .and_then(|rest| rest.strip_suffix(']'))
                .and_then(|digits| digits.parse::<u64>().ok());
            match inode {
                Some(inode) => {
                    sources.push(path.display().to_string());
                    Value::Int(i128::from(inode))
                }
                // A link in a shape this shell does not model is not an inode it may invent.
                None => Value::Null,
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            io_error(&error, &path).into_value()
        }
        // `readlink` on a namespace link fails with EACCES for another user's process on some
        // kernels and with ENOENT on a kernel built without namespaces; neither is an inode.
        Err(_) => Value::Null,
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
        // `signal` is the target of `send signal`, whose objects are the processes that arrive
        // through the pipeline (ADR-0092 §2); nothing enumerates it.
        &["process", "signal"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        [schemas::process_id(), schemas::process_detail_id()]
            .iter()
            .filter_map(|id| schemas::require(id).ok())
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("process.list", Risk::Read),
            Capability::new("process.inspect", Risk::Read),
            Capability::new("process.signal", Risk::Mutate),
            Capability::new("process.set", Risk::Mutate),
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
        // `inspect process` asks for the detail view (spec §33.1): the same enumeration, each
        // process read closer (ADR-0091).
        let detail = query
            .flag("detail")
            .then(|| schemas::require(&schemas::process_detail_id()))
            .transpose()?;
        let reader = self.reader.refreshed();
        let pinned = Self::pinned_pid(query);
        let pids = match pinned {
            Some(pid) => vec![pid],
            None => reader.pids()?,
        };
        let enumerating = pinned.is_none();
        let limit = query.max().unwrap_or(usize::MAX);
        let tree = query.flag("tree");
        // `--sample <duration>`: buy a rate over an interval the caller chose, by reading the CPU
        // counters now and answering against them after the interval (ADR-0232).
        let sample = match query.option_value("sample") {
            Some(Value::Duration(interval)) if !interval.is_negative() => {
                Some(std::time::Duration::from_nanos(
                    u64::try_from(interval.nanoseconds()).unwrap_or(u64::MAX),
                ))
            }
            Some(Value::Duration(interval)) => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--sample` is an interval to wait, and {interval} runs backwards"),
                ));
            }
            Some(other) => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--sample` is a duration, not {}", other.type_name()),
                ));
            }
            None => None,
        };
        let query = query.clone();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                if let Some(interval) = sample {
                    reader.sample_now(&pids);
                    tokio::time::sleep(interval).await;
                }
                if tree {
                    // `--tree` needs the whole table before the first root can be emitted: a
                    // root is a process whose parent is not in the stream (ADR-0091 §3).
                    let mut records = Vec::new();
                    for pid in pids {
                        match reader.read(pid, &schema).await {
                            Ok(record) if understood_selectors_match(&query, &record) => {
                                records.push(record);
                            }
                            Ok(_) => {}
                            Err(error) if error.code() == ErrorCode::IoNotFound => {}
                            Err(error) => {
                                if sink.fail(error).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    for root in nest(records).into_iter().take(limit) {
                        if sink.send(root).await.is_err() {
                            return;
                        }
                    }
                    return;
                }
                let mut sent = 0;
                for pid in pids {
                    if sent >= limit {
                        break;
                    }
                    let read = match &detail {
                        Some(detail) => reader.read_detail(pid, &schema, detail).await,
                        None => reader.read(pid, &schema).await,
                    };
                    match read {
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
        let reader = self.reader.refreshed();
        let pids =
            match ProcessProvider::pinned_pid(&Query::target("process").with(selector.clone())) {
                Some(pid) => vec![pid],
                None => reader.pids()?,
            };
        let mut found = Vec::new();
        for pid in pids {
            if let Ok(record) = reader.read(pid, &schema).await
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
        if !matches!(operation, "signal" | "send" | "kill" | "stop" | "set") {
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

        if operation == "set" {
            return self.set_attributes(action, pid, expected);
        }
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

/// The process tree of `records`: the roots, each carrying its descendants under the extension
/// key `children` (spec §10.4, ADR-0091 §3).
///
/// Built deepest-first rather than recursively, so a chain of parents as long as the table
/// cannot overflow the stack. A parent that is not among the records — filtered out, or pid 0 —
/// makes its children roots.
fn nest(records: Vec<RecordValue>) -> Vec<Value> {
    let pid_of = |record: &RecordValue| match record.get("pid") {
        Some(Value::Int(pid)) => Some(*pid),
        _ => None,
    };
    let parent_of = |record: &RecordValue| match record.get("ppid") {
        Some(Value::Int(ppid)) => Some(*ppid),
        _ => None,
    };
    let present: HashMap<i128, usize> = records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| pid_of(record).map(|pid| (pid, index)))
        .collect();
    let parent = |record: &RecordValue| parent_of(record).filter(|ppid| present.contains_key(ppid));

    // The depth of each process is the length of its chain of present parents; a cycle —
    // impossible in a kernel table, cheap to guard against in a fixture — ends the walk.
    let depth_of = |record: &RecordValue| {
        let mut depth = 0usize;
        let mut current = parent(record);
        while let Some(ppid) = current
            && depth <= records.len()
        {
            depth += 1;
            current = present
                .get(&ppid)
                .and_then(|index| parent(&records[*index]));
        }
        depth
    };
    let mut order: Vec<(usize, usize)> = records
        .iter()
        .enumerate()
        .map(|(index, record)| (depth_of(record), index))
        .collect();
    order.sort_by(|left, right| right.cmp(left));

    let mut children: HashMap<i128, Vec<Value>> = HashMap::new();
    let mut roots = Vec::new();
    for (_, index) in order {
        let record = &records[index];
        let own = pid_of(record)
            .and_then(|pid| children.remove(&pid))
            .unwrap_or_default();
        let value = with_children(record, own);
        match parent(record) {
            Some(ppid) => children.entry(ppid).or_default().push(value),
            None => roots.push(value),
        }
    }
    // Deepest-first building reversed the order within each level; the table's order is pid
    // order, which is what a reader expects of siblings.
    roots.reverse();
    roots
}

/// `record` with `children` nested beneath it.
fn with_children(record: &RecordValue, mut children: Vec<Value>) -> Value {
    children.reverse();
    let mut builder =
        RecordValue::builder(Arc::clone(record.schema()), record.provenance().clone());
    for field in record.schema().fields() {
        if let Some(value) = record.get(field.name())
            && let Ok(with_field) = builder.clone().set(field.name(), value.clone())
        {
            builder = with_field;
        }
    }
    builder
        .set_extra("children", Value::list(children))
        .build()
        .into_value()
}

/// Whether the selectors this provider understands accept `record`.
///
/// Selectors it does not understand are left to the pipeline, which is what [`Query`] permits:
/// pushing a selector down is an optimisation, never a correctness condition.
fn understood_selectors_match(query: &Query, record: &RecordValue) -> bool {
    // Every selector filters. Spec §27.1 allows a provider to push a selector down or to filter
    // by it afterwards; ignoring one is the one thing it may not do, because an ignored selector
    // widens silently — inside a context frame (spec §14.3) that would mean `get process`
    // answering with the whole machine while the prompt says `service/nginx`. `pid` is pushed
    // down (the enumeration reads one directory); everything else filters here, against the
    // record, which knows every field the schema declares.
    query
        .selectors()
        .iter()
        .all(|selector| selector.matches(record))
        && ownership_matches(query, "user", "uid", record)
        && ownership_matches(query, "group", "gid", record)
}

/// Whether the `--user` / `--group` option, if given, accepts `record`.
///
/// The option is a `ref<ono.user/1>` written as a word — `root` or `0` — and the record carries
/// the reference with both the name and the numeric id, so either spelling matches. A process
/// whose ownership could not be read matches nothing: unknown is not equal to anything
/// (ADR-0014).
fn ownership_matches(query: &Query, option: &str, id_field: &str, record: &RecordValue) -> bool {
    let Some(wanted) = query.option_value(option) else {
        return true;
    };
    let Some(Value::Record(reference)) = record.get(option) else {
        return false;
    };
    match wanted {
        Value::Int(id) => reference.get(id_field) == Some(&Value::Int(*id)),
        Value::String(text) => {
            reference.get("name") == Some(&Value::String(text.clone()))
                || text
                    .parse::<i128>()
                    .is_ok_and(|id| reference.get(id_field) == Some(&Value::Int(id)))
        }
        _ => false,
    }
}
