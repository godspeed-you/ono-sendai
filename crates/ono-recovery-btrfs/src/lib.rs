//! The first-party Btrfs recovery provider of v0.6 (spec §14, Appendix D.6–D.9).
//!
//! §14 opens with the sentence the whole crate is built around: *"Btrfs is a first-party v0.6
//! reference provider, but its semantics MUST NOT be modeled as if it were ZFS."* The difference
//! that matters most is §14.3.
//!
//! # Snapshots are not recursive
//!
//! A Btrfs snapshot of a parent subvolume does **not** contain the live contents of a subvolume
//! nested inside it. Where the nested subvolume is mounted, the snapshot holds an empty directory.
//! The recorded fixtures in `tests/fixtures/` show it rather than assert it: `nested-live.txt`
//! lists a `state` directory inside `/mnt/root/var/lib-app`, and `nested-in-snapshot.txt` lists
//! the same path inside a read-only snapshot of its parent `@var` — empty.
//!
//! Everything else follows. [`SubvolumeLayout::required_for`] computes the set of subvolumes a
//! plan must snapshot *separately*, so §14.3's own worked example — change `/etc/nginx/nginx.conf`
//! and `/var/lib/app/state.db`, snapshot `@root` and `@var`, leave `@home` alone — is arithmetic
//! rather than advice. Every candidate a snapshot produces carries a
//! [`RecoveryExclusion`](ono_change_core::RecoveryExclusion) naming each nested subvolume it does
//! not hold, and [`SafetyFact::NoRecursiveCoverageAssumption`] blocks a recovery that would have
//! relied on the opposite.
//!
//! # What it will not claim
//!
//! - **No in-place rollback.** §14.4 forbids presenting Btrfs as having a generic
//!   `rollback snapshot` primitive. Every recovery this crate plans declares which of §14.4's four
//!   workflows it is — selective file restore, clone and copy, subvolume replacement, offline root
//!   recovery — and the root case distinguishes §14.6's three explicitly, including the one that
//!   only takes effect after a reboot.
//! - **No invented atomicity.** Appendix D.7 requires a multi-subvolume set to record that its
//!   members were created sequentially, and [`RecoveryAssetSet::composed_consistency`] computes a
//!   set of two subvolumes taken one after another as crash-consistent. There is no setter for the
//!   stronger word.
//! - **No snapshot called a backup.** §14.7 requires the provider to say that snapshots share the
//!   filesystem and storage failure domain of what they protect, and every candidate says it.
//! - **No guessed recovery.** §56.2 lists ten facts a destructive Btrfs recovery must prove, and
//!   [`SafetyChecklist`] starts with all ten outstanding. §56.3 turns any one of them still
//!   outstanding into `recovery.plan_incomplete`, so forgetting to check and checking and failing
//!   are the same outcome.
//! - **No identity from a name.** Appendix B.9: a subdirectory named like a subvolume is not
//!   evidence of one. Every subvolume id here comes from `btrfs subvolume show`, `btrfs subvolume
//!   list` or a mount's `subvolid=` option.
//!
//! # Using it
//!
//! ```no_run
//! use std::sync::Arc;
//! use ono_change_core::{ProtectionMode, RecoveryObjective, RecoveryProvider};
//! use ono_recovery_btrfs::{BtrfsProvider, ProcessRunner};
//!
//! let provider = BtrfsProvider::new(Arc::new(ProcessRunner::new()));
//! if let Some(domain) = provider.resolve_domain("/etc/nginx/nginx.conf")? {
//!     let candidates = provider.discover(&domain, RecoveryObjective::PreserveExact)?;
//!     let actions = provider.plan_protection(&candidates, ProtectionMode::Prefer)?;
//!     // §2.1: nothing exists yet. `create` is what makes a snapshot.
//!     assert!(actions.iter().all(|action| action.is_required()));
//! }
//! # Ok::<(), ono_value::ErrorValue>(())
//! ```

#![forbid(unsafe_code)]

pub mod assets;
pub mod boundary;
pub mod config;
pub mod error;
pub mod files;
pub mod mount;
pub mod newer;
pub mod parse;
pub mod provider;
pub mod runner;
pub mod safety;
pub mod subvolume;

pub use assets::{ProtectionShortfall, RecoveryAssetSet, SetAtomicity};
pub use boundary::{RequiredProtection, RequiredSubvolume, SubvolumeBoundary, SubvolumeLayout};
pub use config::{
    BtrfsConfig, DEFAULT_SNAPSHOT_LOCATION, RootRecovery, sanitised_name, snapshot_name,
};
pub use error::PROVIDER_ID;
pub use files::{FileStore, METADATA_COVERAGE, RecordedFiles, SystemFiles};
pub use mount::{BtrfsMount, BtrfsMounts, MOUNTINFO};
pub use newer::{classify_object, classify_subvolume};
pub use parse::{
    BtrfsVersion, DefaultSubvolume, FS_TREE_ID, FilesystemInfo, FilesystemUsage, ShowOutcome,
    SubvolumeEntry, SubvolumeShow,
};
pub use provider::{BTRFS, BtrfsProvider, SCOPE_KIND, VALIDATED_VERSIONS};
pub use runner::{ProcessRunner, TOOL_PATH};
pub use safety::{SafetyChecklist, SafetyFact};
pub use subvolume::SubvolumeRef;
