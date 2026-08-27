//! The shell's controlling terminal: ownership, attributes and window size (spec §18.1).

use std::fs::OpenOptions;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use nix::libc;
use nix::sys::signal::{SigSet, SigmaskHow, Signal as NixSignal, sigprocmask};
use nix::sys::termios::{LocalFlags, SetArg, Termios, tcgetattr, tcsetattr};
use nix::unistd::{Pid, getpgrp, isatty, tcgetpgrp, tcsetpgrp};

use crate::error::{Error, Result};
use crate::spawn::system;

/// The size of a terminal window, in character cells.
///
/// ```
/// use ono_process::WindowSize;
/// let size = WindowSize::new(24, 80);
/// assert_eq!((size.rows, size.columns), (24, 80));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowSize {
    /// Rows of characters.
    pub rows: u16,
    /// Columns of characters.
    pub columns: u16,
}

impl WindowSize {
    /// A window of `rows` by `columns` cells.
    #[must_use]
    pub const fn new(rows: u16, columns: u16) -> Self {
        Self { rows, columns }
    }

    pub(crate) const fn to_winsize(self) -> libc::winsize {
        libc::winsize {
            ws_row: self.rows,
            ws_col: self.columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }

    pub(crate) const fn from_winsize(size: &libc::winsize) -> Self {
        Self {
            rows: size.ws_row,
            columns: size.ws_col,
        }
    }
}

/// The terminal the shell runs on, if it has one.
///
/// A shell that is not attached to a terminal — a script, a `-c` invocation, a process at the
/// end of a pipe — still runs commands, still builds jobs and still reports statuses. It simply
/// never moves terminal ownership around and never touches terminal attributes. Spec §50 and
/// `docs/ACCEPTANCE.md` §4.2 require that path to behave identically, so it is the same code
/// with the terminal absent rather than a separate mode.
#[derive(Debug)]
pub struct Terminal {
    tty: Option<OwnedFd>,
    shell_group: i32,
    saved: Option<Termios>,
}

impl Terminal {
    /// Opens the controlling terminal, or reports that there is none.
    ///
    /// # Errors
    ///
    /// Never fails: a terminal that cannot be opened is simply absent.
    pub fn open() -> Result<Self> {
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .ok()
            .map(OwnedFd::from)
            .filter(|fd| isatty(fd).unwrap_or(false));
        Ok(Self {
            tty,
            shell_group: getpgrp().as_raw(),
            saved: None,
        })
    }

    /// A terminal-free shell, whatever the process is actually attached to.
    #[must_use]
    pub fn detached() -> Self {
        Self {
            tty: None,
            shell_group: getpgrp().as_raw(),
            saved: None,
        }
    }

    /// The terminal's raw descriptor, for a child to claim the foreground with before `exec`.
    ///
    /// The number stays valid in the child because descriptors survive `fork`, and the child
    /// only reads it — ownership stays here.
    #[must_use]
    pub(crate) fn descriptor(&self) -> Option<i32> {
        use std::os::fd::AsRawFd as _;
        self.tty.as_ref().map(|tty| tty.as_raw_fd())
    }

    /// Whether the shell has a terminal to hand to foreground jobs.
    #[must_use]
    pub const fn is_interactive(&self) -> bool {
        self.tty.is_some()
    }

    /// The shell's own process group, which owns the terminal between commands.
    #[must_use]
    pub fn shell_group(&self) -> u32 {
        u32::try_from(self.shell_group).unwrap_or(0)
    }

    /// The process group that currently owns the terminal, if there is a terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal exists but the query fails.
    pub fn foreground_group(&self) -> Result<Option<u32>> {
        let Some(tty) = &self.tty else {
            return Ok(None);
        };
        let group =
            tcgetpgrp(tty).map_err(|errno| system("reading the foreground group", errno))?;
        Ok(Some(u32::try_from(group.as_raw()).unwrap_or(0)))
    }

    /// The terminal's window size, if there is a terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal exists but the query fails.
    pub fn window_size(&self) -> Result<Option<WindowSize>> {
        let Some(tty) = &self.tty else {
            return Ok(None);
        };
        get_window_size(tty.as_fd()).map(Some)
    }

    /// Whether the terminal echoes what is typed at it.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no terminal, or if its attributes cannot be read.
    pub fn echo_enabled(&self) -> Result<bool> {
        let attributes = tcgetattr(self.require()?)
            .map_err(|errno| system("reading the terminal attributes", errno))?;
        Ok(attributes.local_flags.contains(LocalFlags::ECHO))
    }

    fn require(&self) -> Result<BorrowedFd<'_>> {
        self.tty.as_ref().map(AsFd::as_fd).ok_or_else(|| {
            Error::new(
                ono_core::ErrorCode::ProviderUnavailable,
                "there is no controlling terminal",
            )
        })
    }

