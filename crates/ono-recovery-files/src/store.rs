//! The private recovery store (spec v0.6 §15.3, §44.3).
//!
//! §15.3: *"Recovery copies MUST live in a protected Ono recovery store with permissions
//! preventing other users from reading sensitive configuration."* §44.3 says the same thing from
//! the privacy side, and §44 lists what a copy may hold: credentials, private keys, shell
//! configuration, application secrets.
//!
//! The modes are applied at creation and never afterwards, which is the rule
//! `ono-temporal-ledger`'s store already follows (v0.5 §30.2): a directory that was briefly
//! world-traversable *was* world-traversable, and a `chmod` after the fact does not take that
//! back. `DirBuilder::mode` and `OpenOptions::mode` put the bits in the syscall that creates the
//! object, so there is no window at all.
//!
//! The store root is a parameter. Nothing here reads a home directory or an environment variable,
//! so a test writes into a scratch directory and the provider that ships reads its root from the
//! configuration §53 owns.

use std::fs::{DirBuilder, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use ono_change_core::RecoveryAssetId;
use ono_change_core::error::{asset_create_failed, asset_not_found, store_unavailable};
use ono_value::ErrorValue;

use crate::PROVIDER_ID;
use crate::manifest::Manifest;

/// The mode §15.3 and §44.3 give every directory in the store: the owner, and nobody else.
pub const DIRECTORY_MODE: u32 = 0o700;

/// The mode §15.3 and §44.3 give every file in the store.
pub const FILE_MODE: u32 = 0o600;

/// The file inside an asset directory that describes what the archive holds.
pub const MANIFEST_NAME: &str = "manifest";

/// The directory inside an asset directory that holds the copied bytes.
pub const OBJECTS_DIRECTORY: &str = "objects";

/// Where this provider's recovery copies live (§15.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecoveryStore {
    root: PathBuf,
}

