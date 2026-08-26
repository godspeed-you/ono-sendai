//! Test fixtures and harness helpers shared by every crate in the workspace.
//!
//! Tests in this project assert observable outcomes, never internal structure (AGENTS.md §11).
//! The helpers here exist to make that easy: run the real binary the way a user would, capture
//! exactly what a user would see, and never hang the suite while doing it.
//!
//! ```no_run
//! use ono_testkit::Shell;
//! let run = Shell::new().args(["--version"]).run();
//! run.assert_success();
//! assert!(run.stdout().starts_with("ono "));
//! ```

#![forbid(unsafe_code)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "this crate is only ever linked into tests, where a failed precondition should abort loudly"
)]

mod run;
mod scratch;

pub use run::{Run, RunError, Shell};
pub use scratch::{Scratch, scratch};

use std::path::PathBuf;

/// Absolute path of the `ono` binary belonging to the current build profile.
///
/// # Panics
///
/// Panics if the binary is missing, which means the test was run without building it.
#[must_use]
pub fn ono_binary() -> PathBuf {
    let mut path = target_dir();
    path.push("ono");
    assert!(
        path.is_file(),
        "the ono binary is not built at {}: run `cargo build` first",
        path.display()
    );
    path
}

/// Directory the current build profile writes its binaries to.
fn target_dir() -> PathBuf {
    // `CARGO_BIN_EXE_*` is only available to the crate that declares the binary, so the path is
    // derived from this crate's manifest instead, which keeps the helper usable everywhere.
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("target");
    path.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    path
}
