//! A shell that leaves ends the jobs it owns (issue #303, ADR-0959).
//!
//! However it leaves — the end of a script, `exit`, its terminal hanging up, `SIGTERM`, `SIGINT`
//! outside an interactive session — the shell ends every process group it still owns the same
//! way: `SIGTERM` and `SIGCONT` once to each, one grace period shared by all, `SIGKILL` to what is
//! left, a bounded moment for it to go. Nothing survives the shell by accident, and nothing makes
//! leaving take longer than the two bounds.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use ono_process::{OwnedGroup, Signal};

/// How long the groups a leaving shell has asked to stop get, all of them together, before the
/// shell makes them.
pub const GRACE: Duration = Duration::from_secs(2);

/// How long a leaving shell waits for what it killed to be gone.
pub const REAP: Duration = Duration::from_secs(1);

/// Ends every group `owned` names, and waits for `settled` as well, within [`GRACE`] and
/// [`REAP`].
///
/// `owned` is asked again on every look, so a group that appears while the shell is leaving — a
/// job's evaluator starting its next program in the instant before it noticed it was cancelled —
/// is asked to stop too. Each group is sent `SIGTERM` (with `SIGCONT`, so a stopped one can act on
/// it) once, not once per look; what is still running when the grace period is over — or as soon
/// as a second termination signal says to hurry — gets `SIGKILL`. Nothing is collected here: the
/// owner of each group collects it (review M1). `settled` says whether the caller's own work — its
/// jobs' evaluators — has finished, so the shell does not wait out a grace period nothing needs.
pub fn end_owned(owned: impl Fn() -> Vec<OwnedGroup>, settled: impl Fn() -> bool) {
    // A pipeline being started publishes its group only once its first stage exists; a start in
    // progress — none begins once the shell is ending — is waited for, within the grace period,
    // so every group it started is owned before anything is looked at (review Y2).
    let started = Instant::now();
    while ono_process::starts_in_flight() > 0 && started.elapsed() < GRACE {
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut asked: BTreeSet<i32> = BTreeSet::new();
    let ask = |asked: &mut BTreeSet<i32>, signal: Signal| {
        for group in owned() {
            let id = group.group();
            if id != 0 && !group.is_gone() && asked.insert(id) {
                group.send(signal);
                if signal == Signal::TERM {
                    group.send(Signal::CONT);
                }
            }
        }
    };
    ask(&mut asked, Signal::TERM);
    // A second termination signal — the user pressing again — says not to wait (review M2).
    let hurry = || ono_process::termination_signals() >= 2;
    if wait(
        &owned,
        &settled,
        GRACE,
        &hurry,
        |asked| ask(asked, Signal::TERM),
        &mut asked,
    ) {
        let mut killed = BTreeSet::new();
        ask(&mut killed, Signal::KILL);
        // `SIGKILL` is sent again on every look to whatever still runs: a member that joined
        // its group after the first one — a stage forked in the instant before the shell saw it
        // was ending — is not missed (review X1).
        let _ = wait(
            &owned,
            &settled,
            REAP,
            &|| false,
            |_| {
                for group in owned() {
                    if !group.is_gone() {
                        group.send(Signal::KILL);
                    }
                }
            },
            &mut killed,
        );
    }
}

/// [`end_owned`] for the end of the whole shell — a session's own end, or a termination signal —
/// one at a time: a script's end racing a signal runs the two one after the other, the second
/// finding little or nothing left to ask (review X5). A job's session, ending inside the
/// shell's, does not take part: the shell's end waits for the job, which must not wait for it.
pub fn end_shell(owned: impl Fn() -> Vec<OwnedGroup>, settled: impl Fn() -> bool) {
    let _one = ESCALATION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    end_owned(owned, settled);
}

/// Waits until nothing owned runs and `settled` holds, or `budget` has passed, or `hurry` says
/// to stop waiting, asking what newly appeared; answers whether anything was left.
fn wait(
    owned: &impl Fn() -> Vec<OwnedGroup>,
    settled: &impl Fn() -> bool,
    budget: Duration,
    hurry: &impl Fn() -> bool,
    mut ask: impl FnMut(&mut BTreeSet<i32>),
    asked: &mut BTreeSet<i32>,
) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        let remaining = owned().iter().any(|group| !group.is_gone());
        if !remaining && settled() {
            return false;
        }
        if Instant::now() >= deadline || hurry() {
            return remaining || !settled();
        }
        std::thread::sleep(Duration::from_millis(10));
        ask(asked);
    }
}

/// Held while an escalation runs.
static ESCALATION: Mutex<()> = Mutex::new(());

/// The cancellation flags of every job's evaluator in the process, so a signal that ends the
/// shell stops them as `kill %N` would (review M1).
static JOB_CANCELS: Mutex<Vec<Weak<AtomicBool>>> = Mutex::new(Vec::new());

/// Records a job's cancellation flag for [`on_termination`].
pub fn register_job(cancel: &Arc<AtomicBool>) {
    let mut cancels = JOB_CANCELS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cancels.retain(|weak| weak.strong_count() > 0);
    cancels.push(Arc::downgrade(cancel));
}

/// Makes `SIGHUP` and `SIGTERM` — and `SIGINT`, outside an interactive session, where nothing else
/// takes it — end the shell the way leaving does (ADR-0959).
///
/// The handler only notes the signal: from then on no statement starts and no program is started,
/// and what is running is cancelled as Ctrl-C cancels it (review B1). A thread then cancels every
/// job, ends everything the process owns within the bounds above, puts the terminal's settings
/// back where the shell is the terminal's foreground group (review B3), and lets the signal end
/// the process with its default disposition, so whoever waits for it sees a death by that signal
/// (review S1). A signal the shell was started with ignored stays ignored (POSIX, review B2). An
/// interactive shell's `SIGINT` stays Ctrl-C (spec §18.5).
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
        for cancel in JOB_CANCELS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter_map(Weak::upgrade)
        {
            cancel.store(true, Ordering::SeqCst);
        }
        // The children commands started themselves are asked with the jobs, and killed after
        // the grace period with what is left of them (review Y3).
        signal_recorded(nix::sys::signal::Signal::SIGTERM);
        end_shell(ono_process::owned_groups, || !recorded_running());
        signal_recorded(nix::sys::signal::Signal::SIGKILL);
        if let Some(saved) = &saved {
            ono_process::restore_if_foreground(saved);
        }
        ono_process::die_of(signal);
    })
    .map_err(|error| ono_value::ErrorValue::new(error.code(), error.message().to_owned()))
}

/// Waits, without end, while a signal is ending the shell.
///
/// The thread ending the jobs ends the process by the signal when it is done. The main thread may
/// reach its own end first — its foreground program was one of the things that thread ended — and
/// must not exit with that program's status in its place.
pub fn yield_to_termination() {
    while ono_process::terminating() {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Sends `signal` to every child a command started itself and is still waiting for.
fn signal_recorded(signal: nix::sys::signal::Signal) {
    ono_core::children::for_each(|pid| {
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), signal);
    });
}

/// Whether a child a command started itself is still running (an exited one waits only to be
/// collected by its command, and counts as gone).
fn recorded_running() -> bool {
    let mut running = false;
    ono_core::children::for_each(|pid| {
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok();
        let alive = state
            .as_deref()
            .and_then(|stat| stat.rsplit_once(')'))
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .is_some_and(|state| state != "Z" && state != "X");
        running |= alive;
    });
    running
}
