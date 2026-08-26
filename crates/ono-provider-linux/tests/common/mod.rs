//! Fixtures the provider tests share: a `/proc` tree, an account database, a clock and a signal
//! recorder.
//!
//! Everything faked here is the *outside world* — the kernel's files, NSS, the monotonic clock,
//! `kill(2)`. Nothing fakes a layer of this crate, which is what keeps the tests assertions about
//! behaviour rather than about structure (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    dead_code,
    reason = "a shared test fixture states its preconditions the way a test does, and not every \
              test file uses every helper"
)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ono_pipeline::{Collected, ValueStream};
use ono_provider_linux::accounts::{Accounts, GroupAccount, UserAccount};
use ono_provider_linux::{Clock, Signals};
use ono_value::{ErrorValue, RecordValue, Value};

/// The `btime` every `/proc` fixture declares, so a start time is an exact expectation.
pub const FIXTURE_BOOT_TIME: i64 = 1_700_000_000;

/// The tick rate every Linux userspace ABI uses. `sysconf(_SC_CLK_TCK)` returns it, and the
/// fixtures' expected start times are computed against it.
pub const USER_HZ: u64 = 100;

/// Everything a bounded stream produced, with a deadline so a hung provider fails the test
/// instead of hanging the suite.
pub async fn drain(stream: ValueStream) -> Collected {
    tokio::time::timeout(Duration::from_secs(10), stream.collect())
        .await
        .expect("the provider finished its bounded stream within ten seconds")
}

/// The records among a stream's values.
pub fn records(collected: &Collected) -> Vec<Arc<RecordValue>> {
    collected
        .values()
        .iter()
        .map(|value| match value {
            Value::Record(record) => Arc::clone(record),
            other => panic!("a provider yielded {other:?} rather than a record"),
        })
        .collect()
}

/// The record whose `field` renders as `wanted`.
pub fn find<'a>(
    records: &'a [Arc<RecordValue>],
    field: &str,
    wanted: &str,
) -> Option<&'a Arc<RecordValue>> {
    records.iter().find(|record| {
        record
            .get(field)
            .and_then(|value| ono_value::canonical_text(value).ok())
            .is_some_and(|text| text == wanted)
    })
}

/// A `/proc` tree a test owns.
pub struct ProcFixture {
    root: tempfile::TempDir,
}

impl ProcFixture {
    /// A fixture whose `/proc/stat` declares [`FIXTURE_BOOT_TIME`].
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("a temporary directory");
        let proc = root.path().join("proc");
        fs::create_dir_all(&proc).expect("the proc directory");
        fs::write(
            proc.join("stat"),
            format!("cpu  1 2 3\nbtime {FIXTURE_BOOT_TIME}\nprocesses 12345\n"),
        )
        .expect("the proc/stat file");
        Self { root }
    }

    /// The root to hand a provider.
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    /// The `/proc` directory itself.
    pub fn proc(&self) -> PathBuf {
        self.root.path().join("proc")
    }

    /// Starts a process directory. Nothing is written until a builder method is called, which is
    /// how the "listed, then gone" race is expressed: a bare directory has no `stat`.
    pub fn process(&self, pid: i64) -> ProcessFixture {
        let dir = self.proc().join(pid.to_string());
        fs::create_dir_all(&dir).expect("the process directory");
        ProcessFixture { dir }
    }

    /// Removes a process's `stat`, as an exit between enumeration and detail read does.
    pub fn vanish(&self, pid: i64) {
        let _ = fs::remove_file(self.proc().join(pid.to_string()).join("stat"));
    }
}

/// One process inside a [`ProcFixture`].
pub struct ProcessFixture {
    dir: PathBuf,
}

/// The `/proc/<pid>/stat` numbers a test cares about.
#[derive(Debug, Clone, Copy)]
pub struct StatFields {
    pub state: char,
    pub ppid: i64,
    pub utime: u64,
    pub stime: u64,
    pub threads: i64,
    pub starttime: u64,
    pub vsize: u64,
    pub rss_pages: i64,
}

impl Default for StatFields {
    fn default() -> Self {
        Self {
            state: 'S',
            ppid: 1,
            utime: 0,
            stime: 0,
            threads: 1,
            starttime: 100 * USER_HZ,
            vsize: 4096,
            rss_pages: 1,
        }
    }
}

impl ProcessFixture {
    /// Writes `/proc/<pid>/stat`, in the field order `proc(5)` documents.
    pub fn stat(self, name: &str, fields: StatFields) -> Self {
        let pid = self
            .dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("0");
        let line = format!(
            "{pid} ({name}) {state} {ppid} 0 0 0 -1 4194304 0 0 0 0 {utime} {stime} 0 0 20 0 \
             {threads} 0 {starttime} {vsize} {rss}\n",
            state = fields.state,
            ppid = fields.ppid,
            utime = fields.utime,
            stime = fields.stime,
            threads = fields.threads,
            starttime = fields.starttime,
            vsize = fields.vsize,
            rss = fields.rss_pages,
        );
        fs::write(self.dir.join("stat"), line).expect("the stat file");
        self
    }

    /// Writes `/proc/<pid>/status` with the four-id `Uid:`/`Gid:` lines.
    pub fn status(self, uid: u32, gid: u32) -> Self {
        fs::write(
            self.dir.join("status"),
            format!("Name:\tfixture\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\nGid:\t{gid}\t{gid}\t{gid}\t{gid}\n"),
        )
        .expect("the status file");
        self
    }

