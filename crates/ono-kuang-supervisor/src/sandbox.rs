//! What a `native-process` instance runs inside (spec §31.10, §31.15, §31.34).
//!
//! Spec §31.10 puts a native out-of-process host at tier T1: "a separate process, native
//! executable protocol". A separate process is where the isolation *starts*, not where it ends —
//! by default a child inherits the shell's environment, its working directory, its process group,
//! and every resource ceiling the user's login shell was given. The package's own declaration
//! then means nothing: spec §31.15 requires "per-plugin memory ceilings" and "CPU/fuel/budget
//! controls appropriate to runtime", and a `memory_max` nobody applies is neither.
//!
//! This module is what the host actually applies before the artifact runs. Everything here is
//! set in the child between `fork` and `exec`, so the artifact never executes an instruction
//! outside it:
//!
//! - **memory** — `RLIMIT_DATA` at the negotiated `memory_max`. `RLIMIT_DATA` bounds the brk and
//!   the private anonymous mappings, which is the memory a package *allocates*; `RLIMIT_AS`
//!   would instead bound reserved address space, most of which in any threaded program is stack
//!   guard pages nobody ever touches, and would refuse a package that allocated nothing.
//! - **scheduling** — the `cpu_budget` class as a nice level. Spec §31.15 calls `cpu_budget` a
//!   scheduling class and §31.67 requires that a plugin "must never block terminal input", which
//!   is a priority statement, not a CPU-seconds quota.
//! - **file size, open files, core dumps** — bounded, so a package cannot exhaust the shell's
//!   descriptors, fill the disk, or write its address space to a file (spec §31.15, §31.80).
//!   The number of *processes* is deliberately not bounded here: `RLIMIT_NPROC` counts every
//!   process the real user owns, so setting it for one package would refuse a fork because of
//!   processes that package never created, on a machine where the user is simply busy. Bounding
//!   what one package spawns needs a cgroup `pids.max`, which needs a delegated cgroup the shell
//!   does not have as an unprivileged user (ADR-0283).
//! - **its own session** — `setsid`, so the package cannot signal the shell's process group and a
//!   Ctrl-C meant for the pipeline does not reach it.
//! - **no new privileges** — `PR_SET_NO_NEW_PRIVS`, so a setuid binary the package execs cannot
//!   gain authority the package was never granted (spec §31.80).
//! - **a working directory and an environment it did not choose** — the package starts in its own
//!   private directory with an environment built from nothing, so the user's cwd and the shell's
//!   variables (which carry tokens, paths and session identity) are not a side channel.
//!
//! What it deliberately does not claim: this is confinement of *resources and inheritance*, not
//! of the filesystem or the network. A native-process package can still open any file its user
//! can. `Sandbox::filesystem` and `Sandbox::network` say so in as many words, because spec §31.16
//! forbids presenting a scope as a security boundary when it is not one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_kuang_protocol::{
    Control, CpuBudget, ExecutionTier, KuangError, KuangErrorCode, Requirement,
};

use crate::platform::{ConfinementPlan, ConfinementPlatform, PLATFORM_CONTROLS};
use crate::report::{self, ConfinementReport, ControlResult};

/// The confinement one instance was started under, as it was actually applied (spec §31.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sandbox {
    /// The named execution tier this instance runs in (v0.4.1 §17.2).
    ///
    /// A name rather than a boolean, so that "which controls are in force" has an answer that
    /// stronger isolation can extend without changing what the old answer meant (§17.3).
    pub tier: ExecutionTier,
    /// The ceiling on allocated memory in bytes: `RLIMIT_DATA`.
    pub memory_max: u64,
    /// The scheduling class the package declared.
    pub cpu_class: CpuBudget,
    /// The nice level that class becomes.
    pub nice: i32,
    /// The ceiling on open descriptors.
    pub open_files: u64,
    /// The ceiling on the size of any single file the package writes, in bytes.
    pub file_size: u64,
    /// The directory the instance starts in and the only one it is given.
    pub working_directory: PathBuf,
    /// The environment variable names the instance receives. Nothing else is inherited.
    pub environment: &'static [&'static str],
    /// How far the host confines the package's filesystem access.
    pub filesystem: Confinement,
    /// How far the host confines the package's network access.
    pub network: Confinement,
}

