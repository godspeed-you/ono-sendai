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
        let _ = wait(
            &owned,
            &settled,
            REAP,
            &|| false,
            |killed| ask(killed, Signal::KILL),
            &mut killed,
        );
    }
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
        end_owned(ono_process::owned_groups, || true);
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
