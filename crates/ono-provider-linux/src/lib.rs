//! The Linux core providers of Ono-Sendai (spec §23, §28, §35.3).
//!
//! Six providers, each answering from a kernel or system interface and never from the output of
//! a program — spec §50's last bullet and AGENTS.md §6 both forbid the latter, and `/proc`,
//! `statvfs` and NSS make it unnecessary:
//!
//! | Target(s) | Provider | Source |
//! |---|---|---|
//! | `process` | [`ProcessProvider`] | `/proc/<pid>/{stat,status,cmdline,exe,cwd,cgroup}` |
//! | `file`, `dir` | [`FileProvider`] | `openat`-relative `fstatat`/`readlinkat` |
//! | `user`, `group` | [`IdentityProvider`] | NSS, and the account databases for enumeration |
//! | `env` | [`EnvProvider`] | the session's own bindings |
//! | `mount`, `filesystem` | [`StorageProvider`] | `/proc/self/mountinfo` and `statvfs(3)` |
//! | `device` | [`DeviceProvider`] | the nodes under `/dev`, `stat(2)` and `/sys/dev` |
//!
//! # The three kinds of nothing
//!
//! Spec §10.5 keeps three answers apart and so does every provider here:
//!
//! - the schema has no such field — a `type.unknown_field` error, before anything runs;
//! - the field exists and its value is not known — `null`, never a fabricated zero;
//! - the field exists and could not be read — an [`ErrorValue`](ono_value::ErrorValue) *in the
//!   field*, so `get process | where user.name == "root"` cannot silently skip the processes
//!   whose owner this user may not see.
//!
//! On any real machine all three occur in the same `get process`, which is why they are three
//! things rather than one.
//!
//! # Wiring the shell up
//!
//! ```no_run
//! use ono_provider_api::ProviderRegistry;
//! use ono_provider_linux::{EnvBinding, register};
//!
//! let mut registry = ProviderRegistry::new();
//! register(
//!     &mut registry,
//!     std::env::vars().map(|(name, value)| EnvBinding::inherited(name, value)),
//! );
//! assert!(registry.provider_for("process").is_ok());
//! ```
//!
//! Each provider can also be built and registered on its own — that is what a fixture-backed
//! test does, and what a shell that wants a different environment source does.

#![forbid(unsafe_code)]

pub mod account_tools;
pub mod accounts;
mod common;
mod device;
mod env;
mod file;
mod file_mutations;
mod file_watch;
mod identity;
mod package_sources;
mod packages;
mod packages_rpm;
mod process;
mod procfs;
pub mod schemas;
mod storage;

use std::sync::Arc;

use ono_provider_api::ProviderRegistry;

/// The pure text decoders of the kernel and system interfaces this crate reads.
///
/// Spec §35.6 requires the procfs decoders to be fuzzed, and a decoder a fuzzer cannot call is a
/// decoder nothing fuzzes: reaching these through a provider means writing a directory tree and
/// driving an async runtime for every input. They are pure functions from the bytes the kernel
/// or the administrator wrote to a struct, each with a contract of its own, and they are exposed
/// here so that contract can be exercised directly (ADR-0313 §5).
///
/// ```
/// let stat = ono_provider_linux::decoders::parse_stat(
///     "4419 ((weird) name) S 1 0 0 0 -1 4194304 0 0 0 0 1 2 0 0 20 0 3 0 100 4096 64",
/// )
/// .expect("a `/proc/<pid>/stat` line");
/// // The executable name may contain spaces and parentheses, so the split is on the last `)`.
/// assert_eq!(stat.comm, "(weird) name");
/// assert_eq!(stat.state, 'S');
/// ```
pub mod decoders {
    pub use crate::procfs::{
        ProcStat, parse_cmdline, parse_stat, parse_status_ids, service_unit, state_name,
    };
    pub use crate::storage::{MountDefinition, MountInfo, parse_fstab, parse_mountinfo};
}

pub use account_tools::AccountCommand;
pub use accounts::{Accounts, GroupAccount, NSS_TIMEOUT, NssAccounts, UserAccount};
pub use device::DeviceProvider;
pub use env::{EnvBinding, EnvProvider, EnvSource};
pub use file::FileProvider;
pub use identity::IdentityProvider;
pub use package_sources::package_source_schema;
pub use packages::{PACKAGE_PROVIDER_ID, PackageProvider, package_schema};
pub use packages_rpm::{RPM_PROVIDER_ID, RpmPackageProvider};
pub use process::{
    Clock, KernelPriorities, KernelSignals, Priorities, ProcessProvider, Signals, SystemClock,
};
pub use storage::StorageProvider;

/// Registers every Linux provider, in the order a shell should consult them.
///
/// `environment` is the session's own bindings: this crate never reads the shell process's
/// environment for it, because the session owns that state and `get env` must answer for the
/// scope the user is in.
pub fn register(
    registry: &mut ProviderRegistry,
    environment: impl IntoIterator<Item = EnvBinding>,
) {
    register_with_env(registry, Arc::new(EnvProvider::new(environment)));
}

/// The same providers, with an `env` provider the caller keeps a handle on — so the session can
/// [`EnvProvider::publish`] its bindings as they change.
pub fn register_with_env(registry: &mut ProviderRegistry, env: Arc<EnvProvider>) {
    registry.register(Arc::new(ProcessProvider::new()));
    registry.register(Arc::new(FileProvider::new()));
    registry.register(Arc::new(IdentityProvider::new()));
    registry.register(env);
    registry.register(Arc::new(StorageProvider::new()));
    registry.register(Arc::new(DeviceProvider::new()));
    // Both package databases are registered on every machine, and the registry answers with
    // the first that is available: dpkg on Debian, rpm on Red Hat and SUSE, and where neither is
    // present each says what it looked for rather than either claiming there are no packages
    // (spec §35.3, ADR-0422).
    registry.register(Arc::new(PackageProvider::new()));
    registry.register(Arc::new(RpmPackageProvider::new()));
}
