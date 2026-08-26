//! Turning a prepared stage into a running process.
//!
//! Everything that can be decided in the parent has been decided by the time this runs. What is
//! left is the handful of things that only exist between `fork` and `exec`: moving descriptors
//! into place, joining or leading a session, claiming a controlling terminal, and giving the
//! program back the default signal dispositions the shell took away from itself. ADR-0007
//! confines that work to this file.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Stdio;

use nix::libc;
use nix::unistd::Pid;
use ono_core::{ErrorCode, ExitStatus};

use crate::error::Error;
use crate::plan::FdPlan;
use crate::signals::RESET_IN_CHILD;

/// Everything the parent decided about one child.
pub(crate) struct SpawnRequest<'a> {
    /// The resolved path to execute.
    pub(crate) program: &'a Path,
    /// The arguments after the program name.
    pub(crate) args: &'a [OsString],
    /// Environment assignments and removals, in order.
    pub(crate) env: Option<&'a [(OsString, Option<OsString>)]>,
    /// Whether to start from an empty environment.
    pub(crate) clear_env: bool,
    /// The directory to run in.
    pub(crate) cwd: Option<&'a Path>,
    /// The process group to join; `0` means "lead a new one".
    pub(crate) process_group: Option<i32>,
    /// The descriptor number, as the child will see it, to claim as controlling terminal.
    pub(crate) controlling_terminal: Option<i32>,
}

/// Starts the child described by `request` with the descriptor table `plan` prepared.
///
/// The parent keeps `plan` alive across the call, because the child reads the descriptor
/// numbers out of it after `fork`.
pub(crate) fn spawn(request: &SpawnRequest<'_>, plan: &FdPlan) -> io::Result<i32> {
    let mut command = std::process::Command::new(request.program);
    command.args(request.args);
    // Every descriptor, including the standard three, is moved by the child itself, so that one
    // mechanism covers `2>&1` and `3>file` as well as a plain pipe.
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    if request.clear_env {
        command.env_clear();
    }
    for (key, value) in request.env.unwrap_or(&[]) {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }
    if let Some(directory) = request.cwd {
        command.current_dir(directory);
    }
    if request.controlling_terminal.is_none()
        && let Some(group) = request.process_group
    {
        // A child that claims a controlling terminal calls `setsid` instead, and `setsid` fails
        // for a process that is already a group leader.
        std::os::unix::process::CommandExt::process_group(&mut command, group);
    }

    let moves = plan.moves();
    let controlling_terminal = request.controlling_terminal;
    // SAFETY: `pre_exec` requires the closure to be async-signal-safe, because it runs in a
    // child that has forked away from a possibly multi-threaded parent. `child_setup` calls
    // only `dup2`, `setsid`, `ioctl` and `signal`, all of which POSIX lists as
    // async-signal-safe; it allocates nothing, takes no lock, performs no Rust I/O and has no
    // panicking path. `moves` and `controlling_terminal` are plain integers captured by value.
    // The descriptor numbers in `moves` are valid because the caller keeps the `FdPlan` that
    // owns them alive until `spawn` returns, and this closure only ever runs inside `spawn`.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut command, move || {
            child_setup(&moves, controlling_terminal)
        });
    }

    let child = command.spawn()?;
    let pid = i32::try_from(child.id()).unwrap_or(-1);
    // The status is collected with `waitpid` so that stops and continuations are visible, so
    // `Child` must not be asked to wait as well. Dropping it neither waits nor kills.
    drop(child);

    if let Some(group) = request.process_group {
        let group = if group == 0 { pid } else { group };
        // The child sets this too. Doing it in the parent as well removes the window in which
        // the group does not exist yet and `tcsetpgrp` would fail.
        let _ = nix::unistd::setpgid(Pid::from_raw(pid), Pid::from_raw(group));
    }
    Ok(pid)
}

/// The post-`fork`, pre-`exec` setup. Async-signal-safe by construction.
fn child_setup(moves: &[(i32, i32)], controlling_terminal: Option<i32>) -> io::Result<()> {
    for &(target, source) in moves {
        // SAFETY: `dup2` is async-signal-safe. `source` is open in this address space because
        // it was open in the parent at `fork` and is only closed at `exec`; `target` is a small
        // descriptor number, and `dup2` reports any error through `errno` rather than a trap.
        if unsafe { libc::dup2(source, target) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }

    if let Some(terminal) = controlling_terminal {
        // SAFETY: `setsid` is async-signal-safe. It can only fail if this process is already a
        // group leader, which the caller prevents by not requesting a process group as well.
        if unsafe { libc::setsid() } < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `ioctl` is async-signal-safe. `terminal` was just installed by the `dup2`
        // loop above and refers to the slave side of a pseudoterminal; `TIOCSCTTY` takes an
        // `int` argument and returns its error through `errno`.
        if unsafe { libc::ioctl(terminal, libc::TIOCSCTTY as _, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }

    for &signal in RESET_IN_CHILD {
        // SAFETY: `signal` is async-signal-safe. Restoring `SIG_DFL` cannot fail for these
        // signals, and a program must not inherit the dispositions an interactive shell needs
        // for itself (spec §18.1).
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
        }
    }
    Ok(())
}

/// Maps a failure to start the program onto the status conventions of ADR-0008.
pub(crate) fn exec_failure(program: &Path, error: &io::Error) -> (ExitStatus, Error) {
    let name = program.display();
    match error.raw_os_error() {
        Some(libc::ENOENT) => (
            ExitStatus::NOT_FOUND,
            Error::new(
                ErrorCode::ResolveCommandNotFound,
                format!("{name}: command not found"),
            ),
        ),
        Some(libc::ENOEXEC) => (
            ExitStatus::NOT_EXECUTABLE,
            Error::new(
                ErrorCode::IoPermissionDenied,
                format!("{name}: not an executable format"),
            ),
        ),
        _ => (
            ExitStatus::NOT_EXECUTABLE,
            Error::from_io(format!("running {name}"), error),
        ),
    }
}

/// Reports a resolution or spawn failure as a `Result` for callers that cannot continue.
pub(crate) fn cannot_run(program: &Path, error: &io::Error) -> Error {
    exec_failure(program, error).1
}

/// Converts a `nix` error into this crate's error type with some context.
pub(crate) fn system(context: &str, errno: nix::errno::Errno) -> Error {
    Error::from_errno(context, errno)
}
