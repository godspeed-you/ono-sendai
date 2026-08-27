//! What the relationship providers read from `/proc`.
//!
//! Only the two things the object schemas do not already carry are read here: the file
//! descriptor table of a process, and the control group it belongs to. Everything else about an
//! object comes from the provider that owns it, so this crate never becomes a second, divergent
//! process provider.
//!
//! Nothing here parses the output of a program (AGENTS.md §6); `/proc` is a kernel interface,
//! and the two files read are the stable formats `proc(5)` documents.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ono_core::ErrorCode;
use ono_value::{ErrorValue, ValueRef};

/// How a descriptor was opened, from the `O_ACCMODE` bits of `/proc/<pid>/fdinfo/<fd>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// Opened for reading.
    Read,
    /// Opened for writing.
    Write,
    /// Opened for both.
    ReadWrite,
}

impl Access {
    /// The relation an open of this kind contributes, in the vocabulary of spec §22.4.
    pub(crate) const fn relation(self) -> &'static str {
        match self {
            Access::Read => "reads",
            Access::Write => "writes",
            // A descriptor open both ways is neither a read nor a write relationship, and
            // claiming one of them would be an invention.
            Access::ReadWrite => "opens",
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "write",
            Access::ReadWrite => "read-write",
        }
    }
}

/// One entry of a process's file descriptor table.
#[derive(Debug, Clone)]
pub(crate) struct Descriptor {
    /// The descriptor number.
    pub(crate) fd: u32,
    /// What the descriptor's symlink points at: a path, `socket:[inode]`, `pipe:[inode]`, …
    pub(crate) target: String,
    /// How it was opened, or `None` when `fdinfo` could not be read.
    pub(crate) access: Option<Access>,
}

impl Descriptor {
    /// The socket inode this descriptor holds, if it holds a socket.
    pub(crate) fn socket_inode(&self) -> Option<i64> {
        self.target
            .strip_prefix("socket:[")
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|inode| inode.parse().ok())
    }

    /// The file this descriptor holds, if it holds one that still exists.
    ///
    /// A descriptor on a deleted file keeps the path with ` (deleted)` appended. There is no
    /// object left to point an edge at, so it is not one — and saying so is not the same as
    /// failing to read it.
    pub(crate) fn file_path(&self) -> Option<PathBuf> {
        if !self.target.starts_with('/') || self.target.ends_with(" (deleted)") {
            return None;
        }
        Some(PathBuf::from(&self.target))
    }
}

/// The `/proc` directory under `root`.
pub(crate) fn proc_dir(root: &Path) -> PathBuf {
    root.join("proc")
}

/// A process's file descriptor table, in descriptor order.
///
/// # Errors
///
/// Returns the structured error the kernel gave: `io.permission_denied` for a process this user
/// may not look into, `io.not_found` for one that has exited.
pub(crate) fn descriptors(proc: &Path, pid: i64) -> Result<Vec<Descriptor>, ErrorValue> {
    let dir = proc.join(pid.to_string()).join("fd");
    let entries = fs::read_dir(&dir).map_err(|error| io_error(&error, &dir))?;
    let mut descriptors = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io_error(&error, &dir))?;
        let Some(fd) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        // A descriptor closed between the listing and the read is not an error: it is a process
        // going about its business, and the remaining descriptors are still true.
        let Ok(target) = fs::read_link(entry.path()) else {
            continue;
        };
        descriptors.push(Descriptor {
            fd,
            target: target.to_string_lossy().into_owned(),
            access: access_of(proc, pid, fd),
        });
    }
    descriptors.sort_by_key(|descriptor| descriptor.fd);
    Ok(descriptors)
}

/// How a descriptor was opened, or `None` when `fdinfo` says nothing this crate understands.
fn access_of(proc: &Path, pid: i64, fd: u32) -> Option<Access> {
    let path = proc
        .join(pid.to_string())
        .join("fdinfo")
        .join(fd.to_string());
    let text = fs::read_to_string(path).ok()?;
    let flags = text
        .lines()
        .find_map(|line| line.strip_prefix("flags:"))
        .map(str::trim)?;
    let flags = u32::from_str_radix(flags, 8).ok()?;
    match flags & 0b11 {
        0 => Some(Access::Read),
        1 => Some(Access::Write),
        2 => Some(Access::ReadWrite),
        _ => None,
    }
}

/// The control group paths a process belongs to, one per hierarchy.
///
/// # Errors
///
/// Returns the structured error the kernel gave.
pub(crate) fn cgroups(proc: &Path, pid: i64) -> Result<Vec<String>, ErrorValue> {
    let path = proc.join(pid.to_string()).join("cgroup");
    let text = fs::read_to_string(&path).map_err(|error| io_error(&error, &path))?;
    Ok(text
        .lines()
        // `hierarchy:controllers:path` — the path is everything after the second colon, which
        // may itself contain colons.
        .filter_map(|line| line.splitn(3, ':').nth(2).map(str::to_owned))
        .filter(|path| !path.is_empty())
        .collect())
}

/// Where one of a process's magic links points: its `root`, its `cwd`, a namespace under `ns/`.
///
/// `who` is a pid, or `self` for the reader.
///
/// # Errors
///
/// Returns the structured error the kernel gave: `io.permission_denied` for a process this user
/// may not look into, `io.not_found` for one that has exited.
pub(crate) fn link_target(proc: &Path, who: &str, name: &str) -> Result<PathBuf, ErrorValue> {
    let path = proc.join(who).join(name);
    fs::read_link(&path).map_err(|error| io_error(&error, &path))
}

/// The process ids under `/proc`, in numeric order.
///
/// # Errors
///
/// Returns the structured error the kernel gave for `/proc` itself.
pub(crate) fn pids(proc: &Path) -> Result<Vec<i64>, ErrorValue> {
    let entries = fs::read_dir(proc).map_err(|error| io_error(&error, proc))?;
    let mut pids: Vec<i64> = entries
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse().ok())
        })
        .collect();
    pids.sort_unstable();
    Ok(pids)
}

/// The structured error for a failed read, keeping the kernel's own reason.
pub(crate) fn io_error(error: &io::Error, path: &Path) -> ErrorValue {
    let code = match error.kind() {
        io::ErrorKind::PermissionDenied => ErrorCode::IoPermissionDenied,
        io::ErrorKind::NotFound => ErrorCode::IoNotFound,
        io::ErrorKind::NotADirectory => ErrorCode::IoNotDirectory,
        _ => ErrorCode::ProviderUnavailable,
    };
    ErrorValue::new(code, format!("{}: {error}", path.display()))
        .with_target(ValueRef::path(path))
        .with_retryable(matches!(code, ErrorCode::ProviderUnavailable))
}
