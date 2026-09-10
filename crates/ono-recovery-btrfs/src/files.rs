//! The seam a selective restore reads and writes bytes through (§13.5, Appendix C.1).
//!
//! A Btrfs selective restore is not a `btrfs` subcommand. Once the read-only snapshot exists,
//! putting one file back is reading it out of the snapshot path and writing it to the live path —
//! ordinary filesystem work, and the reason [`ToolRunner`](ono_change_core::ToolRunner) is not the
//! right seam for it.
//!
//! It is a seam at all for the same reason `ToolRunner` is: §54.4 asks for real filesystems where
//! the environment permits and deterministic tests everywhere else. [`SystemFiles`] is the real
//! implementation; [`RecordedFiles`] replays recorded content, so the newer-state classification
//! of Appendix C.3 can be exercised against the bytes a real `cat` printed on both sides of the
//! change without a Btrfs filesystem being present.
//!
//! §15.4 and §43.5 decide how [`SystemFiles::copy`] writes: to a sibling file that is flushed and
//! renamed over the live path, and never through a symlink.
//!
//! Appendix C.7 is why [`FileStore::copy`] exists rather than a write of bytes: a restore is
//! judged on what it puts back, and a copy that carries the permission bits can say it restores
//! mode as well as content. What no implementation here claims is owner, ACLs, xattrs,
//! capabilities, SELinux labels or hard-link relationships — [`METADATA_COVERAGE`] states that,
//! and Appendix C.7 requires the gaps to be visible rather than discovered.

use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ono_change_core::MetadataCoverage;
use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

/// What a Btrfs selective restore puts back (Appendix C.7).
///
/// Content and mode travel with a copy inside one filesystem. Everything else is a gap, and
/// Appendix C.7 makes an invisible gap the failure: a configuration file returned without its
/// SELinux label has not been returned.
pub const METADATA_COVERAGE: MetadataCoverage = MetadataCoverage {
    content: true,
    mode: true,
    owner: false,
    acl: false,
    xattrs: false,
    capabilities: false,
    selinux: false,
    hardlinks: false,
};

/// Reads and copies the bytes a selective restore moves (§13.5).
pub trait FileStore: Send + Sync + std::fmt::Debug {
    /// The contents of `path`, or `None` when nothing is there.
    ///
    /// The distinction matters to Appendix C.3: a file that is absent from the live tree and
    /// present in the snapshot is newer state of a different kind from one that was edited.
    ///
    /// # Errors
    ///
    /// A structured error when the path exists and could not be read, which §56.3 turns into a
    /// block rather than into "unchanged".
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, ErrorValue>;

    /// Puts `from` in place of `to`, creating the parent directory where it is missing.
    ///
    /// §15.4: the content is written to a sibling file, flushed, and renamed over `to`, so a
    /// reader of the live path sees the old version or the new one and never a truncated mix.
    /// §43.5: nothing is written through a symlink — not one at `to`, and not one standing in
    /// for a directory on the way to it.
    ///
    /// # Errors
    ///
    /// A structured error when the copy failed or was refused. Appendix F then preserves the
    /// partial state and the evidence rather than retrying blind.
    fn copy(&self, from: &Path, to: &Path) -> Result<(), ErrorValue>;

    /// Moves `from` to `to`, which is how a subvolume is put in another's place (§14.4).
    ///
    /// Replacement is a rename rather than a copy, and the subvolume being displaced is renamed
    /// aside rather than removed: §2.15 and Appendix F both prefer keeping the state a step
    /// displaced over a tidy tree.
    ///
    /// # Errors
    ///
    /// A structured error when the move failed.
    fn rename(&self, from: &Path, to: &Path) -> Result<(), ErrorValue>;

    /// Whether anything at all is at `path`, a dangling symlink included.
    ///
    /// A rename replaces what is at its destination, and a directory rename replaces an empty
    /// directory without complaint, so a step that must not overwrite asks this first.
    ///
    /// # Errors
    ///
    /// A structured error when the answer could not be read.
    fn exists(&self, path: &Path) -> Result<bool, ErrorValue>;
}

/// The real filesystem (§54.4).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemFiles;

