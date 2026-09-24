//! Ending a process together with everything it started.
//!
//! Killing the process a test started is not enough when that process is a shell. `ono` puts
//! every external job in a process group of its own and the journal provider starts `journalctl`
//! in another, so neither the child's pid nor the child's process group reaches the programs it
//! is running — and those are what a sweep on 2026-09-02 found by the hundred, reparented to
//! `systemd --user` long after the test that started them had finished (issues #162, #204).
//!
//! What does reach them is the process tree: the kernel lists every process's children in
//! `/proc/<pid>/task/<tid>/children`. Each process is stopped before its children are read, so
//! nothing can fork past the walk, and only when the whole tree is frozen is it killed
//! (ADR-0892).

use std::path::Path;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

/// How long a signalled process may take to show as stopped before its children are read anyway.
///
/// A process in uninterruptible sleep stops only when the kernel lets it return; the walk does not
/// wait for it indefinitely, because a fork it cannot make while in the kernel is not a fork the
/// walk can miss.
const STOP_PATIENCE: Duration = Duration::from_millis(200);

/// Kills `root` and every process descended from it, and answers once each has been sent
/// `SIGKILL`.
///
/// `root` is not reaped: whoever started it holds its handle and waits for it. Its descendants are
/// reaped by their own parents, or by the process they are reparented to once those are gone.
/// A descendant that was already reparented away before the call — a daemon that double-forked —
/// is no longer in the tree and is not reached.
///
/// `root` must name a process this one started and has not yet reaped, so the pid cannot have been
/// reused; every other pid signalled here is read from the children list of a process that is
/// already stopped, which cannot reap it either.
pub fn kill_tree(root: u32) {
    let mut frozen: Vec<i32> = Vec::new();
    let mut pending = vec![as_pid(root)];
    while !pending.is_empty() {
        for pid in pending.drain(..) {
            if frozen.contains(&pid) {
                continue;
            }
            if kill(Pid::from_raw(pid), Signal::SIGSTOP).is_ok() {
                wait_until_stopped(pid);
                frozen.push(pid);
            }
        }
        // Read again from every frozen process, not only the newest: a child forked between a
        // parent's `SIGSTOP` and its stopping shows up on this pass rather than being missed.
        pending = frozen
            .iter()
            .flat_map(|pid| children_of(*pid))
            .filter(|child| !frozen.contains(child))
            .collect();
    }
    for pid in frozen {
        let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
    }
}

fn as_pid(pid: u32) -> i32 {
    i32::try_from(pid).unwrap_or(i32::MAX)
}

/// Waits, briefly, until `pid` is stopped, a zombie, or gone.
fn wait_until_stopped(pid: i32) {
    let deadline = Instant::now() + STOP_PATIENCE;
    while Instant::now() < deadline {
        match state_of(pid) {
            None | Some('T' | 't' | 'Z' | 'X') => return,
            Some(_) => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}

/// The one-letter state `/proc/<pid>/stat` reports, or `None` once the process is gone.
fn state_of(pid: i32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name is in parentheses and may itself contain spaces or parentheses, so the
    // state is the first field after the *last* closing one.
    stat.rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .and_then(|state| state.chars().next())
}

/// Every child of every thread of `pid`, as the kernel lists them.
fn children_of(pid: i32) -> Vec<i32> {
    let tasks = Path::new("/proc").join(pid.to_string()).join("task");
    let Ok(threads) = std::fs::read_dir(tasks) else {
        return Vec::new();
    };
    threads
        .flatten()
        .filter_map(|thread| std::fs::read_to_string(thread.path().join("children")).ok())
        .flat_map(|listed| {
            listed
                .split_whitespace()
                .filter_map(|pid| pid.parse().ok())
                .collect::<Vec<i32>>()
        })
        .collect()
}
