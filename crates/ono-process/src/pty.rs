//! Running a command under its own pseudoterminal (spec §29.3).
//!
//! `vim` must behave like `vim`. That means a real terminal device, a session of its own with
//! that device as controlling terminal, a window size that matches the shell's, and bytes
//! moving in both directions without interpretation.

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, fcntl};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::pty::openpty;
use nix::sys::termios::{SetArg, Termios, cfmakeraw, tcgetattr, tcsetattr};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, isatty, write as write_fd};
use ono_core::ExitStatus;

use crate::command::Command;
use crate::error::{Error, Result};
use crate::plan;
use crate::resolve::{self, Resolution};
use crate::signals::Signal;
use crate::spawn::{self, SpawnRequest};
use crate::terminal::{WindowSize, get_window_size, set_window_size};

/// A command running under a pseudoterminal of its own.
///
/// The child is a session leader with the terminal as its controlling terminal, so it sees a
/// TTY on all three standard streams, receives the terminal's signals, and is told when the
/// window changes size.
#[derive(Debug)]
pub struct PtySession {
    master: OwnedFd,
    pid: i32,
    status: Option<ExitStatus>,
}

impl PtySession {
    pub(crate) fn start(command: &Command, size: WindowSize) -> Result<Self> {
        let winsize = size.to_winsize();
        let pty = openpty(&winsize, None::<&Termios>)
            .map_err(|errno| spawn::system("allocating a pseudoterminal", errno))?;
        // `openpty` hands back two ordinary descriptors, and an ordinary descriptor survives
        // `exec`. Without this the program started below inherits the *master* side of the very
        // terminal it is reading from, so the last reference to that master is held by the
        // program itself: closing it in the shell can never produce end of file, and a shell
        // started this way waits for input that nobody can ever send (ADR-0160). The slave is
        // closed for the same reason — the child gets its own duplicates on 0, 1 and 2.
        close_on_exec(&pty.master, "the terminal")?;
        close_on_exec(&pty.slave, "the terminal")?;

        let env = command.resolved_env();
        let path = resolve::effective_path(env.as_deref(), command.clears_env());
        let resolved = resolve::resolve(command.program(), path.as_deref(), command.directory());
        let program = match resolved {
            Resolution::Found(program) => program,
            other => {
                let (_, error) = other.failure().unwrap_or_else(|| {
                    (
                        ExitStatus::NOT_FOUND,
                        Error::new(
                            ono_core::ErrorCode::ResolveCommandNotFound,
                            "command not found",
                        ),
                    )
                });
                return Err(error);
            }
        };

        let fd_plan = plan::prepare_pty(&pty.slave)?;
        let request = SpawnRequest {
            program: &program,
            args: command.args_slice(),
            env: env.as_deref(),
            clear_env: command.clears_env(),
            cwd: command.directory(),
            process_group: None,
            controlling_terminal: Some(0),
            // A new session leader on its own pty is the foreground group already.
            claim_foreground: None,
        };
        let pid = spawn::spawn(&request, &fd_plan)
            .map_err(|error| spawn::cannot_run(&program, &error))?;
        drop(fd_plan);
        drop(pty.slave);

        Ok(Self {
            master: pty.master,
            pid,
            status: None,
        })
    }

    /// The process id of the program running under the terminal.
    #[must_use]
    pub fn pid(&self) -> u32 {
        u32::try_from(self.pid).unwrap_or(0)
    }

    /// The terminal's current window size.
    ///
    /// # Errors
    ///
    /// Returns an error if the size cannot be read.
    pub fn window_size(&self) -> Result<WindowSize> {
        get_window_size(self.master.as_fd())
    }

    /// Changes the terminal's window size.
    ///
    /// The kernel sends `SIGWINCH` to the terminal's foreground process group when the size
    /// actually changes, which is how a full-screen program learns to redraw.
    ///
    /// # Errors
    ///
    /// Returns an error if the size cannot be set.
    pub fn resize(&mut self, size: WindowSize) -> Result<()> {
        set_window_size(self.master.as_fd(), size)
    }

