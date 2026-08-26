//! A throwaway directory for a single test.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A directory that exists for the lifetime of the value and is removed when it is dropped.
///
/// Tests that touch the filesystem must not depend on, or leave anything in, the machine that
/// runs them (AGENTS.md §11: isolated).
#[derive(Debug)]
pub struct Scratch {
    path: PathBuf,
}

/// Creates a scratch directory unique to this process and call.
///
/// # Panics
///
/// Panics if the directory cannot be created, which means the test cannot run at all.
#[must_use]
pub fn scratch() -> Scratch {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!("ono-test-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&path)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", path.display()));
    Scratch { path }
}

impl Scratch {
    /// The directory's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `contents` to `relative`, creating parent directories as needed.
    ///
    /// # Panics
    ///
    /// Panics if the file cannot be written.
    pub fn write(&self, relative: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> PathBuf {
        let target = self.path.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("cannot create {}: {error}", parent.display()));
        }
        std::fs::write(&target, contents)
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", target.display()));
        target
    }

    /// Reads `relative` as UTF-8.
    ///
    /// # Panics
    ///
    /// Panics if the file cannot be read or is not UTF-8.
    #[must_use]
    pub fn read(&self, relative: impl AsRef<Path>) -> String {
        let target = self.path.join(relative);
        std::fs::read_to_string(&target)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", target.display()))
    }

    /// Whether `relative` exists inside the scratch directory.
    #[must_use]
    pub fn exists(&self, relative: impl AsRef<Path>) -> bool {
        self.path.join(relative).exists()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A failure here must not mask the test's own outcome.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
