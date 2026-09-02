//! Helpers shared by the `ono-adapter` test suites (v0.4.1 §39.1, ADR-0427, ADR-0515).

#![allow(
    clippy::expect_used,
    dead_code,
    reason = "a test states its preconditions directly, and not every helper is used by every \
              test binary (AGENTS.md section 16)"
)]

use std::path::PathBuf;

pub fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/adapters/fixtures")
        .canonicalize()
        .expect("the fixtures directory exists")
}
