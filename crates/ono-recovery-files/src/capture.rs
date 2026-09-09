//! Walking what is about to change, and refusing what §15.2 excludes (spec v0.6 §15.1, §15.2).
//!
//! The walk is `lstat`-only. A symlink is recorded as a symlink and never opened, which is §15.1's
//! *"symlinks as symlinks"* and §43.5's TOCTOU rule in the same decision: a provider that followed
//! a link would archive whatever the link pointed at when it looked, and a link swapped a moment
//! later would leave an asset holding the wrong object entirely.
//!
//! The walk also stops at a mount boundary. A directory whose device number differs from the
//! root's is a separate filesystem, and copying across it would put objects into an archive whose
//! scope claims one persistence domain — the same truth Appendix G.2 asks providers to be tested
//! against, in the form a file copy meets it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use ono_change_core::error::asset_create_failed;
use ono_change_core::{RecoveryExclusion, error::target_unresolved};
use ono_value::ErrorValue;

use crate::PROVIDER_ID;
use crate::identity::{FilesystemFacts, ObjectIdentity, ObjectKind, filesystem_of, lstat};
use crate::limits::{FileProtectionLimits, limit_exceeded};
use crate::manifest::{ArchiveEntry, Manifest, digest_of};

/// Whether a walk is measuring a candidate or copying the bytes (§5.5).
///
/// Discovery is read-only and only needs the sizes; creation needs the content. The same walk
/// answers both so that a candidate's cost and the archive that follows it cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// Measure the tree without reading file contents (§5.5's read-only discovery).
    Measure,
    /// Read the content, so the copy can be written (§4.5's PREPARE).
    Capture,
}

/// What one walk found (§15.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    manifest: Manifest,
    contents: Vec<(String, Vec<u8>)>,
    exclusions: Vec<RecoveryExclusion>,
    filesystem: FilesystemFacts,
    owners_beyond_process: bool,
}

impl Capture {
    /// The manifest describing everything the walk found.
    #[must_use]
    pub const fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The copied bytes, keyed by blob name. Empty for a [`ScanMode::Measure`] walk.
    #[must_use]
    pub fn contents(&self) -> &[(String, Vec<u8>)] {
        &self.contents
    }

    /// What the archive will not hold, stated on the candidate and on the asset (§11.1).
    #[must_use]
    pub fn exclusions(&self) -> &[RecoveryExclusion] {
        &self.exclusions
    }

    /// The filesystem the tree lives on (Appendix B).
    #[must_use]
    pub const fn filesystem(&self) -> &FilesystemFacts {
        &self.filesystem
    }

    /// Whether any object is owned by somebody this process cannot restore ownership for (§43.4).
    #[must_use]
    pub const fn owners_beyond_process(&self) -> bool {
        self.owners_beyond_process
    }

    /// The total size of the content.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.manifest.total_bytes()
    }

    /// How many objects the walk found, which is Appendix A.4's scope width.
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.manifest.entries().len()
    }
}

/// Walks `root`, refusing everything §15.2 excludes and everything outside `limits`.
///
/// # Errors
///
/// - `change.target_unresolved` when the path cannot be stated.
/// - `recovery.asset_create_failed` when the tree holds a socket, a FIFO or a device node, when it
///   crosses a mount boundary, or when it is larger than `limits` allows. Every one of those names
///   what it refused and archives nothing (§15.2, §62.1).
pub fn scan(
    root: &Path,
    limits: &FileProtectionLimits,
    mode: ScanMode,
    created_at_nanos: i128,
) -> Result<Capture, ErrorValue> {
    let scope = root.display().to_string();
    let parent = root.parent().unwrap_or(Path::new("/"));
    let root_parent = ObjectIdentity::read(parent)?;
    // `statfs` follows the path it is given, so a symlink is asked about through the directory
    // that holds it: §15.1 protects a symlink as a symlink, and a dangling one is still a link.
    let filesystem = if ObjectKind::of(&lstat(root)?) == ObjectKind::Symlink {
        filesystem_of(parent)?
    } else {
        filesystem_of(root)?
    };
    let mut manifest = Manifest::new(root.to_path_buf(), root_parent, created_at_nanos);
    let mut contents = Vec::new();
    let mut exclusions = Vec::new();
    let mut owners_beyond_process = false;
    let effective_uid = rustix::process::geteuid().as_raw();
    let mut total_bytes = 0_u64;
    let mut count = 0_usize;
    let mut queue = VecDeque::from([PathBuf::new()]);

    while let Some(relative) = queue.pop_front() {
        let path = if relative.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(&relative)
        };
        let metadata = lstat(&path)?;
        let kind = ObjectKind::of(&metadata);
        if let Some(reason) = kind.exclusion() {
            return Err(excluded_object(&scope, &path, kind, reason));
        }
        if metadata_device(&metadata) != filesystem.device() {
            return Err(crossed_mount(&scope, &path));
        }
        count += 1;
        let size = if kind == ObjectKind::Directory {
            0
        } else {
            metadata.len()
        };
        total_bytes = total_bytes.saturating_add(size);
        if let Err(kind) = limits.admits(count, total_bytes, size) {
            let measured = match kind {
                crate::limits::LimitKind::ObjectCount => count as u64,
                crate::limits::LimitKind::ObjectBytes => size,
                crate::limits::LimitKind::TotalBytes => total_bytes,
            };
            return Err(limit_exceeded(
                kind,
                &scope,
                &path.display().to_string(),
                limits.bound(kind),
                measured,
            ));
        }
        if metadata_uid(&metadata) != effective_uid {
            owners_beyond_process = true;
        }
        if kind == ObjectKind::RegularFile && metadata_nlink(&metadata) > 1 {
            exclusions.push(RecoveryExclusion::new(
                path.display().to_string(),
                "Appendix C.7: the file has more than one name, and restoring it writes one file \
                 rather than the hard-link relationship it took part in",
            ));
        }
        let entry = read_entry(
            &path,
            &relative,
            &metadata,
            kind,
            count,
            mode,
            &mut contents,
        )?;
        manifest = manifest.with_entry(entry);
        if kind == ObjectKind::Directory {
            for child in children_of(&path)? {
                queue.push_back(relative.join(child));
            }
        }
    }

    Ok(Capture {
        manifest,
        contents,
        exclusions,
        filesystem,
        owners_beyond_process,
    })
}

