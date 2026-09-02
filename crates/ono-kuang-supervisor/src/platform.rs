//! Installing one confinement control, and being able to fail at it (v0.4.1 §16.2, §59.7).
//!
//! Every control the supervisor installs between `fork` and `exec` is a syscall that can be
//! refused, and §16.2 requires every one of those return values to be checked. Before this module
//! existed the calls sat inline in a `pre_exec` closure that discarded all of them and ended in
//! an unconditional `Ok(())` — §0.5.3, and §65.4's named failure mode of this release: *"Calling
//! a confinement syscall, discarding its result and executing the plugin anyway is forbidden."*
//!
//! Two things follow from putting them behind [`ConfinementPlatform`] instead.
//!
//! **A refusal becomes testable.** §59.7 requires an acceptance scenario in which
//! `PR_SET_NO_NEW_PRIVS` fails and the plugin never runs. `PR_SET_NO_NEW_PRIVS` does not fail on
//! any Linux that has it, so nothing outside the process can arrange that failure (ADR-0430) —
//! the platform has to be a seam. It is a real one: production passes [`NativePlatform`], the
//! trait has no test-only method, and nothing in this crate can ask for a control to fail.
//!
//! **A new control cannot be installed unchecked.** [`ConfinementPlatform::install`] takes a
//! [`Control`] and returns `io::Result<()>`, so adding a syscall means adding a match arm that
//! has to produce a `Result`, and `sandbox.rs` decides what to do with it from the requirement
//! the central table gives that control. There is no path from a new syscall to a spawn that
//! ignores it, and `xtask`'s scan of this file refuses a raw `libc::` call whose result is
//! dropped (ADR-0443).
//!
//! # What runs between fork and exec
//!
//! Everything [`NativePlatform::install`] does is async-signal-safe: direct syscall wrappers,
//! values computed before the fork, no allocation and no locks. `io::Error::from_raw_os_error`
//! and `io::Error::last_os_error` allocate nothing, which is what makes returning a real error
//! from that context legal at all.

use std::io;

use ono_kuang_protocol::Control;

/// The values a pre-exec control needs, computed in the parent before the fork.
///
/// A plain `Copy` record rather than a reference into the [`Sandbox`](crate::Sandbox), because
/// the closure that uses it runs in a forked child where dereferencing anything the parent might
/// have been mutating is a hazard, and because everything here is a machine word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfinementPlan {
    /// The ceiling on allocated memory, in bytes: `RLIMIT_DATA`.
    pub memory_max: u64,
    /// The ceiling on open descriptors: `RLIMIT_NOFILE`.
    pub open_files: u64,
    /// The ceiling on the size of any single file the instance writes: `RLIMIT_FSIZE`.
    pub file_size: u64,
    /// The nice level the declared scheduling class becomes.
    pub nice: i32,
}

/// Installs the process-level confinement controls of v0.4.1 §16.1.
///
/// Implementations are called in the forked child, between `fork` and `exec`, so `install` must
/// be async-signal-safe: no allocation, no locks, no reentrant libc.
///
/// The supervisor takes its platform from the caller. Production passes [`NativePlatform`]; a
/// test passes an implementation that refuses one control, which is the injectable platform layer
/// §59.7 asks for. Nothing here can express "fail on purpose" — that lives in the test's own
/// implementation, never in a shipped one (§2.3, ADR-0443).
pub trait ConfinementPlatform: Send + Sync + 'static {
    /// Installs `control` with the values in `plan`.
    ///
    /// Returns `Err` with the operating system's own reason when the kernel refuses. The caller
    /// decides what a refusal means from the requirement the central table gives the control:
    /// mandatory refusals abandon the spawn (§16.3), best-effort refusals are recorded (§16.4).
    ///
    /// A control this platform does not install — because it belongs to a stronger tier, or to a
    /// platform this is not — is [`io::ErrorKind::Unsupported`]. That is a refusal like any
    /// other, so a mandatory control nobody implements fails the spawn rather than passing
    /// silently.
    fn install(&self, control: Control, plan: &ConfinementPlan) -> io::Result<()>;
}

