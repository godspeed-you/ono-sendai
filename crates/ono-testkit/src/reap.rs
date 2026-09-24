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
use std::process::Child;
use std::time::{Duration, Instant};

use rustix::fd::OwnedFd;
use rustix::process::{PidfdFlags, Signal as RustixSignal, pidfd_open, pidfd_send_signal};

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
/// `root` must name a process this one started and has not yet reaped. Every process is signalled
/// through a pidfd opened when it was found, never by its number, so no signal can reach a process
/// that took over a reaped pid (ADR-0893).
pub fn kill_tree(root: u32) {
    match Kernel.open(as_pid(root), None) {
        Some(handle) => walk_and_kill(handle, &mut Kernel),
        // A kernel without `pidfd_open` (before Linux 5.3): the root alone, by its number, which
        // the caller's unreaped handle keeps from being reused.
        None => {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(as_pid(root)),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

/// What the walk needs from the machine: a handle on a process, stopping it, reading its
/// children, killing it.
///
/// The walk is the part with a contract of its own — it must end, and it must not signal a
/// process that is not in the tree — so it runs over this rather than over the kernel directly,
/// and its unit tests run it over a table they control.
trait ProcessTable {
    /// A reference to one process that stays with that process, whatever becomes of its pid.
    type Handle;
    /// A handle on `pid`, confirmed to be a child of `parent` once opened — or, without a parent,
    /// on the root, which the caller vouches for. `None` if it is gone or no longer that child.
    fn open(&mut self, pid: i32, parent: Option<&Self::Handle>) -> Option<Self::Handle>;
    /// The pid the handle was opened on.
    fn pid(&self, handle: &Self::Handle) -> i32;
    /// Stops the process and waits briefly until it shows as stopped; answers whether it was
    /// signalled.
    fn stop(&mut self, handle: &Self::Handle) -> bool;
    /// The pids of every child of every thread of the process; none once it has been reaped.
    fn children(&self, handle: &Self::Handle) -> Vec<i32>;
    /// Sends the process `SIGKILL`.
    fn kill(&mut self, handle: &Self::Handle);
}

/// The running machine, through pidfds.
struct Kernel;

impl ProcessTable for Kernel {
    type Handle = (i32, OwnedFd);

    fn open(&mut self, pid: i32, parent: Option<&Self::Handle>) -> Option<Self::Handle> {
        let pidfd = pidfd_open(rustix::process::Pid::from_raw(pid)?, PidfdFlags::empty()).ok()?;
        let handle = (pid, pidfd);
        // The pid was read from the parent's children list before the pidfd existed. Still being
        // listed there afterwards, by a parent that is stopped and cannot fork, means the pidfd
        // names that child and not a process that took its pid in between.
        match parent {
            Some(parent) if !self.children(parent).contains(&pid) => None,
            _ => Some(handle),
        }
    }

    fn pid(&self, handle: &Self::Handle) -> i32 {
        handle.0
    }

    fn stop(&mut self, handle: &Self::Handle) -> bool {
        if pidfd_send_signal(&handle.1, RustixSignal::STOP).is_ok() {
            wait_until_stopped(handle.0);
            true
        } else {
            false
        }
    }

    fn children(&self, handle: &Self::Handle) -> Vec<i32> {
        // `/proc` is read by number, so the read is bracketed by two signals through the pidfd:
        // a process that answers both was not reaped in between — reaping cannot be undone — and
        // so the number named it for the whole read. `SIGSTOP` is the probe because every
        // process asked about here is one the walk has already stopped.
        let alive = || pidfd_send_signal(&handle.1, RustixSignal::STOP).is_ok();
        if !alive() {
            return Vec::new();
        }
        let children = children_of(handle.0);
        if alive() { children } else { Vec::new() }
    }

    fn kill(&mut self, handle: &Self::Handle) {
        let _ = pidfd_send_signal(&handle.1, RustixSignal::KILL);
    }
}

/// Freezes the tree under `root` in `table`, then kills every process it froze, leaves first.
fn walk_and_kill<T: ProcessTable>(root: T::Handle, table: &mut T) {
    // Every pid the walk has tried, whether or not it could be stopped: a descendant that refuses
    // `SIGSTOP` (it changed its uid) stays listed under its stopped parent, and trying it again on
    // every pass would never end.
    let mut tried: Vec<i32> = vec![table.pid(&root)];
    let mut frozen: Vec<T::Handle> = Vec::new();
    if table.stop(&root) {
        frozen.push(root);
    }
    loop {
        // Read again from every frozen process, not only the newest: a child forked between a
        // parent's `SIGSTOP` and its stopping shows up on a later pass rather than being missed.
        let mut found: Vec<(i32, usize)> = Vec::new();
        for (index, parent) in frozen.iter().enumerate() {
            for child in table.children(parent) {
                if !tried.contains(&child) && !found.iter().any(|(pid, _)| *pid == child) {
                    found.push((child, index));
                }
            }
        }
        if found.is_empty() {
            break;
        }
        for (pid, parent) in found {
            tried.push(pid);
            let Some(handle) = table.open(pid, Some(&frozen[parent])) else {
                continue;
            };
            if table.stop(&handle) {
                frozen.push(handle);
            }
        }
    }
    // Discovery order puts every process after the one it was found under, so the reverse kills
    // every child before its parent: no process in the tree loses its parent — and so is
    // reparented, reaped, or resumed by an orphaned group's SIGCONT — while it is still to be
    // killed (ADR-0893).
    for handle in frozen.iter().rev() {
        table.kill(handle);
    }
}

/// A child process that is killed — with everything it started — and reaped when the value is
/// dropped, including when the test holding it panics.
///
/// `std::process::Child` does neither on drop, so a test that spawns a process, asserts, and only
/// then kills it leaves the process running whenever the assertion fails. This is the type that
/// makes the kill unconditional (ADR-0516, ADR-0892). It dereferences to the `Child`, so a test
/// can still read its pipes, signal it, or wait for it; a child the test has already waited for
/// is left alone.
#[derive(Debug)]
pub struct OwnedChild {
    child: Option<Child>,
}

impl OwnedChild {
    /// Takes ownership of `child`'s death.
    #[must_use]
    pub fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    /// Waits for the child and collects what is left in its piped streams, as
    /// [`Child::wait_with_output`] does; the child is reaped by then, so nothing is left to own.
    ///
    /// # Errors
    ///
    /// As [`Child::wait_with_output`].
    pub fn wait_with_output(mut self) -> std::io::Result<std::process::Output> {
        self.child
            .take()
            .expect("an owned child is present until it is given up")
            .wait_with_output()
    }
}

impl From<Child> for OwnedChild {
    fn from(child: Child) -> Self {
        Self::new(child)
    }
}

impl std::ops::Deref for OwnedChild {
    type Target = Child;

    fn deref(&self) -> &Child {
        self.child
            .as_ref()
            .expect("an owned child is present until it is given up")
    }
}

impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        self.child
            .as_mut()
            .expect("an owned child is present until it is given up")
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        if matches!(child.try_wait(), Ok(None)) {
            kill_tree(child.id());
        }
        let _ = child.wait();
    }
}

/// Kills the process tree under a pid someone else owns when the value is dropped.
///
/// For a process whose handle is not a `std::process::Child` — a pseudo-terminal session, whose own
/// `Drop` signals only its process group and so misses the jobs the shell under it started in
/// groups of their own. Declare it *after* the handle it guards, so it is dropped first: the tree
/// dies here, and the handle's own `Drop` then reaps the leader.
#[derive(Debug)]
pub struct OwnedTree {
    root: u32,
}

impl OwnedTree {
    /// Takes ownership of the death of every process under `root`, `root` included.
    #[must_use]
    pub fn of(root: u32) -> Self {
        Self { root }
    }
}

impl Drop for OwnedTree {
    fn drop(&mut self) {
        kill_tree(self.root);
    }
}

fn as_pid(pid: u32) -> i32 {
    i32::try_from(pid).unwrap_or(i32::MAX)
}

/// Waits, briefly, until every thread of `pid` is stopped, a zombie, or gone.
///
/// Each thread is read, not only the thread-group leader: the leader shows as stopped while
/// another thread is still running, and that thread can fork.
fn wait_until_stopped(pid: i32) {
    let deadline = Instant::now() + STOP_PATIENCE;
    while Instant::now() < deadline {
        let settled = thread_states(pid)
            .iter()
            .all(|state| matches!(state, 'T' | 't' | 'Z' | 'X'));
        if settled {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The one-letter state of every thread of `pid`, as `/proc/<pid>/task/<tid>/stat` reports it;
/// empty once the process is gone.
fn thread_states(pid: i32) -> Vec<char> {
    let tasks = Path::new("/proc").join(pid.to_string()).join("task");
    let Ok(threads) = std::fs::read_dir(tasks) else {
        return Vec::new();
    };
    threads
        .flatten()
        .filter_map(|thread| std::fs::read_to_string(thread.path().join("stat")).ok())
        .filter_map(|stat| {
            // The command name is in parentheses and may itself contain spaces or parentheses,
            // so the state is the first field after the *last* closing one.
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .and_then(|state| state.chars().next())
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::{ProcessTable, walk_and_kill};

    /// A process table the test writes: who is whose child, who refuses to be signalled, and who
    /// is gone by the time the walk tries to open it.
    #[derive(Default)]
    struct Table {
        children: Vec<(i32, i32)>,
        unsignalable: Vec<i32>,
        gone: Vec<i32>,
        opened: Vec<i32>,
        stops: Vec<i32>,
        kills: Vec<i32>,
    }

    impl ProcessTable for Table {
        type Handle = i32;

        fn open(&mut self, pid: i32, parent: Option<&i32>) -> Option<i32> {
            self.opened.push(pid);
            let listed = parent.is_none_or(|parent| self.children(parent).contains(&pid));
            (listed && !self.gone.contains(&pid)).then_some(pid)
        }

        fn pid(&self, handle: &i32) -> i32 {
            *handle
        }

        fn stop(&mut self, handle: &i32) -> bool {
            self.stops.push(*handle);
            assert!(
                self.stops.len() < 1_000,
                "the walk keeps trying to stop the same processes: {:?}",
                &self.stops[..20]
            );
            !self.unsignalable.contains(handle)
        }

        fn children(&self, handle: &i32) -> Vec<i32> {
            self.children
                .iter()
                .filter(|(parent, _)| parent == handle)
                .map(|(_, child)| *child)
                .collect()
        }

        fn kill(&mut self, handle: &i32) {
            self.kills.push(*handle);
        }
    }

    #[test]
    fn should_end_and_try_each_process_once_when_a_descendant_cannot_be_signalled() {
        // A child that changed its uid answers EPERM to SIGSTOP. It stays listed under its
        // stopped parent, and a walk that only remembered the processes it froze would try it
        // again on every pass, for ever.
        let mut table = Table {
            children: vec![(1, 2), (1, 3), (3, 4)],
            unsignalable: vec![3],
            ..Table::default()
        };
        walk_and_kill(1, &mut table);
        let mut stops = table.stops.clone();
        stops.sort_unstable();
        assert_eq!(stops, vec![1, 2, 3], "each process is tried exactly once");
        assert!(
            !table.kills.contains(&3),
            "a process that could not be stopped is not part of the frozen tree"
        );
    }

    #[test]
    fn should_kill_every_process_before_the_process_it_descends_from() {
        // Killing a parent first lets its children be reparented and reaped — and their pids
        // reused — before the walk reaches them, and orphans a stopped process group, which the
        // kernel answers with SIGHUP and SIGCONT: a descendant that ignores the hangup resumes and
        // can fork. Leaves first, no process is left without the process above it (ADR-0893).
        let mut table = Table {
            children: vec![(1, 2), (1, 3), (2, 4), (4, 5), (3, 6)],
            ..Table::default()
        };
        walk_and_kill(1, &mut table);
        let position = |pid: i32| {
            table
                .kills
                .iter()
                .position(|killed| *killed == pid)
                .unwrap_or_else(|| panic!("{pid} was killed, got {:?}", table.kills))
        };
        for (parent, child) in &table.children {
            assert!(
                position(*child) < position(*parent),
                "{child} is killed before its parent {parent}, got {:?}",
                table.kills
            );
        }
    }

    #[test]
    fn should_signal_no_process_it_could_not_open_as_a_child_of_a_frozen_parent() {
        // A pid read from a children list names that child only while the child is not reaped.
        // One that is gone by the time the walk opens it — reaped, its pid free for anyone — is
        // neither stopped nor killed.
        let mut table = Table {
            children: vec![(1, 2), (1, 3), (3, 4)],
            gone: vec![3],
            ..Table::default()
        };
        walk_and_kill(1, &mut table);
        assert!(!table.stops.contains(&3) && !table.kills.contains(&3));
        assert!(
            !table.opened.contains(&4),
            "the children of a process the walk could not open are not its to walk"
        );
    }
}