    /// Reads whatever the program has written, blocking until there is something.
    ///
    /// Returns `0` at end of file, which on a pseudoterminal means the program has gone.
    ///
    /// # Errors
    ///
    /// Returns an error if the read fails for a reason other than the program exiting.
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        loop {
            match nix::unistd::read(&self.master, buffer) {
                Ok(read) => return Ok(read),
                // The last slave descriptor closed: the program is gone, so this is end of file.
                Err(Errno::EIO) => return Ok(0),
                Err(Errno::EINTR) => continue,
                Err(errno) => return Err(spawn::system("reading the terminal", errno)),
            }
        }
    }

    /// Reads whatever the program has written, giving up after `timeout`.
    ///
    /// `None` means nothing arrived in time; `Some(0)` means the program has gone.
    ///
    /// # Errors
    ///
    /// Returns an error if the read fails for a reason other than the program exiting.
    pub fn read_timeout(&mut self, buffer: &mut [u8], timeout: Duration) -> Result<Option<usize>> {
        if !wait_readable(self.master.as_fd(), timeout)? {
            return Ok(None);
        }
        self.read(buffer).map(Some)
    }

    /// Writes bytes to the program as if they had been typed.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize> {
        loop {
            match write_fd(&self.master, bytes) {
                Ok(written) => return Ok(written),
                Err(Errno::EINTR) => continue,
                Err(errno) => return Err(spawn::system("writing to the terminal", errno)),
            }
        }
    }

    /// Writes every byte, or reports why it could not.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    pub fn write_all(&mut self, mut bytes: &[u8]) -> Result<()> {
        while !bytes.is_empty() {
            let written = self.write(bytes)?;
            if written == 0 {
                return Err(Error::new(
                    ono_core::ErrorCode::IoPermissionDenied,
                    "the terminal accepted no bytes",
                ));
            }
            bytes = &bytes[written..];
        }
        Ok(())
    }

    /// Sends a signal to the program's process group.
    ///
    /// # Errors
    ///
    /// Returns an error if the signal is not a signal, or cannot be sent.
    pub fn signal(&self, signal: Signal) -> Result<()> {
        match nix::sys::signal::killpg(Pid::from_raw(self.pid), signal.to_nix()?) {
            Ok(()) | Err(Errno::ESRCH) => Ok(()),
            Err(errno) => Err(spawn::system("signalling the terminal session", errno)),
        }
    }

    /// Waits for the program to finish and reports its status (ADR-0008).
    ///
    /// # Errors
    ///
    /// Returns an error if waiting fails.
    pub fn wait(&mut self) -> Result<ExitStatus> {
        loop {
            if let Some(status) = self.status {
                return Ok(status);
            }
            match waitpid(Pid::from_raw(self.pid), None) {
                Ok(status) => self.absorb(status),
                Err(Errno::EINTR) => continue,
                Err(Errno::ECHILD) => {
                    self.status = Some(ExitStatus::SUCCESS);
                }
                Err(errno) => return Err(spawn::system("waiting for the terminal session", errno)),
            }
        }
    }

    /// Reports the program's status if it has finished, without waiting.
    ///
    /// # Errors
    ///
    /// Returns an error if waiting fails.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if self.status.is_some() {
            return Ok(self.status);
        }
        match waitpid(Pid::from_raw(self.pid), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => Ok(None),
            Ok(status) => {
                self.absorb(status);
                Ok(self.status)
            }
            Err(Errno::EINTR) => Ok(None),
            Err(Errno::ECHILD) => {
                self.status = Some(ExitStatus::SUCCESS);
                Ok(self.status)
            }
            Err(errno) => Err(spawn::system("waiting for the terminal session", errno)),
        }
    }

    /// Waits for the program to finish, giving up after `timeout`.
    ///
    /// # Errors
    ///
    /// Returns an error if waiting fails.
    pub fn wait_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Carries bytes between `input`/`output` and the terminal until the program exits.
    ///
    /// This is what the shell does while an interactive program owns the screen: it stops
    /// rendering anything of its own and becomes a wire (spec §29.3). When `input` is itself a
    /// terminal it is put into raw mode for the duration, so the program sees every keystroke,
    /// and its window size is mirrored onto the pseudoterminal — including while the program
    /// runs, so resizing the real window resizes the program's.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal cannot be read or written.
    pub fn relay(&mut self, input: &impl AsFd, output: &impl AsFd) -> Result<ExitStatus> {
        let input = input.as_fd();
        let output = output.as_fd();
        let _raw = RawMode::enter(input)?;
        let mut mirrored = self.mirror_size(input).ok();

        let mut buffer = [0u8; 8192];
        let mut input_open = true;
        loop {
            let (ready, master_ready, input_ready) = {
                let mut watched = Vec::with_capacity(2);
                watched.push(PollFd::new(self.master.as_fd(), PollFlags::POLLIN));
                if input_open {
                    watched.push(PollFd::new(input, PollFlags::POLLIN));
                }
                let ready = match poll(&mut watched, PollTimeout::from(100u8)) {
                    Ok(ready) => ready,
                    Err(Errno::EINTR) => continue,
                    Err(errno) => return Err(spawn::system("waiting on the terminal", errno)),
                };
                let readable = |events: Option<PollFlags>| {
                    events.is_some_and(|events| {
                        events
                            .intersects(PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR)
                    })
                };
                (
                    ready,
                    readable(watched[0].revents()),
                    input_open && readable(watched.get(1).and_then(PollFd::revents)),
                )
            };

            if ready == 0 {
                // No traffic: a good moment to notice that the real window changed size.
                if let Ok(current) = get_window_size(input)
                    && mirrored != Some(current)
                {
                    let _ = self.resize(current);
                    mirrored = Some(current);
                }
                if self.try_wait()?.is_some() {
                    // The program is gone; pass on whatever is still buffered and stop.
                    while let Some(read) = self.read_timeout(&mut buffer, Duration::ZERO)? {
                        if read == 0 {
                            break;
                        }
                        write_all_fd(output, &buffer[..read])?;
                    }
                    break;
                }
                continue;
            }

            if master_ready {
                let read = self.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                write_all_fd(output, &buffer[..read])?;
            }

            if input_ready {
                match nix::unistd::read(input, &mut buffer) {
                    Ok(0) => input_open = false,
                    Ok(read) => self.write_all(&buffer[..read])?,
                    Err(Errno::EINTR) => {}
                    Err(_) => input_open = false,
                }
            }
        }

        self.wait()
    }

    fn mirror_size(&mut self, input: BorrowedFd<'_>) -> Result<WindowSize> {
        let size = get_window_size(input)?;
        self.resize(size)?;
        Ok(size)
    }

    fn absorb(&mut self, status: WaitStatus) {
        match status {
            WaitStatus::Exited(_, code) => {
                self.status = Some(ExitStatus::from_code(
                    u8::try_from(code & 0xff).unwrap_or(1),
                ));
            }
            WaitStatus::Signaled(_, signal, _) => {
                self.status = Some(ExitStatus::from_signal(
                    u8::try_from(signal as i32).unwrap_or(0),
                ));
            }
            _ => {}
        }
    }
}

