//! What the plan-store suites share: a store that belongs to one test and nobody else.
//!
//! v0.4.1 §39.1 wants one definition per helper job. A store under a temporary directory is the
//! outside world these suites stand on, so it lives here once rather than in every suite.

#![allow(
    dead_code,
    clippy::expect_used,
    reason = "a shared test fixture is used by some suites and not by others (AGENTS.md section 16)"
)]

use ono_change_plan::store::PlanStore;
use tempfile::TempDir;

/// A plan store in a fresh temporary directory, removed when the returned guard is dropped.
pub fn store() -> (TempDir, PlanStore) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let store = PlanStore::open(&directory.path().join("plans.sqlite3")).expect("a store opens");
    (directory, store)
}
