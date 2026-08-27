//! Resolving users and groups, and enumerating the accounts a system knows about.
//!
//! Spec §23.6: "NSS lookups can be blocking and network-backed. The provider architecture SHOULD
//! allow asynchronous resolution and represent unresolved IDs without discarding numeric
//! identity." Both halves are load-bearing here.
//!
//! **Resolution goes through NSS.** [`NssAccounts::user`], [`NssAccounts::user_named`] and their
//! group counterparts call `getpwuid_r`/`getpwnam_r`/`getgrgid_r`/`getgrnam_r` through `nix`, so
//! an account that lives in LDAP or SSSD resolves exactly like a local one. Each call runs on a
//! blocking thread with a timeout, and the answers — including the negative ones — are cached,
//! because `get process` on a busy machine asks for the same handful of uids hundreds of times
//! and one hanging lookup must not stall the enumeration.
//!
//! **Enumeration reads the account database files.** `nix` 0.31 exposes no safe binding for
//! `getpwent`/`getgrent`, and this crate is `#![forbid(unsafe_code)]`, so `get user` with no
//! selector lists what `/etc/passwd` and `/etc/group` declare. That is a real limitation and it
//! is stated rather than hidden: a directory-only account does not appear in a bare `get user`,
//! but `get user <name>` and `get user --uid <n>` find it, because those go through NSS. The
//! files are a POSIX-specified colon-delimited database, not the output of a program, so reading
//! them is not the text scraping spec §50 forbids.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use nix::unistd::{Gid, Group, Uid, User};
use ono_value::ErrorValue;

use crate::common::io_error;

/// How long one NSS lookup may take before the provider gives up on it.
///
/// Spec §34 names a network-backed NSS lookup as a pathological case. Giving up leaves the
/// numeric id in place, which spec §23.6 requires anyway, and keeps an enumeration moving.
pub const NSS_TIMEOUT: Duration = Duration::from_millis(250);

/// A user account as the system's databases describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAccount {
    /// Numeric user id.
    pub uid: u32,
    /// Primary group id.
    pub gid: u32,
    /// Login name.
    pub name: String,
    /// Home directory, as recorded — not checked for existence.
    pub home: PathBuf,
    /// Login shell, as recorded.
    pub shell: PathBuf,
    /// The GECOS field, unparsed.
    pub gecos: String,
}

/// A group account as the system's databases describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupAccount {
    /// Numeric group id.
    pub gid: u32,
    /// Group name.
    pub name: String,
    /// Login names listed as supplementary members.
    pub members: Vec<String>,
}

/// Where a provider gets accounts from.
///
/// A provider takes one of these rather than calling NSS itself, so a test can state exactly
/// which accounts exist — and so a lookup that never returns can be exercised without a
/// directory server.
#[async_trait::async_trait]
pub trait Accounts: Send + Sync + std::fmt::Debug {
    /// The account owning `uid`, or `None` when nothing resolves it.
    async fn user(&self, uid: u32) -> Option<UserAccount>;

    /// The account named `name`, or `None` when nothing resolves it.
    async fn user_named(&self, name: &str) -> Option<UserAccount>;

    /// The group owning `gid`, or `None` when nothing resolves it.
    async fn group(&self, gid: u32) -> Option<GroupAccount>;

    /// The group named `name`, or `None` when nothing resolves it.
    async fn group_named(&self, name: &str) -> Option<GroupAccount>;

    /// Every account the enumeration source declares.
    ///
    /// # Errors
    ///
    /// Returns a structured error when the source cannot be read at all.
    fn users(&self) -> Result<Vec<UserAccount>, ErrorValue>;

    /// Every group the enumeration source declares.
    ///
    /// # Errors
    ///
    /// Returns a structured error when the source cannot be read at all.
    fn groups(&self) -> Result<Vec<GroupAccount>, ErrorValue>;
}

/// The system's own accounts: NSS for resolution, the account database files for enumeration.
#[derive(Debug)]
pub struct NssAccounts {
    root: PathBuf,
    timeout: Duration,
    users: Mutex<HashMap<u32, Option<UserAccount>>>,
    groups: Mutex<HashMap<u32, Option<GroupAccount>>>,
}

impl Default for NssAccounts {
    fn default() -> Self {
        Self::new()
    }
}

impl NssAccounts {
    /// The accounts of the machine this shell runs on.
    #[must_use]
    pub fn new() -> Self {
        Self::rooted("/")
    }