/// How a resource class is confined, in the honest vocabulary of spec §31.16: a scope that the
/// host cannot enforce is never presented as if it were a boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confinement {
    /// The kernel refuses what the scope does not allow, whatever the package tries.
    Kernel,
    /// The host refuses what the scope does not allow *when the package asks the host*. A
    /// `native-process` package that goes around the host API is not stopped by this.
    Broker,
}

impl Confinement {
    /// The word the record and the operator see.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Confinement::Kernel => "kernel",
            Confinement::Broker => "broker",
        }
    }
}

/// The nice level a scheduling class becomes (spec §31.15, §31.67).
///
/// `interactive` keeps the default priority, because §31.67's budget for a plugin-backed stage is
/// measured in milliseconds and a niced process misses it. `batch` and `background` step down, so
/// an analysis that runs for minutes yields to the prompt.
#[must_use]
pub const fn nice_of(class: CpuBudget) -> i32 {
    match class {
        CpuBudget::Interactive => 0,
        CpuBudget::Batch => 5,
        CpuBudget::Background => 10,
    }
}

/// The confinement for a `native-process` instance with `memory_max` bytes of allocation, the
/// declared scheduling class, and `working_directory` to run in.
#[must_use]
pub fn native_process(
    memory_max: u64,
    cpu_class: CpuBudget,
    working_directory: PathBuf,
) -> Sandbox {
    Sandbox {
        tier: ExecutionTier::NativeConfined,
        memory_max,
        cpu_class,
        nice: nice_of(cpu_class),
        // Enough for the protocol pipes, the artifact's own mappings and a working set of files;
        // far below the shell's own ceiling, so a descriptor leak in a package is the package's
        // problem (spec §31.34).
        open_files: 256,
        // 64 MiB: enough for a package to write a report, not enough to fill a disk unnoticed.
        file_size: 64 * 1024 * 1024,
        working_directory,
        environment: ENVIRONMENT,
        // Nothing here confines the filesystem or the network: the broker refuses a host call
        // outside the granted scope, and a native process that opens a file itself is not asking
        // the host at all.
        filesystem: Confinement::Broker,
        network: Confinement::Broker,
    }
}

/// The environment names an instance receives. Everything else the shell holds — tokens, paths,
/// session identity, the user's own variables — stops at the boundary (spec §31.80).
///
/// `LC_ALL` and `TZ` are fixed rather than inherited, because spec §31.69 requires a plugin's
/// behaviour to be reproducible and a locale is exactly the kind of ambient difference that makes
/// it not.
const ENVIRONMENT: &[&str] = &["PATH", "HOME", "LC_ALL", "TZ"];

/// The sandbox of a component (ADR-0569): the ceilings the runtime enforces, and the ones a
/// process would have and a component has no use for.
#[must_use]
pub fn wasm_component(
    memory_max: u64,
    cpu_class: CpuBudget,
    working_directory: PathBuf,
) -> Sandbox {
    Sandbox {
        tier: ExecutionTier::Wasm,
        memory_max,
        cpu_class,
        nice: 0,
        open_files: 0,
        file_size: 0,
        working_directory,
        environment: &[],
        filesystem: Confinement::Kernel,
        network: Confinement::Kernel,
    }
}