    /// Remembers the current attributes so they can be put back after a command.
    pub(crate) fn remember_attributes(&mut self) {
        if let Some(tty) = &self.tty {
            self.saved = tcgetattr(tty).ok();
        }
    }

    /// Hands the terminal to `group`, which must already exist.
    ///
    /// `SIGTTOU` is blocked around the call: a shell that has itself been moved into the
    /// background would otherwise be stopped by its own attempt to give the terminal away.
    pub(crate) fn give_to(&self, group: i32) -> Result<()> {
        let Some(tty) = &self.tty else {
            return Ok(());
        };
        without_terminal_stops(|| {
            tcsetpgrp(tty, Pid::from_raw(group))
                .map_err(|errno| system("handing the terminal to the foreground job", errno))
        })
    }

    /// Takes the terminal back and restores the attributes remembered before handing it over.
    ///
    /// A program that dies in raw mode leaves the terminal unusable; putting the saved
    /// attributes back is what stops that from wrecking the shell.
    pub(crate) fn reclaim(&self) -> Result<()> {
        let Some(tty) = &self.tty else {
            return Ok(());
        };
        without_terminal_stops(|| {
            tcsetpgrp(tty, Pid::from_raw(self.shell_group))
                .map_err(|errno| system("taking the terminal back", errno))?;
            if let Some(saved) = &self.saved {
                tcsetattr(tty, SetArg::TCSADRAIN, saved)
                    .map_err(|errno| system("restoring the terminal attributes", errno))?;
            }
            Ok(())
        })
    }
}

/// Runs `body` with the signals a terminal sends to a background process group blocked.
fn without_terminal_stops<T>(body: impl FnOnce() -> Result<T>) -> Result<T> {
    let mut blocked = SigSet::empty();
    blocked.add(NixSignal::SIGTTOU);
    blocked.add(NixSignal::SIGTTIN);
    blocked.add(NixSignal::SIGTSTP);
    let mut previous = SigSet::empty();
    let masked = sigprocmask(SigmaskHow::SIG_BLOCK, Some(&blocked), Some(&mut previous)).is_ok();
    let outcome = body();
    if masked {
        let _ = sigprocmask(SigmaskHow::SIG_SETMASK, Some(&previous), None);
    }
    outcome
}

/// Reads a terminal's window size.
pub(crate) fn get_window_size(fd: BorrowedFd<'_>) -> Result<WindowSize> {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `TIOCGWINSZ` writes a `struct winsize` through the pointer it is given. `size` is
    // a live, correctly typed local, and `fd` is a borrowed descriptor that outlives the call.
    let outcome = unsafe {
        libc::ioctl(
            std::os::fd::AsRawFd::as_raw_fd(&fd),
            libc::TIOCGWINSZ as _,
            &raw mut size,
        )
    };
    if outcome < 0 {
        return Err(Error::from_io(
            "reading the terminal size",
            &std::io::Error::last_os_error(),
        ));
    }
    Ok(WindowSize::from_winsize(&size))
}

/// Sets a terminal's window size. The kernel signals the foreground group when it changes.
pub(crate) fn set_window_size(fd: BorrowedFd<'_>, size: WindowSize) -> Result<()> {
    let size = size.to_winsize();
    // SAFETY: `TIOCSWINSZ` reads a `struct winsize` through the pointer it is given. `size` is a
    // live, correctly typed local, and `fd` is a borrowed descriptor that outlives the call.
    let outcome = unsafe {
        libc::ioctl(
            std::os::fd::AsRawFd::as_raw_fd(&fd),
            libc::TIOCSWINSZ as _,
            &raw const size,
        )
    };
    if outcome < 0 {
        return Err(Error::from_io(
            "setting the terminal size",
            &std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_have_no_terminal_when_it_is_detached() {
        let terminal = Terminal::detached();
        assert!(!terminal.is_interactive());
        assert_eq!(
            terminal.foreground_group().expect("no terminal, no error"),
            None
        );
        assert_eq!(terminal.window_size().expect("no terminal, no error"), None);
        assert!(terminal.echo_enabled().is_err());
    }

    #[test]
    fn should_do_nothing_when_a_detached_terminal_is_moved_around() {
        let terminal = Terminal::detached();
        terminal.give_to(1).expect("handing over is a no-op");
        terminal.reclaim().expect("taking back is a no-op");
    }

    #[test]
    fn should_report_the_shell_process_group() {
        let terminal = Terminal::detached();
        assert!(terminal.shell_group() > 0);
    }

    #[test]
    fn should_convert_between_its_own_size_type_and_the_system_one() {
        let size = WindowSize::new(40, 100);
        assert_eq!(WindowSize::from_winsize(&size.to_winsize()), size);
    }
}
