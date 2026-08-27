//! The Unix execution layer of the Ono-Sendai shell.
//!
//! Spec §29 lists what a shell must do before it is a shell at all: run external programs,
//! redirect their descriptors, wire them into pipelines, give interactive programs a real
//! terminal, forward signals, and manage jobs. This crate does exactly that, and nothing above
//! it — it knows about processes and descriptors, not about values, rendering or the language.
//!
//! # Shape of the API
//!
//! - a [`Command`] describes one external program: argv, environment changes, working
//!   directory, base stdio disposition and a list of [`Redirect`]s;
//! - a [`Pipeline`] is a sequence of commands connected by real `pipe(2)`s;
//! - an [`Executor`] runs pipelines in the foreground or in the background, owns the
//!   [`Terminal`] and the job table, and hands out a [`Canceller`] for spec §18.5;
//! - a [`PtySession`] runs one command under its own pseudoterminal (spec §29.3).
//!
//! ```no_run
//! use ono_process::{Command, Executor, Output};
//!
//! let mut executor = Executor::new()?;
//! let command = Command::new("uname").arg("-r").stdout(Output::Capture);
//! let outcome = executor.run_foreground(&command.into())?;
//! if let Some(outcome) = outcome.completed() {
//!     println!("uname said {}", String::from_utf8_lossy(outcome.stdout()));
//! }
//! # Ok::<(), ono_process::Error>(())
//! ```
//!
//! # Exit status
//!
//! Statuses follow ADR-0008 and are always [`ono_core::ExitStatus`]. A child's own status is
//! passed through unchanged; `126`, `127` and `128 + N` are originated by this crate only when
//! it was this crate that failed to execute the program or that observed the signal.
//!
//! # Errors
//!
//! Failures that stop a command from running at all are [`Error`]s carrying an
//! [`ono_core::ErrorCode`] from the taxonomy of spec §43. The taxonomy has no generic I/O code,
//! so an operating-system failure that is not "not found", "already exists" or "not a
//! directory" is reported as `io.permission_denied` — "the operating system refused the
//! operation" — with the system's own message preserved in the error text.
//!
//! # `unsafe`
//!
//! Per ADR-0007 this is the only crate in the workspace that may use `unsafe`, and it uses it
//! only where a safe API does not exist: the post-`fork` child setup that must run between
//! `fork` and `exec`, the `TIOCSCTTY`/`TIOCGWINSZ`/`TIOCSWINSZ` ioctls that `nix` does not wrap,
//! and installing a signal handler. No `unsafe` API and no raw descriptor crosses this crate's
//! boundary.

#![deny(unsafe_op_in_unsafe_fn)]
#![allow(
    unsafe_code,
    reason = "ADR-0007: post-fork terminal and process-group setup is only expressible in a pre_exec closure"
)]

mod command;
mod error;
mod executor;
mod fd;
mod job;
mod pipeline;
mod plan;
mod pty;
mod resolve;
mod signals;
mod spawn;
mod terminal;

pub use command::{Command, Input, Output, Redirect};
pub use error::{Error, Result};
pub use executor::{Canceller, Executor, Foreground, ForegroundOutcome};
pub use fd::Fd;
pub use job::{Job, JobChange, JobId, JobProcess, JobState};
pub use pipeline::{Pipeline, PipelineOutcome, StageOutcome};
pub use pty::PtySession;
/// The effective user id this shell runs as.
///
/// Spec §17.2 requires an elevated context to be impossible to miss, and the prompt is where
/// nobody can miss it — so the answer has to come from the kernel, not from `$USER`, which any
/// caller can set to anything.
#[must_use]
pub fn effective_uid() -> u32 {
    nix::unistd::geteuid().as_raw()
}

pub use signals::{
    Signal, install_child_watch, install_shell_signals, take_child_transition, take_interrupt,
};
pub use terminal::{Terminal, WindowSize};
