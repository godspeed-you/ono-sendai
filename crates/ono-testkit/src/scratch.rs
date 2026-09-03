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

/// Where scratch directories are made.
///
/// `CARGO_TARGET_TMPDIR` when cargo provides it, which puts test scratch inside `target/` on the
/// same filesystem as the build. The system temporary directory is often a small shared tmpfs,
/// and a suite that writes there competes with everything else on the machine for it — this
/// project has already had one runaway file fill it and take every tool on the box down, which is
/// a failure that has nothing to do with the code under test.
fn scratch_root() -> PathBuf {
    std::env::var_os("CARGO_TARGET_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from)
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
    let mut path = scratch_root();
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

/// Writes an executable script and answers where it is.
///
/// Two suites had written this by hand and both were bitten by the same race (issue #27, issue
/// #7): see [`while_text_file_busy`](crate::while_text_file_busy) for what it is.
///
/// # Panics
///
/// Panics if the script cannot be written or made executable.
pub fn executable_script(directory: &std::path::Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = directory.join(name);
    std::fs::write(&path, body).expect("the script must be writable");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("the script must be made executable");
    path
}

/// Runs `attempt` again while it answers that the file it is running is busy.
///
/// `cargo test` runs a crate's tests in threads of one process. A thread that `fork`s between
/// another thread's `open` and `close` of a file inherits the write descriptor, and until that
/// child `exec`s, `execve` on the file answers `ETXTBSY` — *text file busy*. A test that writes a
/// script and runs it therefore fails, at exit 126, for something no part of the shell did: issue
/// #27 saw it once under a `cargo test --workspace` with a container build beside it, and issue
/// #7 is the same race one crate over, where the shim the shell was told to run could not be
/// exec'd.
///
/// The retry is bounded and it is not a blanket one. `busy` is asked whether *this* answer is the
/// machine reporting a busy file — the diagnostic says so in as many words — so every other
/// failure is returned on the first attempt, unretried. A file that stays busy for
/// one second is a finding and is answered as one.
pub fn while_text_file_busy<T>(busy: impl Fn(&T) -> bool, mut attempt: impl FnMut() -> T) -> T {
    let deadline = std::time::Instant::now() + BUSY_PATIENCE;
    loop {
        let outcome = attempt();
        if !busy(&outcome) || std::time::Instant::now() >= deadline {
            return outcome;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// How long a file may stay busy before the answer stands.
///
/// The window is the distance between a `fork` and the `exec` that follows it, which is
/// microseconds. A second is four orders of magnitude of headroom and still fails fast enough to
/// read.
const BUSY_PATIENCE: std::time::Duration = std::time::Duration::from_secs(1);