impl FileStore for SystemFiles {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, ErrorValue> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_error(path, &error, "could not be read")),
        }
    }

    fn copy(&self, from: &Path, to: &Path) -> Result<(), ErrorValue> {
        let source = std::fs::symlink_metadata(from)
            .map_err(|error| io_error(from, &error, "could not be read"))?;
        refuse_symlinked_path(to)?;
        let (Some(parent), Some(name)) = (to.parent(), to.file_name()) else {
            return Err(refusal(
                to,
                "names no file inside a directory, so there is nothing to replace",
            ));
        };
        if !parent.as_os_str().is_empty() && std::fs::symlink_metadata(parent).is_err() {
            // Every existing component was checked above, so what is created here is the
            // provider's own and cannot lead anywhere else.
            std::fs::create_dir_all(parent)
                .map_err(|error| io_error(parent, &error, "could not be created"))?;
        }
        let owner = std::fs::symlink_metadata(to)
            .ok()
            .filter(|live| live.is_file());
        let temporary = temporary_beside(parent, name);
        let written = if source.file_type().is_symlink() {
            std::fs::read_link(from)
                .and_then(|target| std::os::unix::fs::symlink(target, &temporary))
                .map_err(|error| io_error(&temporary, &error, "could not be created"))
        } else if source.is_file() {
            write_regular(from, &temporary, &source, owner.as_ref())
        } else {
            Err(refusal(
                from,
                "is neither a regular file nor a symlink, and a selective restore puts back only \
                 those two",
            ))
        };
        if let Err(error) = written {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = std::fs::rename(&temporary, to) {
            let _ = std::fs::remove_file(&temporary);
            return Err(io_error(to, &error, "could not be replaced"));
        }
        // The rename is durable only once the directory holding it is.
        std::fs::File::open(if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        })
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error(parent, &error, "could not be flushed after the rename"))
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), ErrorValue> {
        std::fs::rename(from, to).map_err(|error| io_error(from, &error, "could not be moved"))
    }

    fn exists(&self, path: &Path) -> Result<bool, ErrorValue> {
        match std::fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(io_error(path, &error, "could not be examined")),
        }
    }
}

/// Refuses a destination that is, or leads through, a symlink (§43.5).
///
/// Every component that exists is examined without following it. One that does not exist yet is
/// one the copy will create itself. What is left is a race with somebody replacing a checked
/// directory by a symlink between this check and the rename; the rename itself never follows a
/// symlink at `to`, it replaces the link.
fn refuse_symlinked_path(to: &Path) -> Result<(), ErrorValue> {
    for component in to.ancestors() {
        if component.as_os_str().is_empty() {
            continue;
        }
        match std::fs::symlink_metadata(component) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(refusal(
                    to,
                    &format!(
                        "leads through the symlink {}, and a restore does not write through a \
                         symlink it did not create: protecting one object and then writing to a \
                         replaced symlink's target is what §43.5 forbids",
                        component.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(component, &error, "could not be examined")),
        }
    }
    Ok(())
}

/// A name beside `name` in `parent` that nothing else is using.
fn temporary_beside(parent: &Path, name: &std::ffi::OsStr) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".{}.ono-restore-{}-{serial}",
        name.to_string_lossy(),
        std::process::id()
    ))
}

/// Writes the content and mode of `from` to a new file at `temporary`, and flushes it.
///
/// `create_new` refuses a path that already exists, a planted symlink included. The live file's
/// owner is kept where there was a live file, which is what the in-place copy this replaced did;
/// restoring the owner the snapshot recorded is not claimed ([`METADATA_COVERAGE`]).
fn write_regular(
    from: &Path,
    temporary: &Path,
    source: &std::fs::Metadata,
    live: Option<&std::fs::Metadata>,
) -> Result<(), ErrorValue> {
    let mut input =
        std::fs::File::open(from).map_err(|error| io_error(from, &error, "could not be read"))?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temporary)
        .map_err(|error| io_error(temporary, &error, "could not be created"))?;
    std::io::copy(&mut input, &mut output)
        .map_err(|error| io_error(temporary, &error, "could not be written"))?;
    if let Some(live) = live {
        std::os::unix::fs::fchown(&output, Some(live.uid()), Some(live.gid())).map_err(
            |error| {
                io_error(
                    temporary,
                    &error,
                    "could not be given the live file's owner, and replacing the live file would \
                 have changed who owns it",
                )
            },
        )?;
    }
    output
        .set_permissions(std::fs::Permissions::from_mode(source.mode() & 0o7777))
        .map_err(|error| io_error(temporary, &error, "could not be given the snapshot's mode"))?;
    output
        .sync_all()
        .map_err(|error| io_error(temporary, &error, "could not be flushed"))
}

