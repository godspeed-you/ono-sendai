//! The first-party file and configuration recovery provider of v0.6 (spec §15).
//!
//! §15 exists because *"Not every system uses snapshot-capable storage"*, and v0.6 *"MUST provide
//! a first-party narrow file/config recovery provider"*. This crate is that provider: it copies
//! the objects a change plan will touch into a private store, validates what it copied, plans the
//! way back, and writes it back one object at a time without ever truncating a live file.
//!
//! # What the spec fixes, and where it lives here
//!
//! | Rule | Where |
//! |---|---|
//! | §15.1 scope: files, symlinks *as symlinks*, small trees, metadata | [`capture`], [`identity`] |
//! | §15.2 exclusions: sockets, devices, pseudo-filesystems, large trees | [`limits`], [`capture`] |
//! | §15.3 storage: a private store other users cannot read | [`store`] |
//! | §15.4 replacement: temp file, fsync, atomic rename | [`restore`] |
//! | §15.5 and §44.2 secrets: contents are never rendered | the absence of an accessor |
//! | §11.4 validation: existence, identity, scope, restore path, permissions | [`RecoveryProvider::validate`][ono_change_core::RecoveryProvider::validate] |
//! | §43.5 TOCTOU: identity recorded, identity re-checked | [`identity::ObjectIdentity`] |
//! | Appendix C.6 directory restore: newer files are kept by default | [`restore`] |
//! | Appendix C.7 metadata coverage: measured, and its gaps made visible | [`coverage`] |
//!
//! # The one thing this crate deliberately cannot do
//!
//! §15.5: *"Recovery copies may contain secrets. They MUST inherit the strictest secret/history
//! policy and MUST NOT be rendered by default."* §44.2 repeats it for rendering. There is
//! therefore no method anywhere in this crate that returns the bytes of a protected file to a
//! caller. A [`ono_change_core::RecoveryAsset`] carries paths, sizes, digests, modes and owners; a
//! [`crate::manifest::Manifest`] carries the same; the bytes exist only inside the store, behind
//! mode `0600`, and travel from there straight into the destination file. A renderer cannot print
//! what it cannot obtain.
//!
//! # Example
//!
//! ```
//! use jiff::Timestamp;
//! use ono_change_core::{RecoveryObjective, RecoveryProvider};
//! use ono_recovery_files::{FileRecoveryProvider, FileRecoveryStore};
//!
//! # fn main() -> Result<(), ono_value::ErrorValue> {
//! # // `target/` rather than `/tmp`: the store must be on a persistent filesystem, and a tmpfs
//! # // is exactly what §15's provider refuses (Appendix B.7).
//! # let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
//! #     .join("../../target/ono-doc")
//! #     .join(std::process::id().to_string());
//! # std::fs::create_dir_all(scratch.join("etc")).ok();
//! # std::fs::write(scratch.join("etc/nginx.conf"), b"worker_processes 1;\n").ok();
//! let store = FileRecoveryStore::open(scratch.join("store"))?;
//! let provider = FileRecoveryProvider::new(store, Timestamp::UNIX_EPOCH);
//!
//! let configuration = scratch.join("etc/nginx.conf");
//! let domain = provider
//!     .resolve_domain(&configuration.display().to_string())?
//!     .expect("a file on a persistent filesystem has a domain");
//! let candidates = provider.discover(&domain, RecoveryObjective::PreserveExact)?;
//! assert_eq!(candidates.len(), 1);
//! # std::fs::remove_dir_all(&scratch).ok();
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod capture;
pub mod coverage;
pub mod identity;
pub mod limits;
pub mod manifest;
pub mod restore;
pub mod store;
pub mod xattr;

mod provider;

pub use capture::{Capture, ScanMode, scan};
pub use identity::{FilesystemFacts, ObjectIdentity, ObjectKind, filesystem_of};
pub use limits::{
    DEFAULT_MAX_OBJECT_BYTES, DEFAULT_MAX_OBJECT_COUNT, DEFAULT_MAX_TOTAL_BYTES,
    FileProtectionLimits, LimitKind,
};
pub use manifest::{ArchiveEntry, MANIFEST_VERSION, Manifest};
pub use provider::{FileRecoveryProvider, PROVIDER_ID};
pub use restore::RestoreReport;
pub use store::{DIRECTORY_MODE, FILE_MODE, FileRecoveryStore};
