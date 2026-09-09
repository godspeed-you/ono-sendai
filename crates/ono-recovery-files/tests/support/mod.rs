#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16), and each test \
              binary uses the part of this fixture it needs"
)]

//! The fixture every suite in this crate shares: a scratch tree, a private store, and the walk
//! from `discover` to a validated asset that §4.5's PREPARE performs.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use ono_change_core::{
    ActionRole, ChangePlan, Execution, Intent, PlanAction, PlanId, ProtectionMode, RecoveryAsset,
    RecoveryObjective, RecoveryProvider,
};
use ono_recovery_files::{FileRecoveryProvider, FileRecoveryStore};

/// A throwaway directory on the filesystem `cargo` builds into, removed when it is dropped.
///
/// [`ono_testkit::scratch`] is the usual answer and it is the wrong one here. It falls back to the
/// system temporary directory when cargo does not export `CARGO_TARGET_TMPDIR` at run time, and on
/// a machine where `/tmp` is a tmpfs that would put every fixture on a volatile filesystem — which
/// is exactly what §15's provider refuses to protect (Appendix B.7). `CARGO_TARGET_TMPDIR` read at
/// *compile* time always names a directory inside `target/`, which is where the build already is.
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "ono-recovery-files-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("the scratch directory can be created");
        Self { path }
    }

    /// The directory's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `contents` to `relative`, creating parent directories as needed.
    pub fn write(&self, relative: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> PathBuf {
        let target = self.path.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("the parent directory can be created");
        }
        std::fs::write(&target, contents).expect("the file can be written");
        target
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The instant every fixture works from: 2026-01-01T00:00:00Z, and never the wall clock.
pub const NOW: i64 = 1_767_225_600;

/// A scratch directory, a private store inside it, and a provider bound to both.
pub struct Fixture {
    pub scratch: Scratch,
    pub provider: FileRecoveryProvider,
}

impl Fixture {
    /// A provider whose store is inside the scratch directory, so nothing touches a real home.
    pub fn new() -> Self {
        let scratch = Scratch::new();
        let store = FileRecoveryStore::open(scratch.path().join("store"))
            .expect("a store can be created inside the scratch directory");
        let provider = FileRecoveryProvider::new(store, at(NOW));
        Self { scratch, provider }
    }

    /// The scratch directory's path.
    pub fn root(&self) -> &Path {
        self.scratch.path()
    }

    /// Writes `contents` to `relative` and answers the absolute path.
    pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
        self.scratch.write(relative, contents)
    }

    /// The absolute path of `relative` inside the scratch directory.
    pub fn path(&self, relative: &str) -> PathBuf {
        self.scratch.path().join(relative)
    }

    /// Everything §4.5 does to protect `path`: discover, plan, create.
    pub fn protect(&self, path: &Path) -> RecoveryAsset {
        self.try_protect(path).expect("the object can be protected")
    }

    /// The same walk, with the refusal visible.
    pub fn try_protect(&self, path: &Path) -> Result<RecoveryAsset, ono_value::ErrorValue> {
        let domain = self
            .provider
            .resolve_domain(&path.display().to_string())?
            .expect("a file on a persistent filesystem resolves to a domain");
        let candidates = self
            .provider
            .discover(&domain, RecoveryObjective::PreserveExact)?;
        let actions = self
            .provider
            .plan_protection(&candidates, ProtectionMode::Prefer)?;
        let action = actions.first().expect("one candidate makes one action");
        self.provider.create(action)
    }
}

/// A recovery action naming `object`, as `plan_recovery` would emit it.
pub fn recovery_action(object: Option<&Path>) -> PlanAction {
    let plan = PlanId::derive(&["test"]);
    let action = PlanAction::new(
        &plan,
        0,
        ActionRole::Recover,
        "restore the protected object",
        Execution::RecoveryOperation {
            provider: std::sync::Arc::from(ono_recovery_files::PROVIDER_ID),
            capability: std::sync::Arc::from("recovery.restore"),
            arguments: Vec::new(),
        },
    );
    match object {
        Some(path) => action.on(path.display().to_string()),
        None => action,
    }
}

/// A plan whose one mutating action targets `object`, which is what a recovery restores.
pub fn plan_touching(object: &Path) -> ChangePlan {
    let plan = ChangePlan::draft(
        Intent::new("set worker_processes", "set file worker_processes"),
        "session",
        at(NOW),
    );
    let action = PlanAction::new(
        plan.id(),
        0,
        ActionRole::Mutate,
        "write the configuration",
        Execution::ProviderAction {
            provider: std::sync::Arc::from("linux.fs"),
            operation: std::sync::Arc::from("ono.file.write"),
            arguments: Vec::new(),
        },
    )
    .on(object.display().to_string());
    plan.with_action(action).expect("a draft takes an action")
}

/// A timestamp `seconds` after the epoch.
pub fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).expect("the fixture instant is a valid timestamp")
}

/// Whether this process can hand a file it owns to another user (§43.4).
///
/// The suites that assert on ownership ask the question of the machine rather than of the
/// provider, so the same test states the contract whether it runs as root or not.
pub fn can_change_owner(directory: &Path) -> bool {
    let probe = directory.join(".ownership-probe");
    std::fs::write(&probe, b"").expect("the scratch directory is writable");
    let answer = rustix::fs::chownat(
        rustix::fs::CWD,
        &probe,
        Some(rustix::fs::Uid::from_raw(65534)),
        None,
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
    )
    .is_ok();
    std::fs::remove_file(&probe).expect("the probe can be removed");
    answer
}

/// Whether the filesystem under `directory` carries user extended attributes.
pub fn supports_xattrs(directory: &Path) -> bool {
    let probe = directory.join(".xattr-probe");
    std::fs::write(&probe, b"").expect("the scratch directory is writable");
    let answer = rustix::fs::lsetxattr(
        &probe,
        "user.ono.suite-probe",
        b"1",
        rustix::fs::XattrFlags::empty(),
    )
    .is_ok();
    std::fs::remove_file(&probe).expect("the probe can be removed");
    answer
}
