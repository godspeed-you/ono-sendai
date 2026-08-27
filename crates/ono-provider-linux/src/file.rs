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
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
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
}

/// What a query asked the walk to do.
#[derive(Debug, Clone)]
struct Request {
    root: PathBuf,
    list_contents: bool,
    recursive: bool,
    follow_symlinks: bool,
    include_hidden: bool,
    max_depth: usize,
    limit: usize,
}

impl Request {
    fn from(query: &Query) -> Self {
        let mut named_root = false;
        let root = query
            .selectors()
            .iter()
            .find_map(|selector| match selector {
                Selector::Field { name, value } if name == "path" || name == "root" => {
                    named_root = name == "root";
                    match value {
                        Value::Path(path) => Some(path.to_path_buf()),
                        Value::String(text) => Some(PathBuf::from(text.as_ref())),
                        _ => None,
                    }
                }
                _ => None,
            })
            .unwrap_or_else(|| PathBuf::from("."));
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
        Self {
            root,
            list_contents: listing,
            recursive,
            follow_symlinks: query.flag("follow-symlinks"),
            // `docs/spec/commands/file.yaml` gives `--all` to `get dir` only, so a `file` walk
            // reports every entry it reaches and a `dir` listing hides dot entries by default.
            include_hidden: !listing || query.flag("all"),
            max_depth: depth.unwrap_or(if recursive { usize::MAX } else { 0 }),
            limit: query.max().unwrap_or(usize::MAX),
        }
    }
}

/// Everything describing one entry needs.
#[derive(Clone)]
struct Describer {
    schema: Arc<Schema>,
    user_schema: Arc<Schema>,
    group_schema: Arc<Schema>,
    accounts: Arc<dyn Accounts>,
}

impl Describer {
    /// Describes the entry `lookup` names relative to `dirfd`, recorded under the logical
    /// `path` it was reached by.
    async fn describe<Fd: AsFd>(
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

/// Emits the walk, breadth first, sending each entry as it is stated.
async fn walk(request: Request, describer: Describer, sink: StreamSink) {
    let mut sent = 0usize;

    if !request.list_contents {
        match describer
            .describe(
                AT_FDCWD,
                request.root.as_os_str(),
                request.root.clone(),
                request.follow_symlinks,
            )
            .await
        {
            Ok(record) => {
                if sink.send(record.into_value()).await.is_err() {
                    return;
                }
                sent += 1;
            }
            Err(error) => {
                let _ = sink.fail(error).await;
                return;
            }
        }
        if !request.recursive {
            return;
        }
    }

    let root = match open_root(&request.root) {
        Ok(fd) => fd,
        Err(error) => {
            // A plain `get file <path>` on a non-directory has already reported the entry; the
            // failure to open it as a directory is not a second answer.
            if request.list_contents {
                let _ = sink.fail(error).await;
            }
            return;
        }
    };

    // The frontier holds *paths relative to the held root*, never descriptors: a tree wider
    // than the descriptor table was unwalkable when every pending directory kept one open
    // (ADR-0015, F11). Each directory is re-opened from the root when its turn comes, through
    // `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` — so the walk still cannot be
    // redirected out of the tree by a swapped component (T14): a symlink appearing anywhere in
    // the recorded path makes the open fail loudly instead of following it. At most two
    // descriptors are ever held: the root, and the directory being read.
    let mut queue: VecDeque<(PathBuf, PathBuf, usize)> = VecDeque::new();
    queue.push_back((PathBuf::new(), request.root.clone(), 0));

    while let Some((relative, path, depth)) = queue.pop_front() {
        let fd = if relative.as_os_str().is_empty() {
            match root.try_clone() {
                Ok(fd) => fd,
                Err(error) => {
                    let _ = sink
                        .fail(errno_error(
                            Errno::from_raw(error.raw_os_error().unwrap_or(0)),
                            &path,
                        ))
                        .await;
                    return;
                }
            }
        } else {
            match reopen_beneath(&root, &relative) {
                Ok(fd) => fd,
                // The directory vanished or a component stopped being a plain directory between
                // the listing and this turn — the swap of ADR-0015 T14. Reported, not followed.
                Err(errno) => {
                    if sink.fail(errno_error(errno, &path)).await.is_err() {
                        return;
                    }
                    continue;
                }
            }
        };
        let names = match entry_names(&fd, &path) {
            Ok(names) => names,
            Err(error) => {
                if sink.fail(error).await.is_err() {
                    return;
                }
                continue;
            }
        };
        for name in names {
            if sent >= request.limit {
                return;
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
                    if sink.send(record.into_value()).await.is_err() {
                        return;
                    }
                    sent += 1;
                    kind
                }
                Err(error) => {
                    if sink.fail(error).await.is_err() {
                        return;
                    }
                    continue;
                }
            };
            if kind == "dir" && depth < request.max_depth {
                queue.push_back((relative.join(&name), child_path, depth + 1));
            }
        }
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
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move { walk(request, describer, sink).await },
        ))
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
}
