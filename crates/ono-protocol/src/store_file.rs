//! Replacing a file that holds a trust decision, and serialising the updates around it
//! (v0.4.1 §9.8; issue #195, ADR-1041).
//!
//! Two files in the configuration directory decide who this machine trusts: `trusted_hosts`,
//! the hosts this shell will link to, and `authorized_clients`, the clients its listening agent
//! will serve. An account that chooses either file's contents chooses who is trusted, so both are
//! replaced the one way §9.8 describes, by the one function here:
//!
//! 1. under an exclusive lock on the directory the store lives in, so two updates — from two
//!    shells, or two threads of one — run one after the other and the second starts from what
//!    the first wrote;
//! 2. a temporary beside the store, created exclusively (`O_CREAT|O_EXCL`, which never follows a
//!    link) with mode `0600`. A temporary already there is what an update that died part-way
//!    left behind — the lock is what makes that certain — so it is unlinked, never opened, never
//!    truncated and never read;
//! 3. the contents written and synced, the temporary renamed over the store, the directory
//!    synced so the rename itself is durable.
//!
//! A reader therefore sees the old store or the new one, whole, and never a file another account
//! planted. The lock is advisory: it orders Ono's own updates, which is all it is for. It is taken
//! on the directory rather than on a lock file beside the store, so the directory gains no file
//! and there is no second path an attacker could plant a link at.

use std::fs::File;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// An exclusive lock on the directory a store lives in, released when dropped.
#[derive(Debug)]
pub(crate) struct Locked {
    _directory: File,
}

/// The directory `path` lives in, as a path that can be opened.
fn directory_of(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Takes the update lock for the store at `path`, creating its directory if it is not there yet.
///
/// Blocks until any other update of a store in the same directory has finished.
///
/// # Errors
///
/// The path that could not be created or opened, and why.
pub(crate) fn lock(path: &Path) -> Result<Locked, (PathBuf, io::Error)> {
    let directory = directory_of(path);
    std::fs::create_dir_all(directory).map_err(|error| (directory.to_path_buf(), error))?;
    let handle = File::open(directory).map_err(|error| (directory.to_path_buf(), error))?;
    handle
        .lock()
        .map_err(|error| (directory.to_path_buf(), error))?;
    Ok(Locked { _directory: handle })
}

/// Replaces the store at `path` with `contents`, as §9.8 describes. The caller holds `_locked`.
///
/// # Errors
///
/// The path that could not be written, and why. The store at `path` is unchanged whenever this
/// returns an error.
pub(crate) fn replace(
    _locked: &Locked,
    path: &Path,
    contents: &[u8],
) -> Result<(), (PathBuf, io::Error)> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let temporary = path.with_extension("tmp");
    // Under the lock, a temporary that exists belongs to an update that is no longer running.
    // Unlinking removes the name and never follows it, so a planted link costs only itself.
    match std::fs::remove_file(&temporary) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err((temporary, error)),
    }
    // `create_new` is `O_EXCL`: if anything appeared at the name since, this refuses rather than
    // writing into it.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| (temporary.clone(), error))?;
    let written = file
        .write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| (temporary.clone(), error));
    drop(file);
    if let Err(failure) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(failure);
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err((path.to_path_buf(), error));
    }
    // The rename is durable only once the directory entry is. A filesystem that cannot sync a
    // directory handle is no reason to report an update that already happened as failed.
    if let Ok(directory) = File::open(directory_of(path)) {
        let _ = directory.sync_all();
    }
    Ok(())
}