/// Spawns `command` under `sandbox`, or fails before a single plugin instruction runs.
///
/// This is the boundary v0.4.1 §2.3 talks about: *"If Ono claims that a safety control is applied
/// before an operation, failure to apply that control MUST prevent the operation from starting."*
/// Every control the tier calls mandatory is installed and its result checked (§16.2); a
/// mandatory refusal abandons the spawn and returns the structured error of §16.3 naming the
/// control (§67.3); a best-effort refusal is recorded and the spawn proceeds (§16.4). The
/// [`ConfinementReport`] that comes back with the child is §16.5's, and it comes back only when
/// every required control reads `applied`.
///
/// The working directory is created if it does not exist. A package that cannot be given its own
/// directory is not started somewhere else — `working_directory` is mandatory, so the load fails
/// with the operating system's own reason.
///
/// `package` is what the error calls the thing that did not start: the package id where the
/// caller has one.
///
/// # Errors
///
/// `plugin.confinement_failed`, `plugin.resource_limit_failed` or `plugin.no_new_privs_failed`
/// when a mandatory control could not be installed, and `load.runtime_unavailable` when the
/// artifact itself could not be executed.
pub fn spawn(
    command: &mut tokio::process::Command,
    sandbox: &Sandbox,
    platform: &Arc<dyn ConfinementPlatform>,
    package: &str,
) -> Result<(tokio::process::Child, ConfinementReport), KuangError> {
    let outcomes = Arc::new(report::Outcomes::map().map_err(|error| {
        KuangError::new(
            KuangErrorCode::PluginConfinementFailed,
            format!(
                "{package} was not started because the supervisor could not open the channel a \
                 confined child reports its controls through: {error}"
            ),
        )
    })?);
    let parent = apply(command, sandbox);

    // A control the parent installs — the private directory, the sanitized environment, the
    // protocol descriptors — is decided before the fork, so its failure costs no child at all.
    let before = report::parent_only(sandbox.tier, &parent);
    if let Some(refusal) = before.refusal(package) {
        return Err(refusal);
    }

    apply_limits(command, sandbox, platform, &outcomes);

    match command.spawn() {
        Ok(child) => {
            let report = report::build(sandbox.tier, &outcomes, &parent);
            if let Some(refusal) = report.refusal(package) {
                // Unreachable while `install_controls` returns `Err` on a mandatory refusal, and
                // deliberately not an assertion: §16.5's invariant is that a *successful* spawn
                // implies every required control applied, so the honest thing to do with a child
                // that contradicts it is to not have one.
                drop(child);
                return Err(refusal);
            }
            Ok((child, report))
        }
        Err(error) => {
            let report = report::build(sandbox.tier, &outcomes, &parent);
            Err(report.refusal(package).unwrap_or_else(|| {
                KuangError::new(
                    KuangErrorCode::LoadRuntimeUnavailable,
                    format!("cannot start `{package}`: {error}"),
                )
            }))
        }
    }
}

/// The controls the parent installs on `command` before the fork, and what became of each.
///
/// These are §16.1's controls that are not syscalls in the child: the working directory the
/// instance is given, the environment it did not choose, the descriptors it speaks the protocol
/// over, and the supervisor's ownership of its lifetime. Two more — the capability broker and the
/// protocol ceilings — are not installed at a moment at all: they hold for the instance's whole
/// life, because every host call goes through the broker and every frame through the limits, so
/// they are recorded as applied by construction.
fn apply(
    command: &mut tokio::process::Command,
    sandbox: &Sandbox,
) -> Vec<(Control, ControlResult, Option<String>)> {
    let mut parent = Vec::new();
    match std::fs::create_dir_all(&sandbox.working_directory) {
        Ok(()) => {
            command.current_dir(&sandbox.working_directory);
            parent.push((Control::WorkingDirectory, ControlResult::Applied, None));
        }
        Err(error) => parent.push((
            Control::WorkingDirectory,
            ControlResult::Failed,
            Some(error.to_string()),
        )),
    }
    command.env_clear();
    command.env("PATH", "/usr/local/bin:/usr/bin:/bin");
    command.env("HOME", &sandbox.working_directory);
    command.env("LC_ALL", "C");
    command.env("TZ", "UTC");
    parent.push((
        Control::EnvironmentSanitization,
        ControlResult::Applied,
        None,
    ));

    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    parent.push((Control::ProtocolStdio, ControlResult::Applied, None));

    command.kill_on_drop(true);
    parent.push((Control::ProcessLifetime, ControlResult::Applied, None));

    parent.push((Control::CapabilityBroker, ControlResult::Applied, None));
    parent.push((Control::ProtocolLimits, ControlResult::Applied, None));
    parent
}

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "spec §31.15's ceilings can only be set between fork and exec, and the standard \
              library offers exactly one way to run code there (ADR-0283)"
)]
fn apply_limits(
    command: &mut tokio::process::Command,
    sandbox: &Sandbox,
    platform: &Arc<dyn ConfinementPlatform>,
    outcomes: &Arc<report::Outcomes>,
) {
    let plan = ConfinementPlan {
        memory_max: sandbox.memory_max,
        open_files: sandbox.open_files,
        file_size: sandbox.file_size,
        nice: sandbox.nice,
    };
    let tier = sandbox.tier;
    let platform = Arc::clone(platform);
    let outcomes = Arc::clone(outcomes);

    // SAFETY: `pre_exec` runs in the forked child, between `fork` and `execve`, where only
    // async-signal-safe operations are allowed: no allocation, no locks, no reentrant libc.
    // `install_controls` calls direct syscall wrappers with values computed before the fork,
    // records each outcome with one relaxed atomic store into a page mapped before the fork, and
    // builds its errors with `io::Error::last_os_error`, which allocates nothing. Nothing here
    // allocates or takes a lock, so the closure is safe in the one environment it ever runs in
    // (ADR-0443).
    unsafe {
        command.pre_exec(move || install_controls(tier, platform.as_ref(), &plan, &outcomes));
    }
}

