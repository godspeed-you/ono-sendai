//! Building a child's descriptor table in the parent, where failures are reportable.
//!
//! Everything a command needs open is opened, piped or duplicated here, before `fork`. The
//! child then only has to move the descriptors into place, which is one `dup2` each and needs
//! no allocation. Spec §12.5 wants a redirection failure to be a structured error rather than a
//! silent death inside a child that nobody can question.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::path::Path;

use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::unistd::pipe2;

use crate::command::{Command, Input, Output, Redirect};
use crate::error::{Error, Result};

/// The lowest descriptor a child may be handed freely; 0, 1 and 2 always mean something.
const FIRST_FREE: i32 = 3;

/// What the parent keeps once a stage has been prepared.
pub(crate) struct StageIo {
    /// The descriptors the child must end up with, and the parent's copies of them.
    pub(crate) plan: FdPlan,
    /// The write end of a pipe the parent must feed, with the bytes to write.
    pub(crate) feed: Option<(OwnedFd, Vec<u8>)>,
    /// The read end of the pipe collecting the child's standard output.
    pub(crate) stdout: Option<OwnedFd>,
    /// The read end of the pipe collecting the child's standard error.
    pub(crate) stderr: Option<OwnedFd>,
}

/// The descriptors a child is to be given, keyed by the number it will see them as.
pub(crate) struct FdPlan {
    entries: BTreeMap<i32, OwnedFd>,
    /// Descriptors held open only so that nothing else claims a number the child needs.
    guards: Vec<OwnedFd>,
}

impl FdPlan {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            guards: Vec::new(),
        }
    }

    fn set(&mut self, target: i32, source: OwnedFd) {
        self.entries.insert(target, source);
    }

    /// The `(target, source)` pairs the child duplicates into place, as plain numbers.
    ///
    /// The parent must keep `self` alive until the child has been forked.
    pub(crate) fn moves(&self) -> Vec<(i32, i32)> {
        self.entries
            .iter()
            .map(|(target, source)| (*target, source.as_raw_fd()))
            .collect()
    }

    /// Moves every source above every target, then blocks the numbers in between.
    ///
    /// Two things depend on this. A `dup2` sequence is only order-independent when no source
    /// number is also a target number. And `std::process::Command` allocates its own
    /// exec-error pipe just before forking, which would otherwise land on a low free number
    /// that the child is about to overwrite — turning a failed `exec` into a silent success.
    fn normalise(&mut self) -> Result<()> {
        let Some(highest) = self.entries.keys().copied().max() else {
            return Ok(());
        };
        let floor = highest.max(FIRST_FREE - 1) + 1;
        for source in self.entries.values_mut() {
            *source = redup(&*source, floor)?;
        }
        if floor > FIRST_FREE {
            loop {
                let candidate = File::open("/dev/null")
                    .map(OwnedFd::from)
                    .map_err(|error| Error::from_io("reserving a descriptor", &error))?;
                if candidate.as_raw_fd() >= floor {
                    break;
                }
                self.guards.push(candidate);
            }
        }
        Ok(())
    }
}

/// Prepares one stage: opens its files, makes its pipes, and resolves its duplications.
///
/// `piped_input` and `piped_output` are the pipe ends a surrounding pipeline supplies; they
/// take the place of the command's own [`Input`]/[`Output`] and are themselves overridden by an
/// explicit redirection, which is what a shell does.
pub(crate) fn prepare(
    command: &Command,
    piped_input: Option<OwnedFd>,
    piped_output: Option<OwnedFd>,
) -> Result<StageIo> {
    let mut plan = FdPlan::new();
    let mut feed = None;
    let mut stdout = None;
    let mut stderr = None;

    if let Some(read_end) = piped_input {
        plan.set(0, read_end);
    } else {
        match command.input() {
            Input::Inherit => {}
            Input::Null => plan.set(0, open_null_for_reading()?),
            Input::Bytes(bytes) => {
                let (read_end, write_end) = make_pipe()?;
                plan.set(0, read_end);
                feed = Some((write_end, bytes.clone()));
            }
        }
    }

    if let Some(write_end) = piped_output {
        plan.set(1, write_end);
    } else {
        match command.output() {
            Output::Inherit => {}
            Output::Null => plan.set(1, open_null_for_writing()?),
            Output::Capture => {
                let (read_end, write_end) = make_pipe()?;
                plan.set(1, write_end);
                stdout = Some(read_end);
            }
        }
    }

    match command.error_output() {
        Output::Inherit => {}
        Output::Null => plan.set(2, open_null_for_writing()?),
        Output::Capture => {
            let (read_end, write_end) = make_pipe()?;
            plan.set(2, write_end);
            stderr = Some(read_end);
        }
    }

    for redirect in command.redirects() {
        apply(&mut plan, redirect, command.directory())?;
    }

    plan.normalise()?;
    Ok(StageIo {
        plan,
        feed,
        stdout,
        stderr,
    })
}

