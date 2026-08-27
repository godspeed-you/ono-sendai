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

pub mod accounts;
mod common;
mod device;
mod env;
mod file;
mod identity;
mod process;
mod procfs;
pub mod schemas;
mod storage;

use std::sync::Arc;

use ono_provider_api::ProviderRegistry;

pub use accounts::{Accounts, GroupAccount, NSS_TIMEOUT, NssAccounts, UserAccount};
pub use device::DeviceProvider;
pub use env::{EnvBinding, EnvProvider, EnvSource};
pub use file::FileProvider;
pub use identity::IdentityProvider;
pub use process::{Clock, KernelSignals, ProcessProvider, Signals, SystemClock};
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
    registry.register(Arc::new(ProcessProvider::new()));
    registry.register(Arc::new(FileProvider::new()));
    registry.register(Arc::new(IdentityProvider::new()));
    registry.register(Arc::new(EnvProvider::new(environment)));
    registry.register(Arc::new(StorageProvider::new()));
    registry.register(Arc::new(DeviceProvider::new()));
}
