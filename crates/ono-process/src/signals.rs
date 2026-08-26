//! Signals, the shell's own dispositions, and child-transition notification.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::libc;
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal as NixSignal, sigaction};

use crate::error::{Error, Result};

/// A Unix signal number.
///
/// The shell needs to name signals in job records, in `kill`-like commands and in the
/// `128 + N` statuses of ADR-0008, without leaking a third-party enum into its own vocabulary.
///
/// ```
/// use ono_process::Signal;
/// assert_eq!(Signal::INT.number(), 2);
/// assert_eq!(Signal::INT.name(), Some("SIGINT"));
/// assert_eq!(Signal::from_number(9), Some(Signal::KILL));
/// assert_eq!(Signal::from_number(0), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Signal(i32);

macro_rules! signals {
    ($( $konst:ident => $libc:ident, $name:literal, $doc:literal; )*) => {
        impl Signal {
            $( #[doc = $doc] pub const $konst: Self = Self(libc::$libc); )*

            /// Every signal this crate names, in declaration order.
            pub const NAMED: &'static [Signal] = &[ $( Signal::$konst, )* ];

            /// The signal's conventional name, if it is one of the named signals.
            #[must_use]
            pub fn name(self) -> Option<&'static str> {
                match self.0 {
                    $( libc::$libc => Some($name), )*
                    _ => None,
                }
            }
        }
    };
}

signals! {
    HUP => SIGHUP, "SIGHUP", "The terminal or controlling process went away.";
    INT => SIGINT, "SIGINT", "The interrupt character was typed, or a cancel was requested.";
    QUIT => SIGQUIT, "SIGQUIT", "The quit character was typed.";
    ILL => SIGILL, "SIGILL", "Illegal instruction.";
    ABRT => SIGABRT, "SIGABRT", "The process aborted itself.";
    FPE => SIGFPE, "SIGFPE", "Erroneous arithmetic operation.";
    KILL => SIGKILL, "SIGKILL", "Unconditional termination.";
    SEGV => SIGSEGV, "SIGSEGV", "Invalid memory reference.";
    PIPE => SIGPIPE, "SIGPIPE", "Wrote to a pipe with no reader.";
    ALRM => SIGALRM, "SIGALRM", "A timer expired.";
    TERM => SIGTERM, "SIGTERM", "Polite termination request.";
    USR1 => SIGUSR1, "SIGUSR1", "Application-defined signal 1.";
    USR2 => SIGUSR2, "SIGUSR2", "Application-defined signal 2.";
    CHLD => SIGCHLD, "SIGCHLD", "A child changed state.";
    CONT => SIGCONT, "SIGCONT", "Continue a stopped process.";
    STOP => SIGSTOP, "SIGSTOP", "Unconditional stop.";
    TSTP => SIGTSTP, "SIGTSTP", "The suspend character was typed.";
    TTIN => SIGTTIN, "SIGTTIN", "A background process tried to read the terminal.";
    TTOU => SIGTTOU, "SIGTTOU", "A background process tried to write the terminal.";
    WINCH => SIGWINCH, "SIGWINCH", "The terminal window changed size.";
}

impl Signal {
    /// The signal with this number, or `None` if the number is not a valid signal.
    #[must_use]
    pub fn from_number(number: i32) -> Option<Self> {
        NixSignal::try_from(number).ok().map(|_| Self(number))
    }

    /// The signal's number.
    #[must_use]
    pub const fn number(self) -> i32 {
        self.0
    }

    pub(crate) fn to_nix(self) -> Result<NixSignal> {
        NixSignal::try_from(self.0).map_err(|_| {
            Error::new(
                ono_core::ErrorCode::TypeMismatch,
                format!("{} is not a signal", self.0),
            )
        })
    }

    pub(crate) fn from_nix(signal: NixSignal) -> Self {
        Self(signal as i32)
    }
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(f, "signal {}", self.0),
        }
    }
}

/// Set by [`install_child_watch`]'s handler; read and cleared by [`take_child_transition`].
static CHILD_TRANSITION: AtomicBool = AtomicBool::new(false);

extern "C" fn note_child_transition(_signal: libc::c_int) {
    CHILD_TRANSITION.store(true, Ordering::Relaxed);
}