    /// Writes `/proc/<pid>/cmdline`, NUL-separated and NUL-terminated.
    pub fn cmdline(self, arguments: &[&str]) -> Self {
        let mut bytes = Vec::new();
        for argument in arguments {
            bytes.extend_from_slice(argument.as_bytes());
            bytes.push(0);
        }
        fs::write(self.dir.join("cmdline"), bytes).expect("the cmdline file");
        self
    }

    /// Writes `/proc/<pid>/cgroup`.
    pub fn cgroup(self, text: &str) -> Self {
        fs::write(self.dir.join("cgroup"), text).expect("the cgroup file");
        self
    }

    /// Makes `/proc/<pid>/exe` a symlink, the way the kernel does.
    pub fn exe(self, target: &str) -> Self {
        std::os::unix::fs::symlink(target, self.dir.join("exe")).expect("the exe link");
        self
    }

    /// Makes `/proc/<pid>/cwd` a symlink.
    pub fn cwd(self, target: &str) -> Self {
        std::os::unix::fs::symlink(target, self.dir.join("cwd")).expect("the cwd link");
        self
    }

    /// Puts a file where a readable one would be, with no permission for anyone.
    ///
    /// This is how a fixture reproduces the everyday case of another user's process hiding its
    /// command line, without the test needing a second user.
    pub fn unreadable(self, name: &str) -> Self {
        use std::os::unix::fs::PermissionsExt as _;
        let path = self.dir.join(name);
        fs::write(&path, "hidden").expect("the file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("the mode");
        self
    }
}

/// The start time a fixture's `starttime` works out to.
pub fn expected_start(starttime: u64) -> jiff::Timestamp {
    let seconds = FIXTURE_BOOT_TIME + (starttime / USER_HZ) as i64;
    let nanoseconds = (starttime % USER_HZ * 1_000_000_000 / USER_HZ) as i32;
    jiff::Timestamp::new(seconds, nanoseconds).expect("a representable instant")
}

/// An account database a test states in full.
#[derive(Debug, Default)]
pub struct FakeAccounts {
    users: HashMap<u32, UserAccount>,
    groups: HashMap<u32, GroupAccount>,
    /// How long a lookup takes, for the test that a slow directory does not stall an enumeration.
    delay: Option<Duration>,
}

impl FakeAccounts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_user(mut self, uid: u32, gid: u32, name: &str) -> Self {
        self.users.insert(
            uid,
            UserAccount {
                uid,
                gid,
                name: name.to_owned(),
                home: PathBuf::from(format!("/home/{name}")),
                shell: PathBuf::from("/bin/sh"),
                gecos: format!("{name} the fixture"),
            },
        );
        self
    }

    pub fn with_group(mut self, gid: u32, name: &str, members: &[&str]) -> Self {
        self.groups.insert(
            gid,
            GroupAccount {
                gid,
                name: name.to_owned(),
                members: members.iter().map(|member| (*member).to_owned()).collect(),
            },
        );
        self
    }

    /// Makes every lookup take `delay`, standing in for a network-backed directory.
    pub fn slow(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    async fn wait(&self) {
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
    }
}

#[async_trait::async_trait]
impl Accounts for FakeAccounts {
    async fn user(&self, uid: u32) -> Option<UserAccount> {
        self.wait().await;
        self.users.get(&uid).cloned()
    }

    async fn user_named(&self, name: &str) -> Option<UserAccount> {
        self.wait().await;
        self.users.values().find(|user| user.name == name).cloned()
    }

    async fn group(&self, gid: u32) -> Option<GroupAccount> {
        self.wait().await;
        self.groups.get(&gid).cloned()
    }

    async fn group_named(&self, name: &str) -> Option<GroupAccount> {
        self.wait().await;
        self.groups
            .values()
            .find(|group| group.name == name)
            .cloned()
    }

    fn users(&self) -> Result<Vec<UserAccount>, ErrorValue> {
        let mut users: Vec<UserAccount> = self.users.values().cloned().collect();
        users.sort_by_key(|user| user.uid);
        Ok(users)
    }

    fn groups(&self) -> Result<Vec<GroupAccount>, ErrorValue> {
        let mut groups: Vec<GroupAccount> = self.groups.values().cloned().collect();
        groups.sort_by_key(|group| group.gid);
        Ok(groups)
    }
}

/// A monotonic clock a test moves by hand, so a rate has an exact denominator.
#[derive(Debug, Default)]
pub struct TestClock {
    nanos: AtomicU64,
}

impl TestClock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn advance(&self, by: Duration) {
        self.nanos.fetch_add(by.as_nanos() as u64, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now_nanos(&self) -> u128 {
        u128::from(self.nanos.load(Ordering::SeqCst))
    }
}

/// A signal sender that records instead of delivering.
#[derive(Debug, Default)]
pub struct RecordingSignals {
    sent: Mutex<Vec<(i32, i32)>>,
    refuse: Option<ErrorValue>,
}

impl RecordingSignals {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// A sender the kernel refuses, for the permission path.
    pub fn refusing(error: ErrorValue) -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
            refuse: Some(error),
        })
    }

    /// Every `(pid, signal)` pair delivered so far.
    pub fn sent(&self) -> Vec<(i32, i32)> {
        self.sent
            .lock()
            .expect("the recorder is not poisoned")
            .clone()
    }
}

impl Signals for RecordingSignals {
    fn send(&self, pid: i32, signal: i32) -> Result<(), ErrorValue> {
        if let Some(error) = &self.refuse {
            return Err(error.clone());
        }
        self.sent
            .lock()
            .expect("the recorder is not poisoned")
            .push((pid, signal));
        Ok(())
    }
}