/// Installs every control the platform owns, in order, and stops at the first mandatory refusal.
///
/// Runs in the forked child. Returning `Err` is what prevents the `exec`: the standard library
/// abandons the spawn and reports the failure to the parent, so no plugin instruction ever runs
/// (§16.3). The shared page carries *which* control it was, because an errno cannot say (§16.5).
#[cfg(unix)]
fn install_controls(
    tier: ExecutionTier,
    platform: &dyn ConfinementPlatform,
    plan: &ConfinementPlan,
    outcomes: &report::Outcomes,
) -> std::io::Result<()> {
    for &control in PLATFORM_CONTROLS {
        let requirement = tier.requirement(control);
        if requirement == Requirement::NotProvided {
            outcomes.record(control, ControlResult::Skipped, 0);
            continue;
        }
        match platform.install(control, plan) {
            Ok(()) => outcomes.record(control, ControlResult::Applied, 0),
            Err(error) => {
                // A control this platform does not implement was not refused — it was never
                // there. §2.6 makes the difference matter: `skipped` says nothing is in force,
                // where `failed` would imply a kernel that said no. Either way it is not
                // `applied`, so a mandatory one still abandons the spawn below.
                let (result, errno) = match error.kind() {
                    std::io::ErrorKind::Unsupported => (ControlResult::Skipped, 0),
                    _ => (
                        ControlResult::Failed,
                        error.raw_os_error().unwrap_or(libc::EINVAL),
                    ),
                };
                outcomes.record(control, result, errno);
                // v0.4.1 §2.3, §16.3, Appendix D: for a mandatory control the Failure column
                // reads `spawn fails`, and this is where it does.
                if requirement.is_mandatory() {
                    return Err(error);
                }
            }
        }
    }
    Ok(())
}

/// Where an instance runs and keeps what is its own (spec §31.31).
///
/// `private_root` is the package's own directory under the host's state root —
/// `~/.local/state/ono/kuang/<package-id>/` in spec §31.31's words. Its `work` subdirectory is
/// the instance's working directory: the one place on the machine that is the package's, that
/// survives a restart, and that no other package is given.
#[must_use]
pub fn working_directory(private_root: Option<&Path>, artifact: &Path) -> PathBuf {
    match private_root {
        Some(root) => root.join("work"),
        // No state root means no private place. The directory the artifact itself lives in is the
        // next most private thing the host is sure of, and it is emphatically better than the
        // user's current directory, which is what an unconfigured child would have inherited.
        None => artifact
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
    }
}

/// The CPU time a live instance has used, in nanoseconds, from the kernel's own accounting
/// (spec §31.33's `cpu time`).
///
/// The kernel counts in clock ticks, so the resolution is the tick — usually 10 ms. `None` when
/// the process is gone or `/proc` does not answer; an unknown is never a zero (spec §35.3).
#[must_use]
pub fn cpu_nanoseconds(pid: u32) -> Option<i128> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The second field is the comm, in parentheses, and may itself contain spaces and
    // parentheses; everything after the last `)` is positional and safe to split.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    // After the comm, `state` is index 0, so `utime` (field 14) is index 11 and `stime` is 12.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    let ticks = clock_ticks_per_second()?;
    Some(i128::from(utime + stime) * 1_000_000_000 / ticks)
}

