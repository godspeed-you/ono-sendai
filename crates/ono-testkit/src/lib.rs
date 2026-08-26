//! Test fixtures and harness helpers shared by every crate in the workspace.
//!
//! Tests in this project assert observable outcomes, never internal structure (AGENTS.md
//! section 11). The helpers here exist to make that easy: locating the built binary, running it
//! non-interactively, and comparing what a user would actually see.

#![allow(
    clippy::expect_used,
    reason = "this crate is only ever linked into tests, where a failed precondition should abort loudly"
)]

use std::path::PathBuf;
use std::process::{Command, Output};

/// Absolute path of the `ono` binary belonging to the current build profile.
///
/// # Panics
///
/// Panics if the binary is missing, which means the test was run without building it.
#[must_use]
pub fn ono_binary() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("target");
    path.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    path.push("ono");
    assert!(
        path.is_file(),
        "the ono binary is not built at {}: run `cargo build` first",
        path.display()
    );
    path
}

/// Runs the shell binary non-interactively with the given arguments.
///
/// # Panics
///
/// Panics if the binary cannot be executed at all.
#[must_use]
pub fn run_ono(args: &[&str]) -> Output {
    Command::new(ono_binary())
        .args(args)
        .output()
        .expect("the ono binary must be executable")
}

/// Standard output of a run, as the user would see it.
///
/// # Panics
///
/// Panics if the output is not valid UTF-8.
#[must_use]
pub fn stdout_of(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("shell output must be valid UTF-8")
}
