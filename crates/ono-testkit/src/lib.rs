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

mod profile;
mod rng;
mod run;
mod scratch;

pub use profile::{
    BuiltBy, PROFILE_L, PROFILE_M, PROFILE_S, PayloadDeclaration, ProcessPopulation, Profile,
    ProfileDeclaration, SocketPopulation, declared_payloads, declared_profiles, payload,
};
pub use rng::Rng;
pub use run::{Run, RunError, Shell};
pub use scratch::{Scratch, scratch};

use std::path::PathBuf;
use std::time::Duration;

/// Runs `script` through the real binary the way `ono -c <script>` would, and captures what a
/// user would see.
///
/// This is the shape almost every integration test wants, and before it lived here two dozen
/// suites declared it themselves — which is two dozen chances to declare it slightly differently.
/// Use [`Shell`] directly when a test needs an environment, a working directory or standard
/// input; use this when it only needs the answer.
#[must_use]
pub fn ono(script: &str) -> Run {
    Shell::new().args(["-c", script]).run()
}

/// As [`ono`], with an explicit budget for a script that outlives the default one.
///
/// A suite that spawns real children, waits on a live stream or drives a provider that talks to
/// the system needs longer than [`Shell`]'s default. Naming the budget keeps it in the call
/// rather than in a per-file copy of this function.
#[must_use]
pub fn ono_within(script: &str, budget: Duration) -> Run {
    Shell::new().args(["-c", script]).timeout(budget).run()
}

/// Announces that a test could not exercise its subject on this host, and why.
///
/// `cargo test` knows two outcomes. A test whose precondition this host cannot meet — no second
/// mount to cross, no `git` on `PATH`, running as root where the assertion is what a normal user
/// is refused — is neither of them: it returns early, the summary counts it as `ok`, and the
/// suite reports coverage it did not have. That is the one failure mode a green suite must not
/// hide.
///
/// There is no third outcome to return, so the honesty is in the record: every skip prints the
/// same marker, naming the test and its reason, on the stream a test harness shows. `SKIPPED` is
/// greppable in a CI log, and `xtask spec-check` refuses a skip announced any other way, so the
/// count of them is a number somebody can look up rather than a thing nobody knows.
///
/// A skip is a last resort. Prefer arranging the precondition — spawning the child, binding the
/// listener, creating the file — over asking the host for it (ADR-0417).
pub fn skipped(reason: &str) {
    // `cargo test` names each test's thread after the test, which makes the marker self-locating
    // without the caller repeating a name that could go stale.
    let test = std::thread::current()
        .name()
        .unwrap_or("<unnamed>")
        .to_owned();
    eprintln!("SKIPPED {test}: {reason}");
}

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