#[allow(
    unsafe_code,
    reason = "`sysconf` is how the kernel's tick rate is asked for, and has no safe wrapper"
)]
fn clock_ticks_per_second() -> Option<i128> {
    // SAFETY: `sysconf` reads a process-wide constant and touches no memory the caller owns.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    (ticks > 0).then(|| i128::from(ticks))
}

/// The peak allocated memory of a live instance, in bytes, from the kernel's own accounting.
///
/// `VmData` is what `RLIMIT_DATA` bounds, so this and [`Sandbox::memory_max`] measure the same
/// thing — which is what lets the host say a package reached its ceiling instead of guessing
/// (spec §31.33's `memory/current/limit`, spec §31.34's resource-limit failure class).
///
/// `None` when the process is gone or `/proc` does not answer; an unknown is never a zero
/// (spec §35.3).
#[must_use]
pub fn allocated_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmData:") {
            let mut words = rest.split_whitespace();
            let value: u64 = words.next()?.parse().ok()?;
            // `/proc` reports it in kibibytes, and says so in the next word.
            return match words.next() {
                Some("kB") => Some(value * 1024),
                _ => None,
            };
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_read_the_allocated_memory_of_a_live_process() {
        let allocated = allocated_bytes(std::process::id())
            .expect("this process is alive, so the kernel accounts for its data segment");
        assert!(
            allocated > 0,
            "a running process has allocated something; spec §35.3 forbids reporting an unknown \
             as a zero, so a zero here means the reader did not read"
        );
    }

    #[test]
    fn should_read_the_cpu_time_of_a_live_process() {
        assert!(
            cpu_nanoseconds(std::process::id()).is_some(),
            "spec §31.33 asks `inspect plugin` for the instance's cpu time, and the kernel keeps it"
        );
    }

    #[test]
    fn should_answer_nothing_for_a_process_that_does_not_exist() {
        // Not a fabricated zero: a figure the host cannot observe is null (spec §35.3).
        assert_eq!(allocated_bytes(u32::MAX), None);
        assert_eq!(cpu_nanoseconds(u32::MAX), None);
    }

    #[test]
    fn should_lower_the_priority_of_work_that_may_wait() {
        // Spec §31.67: a plugin-backed stage "must never block terminal input", so the
        // interactive class keeps the default priority and the other two step down.
        assert_eq!(nice_of(CpuBudget::Interactive), 0);
        assert!(nice_of(CpuBudget::Batch) > nice_of(CpuBudget::Interactive));
        assert!(nice_of(CpuBudget::Background) > nice_of(CpuBudget::Batch));
    }

    #[test]
    fn should_give_an_instance_its_own_directory_under_the_state_root() {
        let root = PathBuf::from("/var/lib/ono/kuang/dev.example.echo");
        assert_eq!(
            working_directory(Some(&root), Path::new("/opt/pkg/runtime/echo")),
            root.join("work")
        );
    }

    #[test]
    fn should_fall_back_to_the_artifacts_own_directory_when_there_is_no_state_root() {
        // Never the user's current directory, which is what an unconfigured child inherits.
        assert_eq!(
            working_directory(None, Path::new("/opt/pkg/runtime/echo")),
            PathBuf::from("/opt/pkg/runtime")
        );
    }

    #[test]
    fn should_never_present_a_broker_check_as_a_kernel_boundary() {
        // Spec §31.16: "A scope that cannot be enforced reliably MUST NOT be offered as if it
        // were a security boundary." A native process opens files without asking the host, so
        // the filesystem scope it is under is the broker's, and says so.
        let sandbox = native_process(
            64 * 1024 * 1024,
            CpuBudget::Interactive,
            PathBuf::from("/tmp/work"),
        );
        assert_eq!(sandbox.filesystem, Confinement::Broker);
        assert_eq!(sandbox.filesystem.as_str(), "broker");
        assert_eq!(sandbox.network, Confinement::Broker);
    }
}