/// The controls [`ConfinementPlatform`] installs, in the order the child installs them.
///
/// Ordering is deliberate. The resource ceilings come first, so that a refusal costs nothing that
/// has already been given up; `session_separation` and `no_new_privs` come last, because they are
/// the two whose failure is most likely and their refusal should not leave the child having
/// already lowered its own priority. Descriptor hygiene is first of all: it is the only control
/// whose absence would let a later failure leak something.
pub const PLATFORM_CONTROLS: &[Control] = &[
    Control::FdHygiene,
    Control::RlimitData,
    Control::RlimitOpenFiles,
    Control::RlimitFileSize,
    Control::RlimitCore,
    Control::SchedulingPriority,
    Control::SessionSeparation,
    Control::NoNewPrivs,
];

/// Whether `control` is one a [`ConfinementPlatform`] installs with a syscall in the child.
///
/// The other controls of §16.1 — the sanitized environment, the working directory, the protocol
/// descriptors, the supervisor's ownership of the child's lifetime — are installed by the parent
/// on the [`Command`](tokio::process::Command) before the fork, and the capability broker and the
/// protocol ceilings hold for the instance's whole life rather than being installed at all.
#[must_use]
pub fn is_installed_by_the_platform(control: Control) -> bool {
    PLATFORM_CONTROLS.contains(&control)
}

/// The controls this build installs with a real syscall on this operating system.
///
/// One unit struct rather than a factory, so that the production platform has no state a test
/// could reach into and no configuration that could turn a control off.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativePlatform;

impl NativePlatform {
    /// The platform the shell spawns plugins with.
    #[must_use]
    pub fn shared() -> std::sync::Arc<dyn ConfinementPlatform> {
        std::sync::Arc::new(NativePlatform)
    }
}

#[cfg(unix)]
impl ConfinementPlatform for NativePlatform {
    #[allow(
        unsafe_code,
        reason = "v0.4.1 §16.1's controls are syscalls with no safe wrapper, and §16.2 requires \
                  each of their return values to be checked here (ADR-0443)"
    )]
    fn install(&self, control: Control, plan: &ConfinementPlan) -> io::Result<()> {
        match control {
            // Every descriptor above the three protocol ones is marked close-on-exec, so nothing
            // the host happened to hold survives into the artifact. Marking rather than closing:
            // the standard library reports a `pre_exec` failure to the parent over a pipe it owns
            // above fd 2, and closing that pipe would turn a refused control into a silent
            // success — the exact failure §65.4 names (§16.1's "fd inheritance hygiene").
            Control::FdHygiene => close_range_cloexec(),
            Control::RlimitData => set_limit(libc::RLIMIT_DATA, plan.memory_max),
            Control::RlimitOpenFiles => set_limit(libc::RLIMIT_NOFILE, plan.open_files),
            Control::RlimitFileSize => set_limit(libc::RLIMIT_FSIZE, plan.file_size),
            // A core dump of the instance's address space would write whatever it held to a file
            // the operator did not ask for (spec §31.20).
            Control::RlimitCore => set_limit(libc::RLIMIT_CORE, 0),
            Control::SchedulingPriority => {
                if plan.nice == 0 {
                    // The default priority is what the process already has, so there is nothing
                    // to install and nothing that could fail.
                    return Ok(());
                }
                // SAFETY: `setpriority` takes three integers and touches no memory the caller
                // owns. It is async-signal-safe, which is what makes it legal in this context.
                checked(unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, plan.nice) })
            }
            // Its own session: the instance cannot signal the shell's process group, and the
            // terminal's signals do not reach it.
            Control::SessionSeparation => {
                // SAFETY: `setsid` takes no arguments and is async-signal-safe. It returns the
                // new session id, or -1 with `errno` set when the caller already leads a group.
                checked(unsafe { libc::setsid() })
            }
            // A setuid program the instance execs gains it nothing (spec §31.80).
            Control::NoNewPrivs => {
                // SAFETY: `prctl` with `PR_SET_NO_NEW_PRIVS` reads only its integer arguments.
                checked(unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) })
            }
            other => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                // A `&'static str` carries no allocation, so this stays legal in the child.
                other.summary(),
            )),
        }
    }
}