impl FileRecoveryStore {
    /// Opens — creating if needed — the store rooted at `root`, mode `0700` (§15.3, §44.3).
    ///
    /// # Errors
    ///
    /// `change.store_unavailable` when the directory cannot be created or cannot be made private.
    /// A store that cannot be made private is not a store: §44.3 requires restrictive permissions,
    /// and a provider that carried on would write secrets into a directory other users can read.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ErrorValue> {
        let root = root.into();
        create_private_directory(&root)?;
        Ok(Self { root })
    }

    /// The store's root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether `path` is inside the store, which is how the provider declines to protect itself.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }

    /// The directory an asset's archive lives in, which becomes the asset's reference (§11.1).
    ///
    /// Every later operation reads the asset's own reference rather than deriving this again. An
    /// asset's identity may legitimately be re-derived by the layer above — attributing it to a
    /// plan changes it (§11.1's `source_plan`) — and a store that recomputed its path from the id
    /// would then look in a directory that does not exist.
    #[must_use]
    pub fn asset_directory(&self, asset: &RecoveryAssetId) -> PathBuf {
        self.root.join(asset.as_str())
    }

    /// Whether the store is usable right now: present, a directory, private and writable.
    ///
    /// The answer is a sentence rather than a boolean because §12.2's availability has to say
    /// why: a provider that answers `discover` with nothing is indistinguishable from a provider
    /// that found nothing to protect (§55.6 case 29).
    #[must_use]
    pub fn unusable_reason(&self) -> Option<String> {
        let metadata = match std::fs::metadata(&self.root) {
            Ok(metadata) => metadata,
            Err(error) => {
                return Some(format!(
                    "the recovery store `{}` cannot be read: {error}",
                    self.root.display()
                ));
            }
        };
        if !metadata.is_dir() {
            return Some(format!(
                "the recovery store `{}` is not a directory",
                self.root.display()
            ));
        }
        if metadata.permissions().mode() & 0o7777 != DIRECTORY_MODE {
            return Some(format!(
                "the recovery store `{}` is not private to its owner, and §44.3 requires it to be",
                self.root.display()
            ));
        }
        if !is_writable(&self.root) {
            return Some(format!(
                "the recovery store `{}` is not writable",
                self.root.display()
            ));
        }
        None
    }

    /// Writes `manifest` and `contents` into the asset's own private directory (§15.3).
    ///
    /// `contents` is indexed by the manifest's entry order, and only the entries that carry a blob
    /// name have one.
    ///
    /// # Errors
    ///
    /// `recovery.asset_create_failed` when anything could not be written. Nothing partial is left
    /// behind: §2.3 aborts before mutation when protection fails, and a half-written archive that
    /// looked like an asset would be worse than none.
    pub fn write_archive(
        &self,
        directory: &Path,
        manifest: &Manifest,
        contents: &[(String, Vec<u8>)],
    ) -> Result<(), ErrorValue> {
        let scope = manifest.root().display().to_string();
        self.inside(directory)?;
        // The directory is claimed first, and only what this call created is ever removed: a
        // failure here must not take away a copy that was already in the store (§62.3).
        DirBuilder::new()
            .mode(DIRECTORY_MODE)
            .create(directory)
            .map_err(|error| {
                asset_create_failed(
                    PROVIDER_ID,
                    &scope,
                    &format!(
                        "the recovery store already holds `{}` or could not take it: {error}",
                        directory.display()
                    ),
                )
            })?;
        let result = self.write_archive_inner(directory, manifest, contents);
        if result.is_err() {
            let _ = std::fs::remove_dir_all(directory);
        }
        result.map_err(|error| {
            asset_create_failed(
                PROVIDER_ID,
                &scope,
                &format!("the recovery store could not take the copy: {error}"),
            )
        })
    }

    fn write_archive_inner(
        &self,
        directory: &Path,
        manifest: &Manifest,
        contents: &[(String, Vec<u8>)],
    ) -> Result<(), std::io::Error> {
        let objects = directory.join(OBJECTS_DIRECTORY);
        DirBuilder::new().mode(DIRECTORY_MODE).create(&objects)?;
        for (name, bytes) in contents {
            write_private_file(&objects.join(name), bytes)?;
        }
        write_private_file(&directory.join(MANIFEST_NAME), manifest.encode().as_bytes())
    }

    /// Reads an asset's manifest back.
    ///
    /// # Errors
    ///
    /// `recovery.asset_not_found` when the archive is not in the store, and `change.store_corrupt`
    /// when it is there and unreadable.
    pub fn read_manifest(&self, directory: &Path) -> Result<Manifest, ErrorValue> {
        self.inside(directory)?;
        let path = directory.join(MANIFEST_NAME);
        let text = std::fs::read_to_string(&path)
            .map_err(|_| asset_not_found(&directory.display().to_string()))?;
        Manifest::decode(&text)
    }

    /// The digest of the manifest as it is stored, which is the asset's captured-state fingerprint.
    ///
    /// # Errors
    ///
    /// `recovery.asset_not_found` when the archive is not in the store.
    pub fn manifest_fingerprint(&self, directory: &Path) -> Result<String, ErrorValue> {
        self.inside(directory)?;
        let path = directory.join(MANIFEST_NAME);
        let bytes =
            std::fs::read(&path).map_err(|_| asset_not_found(&directory.display().to_string()))?;
        Ok(crate::manifest::digest_of(&bytes))
    }

    /// Reads one entry's copied bytes.
    ///
    /// # Errors
    ///
    /// `recovery.asset_not_found` when the blob is missing, which is an asset that no longer holds
    /// what its manifest says it holds.
    pub fn read_blob(&self, directory: &Path, blob: &str) -> Result<Vec<u8>, ErrorValue> {
        self.inside(directory)?;
        let path = directory.join(OBJECTS_DIRECTORY).join(blob);
        std::fs::read(&path).map_err(|error| {
            asset_not_found(&format!(
                "the stored copy `{}` could not be read: {error}",
                path.display()
            ))
        })
    }

    /// Removes an asset's archive (§37).
    ///
    /// Removing an archive that is already gone succeeds: §41.1 makes cleanup idempotent, so a
    /// retry after an interrupted removal finishes the job rather than reporting a failure that
    /// has already been resolved.
    ///
    /// # Errors
    ///
    /// `change.store_unavailable` when the directory is there and cannot be removed.
    pub fn remove_archive(&self, directory: &Path) -> Result<(), ErrorValue> {
        self.inside(directory)?;
        if !directory.exists() {
            return Ok(());
        }
        std::fs::remove_dir_all(directory).map_err(|error| {
            store_unavailable(&format!(
                "the recovery copy at `{}` could not be removed: {error}",
                directory.display()
            ))
        })
    }

    /// How much space an asset's archive occupies now, measured rather than estimated (§37.5).
    ///
    /// # Errors
    ///
    /// `recovery.asset_not_found` when the archive is not in the store.
    pub fn occupied_bytes(&self, directory: &Path) -> Result<u64, ErrorValue> {
        self.inside(directory)?;
        if !directory.is_dir() {
            return Err(asset_not_found(&directory.display().to_string()));
        }
        Ok(directory_bytes(directory))
    }

    /// Refuses a path outside the store (§43.2, §48.3).
    ///
    /// An asset arrives from a plan store, and a plan store is a file. A reference pointing
    /// anywhere else would make this provider read and remove paths its own configuration never
    /// named, which is the authority escalation §48.4 forbids a contribution from gaining.
    fn inside(&self, directory: &Path) -> Result<(), ErrorValue> {
        if self.contains(directory) && directory != self.root {
            return Ok(());
        }
        Err(asset_not_found(&format!(
            "`{}` is not a recovery copy inside `{}`",
            directory.display(),
            self.root.display()
        )))
    }
}

