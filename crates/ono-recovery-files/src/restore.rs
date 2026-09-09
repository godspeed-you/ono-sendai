//! Putting a file back without ever truncating the live one (spec v0.6 §15.4, Appendix C.6).
//!
//! §15.4: *"Where possible, restoration SHOULD use temp-file + fsync + atomic rename semantics
//! rather than truncating the live file in place."* Every write here follows that shape, and the
//! order matters as much as the steps: the copy is written to a new file beside the destination,
//! given its permissions, its owner and its extended attributes, flushed to the disk, and only
//! then renamed over the live path. Until the rename the original is untouched and complete, and
//! after the rename the replacement is complete. There is no instant at which the destination
//! holds half a file.
//!
//! Appendix C.6 decides the other half: *"Default selective directory restore MUST NOT delete
//! newer extra files unless the recovery objective explicitly requires exact-tree equivalence."*
//! [`DirectoryRestorePolicy::KeepExtraFiles`] is therefore the default the provider carries, and
//! the exact-tree policy is a thing an operator chooses rather than a thing a restore does.
//!
//! Before any of that, §43.5. Every path is re-stated with `lstat` and compared with the identity
//! that was recorded when the object was protected. A destination that is now a symlink where a
//! regular file was protected is refused: writing through it would put the recovered bytes
//! wherever the link points, which is precisely the substitution §43.5 names unacceptable.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use ono_change_core::error::recovery_apply_failed;
use ono_change_core::{DirectoryRestorePolicy, RecoveryAssetId};
use ono_value::ErrorValue;

use crate::identity::{ObjectIdentity, ObjectKind, identity_changed, lstat};
use crate::manifest::{ArchiveEntry, Manifest, digest_of};
use crate::store::FileRecoveryStore;

/// What one restore put back, and what it could not (Appendix C.6, C.7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreReport {
    restored: Vec<PathBuf>,
    removed: Vec<PathBuf>,
    kept: Vec<PathBuf>,
    metadata_gaps: Vec<String>,
}

impl RestoreReport {
    /// The objects that were written back.
    #[must_use]
    pub fn restored(&self) -> &[PathBuf] {
        &self.restored
    }

    /// The objects an exact-tree restore removed (Appendix C.6).
    #[must_use]
    pub fn removed(&self) -> &[PathBuf] {
        &self.removed
    }

    /// The newer objects the default policy left alone (Appendix C.6).
    #[must_use]
    pub fn kept(&self) -> &[PathBuf] {
        &self.kept
    }

    /// The metadata the restore could not put back, named so §C.7 makes it visible.
    #[must_use]
    pub fn metadata_gaps(&self) -> &[String] {
        &self.metadata_gaps
    }
}

/// Restores `selection` — one object, or the whole archive — from `manifest`.
///
/// # Errors
///
/// - `change.target_changed` when a path is no longer the object that was protected (§43.5).
/// - `recovery.scope_mismatch` when the selected object is not in the archive.
/// - `recovery.apply_failed` when a copy could not be written. The original is untouched: nothing
///   is renamed until the replacement is complete on disk (§15.4).
pub fn restore_objects(
    store: &FileRecoveryStore,
    asset: &RecoveryAssetId,
    manifest: &Manifest,
    selection: Option<&Path>,
    policy: DirectoryRestorePolicy,
) -> Result<RestoreReport, ErrorValue> {
    let root = manifest.root();
    let parent = root.parent().unwrap_or(Path::new("/"));
    let now = ObjectIdentity::read(parent)?;
    if !manifest.root_parent().is_still(&now) {
        return Err(identity_changed(
            parent,
            "the directory that held the protected object is not the one that was protected",
        ));
    }

    let entries = select(manifest, asset, selection)?;
    let mut report = RestoreReport::default();
    for entry in &entries {
        let destination = entry.destination(root);
        guard_path(root, &destination)?;
        guard_destination(&destination, entry)?;
        match entry.kind() {
            ObjectKind::Directory => restore_directory(&destination, entry, &mut report)?,
            ObjectKind::Symlink => restore_symlink(&destination, entry, &mut report)?,
            _ => restore_file(store, asset, &destination, entry, &mut report)?,
        }
    }
    reconcile_extras(manifest, &entries, policy, &mut report)?;
    Ok(report)
}

