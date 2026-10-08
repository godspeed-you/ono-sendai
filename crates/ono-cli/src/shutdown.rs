//! A shell that leaves ends the jobs it owns (issue #303, ADR-0959).
//!
//! However it leaves — the end of a script, `exit`, its terminal hanging up, `SIGTERM`, `SIGINT`
//! outside an interactive session — the shell ends every process group it still owns the same
//! way: `SIGTERM` and `SIGCONT` once to each, one grace period shared by all, `SIGKILL` to what is
//! left, a bounded moment to collect it. Nothing survives the shell by accident, and nothing makes
//! leaving take longer than the two bounds.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ono_process::{OwnedGroup, Signal};

/// How long the groups a leaving shell has asked to stop get, all of them together, before the
/// shell makes them.
pub const GRACE: Duration = Duration::from_secs(2);

/// How long a leaving shell waits for what it killed to be collected.
pub const REAP: Duration = Duration::from_secs(1);

/// Ends every group `owned` names, and waits for `settled` as well, within [`GRACE`] and
/// [`REAP`].
///
/// `owned` is asked again on every look, so a group that appears while the shell is leaving — a
/// job's evaluator starting its next program in the instant before it noticed it was cancelled —
/// is asked to stop too. Each group is sent `SIGTERM` (with `SIGCONT`, so a stopped one can act on
/// it) once, not once per look; what is still there when the grace period is over gets `SIGKILL`.
/// `settled` says whether the caller's own work — its jobs' evaluators — has finished, so the
/// shell does not wait out a grace period nothing needs.
pub fn end_owned(owned: impl Fn() -> Vec<OwnedGroup>, settled: impl Fn() -> bool) {
    let mut asked: BTreeSet<i32> = BTreeSet::new();
    let ask = |asked: &mut BTreeSet<i32>| {
        for group in owned() {
            let id = group.group();
            if id != 0 && asked.insert(id) {
                group.send(Signal::TERM);
                group.send(Signal::CONT);
            }
        }
    };
    ask(&mut asked);
    let left = wait(&owned, &settled, GRACE, |owned| ask(owned), &mut asked);
    if left {
        for group in owned() {
            group.send(Signal::KILL);
        }
        let mut killed = BTreeSet::new();
        let _ = wait(
            &owned,
            &settled,
            REAP,
            |killed: &mut BTreeSet<i32>| {
                for group in owned() {
                    if killed.insert(group.group()) {
                        group.send(Signal::KILL);
                    }
                }
            },
            &mut killed,
        );
    }
}

/// Collects what has ended and asks what newly appeared, until nothing is owned and `settled`
/// holds, or `budget` has passed; answers whether anything was left.
fn wait(
    owned: &impl Fn() -> Vec<OwnedGroup>,
    settled: &impl Fn() -> bool,
    budget: Duration,
    mut ask: impl FnMut(&mut BTreeSet<i32>),
    asked: &mut BTreeSet<i32>,
) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        let mut remaining = false;
        for group in owned() {
            if !group.collect() {
                remaining = true;
            }
        }
        if !remaining && settled() {
            return false;
        }
        if Instant::now() >= deadline {
            return remaining || !settled();
        }
        std::thread::sleep(Duration::from_millis(10));
        ask(asked);
    }
}

/// Makes `SIGHUP` and `SIGTERM` — and `SIGINT`, outside an interactive session, where nothing else
/// takes it — end the shell the way leaving does: its jobs ended within the bounds above, then an
/// exit with `128 + N` (ADR-0959). An interactive shell's `SIGINT` stays Ctrl-C (spec §18.5).
///
/// # Errors
///
/// The operating system's refusal to install the handlers.
pub fn on_termination(interactive: bool) -> Result<(), ono_value::ErrorValue> {
    // The terminal's settings now, to put back if a signal ends the shell while the line editor
    // has the terminal in raw mode.
    let saved = nix::sys::termios::tcgetattr(std::io::stdin()).ok();
    let mut signals = vec![Signal::HUP, Signal::TERM];
    if !interactive {
        signals.push(Signal::INT);
    }
    ono_process::install_termination_watch(&signals, move |signal| {
        LEAVING.store(true, std::sync::atomic::Ordering::SeqCst);
        end_owned(ono_process::owned_groups, || true);
        if let Some(saved) = &saved {
            let _ = nix::sys::termios::tcsetattr(
                std::io::stdin(),
                nix::sys::termios::SetArg::TCSANOW,
                saved,
            );
        }
        std::process::exit(128 + signal.number());
    })
    .map_err(|error| ono_value::ErrorValue::new(error.code(), error.message().to_owned()))
}

/// Set once a signal has begun ending the shell.
static LEAVING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Waits, without end, while a signal is ending the shell.
///
/// The thread ending the jobs exits the process with the signal's status when it is done. The
/// main thread may reach its own end first — its foreground program was one of the things that
/// thread ended — and must not exit with that program's status in its place.
pub fn yield_to_termination() {
    while LEAVING.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(50));
    }
}
