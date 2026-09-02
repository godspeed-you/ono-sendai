//! Helpers shared by the `xtask` test suites.
//!
//! One definition per job, because five suites had written the same `repo()` and two of them had
//! already drifted into spelling its return type differently (v0.4.1 §39.1, ADR-0427, ADR-0515).

#![allow(dead_code, reason = "not every helper is used by every test binary")]

use std::path::Path;

/// The workspace root.
pub fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
        .to_path_buf()
}
