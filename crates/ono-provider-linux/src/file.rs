//! The `file` and `dir` targets, answered with `stat`-family syscalls (spec §23.4, §28.2).
//!
//! The traversal is deliberately `openat`-relative. A directory is opened once, with
//! `O_NOFOLLOW | O_DIRECTORY`, and every later question about its contents is asked against that
//! descriptor — never against a path re-resolved from the root. That closes the symlink race of
//! ADR-0015 T14: swapping a directory for a symlink mid-walk cannot redirect the walk out of the
//! tree, because the walk is no longer looking anything up by name.
//!
//! The walk streams. Entries are emitted as they are stated, one directory's names at a time, so
//! `get dir / --recursive | take 5` shows five rows immediately and never holds the tree.

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nix::dir::Dir;
use nix::errno::Errno;
use nix::fcntl::{AT_FDCWD, AtFlags, OFlag, openat, readlinkat};
use nix::sys::stat::{FileStat, Mode, SFlag, fstatat, major, minor};
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamSink, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, EventStream, ObjectRef, Provider, Query, Risk,
    Selector,
};
use ono_value::{ByteSize, ErrorValue, RecordValue, Schema, Value};

use crate::accounts::{Accounts, NssAccounts};
use crate::common::{errno_error, group_ref, provenance, timestamp, user_ref};
use crate::schemas;

/// The provider's stable id, as it appears in every record's provenance.
pub const PROVIDER_ID: &str = "linux.fs";

/// Files and directories.
///
/// ```no_run
/// use ono_provider_api::{Provider, Query, Selector};
/// use ono_provider_linux::FileProvider;
/// use ono_value::Value;
///
/// let provider = FileProvider::new();
/// let query = Query::target("dir")
///     .with(Selector::field("path", Value::Path(std::sync::Arc::from(
///         std::path::Path::new("/etc"),
///     ))))
///     .option("recursive", Value::Bool(true));
/// let stream = provider.snapshot(&query)?;
/// assert!(stream.boundedness().is_bounded());
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Debug)]
pub struct FileProvider {
    accounts: Arc<dyn Accounts>,
}

impl Default for FileProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FileProvider {
    /// A provider over the filesystem this shell can see.
    #[must_use]
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(NssAccounts::new()),
        }
    }

    /// Resolves owner and group references through `accounts` instead of through the system's NSS.
    #[must_use]
    pub fn with_accounts(mut self, accounts: Arc<dyn Accounts>) -> Self {
        self.accounts = accounts;
        self
    }

    /// The uid a `--owner` names: a number as it is, a name through the accounts database.
    pub(crate) async fn uid_of(&self, value: &Value) -> Result<u32, ErrorValue> {
        if let Some(uid) = numeric_id(value) {
            return Ok(uid);
        }
        let name = value.as_str()?;
        self.accounts
            .user_named(name)
            .await
            .map(|account| account.uid)
            .ok_or_else(|| {
                ErrorValue::new(ErrorCode::IoNotFound, format!("no user named `{name}`"))
            })
    }

    /// The gid a `--group` names: a number as it is, a name through the accounts database.
    pub(crate) async fn gid_of(&self, value: &Value) -> Result<u32, ErrorValue> {
        if let Some(gid) = numeric_id(value) {
            return Ok(gid);
        }
        let name = value.as_str()?;
        self.accounts
            .group_named(name)
            .await
            .map(|account| account.gid)
            .ok_or_else(|| {
                ErrorValue::new(ErrorCode::IoNotFound, format!("no group named `{name}`"))
            })
    }
}

