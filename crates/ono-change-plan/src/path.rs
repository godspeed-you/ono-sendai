//! Where the plan store lives, and how it is created private (spec v0.6 §36.1, §36.2, §43).
//!
//! §36.2 asks for SQLite with a versioned schema, "consistent with v0.5 storage practice where
//! practical". v0.5's practice is `~/.local/share/ono/temporal/ledger.sqlite3`, resolved through
//! XDG, with the directory `0700` and the file `0600` applied at creation. This is the same rule
//! one directory over: `~/.local/share/ono/change/plans.sqlite3`.
//!
//! A separate database file rather than a table in the ledger, deliberately. A plan is not an
//! event: §37 retains it on its own policy, §42.4 locks it, and a v0.5 retention sweep that
//! deleted history has no business deciding whether a sealed plan still exists.
//!
//! The modes are applied at creation rather than afterwards, for the reason `ono-temporal-ledger`
//! gives: a directory that was briefly world-traversable was world-traversable, and a `chmod`
//! after the fact does not take that back. §43.1 puts a plan in the class of things worth that
//! care — it names targets, arguments and provider bindings for changes that have not happened
//! yet.

use std::fs::{DirBuilder, OpenOptions};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use ono_change_core::error;
use ono_value::ErrorValue;

/// The mode the plan store's directory is created with (§43.1).
pub const DIRECTORY_MODE: u32 = 0o700;

/// The mode the plan store's database is created with (§43.1).
pub const DATABASE_MODE: u32 = 0o600;

/// The file name §36.2's store carries.
pub const DATABASE_NAME: &str = "plans.sqlite3";

/// The directory the plan store lives in, resolved the way the rest of the tree resolves XDG.
///
/// `env` reads an environment variable and `home` is the user's home directory. Both are
/// parameters so a test needs neither a real environment nor a real home.
///
/// ```
/// # use std::path::Path;
/// let directory =
///     ono_change_plan::plan_store_directory(|_| None, Some(Path::new("/home/case")))
///         .expect("a home resolves a directory");
/// assert!(directory.ends_with("ono/change"));
/// ```
#[must_use]
pub fn plan_store_directory(
    env: impl Fn(&str) -> Option<String>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let base = match env("XDG_DATA_HOME") {
        Some(data) if !data.is_empty() => PathBuf::from(data),
        _ => home?.join(".local").join("share"),
    };
    Some(base.join(ono_core::SHORT_NAME).join("change"))
}

/// The canonical plan store path (§36.1, §36.2).
///
/// ```
/// # use std::path::Path;
/// let path = ono_change_plan::plan_store_path(|_| None, Some(Path::new("/home/case")))
///     .expect("a home resolves a path");
/// assert!(path.ends_with("ono/change/plans.sqlite3"));
/// ```
#[must_use]
pub fn plan_store_path(
    env: impl Fn(&str) -> Option<String>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    plan_store_directory(env, home).map(|directory| directory.join(DATABASE_NAME))
}

/// Creates the store's directory chain, giving the store directory itself mode `0700` (§43.1).
///
/// Ancestors outside the store's own directory are created with the platform default, because
/// `~/.local/share` is shared with every other application and Ono does not own its mode.
pub(crate) fn create_private_directory(directory: &Path) -> Result<(), ErrorValue> {
    if directory.is_dir() {
        return set_mode(directory, DIRECTORY_MODE);
    }
    if let Some(parent) = directory.parent()
        && !parent.as_os_str().is_empty()
        && !parent.is_dir()
    {
        std::fs::create_dir_all(parent).map_err(|failure| {
            error::store_unavailable(&format!(
                "the plan store directory `{}` could not be created: {failure}",
                parent.display()
            ))
        })?;
    }
    DirBuilder::new()
        .mode(DIRECTORY_MODE)
        .create(directory)
        .or_else(|failure| {
            if failure.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(error::store_unavailable(&format!(
                    "the plan store directory `{}` could not be created: {failure}",
                    directory.display()
                )))
            }
        })?;
    set_mode(directory, DIRECTORY_MODE)
}