/// The entries a selection names: one object, and everything beneath it when it is a directory.
fn select<'a>(
    manifest: &'a Manifest,
    asset: &RecoveryAssetId,
    selection: Option<&Path>,
) -> Result<Vec<&'a ArchiveEntry>, ErrorValue> {
    let root = manifest.root();
    let Some(object) = selection else {
        return Ok(manifest.entries().iter().collect());
    };
    if manifest.entry_for(object).is_none() {
        return Err(ono_change_core::error::scope_mismatch(
            asset,
            &object.display().to_string(),
            &manifest.covered_objects().join(", "),
        ));
    }
    Ok(manifest
        .entries()
        .iter()
        .filter(|entry| entry.destination(root).starts_with(object))
        .collect())
}

/// Refuses a destination reached through a symlink (§43.5).
///
/// Every component between the archive root and the object is stated on its own. A directory that
/// has become a symlink since the object was protected would redirect the write out of the tree,
/// and no amount of care about the final component would catch it.
fn guard_path(root: &Path, destination: &Path) -> Result<(), ErrorValue> {
    let Ok(relative) = destination.strip_prefix(root) else {
        return Ok(());
    };
    let mut cursor = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    let interior = components.len().saturating_sub(1);
    for component in components.into_iter().take(interior) {
        cursor.push(component);
        let metadata = lstat(&cursor)?;
        if ObjectKind::of(&metadata) != ObjectKind::Directory {
            return Err(identity_changed(
                &cursor,
                "a directory inside the protected tree has been replaced by something else",
            ));
        }
    }
    Ok(())
}

/// Refuses to write over an object that is no longer the kind that was protected (§43.5).
fn guard_destination(destination: &Path, entry: &ArchiveEntry) -> Result<(), ErrorValue> {
    let Ok(metadata) = std::fs::symlink_metadata(destination) else {
        return Ok(());
    };
    let kind = ObjectKind::of(&metadata);
    if kind == entry.kind() {
        return Ok(());
    }
    Err(identity_changed(
        destination,
        &format!(
            "a {} was protected here and the path is a {} now",
            entry.kind().as_str(),
            kind.as_str()
        ),
    ))
}

/// Recreates a directory that is missing and puts its metadata back. It is never removed.
fn restore_directory(
    destination: &Path,
    entry: &ArchiveEntry,
    report: &mut RestoreReport,
) -> Result<(), ErrorValue> {
    if !destination.is_dir() {
        std::fs::create_dir(destination).map_err(|error| {
            recovery_apply_failed(
                &destination.display().to_string(),
                &format!("the directory could not be recreated: {error}"),
            )
        })?;
    }
    apply_metadata(destination, entry, report);
    report.restored.push(destination.to_path_buf());
    Ok(())
}

/// Replaces a symlink by creating the new one beside it and renaming it over (§15.4).
fn restore_symlink(
    destination: &Path,
    entry: &ArchiveEntry,
    report: &mut RestoreReport,
) -> Result<(), ErrorValue> {
    let target = entry.link_target().unwrap_or(Path::new(""));
    let staging = staging_path(destination);
    let _ = std::fs::remove_file(&staging);
    std::os::unix::fs::symlink(target, &staging).map_err(|error| {
        recovery_apply_failed(
            &destination.display().to_string(),
            &format!("the symlink could not be recreated: {error}"),
        )
    })?;
    apply_metadata(&staging, entry, report);
    commit(&staging, destination)?;
    report.restored.push(destination.to_path_buf());
    Ok(())
}

/// Writes the copy beside the live file, flushes it, and renames it over (§15.4).
fn restore_file(
    store: &FileRecoveryStore,
    asset: &RecoveryAssetId,
    destination: &Path,
    entry: &ArchiveEntry,
    report: &mut RestoreReport,
) -> Result<(), ErrorValue> {
    let blob = entry.blob().unwrap_or_default();
    let bytes = store.read_blob(asset, blob)?;
    if digest_of(&bytes) != entry.digest() {
        return Err(recovery_apply_failed(
            &destination.display().to_string(),
            "the stored copy no longer matches the digest recorded when it was created, so it is \
             not the state that was protected",
        ));
    }
    let staging = staging_path(destination);
    let _ = std::fs::remove_file(&staging);
    let outcome = write_staged(&staging, &bytes, entry, report);
    if let Err(error) = outcome {
        let _ = std::fs::remove_file(&staging);
        return Err(recovery_apply_failed(
            &destination.display().to_string(),
            &format!(
                "the replacement could not be written, and the original is untouched: {error}"
            ),
        ));
    }
    commit(&staging, destination)?;
    report.restored.push(destination.to_path_buf());
    Ok(())
}