/// A user or group written as its numeric id, in either of the forms a word binds to.
fn numeric_id(value: &Value) -> Option<u32> {
    match value {
        Value::Int(id) => u32::try_from(*id).ok(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// What a query asked the walk to do.
#[derive(Debug, Clone)]
struct Request {
    /// The paths the query names — several when a glob resolved to several (spec §17.3).
    roots: Vec<PathBuf>,
    list_contents: bool,
    recursive: bool,
    follow_symlinks: bool,
    include_hidden: bool,
    max_depth: usize,
    limit: usize,
    /// `find file --name <glob>`: only entries whose name matches are emitted.
    name: Option<globset::GlobMatcher>,
    /// `find file --kind <kind>`: only entries of this kind are emitted; directories of
    /// another kind are still descended into.
    kind: Option<String>,
}

impl Request {
    fn from(query: &Query) -> Self {
        let mut named_root = false;
        let roots = query
            .selectors()
            .iter()
            .find_map(|selector| match selector {
                Selector::Field { name, value } if name == "path" || name == "root" => {
                    named_root = name == "root";
                    Some(paths_of(value))
                }
                _ => None,
            })
            .filter(|roots| !roots.is_empty())
            .unwrap_or_else(|| vec![PathBuf::from(".")]);
        let listing = query.target_name() != "file";
        // `find file /var/log` binds its path to the selector named `root`, and find *is* the
        // walk (docs/spec/commands/file.yaml: "discover files by walking a root"); only
        // `get file`, whose selector is `path`, means the one entry. The contract declares no
        // `--recursive` — the verb carries the intent, and the selector name carries the verb.
        let recursive = named_root || query.flag("recursive");
        let depth = query
            .option_value("depth")
            .and_then(|value| value.as_int().ok())
            .and_then(|depth| usize::try_from(depth).ok());
        let name = query
            .option_value("name")
            .and_then(|value| value.as_str().ok())
            .and_then(|pattern| {
                globset::GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .ok()
            })
            .map(|glob| glob.compile_matcher());
        let kind = query
            .option_value("kind")
            .and_then(|value| value.as_str().ok())
            .map(str::to_owned);
        Self {
            roots,
            list_contents: listing,
            recursive,
            follow_symlinks: query.flag("follow-symlinks"),
            // `docs/spec/commands/file.yaml` gives `--all` to `get dir` only, so a `file` walk
            // reports every entry it reaches and a `dir` listing hides dot entries by default.
            include_hidden: !listing || query.flag("all"),
            // The root's direct entries are depth 1: `--depth 1` lists them and descends no
            // further. Without a bound a recursive walk has none.
            max_depth: depth.unwrap_or(if recursive { usize::MAX } else { 1 }),
            limit: query.max().unwrap_or(usize::MAX),
            name,
            kind,
        }
    }

    /// Whether `--name` and `--kind` let this entry through.
    fn admits(&self, name: &OsStr, kind: &str) -> bool {
        self.name
            .as_ref()
            .is_none_or(|glob| glob.is_match(Path::new(name)))
            && self.kind.as_deref().is_none_or(|wanted| wanted == kind)
    }
}

/// The paths a selector value names: one, or every element of a list a glob resolved to.
fn paths_of(value: &Value) -> Vec<PathBuf> {
    match value {
        Value::Path(path) => vec![path.to_path_buf()],
        Value::String(text) => vec![PathBuf::from(text.as_ref())],
        Value::List(items) => items.iter().flat_map(paths_of).collect(),
        _ => Vec::new(),
    }
}

/// Everything describing one entry needs.
#[derive(Clone)]
pub(crate) struct Describer {
    schema: Arc<Schema>,
    user_schema: Arc<Schema>,
    group_schema: Arc<Schema>,
    accounts: Arc<dyn Accounts>,
}

impl Describer {
    /// Describes the entry `lookup` names relative to `dirfd`, recorded under the logical
    /// `path` it was reached by.
    pub(crate) async fn describe<Fd: AsFd>(
        &self,
        dirfd: Fd,
        lookup: &OsStr,
        path: PathBuf,
        follow: bool,
    ) -> Result<RecordValue, ErrorValue> {
        let flags = if follow {
            AtFlags::empty()
        } else {
            AtFlags::AT_SYMLINK_NOFOLLOW
        };
        let stat = fstatat(&dirfd, lookup, flags).map_err(|errno| errno_error(errno, &path))?;
        let kind = kind_of(&stat);
        let target = if kind == "symlink" {
            match readlinkat(&dirfd, lookup) {
                Ok(target) => Value::Path(Arc::from(PathBuf::from(target))),
                Err(errno) => errno_error(errno, &path).into_value(),
            }
        } else {
            Value::Null
        };
        self.build(&stat, kind, path, target).await
    }

    async fn build(
        &self,
        stat: &FileStat,
        kind: &str,
        path: PathBuf,
        target: Value,
    ) -> Result<RecordValue, ErrorValue> {
        let owner_name = self
            .accounts
            .user(stat.st_uid)
            .await
            .map(|account| account.name);
        let group_name = self
            .accounts
            .group(stat.st_gid)
            .await
            .map(|account| account.name);
        let source = path.display().to_string();
        let name = path.file_name().map_or_else(
            || path.as_os_str().to_string_lossy(),
            |name| name.to_string_lossy(),
        );
        Ok(RecordValue::builder(
            Arc::clone(&self.schema),
            provenance(PROVIDER_ID, self.schema.id(), &source),
        )
        .set("path", Value::Path(Arc::from(path.clone())))?
        .set("name", Value::string(&name))?
        .set("kind", Value::string(kind))?
        .set("size", size_of(stat, kind))?
        .set(
            "owner",
            user_ref(&self.user_schema, stat.st_uid, owner_name.as_deref()),
        )?
        .set(
            "group",
            group_ref(&self.group_schema, stat.st_gid, group_name.as_deref()),
        )?
        // Four octal digits, as `docs/spec/schemas/file.v1.yaml` fixes the representation: the
        // grammar has no octal literal, so `where mode == "0644"` is the comparison a user can
        // actually write.
        .set(
            "mode",
            Value::string(&format!("{:04o}", stat.st_mode & 0o7777)),
        )?
        .set(
            "modified",
            timestamp(stat.st_mtime, stat.st_mtime_nsec).map_or(Value::Null, Value::Timestamp),
        )?
        .set(
            "accessed",
            timestamp(stat.st_atime, stat.st_atime_nsec).map_or(Value::Null, Value::Timestamp),
        )?
        // Birth time needs `statx`, which the `openat`-relative API this walk is built on does
        // not reach. Unknown is null (spec §35.3), never the change time wearing its name.
        .set("created", Value::Null)?
        .set("inode", Value::Int(i128::from(stat.st_ino)))?
        .set("device", device_ref(stat.st_dev))?
        .set("target", target)?
        .build())
    }
}

/// The kind of an entry, from the file-type bits of its mode.
fn kind_of(stat: &FileStat) -> &'static str {
    match SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT {
        SFlag::S_IFREG => "file",
        SFlag::S_IFDIR => "dir",
        SFlag::S_IFLNK => "symlink",
        SFlag::S_IFSOCK => "socket",
        SFlag::S_IFIFO => "fifo",
        SFlag::S_IFBLK | SFlag::S_IFCHR => "device",
        _ => "other",
    }
}

/// The apparent size, for the kinds where the kernel's number means one.
fn size_of(stat: &FileStat, kind: &str) -> Value {
    match kind {
        "file" | "dir" | "symlink" => u128::try_from(stat.st_size).map_or(Value::Null, |size| {
            Value::ByteSize(ByteSize::from_bytes(size))
        }),
        // A socket, a fifo or a device node has no content whose size the number describes.
        _ => Value::Null,
    }
}

/// The device an entry lives on, as the `major:minor` pair the kernel identifies it by.
///
/// Always reported, including for an anonymous device such as tmpfs or an overlay. `device` is
/// half of `ono.file/1`'s identity, and leaving it null for the filesystems that carry no block
/// device would let two files on two different tmpfs mounts claim to be one object as soon as
/// their inode numbers happened to agree.
fn device_ref(dev: u64) -> Value {
    Value::string(&format!("{}:{}", major(dev), minor(dev)))
}

/// The names in one directory, without `.` and `..`.
fn entry_names(fd: &OwnedFd, path: &Path) -> Result<Vec<OsString>, ErrorValue> {
    let duplicate = fd
        .try_clone()
        .map_err(|error| crate::common::io_error(&error, path))?;
    let mut dir = Dir::from_fd(duplicate).map_err(|errno| errno_error(errno, path))?;
    let mut names = Vec::new();
    for entry in dir.iter() {
        let entry = entry.map_err(|errno| errno_error(errno, path))?;
        let name = OsStr::from_bytes(entry.file_name().to_bytes());
        if name == "." || name == ".." {
            continue;
        }
        names.push(name.to_owned());
    }
    Ok(names)
}

/// Opens a directory for descent: relative to its parent, and never through a symlink.
/// Opens the root the user named. Following a symlink here is intended: `get dir /var/run` means
/// the directory the user pointed at. Every descent below it uses `O_NOFOLLOW`.
/// Re-opens a directory strictly beneath `root`, refusing symlinks anywhere in the path.
///
/// `RESOLVE_BENEATH` keeps the open inside the tree even against a hostile rename;
/// `RESOLVE_NO_SYMLINKS` makes a component swapped for a symlink fail with `ELOOP` instead of
/// being followed — the T14 property the descriptor-per-directory walk used to get from holding
/// every directory open, kept without holding them.
fn reopen_beneath(root: &OwnedFd, relative: &Path) -> Result<OwnedFd, Errno> {
    let mut how = nix::fcntl::OpenHow::new()
        .flags(OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC);
    how = how.resolve(
        nix::fcntl::ResolveFlag::RESOLVE_BENEATH | nix::fcntl::ResolveFlag::RESOLVE_NO_SYMLINKS,
    );
    nix::fcntl::openat2(root.as_fd(), relative, how)
}

fn open_root(path: &Path) -> Result<OwnedFd, ErrorValue> {
    openat(
        AT_FDCWD,
        path,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|errno| errno_error(errno, path))
}

/// Emits the walk of every named root, breadth first, sending each entry as it is stated.
async fn walk(request: Request, describer: Describer, sink: StreamSink) {
    let mut sent = 0usize;
    for root in &request.roots {
        if sent >= request.limit {
            return;
        }
        if walk_root(&request, root, &describer, &sink, &mut sent)
            .await
            .is_err()
        {
            return;
        }
    }
}

/// The receiving side went away; nothing more can be delivered.
struct Closed;

/// Emits one root: the entry itself for `get file`, then its tree where the query descends.
async fn walk_root(
    request: &Request,
    root_path: &Path,
    describer: &Describer,
    sink: &StreamSink,
    sent: &mut usize,
) -> Result<(), Closed> {
    if !request.list_contents {
        match describer
            .describe(
                AT_FDCWD,
                root_path.as_os_str(),
                root_path.to_path_buf(),
                request.follow_symlinks,
            )
            .await
        {
            Ok(record) => {
                let kind = record
                    .get("kind")
                    .and_then(|value| value.as_str().ok())
                    .unwrap_or("other")
                    .to_owned();
                let name = root_path.file_name().unwrap_or(root_path.as_os_str());
                if request.admits(name, &kind) {
                    sink.send(record.into_value()).await.map_err(|_| Closed)?;
                    *sent += 1;
                }
            }
            // One root that is not there is that root's answer, not the walk's: the other
            // roots a glob resolved to are still described (spec §16.5).
            Err(error) => {
                sink.fail(error).await.map_err(|_| Closed)?;
                return Ok(());
            }
        }
        if !request.recursive {
            return Ok(());
        }
    }

    let root = match open_root(root_path) {
        Ok(fd) => fd,
        Err(error) => {
            // A plain `get file <path>` on a non-directory has already reported the entry; the
            // failure to open it as a directory is not a second answer.
            if request.list_contents {
                sink.fail(error).await.map_err(|_| Closed)?;
            }
            return Ok(());
        }
    };

    // The frontier holds *paths relative to the held root*, never descriptors: a tree wider
    // than the descriptor table was unwalkable when every pending directory kept one open
    // (ADR-0015, F11). Each directory is re-opened from the root when its turn comes, through
    // `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` — so the walk still cannot be
    // redirected out of the tree by a swapped component (T14): a symlink appearing anywhere in
    // the recorded path makes the open fail loudly instead of following it. At most two
    // descriptors are ever held: the root, and the directory being read.
    // With `--follow-symlinks` a directory can be reached by more than one name, and each name
    // lists it; a link can also point at its own ancestor, and that is the one thing not
    // descended into. So every queued directory carries the `(device, inode)` chain of the
    // directories above it on its own walk path — not a set of everything visited, which would
    // let readdir order decide which of two names to a directory gets listed (ADR-0120).
    let mut queue: VecDeque<(PathBuf, PathBuf, usize, Vec<(u64, u64)>)> = VecDeque::new();
    queue.push_back((PathBuf::new(), root_path.to_path_buf(), 0, Vec::new()));

    while let Some((relative, path, depth, mut ancestors)) = queue.pop_front() {
        let fd = if relative.as_os_str().is_empty() {
            match root.try_clone() {
                Ok(fd) => fd,
                Err(error) => {
                    sink.fail(errno_error(
                        Errno::from_raw(error.raw_os_error().unwrap_or(0)),
                        &path,
                    ))
                    .await
                    .map_err(|_| Closed)?;
                    return Ok(());
                }
            }
        } else if request.follow_symlinks {
            // Following means following: the directory is opened by the name it was reached
            // by, symlinks included. The T14 guarantee is deliberately given up here, which is
            // why the contract keeps this off by default (ADR-0083 §3).
            match open_root(&path) {
                Ok(fd) => fd,
                Err(error) => {
                    sink.fail(error).await.map_err(|_| Closed)?;
                    continue;
                }
            }
        } else {
            match reopen_beneath(&root, &relative) {
                Ok(fd) => fd,
                // The directory vanished or a component stopped being a plain directory between
                // the listing and this turn — the swap of ADR-0015 T14. Reported, not followed.
                Err(errno) => {
                    sink.fail(errno_error(errno, &path))
                        .await
                        .map_err(|_| Closed)?;
                    continue;
                }
            }
        };
        if request.follow_symlinks
            && let Ok(stat) = nix::sys::stat::fstat(&fd)
        {
            let this = (stat.st_dev, stat.st_ino);
            if ancestors.contains(&this) {
                continue;
            }
            ancestors.push(this);
        }
        let names = match entry_names(&fd, &path) {
            Ok(names) => names,
            Err(error) => {
                sink.fail(error).await.map_err(|_| Closed)?;
                continue;
            }
        };
        for name in names {
            if *sent >= request.limit {
                return Ok(());
            }
            if !request.include_hidden && name.as_bytes().starts_with(b".") {
                continue;
            }
            let child_path = path.join(&name);
            let record = describer
                .describe(
                    &fd,
                    name.as_os_str(),
                    child_path.clone(),
                    request.follow_symlinks,
                )
                .await;
            let kind = match record {
                Ok(record) => {
                    let kind = record
                        .get("kind")
                        .and_then(|value| value.as_str().ok())
                        .unwrap_or("other")
                        .to_owned();
                    if request.admits(&name, &kind) {
                        sink.send(record.into_value()).await.map_err(|_| Closed)?;
                        *sent += 1;
                    }
                    kind
                }
                Err(error) => {
                    sink.fail(error).await.map_err(|_| Closed)?;
                    continue;
                }
            };
            if kind == "dir" && depth + 1 < request.max_depth {
                queue.push_back((
                    relative.join(&name),
                    child_path,
                    depth + 1,
                    ancestors.clone(),
                ));
            }
        }
    }
    Ok(())
}

/// `read file`: each named file's content, as one value per file.
///
/// Without an encoding the content stays bytes — spec §12.1 forbids guessing — and with one it
/// is decoded, or refused with the reason. Only UTF-8 is decoded here: a transcoding table is
/// a dependency this build does not carry, and a named encoding it cannot honour is refused
/// rather than approximated.
async fn read_content(roots: Vec<PathBuf>, encoding: Option<String>, sink: StreamSink) {
    for path in roots {
        let outcome = match tokio::fs::read(&path).await {
            Ok(data) => decode(data, encoding.as_deref(), &path),
            Err(error) => Err(crate::common::io_error(&error, &path)),
        };
        let delivered = match outcome {
            Ok(value) => sink.send(value).await.is_ok(),
            Err(error) => sink.fail(error).await.is_ok(),
        };
        if !delivered {
            return;
        }
    }
}

fn decode(data: Vec<u8>, encoding: Option<&str>, path: &Path) -> Result<Value, ErrorValue> {
    match encoding {
        None => Ok(Value::Bytes(data.into())),
        Some(name) if name.eq_ignore_ascii_case("utf-8") || name.eq_ignore_ascii_case("utf8") => {
            String::from_utf8(data).map(Value::from).map_err(|error| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!(
                        "{}: not valid UTF-8 at byte {}",
                        path.display(),
                        error.utf8_error().valid_up_to()
                    ),
                )
                .with_target(ono_value::ValueRef::path(path))
                .with_help("read it without `--encoding` to get the bytes as they are")
            })
        }
        Some(other) => Err(ErrorValue::new(
            ErrorCode::ProviderUnsupported,
            format!("{PROVIDER_ID} decodes utf-8 only, not `{other}`"),
        )
        .with_help("read the bytes and decode them downstream")),
    }
}