/// Creates the database file with mode `0600` before SQLite ever opens it (§43.1).
///
/// SQLite would create the file with `0644 & ~umask`, and the window in which it was readable
/// would be real. A zero-length file is a valid empty database, so making it here first and
/// letting SQLite adopt it costs nothing and closes the window. SQLite gives the write-ahead log
/// and the shared-memory file the mode of the database they belong to, so they follow.
pub(crate) fn create_private_file(path: &Path) -> Result<(), ErrorValue> {
    if path.exists() {
        return set_mode(path, DATABASE_MODE);
    }
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(DATABASE_MODE)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(failure) if failure.kind() == std::io::ErrorKind::AlreadyExists => {
            set_mode(path, DATABASE_MODE)
        }
        Err(failure) => Err(error::store_unavailable(&format!(
            "the plan store `{}` could not be created: {failure}",
            path.display()
        ))),
    }
}

/// Narrows an existing path that a previous release or a careless umask left wider.
fn set_mode(path: &Path, mode: u32) -> Result<(), ErrorValue> {
    use std::os::unix::fs::PermissionsExt as _;
    let metadata = std::fs::metadata(path).map_err(|failure| {
        error::store_unavailable(&format!(
            "`{}` could not be inspected: {failure}",
            path.display()
        ))
    })?;
    if metadata.permissions().mode() & 0o7777 == mode {
        return Ok(());
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|failure| {
        error::store_unavailable(&format!(
            "`{}` could not be made user-private: {failure}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    #[test]
    fn should_put_the_plan_store_beside_the_temporal_ledger_and_not_inside_it() {
        let plans = plan_store_path(|_| None, Some(Path::new("/home/case")))
            .expect("a home resolves a path");
        assert!(
            plans.ends_with("ono/change/plans.sqlite3"),
            "§36.2: the plan store is its own database file, got {}",
            plans.display()
        );
        assert!(
            !plans.to_string_lossy().contains("temporal"),
            "§37 retains plans on their own policy, so a ledger sweep must not reach them"
        );
    }

    #[test]
    fn should_follow_the_data_home_a_session_declares() {
        let path = plan_store_path(
            |name| (name == "XDG_DATA_HOME").then(|| "/srv/state".to_owned()),
            Some(Path::new("/home/case")),
        )
        .expect("an explicit data home resolves a path");
        assert_eq!(path, PathBuf::from("/srv/state/ono/change/plans.sqlite3"));
    }

    #[test]
    fn should_ignore_an_empty_data_home_and_fall_back_to_the_user() {
        let path = plan_store_path(
            |name| (name == "XDG_DATA_HOME").then(String::new),
            Some(Path::new("/home/case")),
        )
        .expect("an empty variable falls back to the home directory");
        assert!(path.starts_with("/home/case/.local/share"));
    }

    #[test]
    fn should_resolve_nothing_when_there_is_no_home_and_no_data_home() {
        assert!(
            plan_store_path(|_| None, None).is_none(),
            "§36.1: without a place to put it there is no store, and guessing one would be worse"
        );
    }

    #[test]
    fn should_create_the_store_directory_private_to_its_owner() {
        let temporary = tempfile::tempdir().expect("a temporary directory");
        let directory = temporary.path().join("change");
        create_private_directory(&directory).expect("the directory is created");
        let mode = std::fs::metadata(&directory)
            .expect("the directory exists")
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(
            mode, DIRECTORY_MODE,
            "§43.1: a plan names targets and arguments for changes that have not happened yet"
        );
    }

    #[test]
    fn should_create_the_database_file_private_before_sqlite_opens_it() {
        let temporary = tempfile::tempdir().expect("a temporary directory");
        let path = temporary.path().join(DATABASE_NAME);
        create_private_file(&path).expect("the file is created");
        let mode = std::fs::metadata(&path)
            .expect("the file exists")
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(mode, DATABASE_MODE);
    }

    #[test]
    fn should_narrow_a_store_a_careless_umask_left_readable() {
        let temporary = tempfile::tempdir().expect("a temporary directory");
        let path = temporary.path().join(DATABASE_NAME);
        std::fs::write(&path, b"").expect("a file is written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("the mode is set");
        create_private_file(&path).expect("an existing file is narrowed");
        let mode = std::fs::metadata(&path)
            .expect("the file exists")
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(
            mode, DATABASE_MODE,
            "a store an earlier release left wide is narrowed on open, not left as it was"
        );
    }
}
