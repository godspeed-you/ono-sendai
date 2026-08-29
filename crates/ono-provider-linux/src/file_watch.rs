//! `watch file` through inotify: the kernel tells the shell, rather than the shell asking again.
//!
//! ADR-0034 left every watch in this build polling; ADR-0078 built `watch file` on that poll, so
//! a file created a moment after a tick waited a whole interval to be reported and a `tail` of a
//! busy log lagged by up to its own interval. ADR-0235 replaces the asking with `inotify(7)`,
//! which is what the kernel offers for exactly this question.
//!
//! What lives here is the plumbing: the descriptors, the reader thread and the cache that lets a
//! deletion still name the object that was deleted. What an entry *is* remains
//! [`super::file::Describer`]'s answer, so a watched entry and a listed one are the same record.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify, WatchDescriptor};
use ono_core::ErrorCode;
use ono_pipeline::PipelineConfig;
use ono_provider_api::{EventStream, ObjectEvent};
use ono_value::{ErrorValue, RecordValue};

use crate::file::Describer;

/// How long the reader waits for the kernel before coming up for air.
///
/// It is not a polling interval: nothing is re-read when it expires. It is only how often the
/// thread notices that nobody is listening any more, so a cancelled watch stops within it.
const REAP: u16 = 200;

/// The events worth waking for on a directory.
///
/// `IN_CLOSE_WRITE` rather than `IN_MODIFY` alone is what makes one `echo >> file` one event
/// instead of several, and `IN_ATTRIB` is what makes a `chmod` a change: `ono.file/1` carries the
/// mode, so a mode that moved is an object that changed.
fn directory_flags() -> AddWatchFlags {
    AddWatchFlags::IN_CREATE
        | AddWatchFlags::IN_DELETE
        | AddWatchFlags::IN_MOVED_TO
        | AddWatchFlags::IN_MOVED_FROM
        | AddWatchFlags::IN_CLOSE_WRITE
        | AddWatchFlags::IN_ATTRIB
}

/// What is being watched, in the terms the file provider's query already resolved.
#[derive(Debug, Clone)]
pub(crate) struct WatchRequest {
    /// The path named on the command line.
    pub(crate) root: PathBuf,
    /// Whether the watch is of a directory's contents rather than of the one entry.
    pub(crate) contents: bool,
    /// Whether subdirectories are watched too.
    pub(crate) recursive: bool,
    /// Whether dot entries take part.
    pub(crate) hidden: bool,
}

/// What the reader thread saw, in the terms the describer can answer.
#[derive(Debug)]
enum Notice {
    /// The entry exists now, and may or may not have existed before.
    Present(PathBuf),
    /// The entry is gone.
    Gone(PathBuf),
}

/// Opens an inotify subscription for `request`, or reports why the kernel would not give one.
///
/// # Errors
///
/// `provider.unavailable` when inotify is not available or the path cannot be watched — a
/// sandbox without it, or a directory this user may not read. The watch runtime falls back to
/// polling on any error, so a refusal here costs the user latency and never the answer.
pub(crate) fn watch(
    request: WatchRequest,
    describer: Describer,
) -> Result<EventStream, ErrorValue> {
    // A watch of one entry is a watch of the directory holding it, filtered by name: only that
    // way does the creation of a file that does not exist yet arrive at all.
    let (directory, only) = if request.contents {
        (request.root.clone(), None)
    } else {
        let parent = request
            .root
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let name = request
            .root
            .file_name()
            .map(OsStr::to_os_string)
            .ok_or_else(|| {
                ErrorValue::new(
                    ErrorCode::ProviderUnavailable,
                    format!("`{}` names no entry to watch", request.root.display()),
                )
            })?;
        (parent, Some(name))
    };

    let inotify =
        Inotify::init(InitFlags::IN_NONBLOCK | InitFlags::IN_CLOEXEC).map_err(|errno| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("inotify could not be opened: {errno}"),
            )
            .with_help("the kernel offers no inotify here, so `watch file` falls back to polling")
        })?;
    let mut watched: HashMap<WatchDescriptor, PathBuf> = HashMap::new();
    add_watch(&inotify, &mut watched, &directory)?;
    if request.contents && request.recursive {
        for child in subdirectories(&directory, request.hidden) {
            // A directory that vanished between the walk and the watch is not a failure of the
            // watch: it is one fewer thing to watch.
            let _ = add_watch(&inotify, &mut watched, &child);
        }
    }

    let recursive = request.contents && request.recursive;
    let hidden = request.hidden;
    Ok(EventStream::spawn(
        PipelineConfig::new(),
        move |sink| async move {
            let (sender, mut receiver) = tokio::sync::mpsc::channel::<Notice>(256);
            std::thread::spawn(move || {
                read_events(
                    &inotify,
                    watched,
                    recursive,
                    hidden,
                    only.as_deref(),
                    &sender,
                );
            });

            // What each watched path was, so a deletion can still say which object went away. The
            // entries that were already there are read once at the start: a file removed an hour
            // after the watch began is still a file the watch has to be able to name.
            let mut known: BTreeMap<PathBuf, Arc<RecordValue>> = BTreeMap::new();
            prime(&describer, &request, &mut known).await;
            while let Some(notice) = receiver.recv().await {
                let event = match notice {
                    Notice::Present(path) => {
                        let Ok(record) = describer
                            .describe(nix::fcntl::AT_FDCWD, path.as_os_str(), path.clone(), false)
                            .await
                        else {
                            // Created and gone again before it could be read: the removal is on its
                            // way through the same queue.
                            continue;
                        };
                        let record = Arc::new(record);
                        match known.insert(path, Arc::clone(&record)) {
                            Some(previous) if previous == record => continue,
                            Some(_) => ObjectEvent::changed(&record, ["modified"]),
                            None => ObjectEvent::added(&record),
                        }
                    }
                    Notice::Gone(path) => match known.remove(&path) {
                        Some(record) => ObjectEvent::removed(&record),
                        // Never seen: the runtime's own snapshot may know it, and it reconciles a
                        // removal it cannot place by dropping it.
                        None => continue,
                    },
                };
                if sink.send(event).await.is_err() {
                    return;
                }
            }
        },
    ))
}