/// `tail file`: the last `lines` existing lines, then — while `follow` — every line appended
/// afterwards, one `string` per line without its terminator (ADR-0083 §3).
///
/// The follow polls the file by name every 100 ms, so a file replaced under the tail (log
/// rotation) is picked up at its new inode on the next poll; a file that shrinks is read again
/// from its start. Polling is explicit here because spec §18.2 asks that it be.
async fn tail_lines(path: PathBuf, lines: usize, follow: bool, sink: StreamSink) {
    use std::io::{Read as _, Seek as _, SeekFrom};

    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => {
            let _ = sink.fail(crate::common::io_error(&error, &path)).await;
            return;
        }
    };
    let mut existing = Vec::new();
    if let Err(error) = file.read_to_end(&mut existing) {
        let _ = sink.fail(crate::common::io_error(&error, &path)).await;
        return;
    }
    let mut offset = existing.len() as u64;
    // A last line without its newline is not a line yet: it stays pending, and the follow
    // completes it when the writer does.
    let complete = existing
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let mut pending: Vec<u8> = existing[complete..].to_vec();
    // Splitting `a\nb\n` gives `a`, `b` and a trailing empty piece that is no line.
    let mut last: Vec<&[u8]> = existing[..complete].split(|byte| *byte == b'\n').collect();
    last.pop();
    let skip = last.len().saturating_sub(lines);
    for line in last.into_iter().skip(skip) {
        if sink.send(line_value(line)).await.is_err() {
            return;
        }
    }
    if !follow {
        if !pending.is_empty() && lines > 0 && sink.send(line_value(&pending)).await.is_err() {
            return;
        }
        return;
    }

    // The kernel says when the file moved; the sweep is the fallback for a filesystem inotify
    // cannot watch, and the bound on how long a rotation can go unnoticed on a silent one
    // (ADR-0241).
    let mut changed = crate::file_watch::changes(&path);
    let sweep = if changed.is_some() {
        std::time::Duration::from_secs(1)
    } else {
        std::time::Duration::from_millis(100)
    };
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        match &mut changed {
            Some(signal) => {
                let _ = tokio::time::timeout(sweep, signal.recv()).await;
            }
            None => tokio::time::sleep(sweep).await,
        }
        if sink.is_cancelled() {
            return;
        }
        // Reopen by name: rotation replaces the file, and the old descriptor would follow the
        // renamed one forever.
        let (length, reopened) = match std::fs::metadata(&path) {
            Ok(metadata) => {
                let same = std::os::unix::fs::MetadataExt::ino(&metadata)
                    == file
                        .metadata()
                        .map(|m| std::os::unix::fs::MetadataExt::ino(&m))
                        .unwrap_or(0);
                (metadata.len(), !same)
            }
            Err(_) => continue,
        };
        if reopened || length < offset {
            match std::fs::File::open(&path) {
                Ok(fresh) => file = fresh,
                Err(_) => continue,
            }
            offset = 0;
            pending.clear();
        }
        if length == offset {
            continue;
        }
        if file.seek(SeekFrom::Start(offset)).is_err() {
            continue;
        }
        loop {
            let read = match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) => {
                    let _ = sink.fail(crate::common::io_error(&error, &path)).await;
                    return;
                }
            };
            offset += read as u64;
            pending.extend_from_slice(&buffer[..read]);
            while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = pending.drain(..=end).collect();
                if sink
                    .send(line_value(&line[..line.len() - 1]))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

