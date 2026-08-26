//! Which process holds which socket.
//!
//! The kernel's `sock_diag` reply names a socket by inode and says nothing about who holds it.
//! The only join is procfs: `/proc/<pid>/fd/<n>` is a symlink reading `socket:[<inode>]`. That is
//! a documented kernel interface, not the output of a program, so reading it is what spec §23.1
//! asks for and not what spec §50 forbids.
//!
//! **The scan is opt-in.** It costs one `readlink` per open descriptor on the machine — on a host
//! with 5 000 processes and 50 000 sockets that is six figures of syscalls, which spec §34's
//! budget for an interactive answer cannot absorb. `get socket` therefore leaves `process` null
//! and `get socket --process` performs exactly one scan for the whole dump, never one per socket.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_value::ErrorValue;

/// The process holding a socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOwner {
    pid: i32,
    name: Option<Arc<str>>,
}

impl ProcessOwner {
    /// The process id.
    #[must_use]
    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// The process name, where `/proc/<pid>/comm` could be read.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

/// A map from socket inode to the process holding it, built by one pass over procfs.
///
/// A socket held by several processes — after a `fork`, or through `SCM_RIGHTS` — is reported
/// against the lowest pid holding it, so that repeating the query on an unchanged machine gives
/// the same answer twice.
#[derive(Debug, Clone, Default)]
pub struct SocketOwners {
    by_inode: BTreeMap<u64, ProcessOwner>,
}

impl SocketOwners {
    /// Scans `/proc`.
    ///
    /// # Errors
    ///
    /// Returns an error only when `/proc` itself cannot be listed. A process whose descriptors
    /// this user may not read is skipped, because that is the ordinary state of every process
    /// belonging to somebody else, and the sockets it holds then report a null owner — which the
    /// schema documents as "no owner, or not visible to you".
    pub fn from_proc() -> Result<Self, ErrorValue> {
        Self::from_proc_root(Path::new("/proc"))
    }

    /// Scans a procfs mounted somewhere other than `/proc`.
    ///
    /// # Errors
    ///
    /// Returns an error when `root` cannot be listed.
    pub fn from_proc_root(root: &Path) -> Result<Self, ErrorValue> {
        let entries = std::fs::read_dir(root).map_err(|error| {
            ErrorValue::new(
                ErrorCode::IoNotFound,
                format!("{} could not be listed: {error}", root.display()),
            )
            .with_help("the owning process of a socket is only discoverable through procfs")
        })?;

        let mut pids: Vec<i32> = entries
            .flatten()
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.parse().ok())
            })
            .filter(|pid| *pid > 0)
            .collect();
        // Ascending, so that a socket held by a parent and its children always reports the same
        // one of them.
        pids.sort_unstable();

        let mut owners = Self::default();
        for pid in pids {
            let inodes = socket_inodes(&root.join(pid.to_string()).join("fd"));
            if inodes.is_empty() {
                continue;
            }
            let name = process_name(&root.join(pid.to_string()).join("comm"));
            for inode in inodes {
                owners
                    .by_inode
                    .entry(inode)
                    .or_insert_with(|| ProcessOwner {
                        pid,
                        name: name.clone(),
                    });
            }
        }
        Ok(owners)
    }

    /// The process holding the socket with this inode, if the scan saw one.
    #[must_use]
    pub fn owner(&self, inode: u64) -> Option<&ProcessOwner> {
        self.by_inode.get(&inode)
    }

    /// How many socket inodes were attributed to a process.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_inode.len()
    }

    /// Whether the scan attributed nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_inode.is_empty()
    }
}

/// The socket inodes a process's descriptor directory points at.
fn socket_inodes(directory: &Path) -> Vec<u64> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        // Somebody else's process, or one that exited between listing `/proc` and reading it.
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .filter_map(|target| {
            let target = target.to_str()?;
            let inode = target.strip_prefix("socket:[")?.strip_suffix(']')?;
            inode.parse().ok()
        })
        .collect()
}

/// The contents of `/proc/<pid>/comm`, without its trailing newline.
fn process_name(path: &Path) -> Option<Arc<str>> {
    let contents = std::fs::read_to_string(path).ok()?;
    let name = contents.trim_end_matches('\n');
    if name.is_empty() {
        return None;
    }
    Some(Arc::from(name))
}