/// Prepares a stage whose three standard streams are a pseudoterminal's slave side.
pub(crate) fn prepare_pty(slave: &OwnedFd) -> Result<FdPlan> {
    let mut plan = FdPlan::new();
    for target in 0..=2 {
        let copy = slave
            .try_clone()
            .map_err(|error| Error::from_io("duplicating the terminal", &error))?;
        plan.set(target, copy);
    }
    plan.normalise()?;
    Ok(plan)
}

fn apply(plan: &mut FdPlan, redirect: &Redirect, cwd: Option<&Path>) -> Result<()> {
    match redirect {
        Redirect::Read { fd, path } => {
            let target = against(path, cwd);
            let file = File::open(&target).map_err(|error| {
                Error::from_io(format!("opening {} for reading", path.display()), &error)
            })?;
            plan.set(fd.raw(), OwnedFd::from(file));
        }
        Redirect::Write { fd, path } => {
            let target = against(path, cwd);
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&target)
                .map_err(|error| {
                    Error::from_io(format!("opening {} for writing", path.display()), &error)
                })?;
            plan.set(fd.raw(), OwnedFd::from(file));
        }
        Redirect::Append { fd, path } => {
            let target = against(path, cwd);
            let file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(&target)
                .map_err(|error| {
                    Error::from_io(format!("opening {} for appending", path.display()), &error)
                })?;
            plan.set(fd.raw(), OwnedFd::from(file));
        }
        Redirect::Duplicate { fd, from } => {
            let source = match plan.entries.get(&from.raw()) {
                Some(existing) => existing.try_clone().map_err(|error| {
                    Error::from_io(format!("duplicating descriptor {from}"), &error)
                })?,
                None => duplicate_inherited(from.raw())?,
            };
            plan.set(fd.raw(), source);
        }
    }
    Ok(())
}

/// A redirection path is relative to the directory the command will run in, as in a shell.
fn against<'a>(path: &'a Path, cwd: Option<&Path>) -> Cow<'a, Path> {
    match cwd {
        Some(dir) if path.is_relative() => Cow::Owned(dir.join(path)),
        _ => Cow::Borrowed(path),
    }
}

/// Copies a descriptor the shell itself holds, so `2>&1` means the shell's standard output.
fn duplicate_inherited(number: i32) -> Result<OwnedFd> {
    // SAFETY: the descriptor is borrowed only for the duration of the `fcntl` call and is never
    // closed through this borrow; if it is not open, `fcntl` fails with `EBADF` and nothing
    // else happens.
    let borrowed = unsafe { BorrowedFd::borrow_raw(number) };
    redup(borrowed, FIRST_FREE).map_err(|error| {
        Error::new(
            error.code(),
            format!("duplicating descriptor {number}: {error}"),
        )
    })
}

/// Duplicates `source` onto the lowest free descriptor at or above `floor`, close-on-exec.
fn redup(source: impl std::os::fd::AsFd, floor: i32) -> Result<OwnedFd> {
    let moved = fcntl(source, FcntlArg::F_DUPFD_CLOEXEC(floor))
        .map_err(|errno| Error::from_errno("reserving a descriptor", errno))?;
    // SAFETY: `F_DUPFD_CLOEXEC` returns a freshly allocated descriptor that no other value
    // owns, which is precisely the contract `OwnedFd::from_raw_fd` requires.
    Ok(unsafe { OwnedFd::from_raw_fd(moved) })
}

fn make_pipe() -> Result<(OwnedFd, OwnedFd)> {
    pipe2(OFlag::O_CLOEXEC).map_err(|errno| Error::from_errno("creating a pipe", errno))
}

fn open_null_for_reading() -> Result<OwnedFd> {
    File::open("/dev/null")
        .map(OwnedFd::from)
        .map_err(|error| Error::from_io("opening /dev/null", &error))
}

fn open_null_for_writing() -> Result<OwnedFd> {
    OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .map(OwnedFd::from)
        .map_err(|error| Error::from_io("opening /dev/null", &error))
}