/// A line as a string, or as bytes when it is not UTF-8 (spec §12.2: never lost).
fn line_value(line: &[u8]) -> Value {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    match std::str::from_utf8(line) {
        Ok(text) => Value::string(text),
        Err(_) => Value::Bytes(line.to_vec().into()),
    }
}

#[async_trait::async_trait]
impl Provider for FileProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["file", "dir"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        schemas::require(&schemas::file_id()).into_iter().collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("file.list", Risk::Read),
            Capability::new("file.find", Risk::Read),
            Capability::new("file.read", Risk::Read),
            Capability::new("file.write", Risk::Mutate),
            Capability::new("file.copy", Risk::Mutate),
            Capability::new("file.move", Risk::Mutate),
            Capability::new("file.remove", Risk::Destructive),
            Capability::new("file.set", Risk::Mutate),
            Capability::new("file.open", Risk::Mutate),
            Capability::new("file.watch", Risk::Observe),
            Capability::new("dir.list", Risk::Read),
        ]
    }

    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let describer = Describer {
            schema: schemas::require(&schemas::file_id())?,
            user_schema: schemas::require(&schemas::user_id())?,
            group_schema: schemas::require(&schemas::group_id())?,
            accounts: Arc::clone(&self.accounts),
        };
        let request = Request::from(query);
        if query.verb() == "tail" {
            let lines = query
                .option_value("lines")
                .and_then(|value| value.as_int().ok())
                .and_then(|lines| usize::try_from(lines).ok())
                .unwrap_or(10);
            let follow = !matches!(query.option_value("follow"), Some(Value::Bool(false)));
            let path = request.roots.into_iter().next().unwrap_or_default();
            return Ok(ValueStream::spawn(
                PipelineConfig::new(),
                if follow {
                    Boundedness::Unbounded
                } else {
                    Boundedness::Bounded
                },
                move |sink| async move { tail_lines(path, lines, follow, sink).await },
            ));
        }
        if query.verb() == "read" {
            let encoding = query
                .option_value("encoding")
                .and_then(|value| value.as_str().ok())
                .map(str::to_owned);
            return Ok(ValueStream::spawn(
                PipelineConfig::new(),
                Boundedness::Bounded,
                move |sink| async move { read_content(request.roots, encoding, sink).await },
            ));
        }
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move { walk(request, describer, sink).await },
        ))
    }

    /// `watch file` and `tail file`, through inotify (spec §18.2, ADR-0235).
    ///
    /// The kernel says what changed under a path; the provider says what the changed entry now
    /// is, with the same describer a listing uses. A path this user may not watch — or a kernel
    /// with no inotify at all — refuses here, and the watch runtime then polls instead: the
    /// answer is the same either way, and only its latency and its `source` differ.
    fn subscribe(&self, query: &Query) -> Result<EventStream, ErrorValue> {
        let describer = Describer {
            schema: schemas::require(&schemas::file_id())?,
            user_schema: schemas::require(&schemas::user_id())?,
            group_schema: schemas::require(&schemas::group_id())?,
            accounts: Arc::clone(&self.accounts),
        };
        let request = Request::from(query);
        let root = request.roots.first().cloned().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{PROVIDER_ID} watches one path at a time"),
            )
        })?;
        // A walk over several roots, a glob or a filtered `find` is not a watch: the kernel
        // answers about a place, and the runtime's poll is the honest answer for the rest.
        if request.roots.len() > 1 || request.name.is_some() || request.kind.is_some() {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{PROVIDER_ID} watches one path, not a filtered walk"),
            ));
        }
        crate::file_watch::watch(
            crate::file_watch::WatchRequest {
                contents: request.list_contents,
                recursive: request.recursive,
                hidden: request.include_hidden,
                root,
            },
            describer,
        )
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let Selector::Field { name, value } = selector else {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{PROVIDER_ID} resolves a file by its path"),
            ));
        };
        if name != "path" {
            return Ok(Vec::new());
        }
        let path = match value {
            Value::Path(path) => path.to_path_buf(),
            Value::String(text) => PathBuf::from(text.as_ref()),
            other => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("a path is a path or a string, not {}", other.type_name()),
                ));
            }
        };
        let describer = Describer {
            schema: schemas::require(&schemas::file_id())?,
            user_schema: schemas::require(&schemas::user_id())?,
            group_schema: schemas::require(&schemas::group_id())?,
            accounts: Arc::clone(&self.accounts),
        };
        let record = describer
            .describe(AT_FDCWD, path.as_os_str(), path.clone(), false)
            .await?;
        Ok(ObjectRef::of(&record).into_iter().collect())
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        crate::file_mutations::act(self, action).await
    }
}