/// Marks `fd` close-on-exec, so no program the shell starts inherits it.
fn close_on_exec(fd: &impl AsFd, what: &str) -> Result<()> {
    fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))
        .map(|_| ())
        .map_err(|errno| spawn::system(&format!("closing {what} on exec"), errno))
}

/// Waits until `fd` has something to read, or `timeout` passes.
fn wait_readable(fd: BorrowedFd<'_>, timeout: Duration) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let millis = u16::try_from(remaining.as_millis()).unwrap_or(u16::MAX);
        let mut watched = [PollFd::new(fd, PollFlags::POLLIN)];
        match poll(&mut watched, PollTimeout::from(millis)) {
            Ok(0) => return Ok(false),
            Ok(_) => return Ok(true),
            Err(Errno::EINTR) => {
                if Instant::now() >= deadline {
                    return Ok(false);
                }
            }
            Err(errno) => return Err(spawn::system("waiting on the terminal", errno)),
        }
    }
}

fn write_all_fd(fd: BorrowedFd<'_>, mut bytes: &[u8]) -> Result<()> {
    while !bytes.is_empty() {
        match write_fd(fd, bytes) {
            Ok(0) => return Ok(()),
            Ok(written) => bytes = &bytes[written..],
            Err(Errno::EINTR) => {}
            Err(errno) => return Err(spawn::system("writing the relayed bytes", errno)),
        }
    }
    Ok(())
}

/// Puts a terminal into raw mode for as long as it is held, and restores it afterwards.
struct RawMode<'fd> {
    fd: BorrowedFd<'fd>,
    saved: Option<Termios>,
}

impl<'fd> RawMode<'fd> {
    fn enter(fd: BorrowedFd<'fd>) -> Result<Self> {
        if !isatty(fd).unwrap_or(false) {
            return Ok(Self { fd, saved: None });
        }
        let saved = tcgetattr(fd)
            .map_err(|errno| spawn::system("reading the terminal attributes", errno))?;
        let mut raw = saved.clone();
        cfmakeraw(&mut raw);
        tcsetattr(fd, SetArg::TCSANOW, &raw)
            .map_err(|errno| spawn::system("entering raw mode", errno))?;
        Ok(Self {
            fd,
            saved: Some(saved),
        })
    }
}

impl Drop for RawMode<'_> {
    fn drop(&mut self) {
        if let Some(saved) = &self.saved {
            let _ = tcsetattr(self.fd, SetArg::TCSADRAIN, saved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_leave_a_non_terminal_alone_when_raw_mode_is_requested() {
        let file = std::fs::File::open("/dev/null").expect("/dev/null must open");
        let guard = RawMode::enter(file.as_fd()).expect("a plain file is simply left alone");
        assert!(guard.saved.is_none());
    }
}
