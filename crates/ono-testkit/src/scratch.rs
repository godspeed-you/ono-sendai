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

/// Where scratch directories are made: `<cargo target directory>/tmp`, which is the directory
/// cargo names `CARGO_TARGET_TMPDIR` for an integration test.
///
/// The system temporary directory is often a small shared tmpfs, and a suite that writes there
/// competes with everything else on the machine for it — this project has already had one runaway
/// file fill it and take every tool on the box down, which is a failure that has nothing to do with
/// the code under test. A tmpfs is also the volatile filesystem v0.6 §15's file recovery provider
/// refuses to protect, so a suite scratching there tests the refusal rather than the feature.
///
/// `CARGO_TARGET_TMPDIR` is a *compile-time* variable of the test being built, and this helper is
/// compiled into `ono-testkit`, where cargo never sets it. So the directory is found at run time
/// instead, from the running test binary: cargo puts every test executable under its target
/// directory, and marks that directory with a `CACHEDIR.TAG` (issue #143, ADR-0890).
fn scratch_root() -> PathBuf {
    let mut workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    workspace.pop();
    workspace.pop();
    scratch_root_in(&Surroundings {
        target_tmpdir: std::env::var_os("CARGO_TARGET_TMPDIR").map(PathBuf::from),
        executable: std::env::current_exe().ok(),
        workspace,
    })
}

/// What the scratch root is decided from, gathered in one place so the decision can be tested
/// against surroundings a test builds.
struct Surroundings {
    /// `CARGO_TARGET_TMPDIR` from the environment, if a runner set it.
    target_tmpdir: Option<PathBuf>,
    /// The running test binary.
    executable: Option<PathBuf>,
    /// The workspace root the testkit was built in.
    workspace: PathBuf,
}

/// The scratch root for `surroundings`.
fn scratch_root_in(surroundings: &Surroundings) -> PathBuf {
    if let Some(directory) = &surroundings.target_tmpdir {
        return directory.clone();
    }
    surroundings
        .executable
        .as_deref()
        .and_then(cargo_target_of)
        .unwrap_or_else(|| {
            // Not a binary cargo built in place — a doc test, which rustdoc links in a temporary
            // directory of its own. The workspace's own target directory is still the right
            // filesystem, and it is where `ono_binary` looks too.
            surroundings.workspace.join("target")
        })
        .join("tmp")
}

/// The cargo target directory `executable` was built into, if it was built into one.
fn cargo_target_of(executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .skip(1)
        .find(|directory| directory.join("CACHEDIR.TAG").is_file())
        .map(Path::to_path_buf)
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

/// Creates a scratch directory that belongs to no source-control checkout.
///
/// [`scratch`] lives in cargo's target directory, which is usually inside the checkout under
/// test, so a test whose premise is "no checkout here" has to own a directory outside it. This
/// one is made in the system temporary directory, which the test only needs to be empty of
/// `.git`, and it refuses to hand out a directory that has a checkout above it anyway.
///
/// # Panics
///
/// Panics if the directory cannot be created, or if a `.git` sits in one of its ancestors — the
/// premise the caller relies on would then be false.
#[must_use]
pub fn scratch_outside_any_checkout() -> Scratch {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "ono-test-unversioned-{}-{unique}",
        std::process::id()
    ));
    if let Some(checkout) = path
        .ancestors()
        .find(|directory| directory.join(".git").exists())
    {
        panic!(
            "{} is inside the checkout at {}; a test that needs a directory outside every \
             checkout cannot have one here",
            path.display(),
            checkout.display()
        );
    }
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

/// Writes an executable script and answers where it is, in a state any thread may `exec` at once.
///
/// Writing the file from this process is what made scripts busy (issue #188). `cargo test` runs a
/// crate's tests as threads of one process, and a thread that starts a process while this one has
/// the file open for writing hands its child a copy of that descriptor; until the child reaches
/// its own `exec`, the kernel refuses to execute the file with `ETXTBSY`. The refusal is per
/// inode, so writing to a temporary name and renaming does not help — the renamed inode is the one
/// the stray descriptor points at. What does help is never holding such a descriptor: the bytes
/// are written by a `/bin/sh` of their own, a separate process no test thread forks from, and by
/// the time it has been waited for, no descriptor open for writing to the file exists anywhere.
///
/// The file is written under a staging name and renamed into place, so a script being replaced
/// is never seen half-written (ADR-0891).
///
/// # Panics
///
/// Panics if the script cannot be written or made executable.
pub fn executable_script(directory: &std::path::Path, name: &str, body: &str) -> PathBuf {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt;

    let path = directory.join(name);
    let staged = staging_name(&path);

    let mut writer = std::process::Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\"", "sh"])
        .arg(&staged)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("/bin/sh must be available to write a script");
    writer
        .stdin
        .take()
        .expect("the writer's standard input was piped")
        .write_all(body.as_bytes())
        .expect("the script's body must reach its writer");
    let written = writer
        .wait_with_output()
        .expect("the script's writer must be waited for");
    assert!(
        written.status.success(),
        "cannot write {}: {}",
        staged.display(),
        String::from_utf8_lossy(&written.stderr)
    );

    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
        .expect("the script must be made executable");
    std::fs::rename(&staged, &path)
        .unwrap_or_else(|error| panic!("cannot move the script to {}: {error}", path.display()));
    path
}

/// Copies the program at `source` to `destination`, executable, in a state any thread may `exec`
/// at once.
///
/// `std::fs::copy` holds the copy open for writing in this process, which is the race
/// [`executable_script`] exists to avoid; the copy here is made by a `cp` of its own under a
/// staging name, then made executable and renamed into place (issue #188, ADR-0891).
///
/// # Panics
///
/// Panics if the program cannot be copied or made executable.
pub fn executable_copy(source: impl AsRef<Path>, destination: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let source = source.as_ref();
    let staged = staging_name(destination);
    let copied = std::process::Command::new("cp")
        .arg("--")
        .arg(source)
        .arg(&staged)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("cp must be available to copy a program");
    assert!(
        copied.status.success(),
        "cannot copy {} to {}: {}",
        source.display(),
        staged.display(),
        String::from_utf8_lossy(&copied.stderr)
    );
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
        .expect("the copy must be made executable");
    std::fs::rename(&staged, destination).unwrap_or_else(|error| {
        panic!("cannot move the copy to {}: {error}", destination.display())
    });
    destination.to_path_buf()
}

/// A name beside `destination`, unique to this process and call, for a file that is renamed into
/// place once it is complete.
fn staging_name(destination: &Path) -> PathBuf {
    static STAGED: AtomicU64 = AtomicU64::new(0);
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    destination.with_file_name(format!(
        ".{name}.{}-{}.staged",
        std::process::id(),
        STAGED.fetch_add(1, Ordering::Relaxed)
    ))
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
