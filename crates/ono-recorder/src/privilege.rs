//! The recorder widens the time a user can see, never the scope (v0.5 §10.5, §22.8, §30.2).
//!
//! §10.5 states four prohibitions rather than four defaults. The recorder MUST NOT become
//! setuid, MUST NOT automatically request elevation, MUST NOT run a privileged system daemon
//! merely to increase visibility, and MUST NOT read data the same user could not query through
//! Ono providers.
//!
//! Three of the four are properties of the code and are held by its shape: the recorder spawns
//! nothing but `systemctl --user` ([`crate::service`]), and every object it records arrives from
//! a `Provider` the shell already holds, so its *scope* is the shell's scope and only its *reach
//! in time* is new. The fourth is a property of the running process, and this module reads it
//! from the kernel rather than from the environment: `/proc/self/status` states the real,
//! effective, saved-set and filesystem user ids, and a `$USER` any caller can set states nothing.

use std::path::Path;

/// §10.5's four prohibitions, in the order `docs/contracts/temporal/recorder.yaml` lists them.
pub const PROHIBITIONS: &[&str] = &[
    "setuid",
    "automatic_sudo",
    "privileged_daemon_for_visibility",
    "read_beyond_user",
];

/// What privilege the recorder is actually running with (§10.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeReport {
    /// The real user id, where `/proc` states one.
    pub real_uid: Option<u32>,
    /// The effective user id, as the kernel answers it.
    pub effective_uid: u32,
    /// The saved-set user id, where `/proc` states one.
    pub saved_uid: Option<u32>,
    /// Who owns the store, where there is one.
    pub store_owner: Option<u32>,
    /// The store's mode, where there is one (§30.2 wants `0600`).
    pub store_mode: Option<u32>,
}

impl PrivilegeReport {
    /// Whether the process is running as somebody other than the user who started it.
    ///
    /// `false` where `/proc` said nothing: an unknown real id is not evidence of elevation, and
    /// claiming it would make the check fail on a host that simply does not have `/proc`.
    #[must_use]
    pub fn is_elevated(&self) -> bool {
        self.real_uid.is_some_and(|real| real != self.effective_uid)
    }

    /// Whether the binary carries a set-user-id bit into this process (§10.5).
    ///
    /// A setuid execution leaves the saved-set id different from the real one, which is the
    /// kernel's own record of the privilege the process could take back.
    #[must_use]
    pub fn is_setuid(&self) -> bool {
        match (self.real_uid, self.saved_uid) {
            (Some(real), Some(saved)) => real != saved,
            _ => false,
        }
    }

    /// Whether the store belongs to the user the recorder runs as (§30.2).
    #[must_use]
    pub fn store_is_own(&self) -> bool {
        self.store_owner
            .is_some_and(|owner| owner == self.effective_uid)
    }

    /// Whether the store is user-private, as §30.2's `0600` requires.
    #[must_use]
    pub fn store_is_private(&self) -> bool {
        self.store_mode.is_some_and(|mode| mode & 0o077 == 0)
    }
}

/// Reads what the kernel says about this process's privilege (§10.5).
///
/// `proc_root` is a parameter so a test needs no assumption about the host, and `store` is the
/// ledger's path where there is one.
#[must_use]
pub fn inspect(proc_root: &Path, store: Option<&Path>) -> PrivilegeReport {
    let (real_uid, saved_uid) = uids(proc_root);
    let (store_owner, store_mode) = store.map_or((None, None), owner_and_mode);
    PrivilegeReport {
        real_uid,
        effective_uid: ono_process::effective_uid(),
        saved_uid,
        store_owner,
        store_mode,
    }
}

/// The real and saved-set user ids from `/proc/self/status`.
///
/// The `Uid:` line is four numbers: real, effective, saved-set, filesystem.
fn uids(proc_root: &Path) -> (Option<u32>, Option<u32>) {
    let Ok(status) = std::fs::read_to_string(proc_root.join("self").join("status")) else {
        return (None, None);
    };
    let Some(line) = status.lines().find(|line| line.starts_with("Uid:")) else {
        return (None, None);
    };
    let mut numbers = line.split_whitespace().skip(1);
    let real = numbers.next().and_then(|value| value.parse().ok());
    let _effective = numbers.next();
    let saved = numbers.next().and_then(|value| value.parse().ok());
    (real, saved)
}

fn owner_and_mode(path: &Path) -> (Option<u32>, Option<u32>) {
    use std::os::unix::fs::MetadataExt as _;
    use std::os::unix::fs::PermissionsExt as _;

    match std::fs::metadata(path) {
        Ok(metadata) => (
            Some(metadata.uid()),
            Some(metadata.permissions().mode() & 0o7777),
        ),
        Err(_) => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_answer_no_elevation_when_the_kernel_says_nothing() {
        let report = inspect(Path::new("/nonexistent-proc"), None);
        assert!(!report.is_elevated());
        assert!(!report.is_setuid());
        assert!(!report.store_is_own());
    }
}
