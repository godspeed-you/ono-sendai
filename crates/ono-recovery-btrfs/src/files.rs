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
//! Appendix C.7 is why [`FileStore::copy`] exists rather than a write of bytes: a restore is
//! judged on what it puts back, and a copy that carries the permission bits can say it restores
//! mode as well as content. What no implementation here claims is owner, ACLs, xattrs,
//! capabilities, SELinux labels or hard-link relationships — [`METADATA_COVERAGE`] states that,
//! and Appendix C.7 requires the gaps to be visible rather than discovered.

use std::path::Path;
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

    /// Copies `from` over `to`, creating the parent directory where it is missing.
    ///
    /// # Errors
    ///
    /// A structured error when the copy failed. Appendix F then preserves the partial state and
    /// the evidence rather than retrying blind.
    fn copy(&self, from: &Path, to: &Path) -> Result<(), ErrorValue>;
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
        if let Some(parent) = to.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| io_error(parent, &error, "could not be created"))?;
        }
        std::fs::copy(from, to)
            .map(|_| ())
            .map_err(|error| io_error(to, &error, "could not be written"))
    }
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