/// Reads the entries that exist when the watch opens, so a later removal can name them.
///
/// The watch reports what changes; the kernel says only that `<name>` under `<directory>` is
/// gone, and a `removed` event has to carry the object that went. Nothing is emitted here — this
/// is memory, not an answer.
async fn prime(
    describer: &Describer,
    request: &WatchRequest,
    known: &mut BTreeMap<PathBuf, Arc<RecordValue>>,
) {
    let mut paths = Vec::new();
    if request.contents {
        let mut directories = vec![request.root.clone()];
        if request.recursive {
            directories.extend(subdirectories(&request.root, request.hidden));
        }
        for directory in directories {
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                if !request.hidden && entry.file_name().as_encoded_bytes().first() == Some(&b'.') {
                    continue;
                }
                paths.push(entry.path());
            }
        }
    } else {
        paths.push(request.root.clone());
    }
    for path in paths {
        if let Ok(record) = describer
            .describe(nix::fcntl::AT_FDCWD, path.as_os_str(), path.clone(), false)
            .await
        {
            known.insert(path, Arc::new(record));
        }
    }
}

/// Adds one directory to the watch, recording which descriptor stands for it.
fn add_watch(
    inotify: &Inotify,
    watched: &mut HashMap<WatchDescriptor, PathBuf>,
    directory: &Path,
) -> Result<(), ErrorValue> {
    let descriptor = inotify
        .add_watch(directory, directory_flags())
        .map_err(|errno| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("{} cannot be watched: {errno}", directory.display()),
            )
        })?;
    watched.insert(descriptor, directory.to_path_buf());
    Ok(())
}

/// Every directory beneath `root`, so a recursive watch reaches them all.
fn subdirectories(root: &Path, hidden: bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !hidden && name.as_encoded_bytes().first() == Some(&b'.') {
                continue;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                found.push(path.clone());
                pending.push(path);
            }
        }
    }
    found
}

/// The reader thread: waits for the kernel, translates what it says, and stops when nobody is
/// listening any more.
fn read_events(
    inotify: &Inotify,
    mut watched: HashMap<WatchDescriptor, PathBuf>,
    recursive: bool,
    hidden: bool,
    only: Option<&OsStr>,
    sender: &tokio::sync::mpsc::Sender<Notice>,
) {
    use nix::poll::{PollFd, PollFlags, PollTimeout};
    use std::os::fd::AsFd;

    loop {
        if sender.is_closed() {
            return;
        }
        let mut fds = [PollFd::new(inotify.as_fd(), PollFlags::POLLIN)];
        match nix::poll::poll(&mut fds, PollTimeout::from(REAP)) {
            Ok(0) => continue,
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(_) => return,
        }
        let Ok(events) = inotify.read_events() else {
            continue;
        };
        for event in events {
            let Some(directory) = watched.get(&event.wd).cloned() else {
                continue;
            };
            let Some(name) = event.name.as_ref() else {
                continue;
            };
            if only.is_some_and(|wanted| wanted != name.as_os_str()) {
                continue;
            }
            if !hidden && name.as_encoded_bytes().first() == Some(&b'.') {
                continue;
            }
            let path = directory.join(name);
            let gone = event
                .mask
                .intersects(AddWatchFlags::IN_DELETE | AddWatchFlags::IN_MOVED_FROM);
            // A directory that appears under a recursive watch is watched from now on, or the
            // files created inside it would never be reported.
            if recursive
                && !gone
                && event.mask.contains(AddWatchFlags::IN_ISDIR)
                && let Ok(descriptor) = inotify.add_watch(&path, directory_flags())
            {
                watched.insert(descriptor, path.clone());
            }
            let notice = if gone {
                Notice::Gone(path)
            } else {
                Notice::Present(path)
            };
            if sender.blocking_send(notice).is_err() {
                return;
            }
        }
    }
}