/// A libc return value as a `Result`, so §16.2's rule is the only way to read one.
///
/// Every raw syscall in this module goes through here. That is what makes the scan in
/// `xtask/src/scan.rs` able to state the rule mechanically: a `libc::` call whose value is not
/// handed to `checked` is a call whose result was dropped.
#[cfg(unix)]
fn checked(result: impl Into<i64>) -> io::Result<()> {
    if result.into() == -1 {
        // Allocates nothing: `last_os_error` carries the errno inline.
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "the resource-limit syscall has no safe wrapper in the standard library (ADR-0283)"
)]
fn set_limit(resource: u32, value: u64) -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    // SAFETY: `rlimit` is a plain repr(C) struct written entirely before the call, and the
    // pointer is to a local that outlives it. `setrlimit` is async-signal-safe, which is what
    // makes it legal in the `pre_exec` child.
    checked(unsafe { libc::setrlimit(resource, &raw const limit) })
}

/// Marks every descriptor above the protocol ones close-on-exec.
#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "`close_range` has no safe wrapper, and `fcntl` is the portable fallback (ADR-0443)"
)]
fn close_range_cloexec() -> io::Result<()> {
    const CLOSE_RANGE_CLOEXEC: libc::c_uint = 4;
    // SAFETY: `close_range` takes three integers. `syscall` is used because the libc this crate
    // builds against does not expose it on every supported release.
    let ranged = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3_u32,
            libc::c_uint::MAX,
            CLOSE_RANGE_CLOEXEC,
        )
    };
    if ranged == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        // A kernel older than 5.9, or a seccomp policy that hides the call. The fallback is the
        // same guarantee one descriptor at a time.
        Some(libc::ENOSYS) | Some(libc::EINVAL) | Some(libc::EPERM) => cloexec_one_at_a_time(),
        _ => Err(error),
    }
}

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "`fcntl` has no safe wrapper and this runs where nothing may allocate (ADR-0443)"
)]
fn cloexec_one_at_a_time() -> io::Result<()> {
    // SAFETY: `getrlimit` writes into a local of the right type; `fcntl` reads and writes only
    // descriptor flags. Both are async-signal-safe.
    unsafe {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        checked(libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit))?;
        // A soft `RLIMIT_NOFILE` of `RLIM_INFINITY` would be an unbounded loop; 4096 is above
        // every default and the descriptors above it are ones this process never opened.
        let highest = if limit.rlim_cur == libc::RLIM_INFINITY {
            4096
        } else {
            limit.rlim_cur.min(4096)
        };
        for fd in 3..highest as libc::c_int {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags == -1 {
                // The descriptor is not open. Not a failure of the control: there is nothing
                // there to inherit.
                continue;
            }
            checked(libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    fn plan() -> ConfinementPlan {
        ConfinementPlan {
            memory_max: 512 * 1024 * 1024,
            open_files: 256,
            file_size: 64 * 1024 * 1024,
            nice: 0,
        }
    }

    #[test]
    fn should_refuse_a_control_this_platform_does_not_install() {
        // §2.6: an unknown is never a success. A tier that asked for Landlock and got silence
        // would be a confinement report claiming a boundary nothing installed.
        let error = NativePlatform
            .install(Control::LandlockAllowlist, &plan())
            .expect_err("this build installs no Landlock policy");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn should_name_every_control_it_installs_in_the_order_it_installs_them() {
        // The list is what `sandbox.rs` iterates, so a control missing from it is a control the
        // child never attempts. Descriptor hygiene first, the two most refusable last.
        assert_eq!(PLATFORM_CONTROLS.first(), Some(&Control::FdHygiene));
        assert_eq!(PLATFORM_CONTROLS.last(), Some(&Control::NoNewPrivs));
        assert!(is_installed_by_the_platform(Control::NoNewPrivs));
        assert!(!is_installed_by_the_platform(Control::WorkingDirectory));
    }
}