/// Gives the shell process the signal dispositions an interactive shell needs (spec §18.1).
///
/// The signals a terminal generates for the foreground job — `SIGINT`, `SIGQUIT`, `SIGTSTP` —
/// are ignored by the shell itself, so they reach the foreground process group and leave the
/// prompt standing. `SIGTTIN` and `SIGTTOU` are ignored so the shell can move the terminal
/// between process groups without stopping itself. `SIGCHLD` is left at its default: setting it
/// to `SIG_IGN` would make the kernel reap children behind the shell's back and lose the
/// transitions the job table is built from.
///
/// Every child this crate spawns has these dispositions reset to their defaults before `exec`,
/// so a program still sees a normal signal environment. Calling this more than once is
/// harmless.
///
/// # Errors
///
/// Returns an error if the operating system refuses to change a disposition.
pub fn install_shell_signals() -> Result<()> {
    for signal in [
        NixSignal::SIGINT,
        NixSignal::SIGQUIT,
        NixSignal::SIGTSTP,
        NixSignal::SIGTTIN,
        NixSignal::SIGTTOU,
    ] {
        let action = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
        // SAFETY: `SIG_IGN` runs no code at all, so no async-signal-safety rule can be broken
        // by it; ADR-0007 permits signal-disposition changes in this crate.
        unsafe { sigaction(signal, &action) }
            .map_err(|errno| Error::from_errno(format!("ignoring {signal:?}"), errno))?;
    }
    Ok(())
}

/// Installs a `SIGCHLD` handler that records that some child changed state.
///
/// The handler does one thing: set a flag. Reaping stays with the job table, which uses
/// `waitpid` with `WUNTRACED | WCONTINUED | WNOHANG`, so no transition can be lost even if
/// several arrive while the shell is busy — the kernel holds each child in its stopped or
/// zombie state until the table asks for it. The flag exists only so a shell blocked on the
/// prompt knows there is something to reap; the handler is installed without `SA_RESTART` so
/// that blocking reads return and the prompt can poll.
///
/// # Errors
///
/// Returns an error if the operating system refuses to install the handler.
pub fn install_child_watch() -> Result<()> {
    let action = SigAction::new(
        SigHandler::Handler(note_child_transition),
        SaFlags::empty(),
        SigSet::empty(),
    );
    // SAFETY: the handler stores a `bool` into a `static AtomicBool` with a relaxed ordering and
    // returns. It calls no library function, allocates nothing, takes no lock and cannot panic,
    // which is exactly the async-signal-safety rule ADR-0007 requires.
    unsafe { sigaction(NixSignal::SIGCHLD, &action) }
        .map_err(|errno| Error::from_errno("watching SIGCHLD", errno))?;
    Ok(())
}

/// Reports whether a child changed state since this was last called, and clears the flag.
///
/// Always `false` unless [`install_child_watch`] has been called.
#[must_use]
pub fn take_child_transition() -> bool {
    CHILD_TRANSITION.swap(false, Ordering::Relaxed)
}

/// The dispositions a child gets back before `exec`, in the order the child resets them.
pub(crate) const RESET_IN_CHILD: &[libc::c_int] = &[
    libc::SIGHUP,
    libc::SIGINT,
    libc::SIGQUIT,
    libc::SIGPIPE,
    libc::SIGTERM,
    libc::SIGCHLD,
    libc::SIGTSTP,
    libc::SIGTTIN,
    libc::SIGTTOU,
    libc::SIGWINCH,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_name_every_signal_it_declares() {
        for signal in Signal::NAMED {
            assert!(signal.name().is_some(), "{signal} must have a name");
            assert_eq!(Signal::from_number(signal.number()), Some(*signal));
        }
    }

    #[test]
    fn should_reject_a_number_that_is_not_a_signal() {
        assert_eq!(Signal::from_number(0), None);
        assert_eq!(Signal::from_number(-1), None);
        assert_eq!(Signal::from_number(9999), None);
    }

    #[test]
    fn should_render_a_signal_the_shell_does_not_name_by_its_number() {
        let other = Signal::from_number(libc::SIGSYS).expect("SIGSYS is a signal");
        assert_eq!(other.name(), None);
        assert_eq!(other.to_string(), format!("signal {}", libc::SIGSYS));
    }
}
