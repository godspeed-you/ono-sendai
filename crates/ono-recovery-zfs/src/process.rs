//! The real [`ToolRunner`]: `execve`, an argument vector, and a bound on the wait (§12.3).
//!
//! §12.3 is two prohibitions in one sentence — *"A provider MUST use direct process APIs ... It
//! MUST NOT generate shell command strings from user-controlled values"* — and §43.6 adds that
//! provider-generated names must be sanitised. [`ProcessRunner`] holds the first: it takes a
//! program and an argument vector, hands both to [`std::process::Command`], and there is no code
//! path through this type that produces a string a shell could see. A dataset called
//! `tank/x; rm -rf /` arrives at ZFS as one argument containing a semicolon.
//!
//! The bounded wait is the third thing §12.3 needs and the one that is easy to forget. A `zfs`
//! command against a pool whose disks are not answering blocks in the kernel, and an unbounded
//! wait turns that into a shell that never returns a prompt. The runner waits [`Self::timeout`]
//! and then kills the child and refuses, so an unresponsive pool is an error a plan can show
//! rather than a hang an operator has to diagnose.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ono_change_core::error::tool_failed;
use ono_change_core::{ToolOutput, ToolRunner};
use ono_value::ErrorValue;

/// How long a ZFS command may run before the runner gives up on it.
///
/// Thirty seconds is chosen against what the commands actually do: `zfs list`, `zfs get` and
/// `zpool list` read in-memory pool state and return in milliseconds, and `zfs snapshot` is the
/// copy-on-write operation §13.2 calls cheap. A command still running after thirty seconds is
/// waiting on hardware, and §56.3's direction is to say so.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// How often the runner looks to see whether the child has finished.
const POLL: Duration = Duration::from_millis(10);

/// Runs a program through `execve` with no shell between (§12.3).
#[derive(Debug, Clone)]
pub struct ProcessRunner {
    timeout: Duration,
}

impl Default for ProcessRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessRunner {
    /// A runner with the default bounded wait.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// A runner that waits at most `timeout` for any one program.
    #[must_use]
    pub const fn within(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// The bound this runner waits.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl ToolRunner for ProcessRunner {
    fn run(&self, program: &str, argv: &[&str]) -> Result<ToolOutput, ErrorValue> {
        let mut child = Command::new(program)
            .args(argv)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                tool_failed(
                    program,
                    &format!(
                        "v0.6 §12.3: the provider runs the program directly, with no shell. It \
                         could not be started: {error}"
                    ),
                )
            })?;

        // The streams are drained on threads of their own. A child that fills a pipe buffer
        // blocks in `write`, and a parent that waits for exit before reading deadlocks with it —
        // which would defeat the bounded wait rather than enforce it.
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(tool_failed(
                            program,
                            &format!(
                                "v0.6 §12.3: it did not finish within {:?} and was terminated. A \
                                 ZFS command that runs this long is waiting on hardware, and \
                                 §56.3 makes that a refusal rather than an assumption",
                                self.timeout
                            ),
                        ));
                    }
                    std::thread::sleep(POLL);
                }
                Err(error) => {
                    return Err(tool_failed(
                        program,
                        &format!("waiting for it to finish failed: {error}"),
                    ));
                }
            }
        };

        // A signalled child has no exit code. §2.14 forbids treating "it stopped" as "it worked",
        // so the absence becomes a distinct non-zero status rather than a zero.
        let code = status.code().unwrap_or(-1);
        Ok(ToolOutput::new(
            code,
            stdout.recv().unwrap_or_default(),
            stderr.recv().unwrap_or_default(),
        ))
    }

    fn is_available(&self, program: &str) -> bool {
        std::fs::metadata(program).is_ok_and(|metadata| metadata.is_file())
    }
}

/// Reads one of the child's streams to the end on a thread, so neither pipe can fill and block.
fn drain<R: Read + Send + 'static>(stream: Option<R>) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    match stream {
        Some(mut stream) => {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = stream.read_to_string(&mut text);
                let _ = sender.send(text);
            });
        }
        None => {
            let _ = sender.send(String::new());
        }
    }
    receiver
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_refuse_a_program_that_is_not_there_rather_than_reporting_empty_output() {
        let runner = ProcessRunner::new();
        let error = runner
            .run("/nonexistent/ono/zfs", &["version"])
            .expect_err("§54.4: a provider whose tool is absent degrades rather than pretends");
        assert_eq!(error.code().name(), "recovery.provider_unavailable");
    }

    #[test]
    fn should_report_a_missing_program_as_unavailable_without_running_it() {
        assert!(
            !ProcessRunner::new().is_available("/nonexistent/ono/zfs"),
            "Appendix G.4: availability is answered before anything is executed"
        );
    }
}