    /// The accounts declared under `root`, for a fixture that owns its own `etc/`.
    ///
    /// Resolution still goes through the real NSS: a fixture that wants to control resolution
    /// implements [`Accounts`] itself.
    #[must_use]
    pub fn rooted(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            timeout: NSS_TIMEOUT,
            users: Mutex::new(HashMap::new()),
            groups: Mutex::new(HashMap::new()),
        }
    }

    /// Changes how long one NSS lookup may take.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn cached_user(&self, uid: u32) -> Option<Option<UserAccount>> {
        self.users.lock().ok()?.get(&uid).cloned()
    }

    fn remember_user(&self, uid: u32, account: Option<UserAccount>) {
        if let Ok(mut cache) = self.users.lock() {
            cache.insert(uid, account);
        }
    }

    fn cached_group(&self, gid: u32) -> Option<Option<GroupAccount>> {
        self.groups.lock().ok()?.get(&gid).cloned()
    }

    fn remember_group(&self, gid: u32, account: Option<GroupAccount>) {
        if let Ok(mut cache) = self.groups.lock() {
            cache.insert(gid, account);
        }
    }
}

/// Runs one blocking NSS call with a deadline, yielding `None` when it does not answer in time.
async fn bounded<T, F>(timeout: Duration, lookup: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> Option<T> + Send + 'static,
{
    let handle = tokio::task::spawn_blocking(lookup);
    // The task is left running on timeout rather than cancelled: a blocking NSS call cannot be
    // interrupted, and abandoning it is what keeps the enumeration moving (spec §34).
    tokio::time::timeout(timeout, handle).await.ok()?.ok()?
}

#[async_trait::async_trait]
impl Accounts for NssAccounts {
    async fn user(&self, uid: u32) -> Option<UserAccount> {
        if let Some(cached) = self.cached_user(uid) {
            return cached;
        }
        let account = bounded(self.timeout, move || {
            User::from_uid(Uid::from_raw(uid)).ok().flatten()
        })
        .await
        .map(user_account);
        self.remember_user(uid, account.clone());
        account
    }

    async fn user_named(&self, name: &str) -> Option<UserAccount> {
        let wanted = name.to_owned();
        let account = bounded(self.timeout, move || {
            User::from_name(&wanted).ok().flatten()
        })
        .await
        .map(user_account);
        if let Some(found) = &account {
            self.remember_user(found.uid, Some(found.clone()));
        }
        account
    }

    async fn group(&self, gid: u32) -> Option<GroupAccount> {
        if let Some(cached) = self.cached_group(gid) {
            return cached;
        }
        let account = bounded(self.timeout, move || {
            Group::from_gid(Gid::from_raw(gid)).ok().flatten()
        })
        .await
        .map(group_account);
        self.remember_group(gid, account.clone());
        account
    }

    async fn group_named(&self, name: &str) -> Option<GroupAccount> {
        let wanted = name.to_owned();
        let account = bounded(self.timeout, move || {
            Group::from_name(&wanted).ok().flatten()
        })
        .await
        .map(group_account);
        if let Some(found) = &account {
            self.remember_group(found.gid, Some(found.clone()));
        }
        account
    }

    fn users(&self) -> Result<Vec<UserAccount>, ErrorValue> {
        let path = self.root.join("etc/passwd");
        Ok(parse_passwd(&read_database(&path)?))
    }

    fn groups(&self) -> Result<Vec<GroupAccount>, ErrorValue> {
        let path = self.root.join("etc/group");
        Ok(parse_group(&read_database(&path)?))
    }
}

fn read_database(path: &Path) -> Result<String, ErrorValue> {
    fs::read_to_string(path).map_err(|error| {
        io_error(&error, path).with_help(
            "`get user <name>` and `get user --uid <n>` still answer through NSS; only the bare \
             enumeration needs this file",
        )
    })
}

fn user_account(user: User) -> UserAccount {
    UserAccount {
        uid: user.uid.as_raw(),
        gid: user.gid.as_raw(),
        name: user.name,
        home: user.dir,
        shell: user.shell,
        gecos: user.gecos.to_string_lossy().into_owned(),
    }
}

fn group_account(group: Group) -> GroupAccount {
    GroupAccount {
        gid: group.gid.as_raw(),
        name: group.name,
        members: group.mem,
    }
}

/// Parses `passwd(5)`: `name:passwd:uid:gid:gecos:home:shell`.
fn parse_passwd(text: &str) -> Vec<UserAccount> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?.to_owned();
            let _password = fields.next()?;
            let uid = fields.next()?.parse().ok()?;
            let gid = fields.next()?.parse().ok()?;
            let gecos = fields.next().unwrap_or_default().to_owned();
            let home = PathBuf::from(fields.next().unwrap_or_default());
            let shell = PathBuf::from(fields.next().unwrap_or_default());
            Some(UserAccount {
                uid,
                gid,
                name,
                home,
                shell,
                gecos,
            })
        })
        .collect()
}

/// Parses `group(5)`: `name:passwd:gid:member,member`.
fn parse_group(text: &str) -> Vec<GroupAccount> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?.to_owned();
            let _password = fields.next()?;
            let gid = fields.next()?.parse().ok()?;
            let members = fields
                .next()
                .unwrap_or_default()
                .split(',')
                .filter(|member| !member.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            Some(GroupAccount { gid, name, members })
        })
        .collect()
}