/// Writes the replacement and flushes it before anything is renamed (§15.4).
fn write_staged(
    staging: &Path,
    bytes: &[u8],
    entry: &ArchiveEntry,
    report: &mut RestoreReport,
) -> Result<(), std::io::Error> {
    use std::io::Write as _;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(crate::store::FILE_MODE)
        .open(staging)?;
    file.write_all(bytes)?;
    apply_metadata(staging, entry, report);
    file.sync_all()?;
    Ok(())
}

/// Renames the replacement over the live path, then flushes the directory (§15.4).
fn commit(staging: &Path, destination: &Path) -> Result<(), ErrorValue> {
    std::fs::rename(staging, destination).map_err(|error| {
        let _ = std::fs::remove_file(staging);
        recovery_apply_failed(
            &destination.display().to_string(),
            &format!("the replacement could not be moved into place, and the original is untouched: {error}"),
        )
    })?;
    if let Some(parent) = destination.parent()
        && let Ok(directory) = File::open(parent)
    {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// Puts back mode, owner and extended attributes, naming whatever it could not (Appendix C.7).
fn apply_metadata(path: &Path, entry: &ArchiveEntry, report: &mut RestoreReport) {
    use std::os::unix::fs::PermissionsExt as _;
    if entry.kind() != ObjectKind::Symlink
        && std::fs::set_permissions(path, std::fs::Permissions::from_mode(entry.mode())).is_err()
    {
        report.metadata_gaps.push(format!(
            "the mode of `{}` could not be restored",
            path.display()
        ));
    }
    if rustix::fs::chownat(
        rustix::fs::CWD,
        path,
        Some(rustix::fs::Uid::from_raw(entry.uid())),
        Some(rustix::fs::Gid::from_raw(entry.gid())),
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
    )
    .is_err()
    {
        report.metadata_gaps.push(format!(
            "the owner of `{}` could not be restored",
            path.display()
        ));
    }
    for name in crate::xattr::write_all(path, entry.xattrs()) {
        report.metadata_gaps.push(format!(
            "the extended attribute `{name}` of `{}` could not be restored",
            path.display()
        ));
    }
}

/// Applies Appendix C.6's deletion semantics to files the archive never held.
fn reconcile_extras(
    manifest: &Manifest,
    entries: &[&ArchiveEntry],
    policy: DirectoryRestorePolicy,
    report: &mut RestoreReport,
) -> Result<(), ErrorValue> {
    let root = manifest.root();
    let known: Vec<PathBuf> = manifest
        .entries()
        .iter()
        .map(|entry| entry.destination(root))
        .collect();
    let mut extras = Vec::new();
    for entry in entries {
        if entry.kind() != ObjectKind::Directory {
            continue;
        }
        let directory = entry.destination(root);
        let Ok(children) = std::fs::read_dir(&directory) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            if !known.contains(&path) && !is_staging(&path) {
                extras.push(path);
            }
        }
    }
    extras.sort();
    match policy {
        DirectoryRestorePolicy::KeepExtraFiles => report.kept.append(&mut extras),
        DirectoryRestorePolicy::ExactTree => {
            for path in extras {
                let removed = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                removed.map_err(|error| {
                    recovery_apply_failed(
                        &path.display().to_string(),
                        &format!(
                            "an exact-tree restore could not remove a file the archive never \
                             held: {error}"
                        ),
                    )
                })?;
                report.removed.push(path);
            }
        }
    }
    Ok(())
}

/// The name the replacement is written under before it is renamed into place (§15.4).
fn staging_path(destination: &Path) -> PathBuf {
    let name = destination.file_name().map_or_else(
        || "object".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    destination.with_file_name(format!(".ono-recovery.{name}.staging"))
}

/// Whether a path is one of this provider's own staging files.
fn is_staging(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(".ono-recovery."))
}