/// A refusal that is not an I/O failure, naming the path it is about.
fn refusal(path: &Path, why: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderInconclusive,
        format!("{} {why}", path.display()),
    )
    .with_help("v0.6 §43.5 and §15.4: nothing was written")
    .with_metadata("path", Value::string(&path.to_string_lossy()))
}

/// A store that replays recorded content, for tests (§54.4).
///
/// It is the [`ScriptedRunner`](ono_change_core::ScriptedRunner) of file content: the bytes come
/// from what a real `cat` printed against the recorded filesystem, so the comparison that decides
/// Appendix C.4's conflict runs over real bytes in the ordinary gate.
#[derive(Debug, Default)]
pub struct RecordedFiles {
    contents: Vec<(Arc<str>, Vec<u8>)>,
    copies: Mutex<Vec<(String, String)>>,
    renames: Mutex<Vec<(String, String)>>,
}

impl RecordedFiles {
    /// A store holding nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `path` holds `contents`.
    #[must_use]
    pub fn holding(mut self, path: impl Into<Arc<str>>, contents: impl Into<Vec<u8>>) -> Self {
        self.contents.push((path.into(), contents.into()));
        self
    }

    /// Every copy that was performed, in order, so a test can assert what a restore put back.
    #[must_use]
    pub fn copies(&self) -> Vec<(String, String)> {
        self.copies
            .lock()
            .map(|copies| copies.clone())
            .unwrap_or_default()
    }

    /// Every move that was performed, in order.
    #[must_use]
    pub fn renames(&self) -> Vec<(String, String)> {
        self.renames
            .lock()
            .map(|renames| renames.clone())
            .unwrap_or_default()
    }
}

impl FileStore for RecordedFiles {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, ErrorValue> {
        let wanted = path.to_string_lossy();
        Ok(self
            .contents
            .iter()
            .find(|(known, _)| known.as_ref() == wanted.as_ref())
            .map(|(_, contents)| contents.clone()))
    }

    fn copy(&self, from: &Path, to: &Path) -> Result<(), ErrorValue> {
        if self.read(from)?.is_none() {
            return Err(ErrorValue::new(
                ErrorCode::IoNotFound,
                format!("{} holds nothing to restore", from.display()),
            )
            .with_metadata("path", Value::string(&from.to_string_lossy())));
        }
        if let Ok(mut copies) = self.copies.lock() {
            copies.push((
                from.to_string_lossy().into_owned(),
                to.to_string_lossy().into_owned(),
            ));
        }
        Ok(())
    }

    fn exists(&self, path: &Path) -> Result<bool, ErrorValue> {
        Ok(self.read(path)?.is_some())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), ErrorValue> {
        if let Ok(mut renames) = self.renames.lock() {
            renames.push((
                from.to_string_lossy().into_owned(),
                to.to_string_lossy().into_owned(),
            ));
        }
        Ok(())
    }
}

/// Turns an I/O failure into the structured refusal §45 requires.
fn io_error(path: &Path, error: &std::io::Error, what: &str) -> ErrorValue {
    ErrorValue::new(
        match error.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::IoNotFound,
            std::io::ErrorKind::PermissionDenied => ErrorCode::IoPermissionDenied,
            _ => ErrorCode::ProviderInconclusive,
        },
        format!("{} {what}: {error}", path.display()),
    )
    .with_help(
        "v0.6 §56.3: a recovery fact that cannot be established blocks the recovery rather than \
         being assumed",
    )
    .with_metadata("path", Value::string(&path.to_string_lossy()))
}
