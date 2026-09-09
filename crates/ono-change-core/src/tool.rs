//! The seam every storage provider runs a program through (spec v0.6 §12.3).
//!
//! §12.3 forbids a provider from generating shell command strings from user-controlled values, and
//! §43.6 requires provider-generated names to be sanitised. [`ToolRunner`] is how both hold: it
//! takes a program and an argument *vector*, there is no variant that takes a command line, and
//! the real implementation calls `execve` with no shell in between.
//!
//! It exists for a second reason too. §54.4 asks for real ZFS and Btrfs where the environment
//! permits and mock providers for deterministic lifecycle testing — but a provider that is only
//! ever mocked has not been tested (this task's own words). [`ScriptedRunner`] therefore replays
//! *recorded output of the real tools*, so the parsing, the boundary arithmetic and the refusals
//! are exercised against real bytes in the ordinary gate, and the gated real-filesystem suite
//! proves the same code against a live pool.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use ono_value::ErrorValue;

/// What a program said when it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    status: i32,
    stdout: String,
    stderr: String,
}

impl ToolOutput {
    /// A successful run that printed `stdout`.
    #[must_use]
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self {
            status: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    /// A failed run that printed `stderr` and exited `status`.
    #[must_use]
    pub fn failed(status: i32, stderr: impl Into<String>) -> Self {
        Self {
            status,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }

    /// A run with both streams and an explicit status.
    #[must_use]
    pub fn new(status: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            status,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    /// The exit status.
    #[must_use]
    pub const fn status(&self) -> i32 {
        self.status
    }

    /// Standard output.
    #[must_use]
    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    /// Standard error.
    #[must_use]
    pub fn stderr(&self) -> &str {
        &self.stderr
    }

    /// Whether the program exited zero.
    ///
    /// §62.9 and §2.14 both apply: this says the program exited zero, and nothing more. Whether
    /// the intended state exists is what verification is for.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        self.status == 0
    }
}

/// Runs a program with an argument vector, and never a shell (§12.3).
pub trait ToolRunner: Send + Sync + std::fmt::Debug {
    /// Runs `program` with `argv`, waiting no longer than the runner's own bound.
    ///
    /// # Errors
    ///
    /// Returns a structured error when the program could not be started, could not be found, or
    /// did not finish. A program that ran and failed is a [`ToolOutput`] with a non-zero status,
    /// not an error: the provider decides what a non-zero status means for its own semantics.
    fn run(&self, program: &str, argv: &[&str]) -> Result<ToolOutput, ErrorValue>;

    /// Whether `program` is present and executable, without running it.
    fn is_available(&self, program: &str) -> bool;
}

/// A runner that replays recorded output, in order, for tests (§54.4).
#[derive(Debug)]
pub struct ScriptedRunner {
    responses: Mutex<VecDeque<(Arc<str>, ToolOutput)>>,
    calls: Mutex<Vec<(String, Vec<String>)>>,
    available: Vec<Arc<str>>,
}

impl ScriptedRunner {
    /// A runner that will answer `responses`, each keyed by the program it belongs to.
    #[must_use]
    pub fn new(responses: Vec<(&str, ToolOutput)>) -> Self {
        Self {
            responses: Mutex::new(
                responses
                    .into_iter()
                    .map(|(program, output)| (Arc::from(program), output))
                    .collect(),
            ),
            calls: Mutex::new(Vec::new()),
            available: Vec::new(),
        }
    }

    /// Declares which programs the runner reports as present.
    #[must_use]
    pub fn with_available(mut self, programs: &[&str]) -> Self {
        self.available = programs.iter().map(|name| Arc::from(*name)).collect();
        self
    }

    /// Every call that was made, in order, so a test can assert an argument vector.
    ///
    /// This is not a mock expectation. AGENTS.md §11 forbids asserting on call counts as a
    /// contract; what this is for is §43.6 — proving that a dataset name containing shell syntax
    /// arrived as one argument.
    #[must_use]
    pub fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl ToolRunner for ScriptedRunner {
    fn run(&self, program: &str, argv: &[&str]) -> Result<ToolOutput, ErrorValue> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((
                program.to_owned(),
                argv.iter().map(|argument| (*argument).to_owned()).collect(),
            ));
        }
        let mut responses = self
            .responses
            .lock()
            .map_err(|_| crate::error::tool_failed(program, "the scripted runner was poisoned"))?;
        let index = responses
            .iter()
            .position(|(name, _)| name.as_ref() == program)
            .ok_or_else(|| {
                crate::error::tool_failed(
                    program,
                    "the scripted runner has no remaining response for this program",
                )
            })?;
        let (_, output) = responses.remove(index).ok_or_else(|| {
            crate::error::tool_failed(program, "the scripted runner lost its response")
        })?;
        Ok(output)
    }

    fn is_available(&self, program: &str) -> bool {
        self.available.iter().any(|name| name.as_ref() == program)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_pass_an_argument_vector_through_without_a_shell() {
        let runner = ScriptedRunner::new(vec![("/usr/sbin/zfs", ToolOutput::ok(""))]);
        let hostile = "tank/data@ono-a82f; rm -rf /";
        runner
            .run("/usr/sbin/zfs", &["snapshot", hostile])
            .expect("the scripted runner answers");
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].1,
            vec!["snapshot".to_owned(), hostile.to_owned()],
            "§12.3 and §43.6: a name containing shell syntax stays one argument"
        );
    }

    #[test]
    fn should_report_a_failed_program_as_output_rather_than_as_an_error() {
        let runner = ScriptedRunner::new(vec![(
            "/usr/sbin/zfs",
            ToolOutput::failed(1, "cannot create snapshot: out of space"),
        )]);
        let output = runner
            .run("/usr/sbin/zfs", &["snapshot", "tank@x"])
            .expect("a program that ran and failed is not a runner error");
        assert!(!output.succeeded());
        assert!(output.stderr().contains("out of space"));
    }

    #[test]
    fn should_refuse_a_call_the_script_does_not_cover() {
        let runner = ScriptedRunner::new(Vec::new());
        let error = runner
            .run("/usr/sbin/zfs", &["list"])
            .expect_err("an unscripted call is a test defect, not a silent empty answer");
        assert_eq!(error.code().name(), "recovery.provider_unavailable");
    }

    #[test]
    fn should_report_availability_only_for_the_programs_it_was_given() {
        let runner = ScriptedRunner::new(Vec::new()).with_available(&["/usr/sbin/zfs"]);
        assert!(runner.is_available("/usr/sbin/zfs"));
        assert!(
            !runner.is_available("/usr/bin/btrfs"),
            "§54.4: a provider whose tool is absent must degrade rather than pretend"
        );
    }
}