/// Whether this process can write into `path`, asked with the effective ids a restore uses.
pub(crate) fn is_writable(path: &Path) -> bool {
    rustix::fs::accessat(
        rustix::fs::CWD,
        path,
        rustix::fs::Access::WRITE_OK,
        rustix::fs::AtFlags::EACCESS,
    )
    .is_ok()
}

/// Creates `directory` with mode `0700` in the syscall that creates it (§15.3, §44.3).
///
/// Ancestors outside the store's own root are created with the platform default, because the
/// directory the store is placed in belongs to whoever configured it and Ono does not own its
/// mode.
pub(crate) fn create_private_directory(directory: &Path) -> Result<(), ErrorValue> {
    if directory.is_dir() {
        return tighten(directory, DIRECTORY_MODE);
    }
    if let Some(parent) = directory.parent()
        && !parent.as_os_str().is_empty()
        && !parent.is_dir()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            store_unavailable(&format!(
                "the recovery store `{}` could not be created: {error}",
                parent.display()
            ))
        })?;
    }
    DirBuilder::new()
        .mode(DIRECTORY_MODE)
        .create(directory)
        .or_else(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(store_unavailable(&format!(
                    "the recovery store `{}` could not be created: {error}",
                    directory.display()
                )))
            }
        })?;
    tighten(directory, DIRECTORY_MODE)
}

/// Creates a file with mode `0600` before a byte is written to it (§15.3, §44.3).
pub(crate) fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(FILE_MODE)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Narrows a directory a previous release or a careless umask left wider (§44.3).
fn tighten(path: &Path, mode: u32) -> Result<(), ErrorValue> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        store_unavailable(&format!(
            "`{}` could not be inspected: {error}",
            path.display()
        ))
    })?;
    if metadata.permissions().mode() & 0o7777 == mode {
        return Ok(());
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|error| {
        store_unavailable(&format!(
            "`{}` could not be made user-private: {error}",
            path.display()
        ))
    })
}

/// The bytes every file beneath `directory` occupies.
fn directory_bytes(directory: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(metadata) if metadata.is_dir() => directory_bytes(&entry.path()),
            Ok(metadata) => metadata.len(),
            Err(_) => 0,
        })
        .sum()
}