/// Reads everything about one object the manifest records.
fn read_entry(
    path: &Path,
    relative: &Path,
    metadata: &std::fs::Metadata,
    kind: ObjectKind,
    ordinal: usize,
    mode: ScanMode,
    contents: &mut Vec<(String, Vec<u8>)>,
) -> Result<ArchiveEntry, ErrorValue> {
    let entry = ArchiveEntry::new(
        relative.to_path_buf(),
        kind,
        metadata_mode(metadata),
        metadata_uid(metadata),
        metadata_gid(metadata),
        ObjectIdentity::of(path, metadata),
    )
    .with_xattrs(crate::xattr::read_all(path)?);
    match kind {
        ObjectKind::Directory => Ok(entry),
        ObjectKind::Symlink => {
            let target = std::fs::read_link(path).map_err(|error| {
                target_unresolved(
                    &path.display().to_string(),
                    &format!("v0.6 §15.1: the symlink is protected as a symlink, and its target could not be read: {error}"),
                )
            })?;
            let bytes = {
                use std::os::unix::ffi::OsStrExt as _;
                digest_of(target.as_os_str().as_bytes())
            };
            Ok(entry
                .with_content(metadata.len(), bytes, None)
                .with_link_target(target))
        }
        _ => {
            let blob = format!("{ordinal:08}");
            let bytes = match mode {
                ScanMode::Measure => Vec::new(),
                ScanMode::Capture => std::fs::read(path).map_err(|error| {
                    asset_create_failed(
                        PROVIDER_ID,
                        &path.display().to_string(),
                        &format!("the file could not be read into the recovery store: {error}"),
                    )
                })?,
            };
            let digest = match mode {
                ScanMode::Measure => String::new(),
                ScanMode::Capture => digest_of(&bytes),
            };
            let size = match mode {
                ScanMode::Measure => metadata.len(),
                ScanMode::Capture => bytes.len() as u64,
            };
            if mode == ScanMode::Capture {
                contents.push((blob.clone(), bytes));
            }
            Ok(entry.with_content(size, digest, Some(blob)))
        }
    }
}

/// The names inside `directory`, sorted, so two walks of one tree produce one manifest.
fn children_of(directory: &Path) -> Result<Vec<PathBuf>, ErrorValue> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        target_unresolved(
            &directory.display().to_string(),
            &format!("the directory could not be listed: {error}"),
        )
    })?;
    let mut names: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| PathBuf::from(entry.file_name()))
        .collect();
    names.sort();
    Ok(names)
}

/// The refusal §15.2 gives a socket, a FIFO or a device node.
fn excluded_object(scope: &str, path: &Path, kind: ObjectKind, reason: &str) -> ErrorValue {
    asset_create_failed(
        PROVIDER_ID,
        scope,
        &format!(
            "v0.6 §15.2: `{}` is a {} and this provider does not archive one. {reason}. Nothing \
             was archived",
            path.display(),
            kind.as_str()
        ),
    )
    .with_metadata(
        "object",
        ono_value::Value::string(&path.display().to_string()),
    )
    .with_metadata("object_kind", ono_value::Value::string(kind.as_str()))
}

/// The refusal a tree spanning two filesystems gets (§11.2, Appendix G.2).
fn crossed_mount(scope: &str, path: &Path) -> ErrorValue {
    asset_create_failed(
        PROVIDER_ID,
        scope,
        &format!(
            "v0.6 §11.2: `{}` is on a different filesystem from the root of this scope, so one \
             archive cannot claim to hold both. Nothing was archived",
            path.display()
        ),
    )
    .with_metadata(
        "object",
        ono_value::Value::string(&path.display().to_string()),
    )
}

fn metadata_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode() & 0o7777
}

fn metadata_uid(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.uid()
}

fn metadata_gid(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.gid()
}

fn metadata_nlink(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.nlink()
}

fn metadata_device(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.dev()
}
