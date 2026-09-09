//! Where the ledger lives, and how it is created private (v0.5 §30.2, §31.1).
//!
//! §31.1 fixes the canonical path at `~/.local/share/ono/temporal/ledger.sqlite3`, resolved
//! through the same XDG rule the rest of the tree uses. §30.2 fixes the modes. Both are applied at
//! creation rather than afterwards: a directory that is briefly world-traversable was
//! world-traversable, and a `chmod` after the fact does not take that back.

use std::fs::{DirBuilder, OpenOptions};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use ono_value::ErrorValue;

/// The mode §30.2 gives the ledger directory.
pub const DIRECTORY_MODE: u32 = 0o700;
/// The mode §30.2 gives the ledger database.
pub const DATABASE_MODE: u32 = 0o600;
/// The file name §31.1 gives the store.
pub const DATABASE_NAME: &str = "ledger.sqlite3";

/// The directory the temporal ledger lives in, resolved the way the rest of the tree resolves XDG.
///
/// `env` reads an environment variable; `home` is the user's home directory. Both are parameters
/// so a test needs neither a real environment nor a real home.
///
/// ```
/// # use std::path::Path;
/// let directory = ono_temporal_ledger::ledger_directory(|_| None, Some(Path::new("/home/case")))
///     .expect("a home resolves a directory");
/// assert!(directory.ends_with("ono/temporal"));
/// ```
#[must_use]
pub fn ledger_directory(
    env: impl Fn(&str) -> Option<String>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let base = match env("XDG_DATA_HOME") {
        Some(data) if !data.is_empty() => PathBuf::from(data),
        _ => home?.join(".local").join("share"),
    };
    Some(base.join(ono_core::SHORT_NAME).join("temporal"))
}

/// The canonical ledger path (§31.1).
///
/// ```
/// # use std::path::Path;
/// let path = ono_temporal_ledger::ledger_path(|_| None, Some(Path::new("/home/case")))
///     .expect("a home resolves a path");
/// assert!(path.ends_with("ono/temporal/ledger.sqlite3"));
/// ```
#[must_use]
pub fn ledger_path(env: impl Fn(&str) -> Option<String>, home: Option<&Path>) -> Option<PathBuf> {
    ledger_directory(env, home).map(|directory| directory.join(DATABASE_NAME))
}

/// Creates the ledger's directory chain, giving the ledger directory itself mode `0700` (§30.2).
///
/// Ancestors outside the ledger's own directory are created with the platform default, because
/// `~/.local/share` is shared with every other application and Ono does not own its mode.
pub(crate) fn create_private_directory(directory: &Path) -> Result<(), ErrorValue> {
    if directory.is_dir() {
        return tighten(directory);
    }
    if let Some(parent) = directory.parent()
        && !parent.as_os_str().is_empty()
        && !parent.is_dir()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            unavailable(&format!(
                "the ledger directory `{}` could not be created: {error}",
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
                Err(unavailable(&format!(
                    "the ledger directory `{}` could not be created: {error}",
                    directory.display()
                )))
            }
        })?;
    tighten(directory)
}

/// Creates the database file with mode `0600` before SQLite ever opens it (§30.2).
///
/// SQLite would create the file with `0644 & ~umask`, and the window in which it was readable
/// would be real. A zero-length file is a valid empty database, so making it here first and
/// letting SQLite adopt it costs nothing and closes the window. SQLite gives the write-ahead log
/// and the shared-memory file the mode of the database they belong to, so they follow.
pub(crate) fn create_private_file(path: &Path) -> Result<(), ErrorValue> {
    if path.exists() {
        return tighten_file(path);
    }
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(DATABASE_MODE)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => tighten_file(path),
        Err(error) => Err(unavailable(&format!(
            "the ledger `{}` could not be created: {error}",
            path.display()
        ))),
    }
}

/// Narrows an existing directory that a previous release or a careless umask left wider.
fn tighten(directory: &Path) -> Result<(), ErrorValue> {
    set_mode(directory, DIRECTORY_MODE)
}

fn tighten_file(path: &Path) -> Result<(), ErrorValue> {
    set_mode(path, DATABASE_MODE)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), ErrorValue> {
    use std::os::unix::fs::PermissionsExt as _;
    let metadata = std::fs::metadata(path).map_err(|error| {
        unavailable(&format!(
            "`{}` could not be inspected: {error}",
            path.display()
        ))
    })?;
    if metadata.permissions().mode() & 0o7777 == mode {
        return Ok(());
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|error| {
        unavailable(&format!(
            "`{}` could not be made user-private: {error}",
            path.display()
        ))
    })
}

fn unavailable(detail: &str) -> ErrorValue {
    ono_temporal_core::error::store_unavailable(detail)
}
