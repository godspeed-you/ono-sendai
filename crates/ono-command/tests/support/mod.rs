//! Helpers shared by the `ono-command` integration suites.
//!
//! Seven suites declared the same accessor for the embedded registry before it moved here. One
//! definition means one answer to "which contracts is this test reading?" — the embedded ones,
//! always, never a registry a suite happened to build differently.

#![allow(
    clippy::expect_used,
    dead_code,
    reason = "a test states its preconditions directly, and not every helper is used by every \
              test binary (AGENTS.md section 16)"
)]

use ono_command::{Candidate, CommandRegistry, StageContext};

/// The command contracts compiled into the binary, which are the ones every command answers from.
pub fn registry() -> &'static CommandRegistry {
    CommandRegistry::embedded().expect("the embedded command contracts must parse")
}

/// The candidates the completer offers for `line`, with the cursor at its end.
pub fn complete(line: &str) -> Vec<Candidate> {
    let cursor = line.len();
    ono_command::complete(registry(), &StageContext::from_line(line, cursor), None)
}
