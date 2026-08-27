//! The mutations of the `file` and `dir` targets: `write`, `copy`, `move`, `remove`, `set`,
//! `open` (spec §9.1, ADR-0082).
//!
//! Every action names its file by path — the identity of `ono.file/1` is `(device, inode)`,
//! but every call the filesystem offers takes a path, and ADR-0082 §1 has the shell hand the
//! path over either as the object's own value or as the action's `source`. What happened is
//! answered per target as an [`ActionOutcome`]: an attempt that failed is a `failed` outcome
//! carrying the structured error, never an `Err`, so a bulk keeps its other rows (spec §16.5).

use std::io::Write as _;
use std::path::{Path, PathBuf};

use ono_core::ErrorCode;
use ono_provider_api::{Action, ActionOutcome};
use ono_value::{ErrorValue, Value, ValueRef};

use crate::common::io_error;
use crate::file::{FileProvider, PROVIDER_ID};

/// Performs `action` on the file it names.
///
/// # Errors
///
/// `provider.unsupported` for an operation this provider does not implement, and
/// `type.mismatch` when the object carries no path to act on. Everything the filesystem
/// refuses is a `failed` outcome, not an error.
pub(crate) async fn act(
    provider: &FileProvider,
    action: &Action,
) -> Result<ActionOutcome, ErrorValue> {
    let path = path_of(action)?;
    match action.operation() {
        "write" => Ok(write(action, &path)),
        "copy" => Ok(copy(action, &path)),
        "move" => Ok(relocate(action, &path)),
        "remove" => Ok(remove(action, &path)),
        "set" => Ok(set(provider, action, &path).await),
        "open" => Ok(open(action, &path)),
        other => Err(ErrorValue::new(
            ErrorCode::ProviderUnsupported,
            format!("{PROVIDER_ID} has no operation `{other}`"),
        )),
    }
}

/// The path the action is about: the object's own value when a `path` selector named it, or
/// the provenance source of the record it came from (ADR-0082 §1, §4).
fn path_of(action: &Action) -> Result<PathBuf, ErrorValue> {
    if let Some(Value::Path(path)) = action.target().values().first() {
        return Ok(path.to_path_buf());
    }
    action.source().map(PathBuf::from).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            "a file is acted on by its path, and this object carries none",
        )
        .with_help("name the path, or pipe the File records `get file` produced")
    })
}

fn flag(action: &Action, name: &str) -> bool {
    matches!(action.argument(name), Some(Value::Bool(true)))
}

/// A refusal this provider makes before touching anything, as the target's failed outcome.
fn refused(action: &Action, code: ErrorCode, path: &Path, message: String) -> ActionOutcome {
    ActionOutcome::failed(
        action,
        ErrorValue::new(code, message).with_target(ValueRef::path(path)),
    )
}

/// `write file`: the pipeline's content onto `path`, under the contract's overwrite policy.
///
/// `--create` (default `true`) creates a missing file; an existing one is replaced only with
/// `--overwrite` and extended only with `--append` — without either, it is left exactly as it
/// was and the outcome is `io.already_exists` (E0303).
fn write(action: &Action, path: &Path) -> ActionOutcome {
    let content: Vec<u8> = match action.argument("content") {
        Some(Value::Bytes(raw)) => raw.to_vec(),
        Some(Value::String(text)) => text.as_bytes().to_vec(),
        Some(other) => {
            return refused(
                action,
                ErrorCode::TypeMismatch,
                path,
                format!(
                    "`write file` writes bytes or text, not a {}",
                    other.type_name()
                ),
            );
        }
        None => Vec::new(),
    };
    let append = flag(action, "append");
    let overwrite = flag(action, "overwrite");
    let create = !matches!(action.argument("create"), Some(Value::Bool(false)));
    let exists = path.symlink_metadata().is_ok();

    if !exists && !create {
        return refused(
            action,
            ErrorCode::IoNotFound,
            path,
            format!(
                "{}: does not exist, and `--create false` was written",
                path.display()
            ),
        );
    }
    if exists && !append && !overwrite {
        return refused(
            action,
            ErrorCode::IoAlreadyExists,
            path,
            format!("{}: already exists", path.display()),
        );
    }
    if action.is_dry_run() {
        let what = if append { "append to" } else { "write" };
        return ActionOutcome::skipped(
            action,
            format!("would {what} {} ({} bytes)", path.display(), content.len()),
        );
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(create);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    let written = options
        .open(path)
        .and_then(|mut file| file.write_all(&content).and_then(|()| file.flush()));
    match written {
        Ok(()) => ActionOutcome::succeeded(action, true),
        Err(error) => ActionOutcome::failed(action, io_error(&error, path)),
    }
}

/// `copy file <source> <destination>`: a file, or a tree with `--recursive`.
///
/// An existing destination is replaced only with `--overwrite`; without it the outcome is
/// `io.already_exists` and nothing is touched. `--preserve` keeps the mode, the timestamps and
/// — where permitted — the ownership of every copied entry.
fn copy(action: &Action, source: &Path) -> ActionOutcome {
    let Some(destination) = destination_of(action) else {
        return refused(
            action,
            ErrorCode::TypeMismatch,
            source,
            "`copy file` needs a destination".to_owned(),
        );
    };
    let metadata = match source.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) => return ActionOutcome::failed(action, io_error(&error, source)),
    };
    let recursive = flag(action, "recursive");
    if metadata.is_dir() && !recursive {
        return refused(
            action,
            ErrorCode::SafetyPolicyDenied,
            source,
            format!(
                "{}: is a directory; write `--recursive` to copy its contents",
                source.display()
            ),
        );
    }
    if let Some(outcome) = destination_taken(action, &destination) {
        return outcome;
    }
    if action.is_dry_run() {
        return ActionOutcome::skipped(
            action,
            format!(
                "would copy {} to {}",
                source.display(),
                destination.display()
            ),
        );
    }
    let preserve = flag(action, "preserve");
    match copy_entry(source, &destination, &metadata, preserve) {
        Ok(()) => ActionOutcome::succeeded(action, true),
        Err((path, error)) => ActionOutcome::failed(action, io_error(&error, &path)),
    }
}

/// `move file <source> <destination>`: a rename, or a copy and a removal across filesystems.
fn relocate(action: &Action, source: &Path) -> ActionOutcome {
    let Some(destination) = destination_of(action) else {
        return refused(
            action,
            ErrorCode::TypeMismatch,
            source,
            "`move file` needs a destination".to_owned(),
        );
    };
    let metadata = match source.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) => return ActionOutcome::failed(action, io_error(&error, source)),
    };
    if let Some(outcome) = destination_taken(action, &destination) {
        return outcome;
    }
    if action.is_dry_run() {
        return ActionOutcome::skipped(
            action,
            format!(
                "would move {} to {}",
                source.display(),
                destination.display()
            ),
        );
    }
    match std::fs::rename(source, &destination) {
        Ok(()) => ActionOutcome::succeeded(action, true),
        // Another filesystem: the kernel cannot rename across it, so the move is a copy that
        // keeps everything a rename would have kept, then the removal of the source.
        Err(error) if error.raw_os_error() == Some(nix::errno::Errno::EXDEV as i32) => {
            let copied = copy_entry(source, &destination, &metadata, true).and_then(|()| {
                remove_entry(source, &metadata, true).map_err(|e| (source.to_path_buf(), e))
            });
            match copied {
                Ok(()) => ActionOutcome::succeeded(action, true),
                Err((path, error)) => ActionOutcome::failed(action, io_error(&error, &path)),
            }
        }
        Err(error) => ActionOutcome::failed(action, io_error(&error, source)),
    }
}

/// `remove file` and `remove dir`: the entry, or the tree with `--recursive`.
///
/// A directory needs `--recursive` unless it is empty and the command is `remove dir`; a
/// refusal is made before anything is unlinked, so refused means untouched.
fn remove(action: &Action, path: &Path) -> ActionOutcome {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) => return ActionOutcome::failed(action, io_error(&error, path)),
    };
    let recursive = flag(action, "recursive");
    let directory_command = action.target_name() == "dir";
    if metadata.is_dir() && !recursive && !directory_command {
        return refused(
            action,
            ErrorCode::SafetyPolicyDenied,
            path,
            format!(
                "{}: is a directory; write `--recursive`, or `remove dir`",
                path.display()
            ),
        );
    }
    if action.is_dry_run() {
        return ActionOutcome::skipped(action, format!("would remove {}", path.display()));
    }
    match remove_entry(path, &metadata, recursive) {
        Ok(()) => ActionOutcome::succeeded(action, true),
        Err(error) => ActionOutcome::failed(action, io_error(&error, path)),
    }
}

/// `set file` / `set dir`: `--mode`, `--owner`, `--group`, optionally `--recursive`.
///
/// `changed` is honest: asking for the state that already holds is a success that changed
/// nothing (action-result.v1).
async fn set(provider: &FileProvider, action: &Action, path: &Path) -> ActionOutcome {
    let mode = match action.argument("mode") {
        None => None,
        Some(value) => match parse_mode(value) {
            Ok(mode) => Some(mode),
            Err(message) => return refused(action, ErrorCode::TypeMismatch, path, message),
        },
    };
    let owner = match action.argument("owner") {
        None => None,
        Some(value) => match provider.uid_of(value).await {
            Ok(uid) => Some(uid),
            Err(error) => return ActionOutcome::failed(action, error),
        },
    };
    let group = match action.argument("group") {
        None => None,
        Some(value) => match provider.gid_of(value).await {
            Ok(gid) => Some(gid),
            Err(error) => return ActionOutcome::failed(action, error),
        },
    };
    if mode.is_none() && owner.is_none() && group.is_none() {
        return ActionOutcome::skipped(action, "nothing to set: no --mode, --owner or --group");
    }
    let request = Attributes { mode, owner, group };
    let recursive = flag(action, "recursive");
    if action.is_dry_run() {
        return ActionOutcome::skipped(
            action,
            format!("would set {} on {}", request, path.display()),
        );
    }
    match apply_attributes(path, &request, recursive) {
        Ok(changed) => ActionOutcome::succeeded(action, changed),
        Err((path, error)) => ActionOutcome::failed(action, io_error(&error, &path)),
    }
}

/// `open file`: the handler named by `--with`, or `xdg-open`, with the path as its argument.
///
/// The handler's exit status is the outcome; a handler that cannot be started is
/// `provider.unavailable` naming it, because nothing on this host can open the file.
fn open(action: &Action, path: &Path) -> ActionOutcome {
    let handler = action
        .argument("with")
        .and_then(|value| value.as_str().ok())
        .map_or_else(|| "xdg-open".to_owned(), str::to_owned);
    if let Err(error) = path.symlink_metadata() {
        return ActionOutcome::failed(action, io_error(&error, path));
    }
    if action.is_dry_run() {
        return ActionOutcome::skipped(
            action,
            format!("would open {} with {handler}", path.display()),
        );
    }
    match std::process::Command::new(&handler)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => ActionOutcome::succeeded(action, true),
        Ok(status) => ActionOutcome::failed(
            action,
            ErrorValue::new(
                ErrorCode::ExternalExitNonzero,
                format!("{handler} exited with {status} opening {}", path.display()),
            )
            .with_target(ValueRef::path(path)),
        ),
        Err(error) => ActionOutcome::failed(
            action,
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("cannot start `{handler}`: {error}"),
            )
            .with_target(ValueRef::path(path))
            .with_help("name a handler with `--with <program>`"),
        ),
    }
}

// --- helpers --------------------------------------------------------------------------------

fn destination_of(action: &Action) -> Option<PathBuf> {
    match action.argument("destination")? {
        Value::Path(path) => Some(path.to_path_buf()),
        Value::String(text) => Some(PathBuf::from(text.as_ref())),
        _ => None,
    }
}

/// The `io.already_exists` refusal when the destination exists and `--overwrite` was not written.
fn destination_taken(action: &Action, destination: &Path) -> Option<ActionOutcome> {
    if destination.symlink_metadata().is_ok() && !flag(action, "overwrite") {
        return Some(refused(
            action,
            ErrorCode::IoAlreadyExists,
            destination,
            format!(
                "{}: already exists; write `--overwrite` to replace it",
                destination.display()
            ),
        ));
    }
    None
}

/// Copies one entry — a file, a symlink, or a whole directory — to `destination`.
///
/// The error names the path it happened at, which in a tree is rarely the root.
fn copy_entry(
    source: &Path,
    destination: &Path,
    metadata: &std::fs::Metadata,
    preserve: bool,
) -> Result<(), (PathBuf, std::io::Error)> {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        let target = std::fs::read_link(source).map_err(|e| (source.to_path_buf(), e))?;
        if destination.symlink_metadata().is_ok() {
            std::fs::remove_file(destination).map_err(|e| (destination.to_path_buf(), e))?;
        }
        std::os::unix::fs::symlink(&target, destination)
            .map_err(|e| (destination.to_path_buf(), e))?;
        return Ok(());
    }
    if file_type.is_dir() {
        match std::fs::create_dir(destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err((destination.to_path_buf(), error)),
        }
        let entries = std::fs::read_dir(source).map_err(|e| (source.to_path_buf(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| (source.to_path_buf(), e))?;
            let child = entry.path();
            let child_metadata = child.symlink_metadata().map_err(|e| (child.clone(), e))?;
            copy_entry(
                &child,
                &destination.join(entry.file_name()),
                &child_metadata,
                preserve,
            )?;
        }
    } else {
        std::fs::copy(source, destination).map_err(|e| (destination.to_path_buf(), e))?;
    }
    if preserve {
        preserve_attributes(destination, metadata).map_err(|e| (destination.to_path_buf(), e))?;
    }
    Ok(())
}

/// Mode, timestamps and — where permitted — ownership of `metadata`, onto `path`.
fn preserve_attributes(path: &Path, metadata: &std::fs::Metadata) -> std::io::Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(metadata.mode() & 0o7777),
    )?;
    let times = std::fs::FileTimes::new()
        .set_accessed(metadata.accessed()?)
        .set_modified(metadata.modified()?);
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_times(times))
        // A directory or a read-only file cannot be opened for writing; the timestamps are
        // then the one attribute "where permitted" does not reach.
        .or(Ok::<(), std::io::Error>(()))?;
    // Only root may give a file away; for everyone else the ownership stays the copier's, which
    // is what "where permitted" means.
    let _ = nix::unistd::chown(
        path,
        Some(nix::unistd::Uid::from_raw(metadata.uid())),
        Some(nix::unistd::Gid::from_raw(metadata.gid())),
    );
    Ok(())
}

/// Removes one entry: a file or symlink, an empty directory, or — with `recursive` — a tree.
fn remove_entry(path: &Path, metadata: &std::fs::Metadata, recursive: bool) -> std::io::Result<()> {
    if metadata.is_dir() {
        if recursive {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_dir(path)
        }
    } else {
        std::fs::remove_file(path)
    }
}

/// What `set` was asked to change.
struct Attributes {
    mode: Option<u32>,
    owner: Option<u32>,
    group: Option<u32>,
}

impl std::fmt::Display for Attributes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if let Some(mode) = self.mode {
            parts.push(format!("mode {mode:04o}"));
        }
        if let Some(owner) = self.owner {
            parts.push(format!("owner {owner}"));
        }
        if let Some(group) = self.group {
            parts.push(format!("group {group}"));
        }
        f.write_str(&parts.join(", "))
    }
}

/// Four octal digits, as `ono.file/1` writes the mode; `755` is accepted as `0755`.
fn parse_mode(value: &Value) -> Result<u32, String> {
    let text = match value {
        Value::String(text) => text.to_string(),
        Value::Int(number) => number.to_string(),
        other => {
            return Err(format!(
                "a mode is four octal digits, not a {}",
                other.type_name()
            ));
        }
    };
    u32::from_str_radix(text.trim(), 8)
        .ok()
        .filter(|mode| *mode <= 0o7777)
        .ok_or_else(|| format!("`{text}` is not a mode: write four octal digits such as `0755`"))
}

/// Applies `request` to `path` — and to everything beneath it with `recursive` — reporting
/// whether anything actually changed.
fn apply_attributes(
    path: &Path,
    request: &Attributes,
    recursive: bool,
) -> Result<bool, (PathBuf, std::io::Error)> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let metadata = path
        .symlink_metadata()
        .map_err(|e| (path.to_path_buf(), e))?;
    let mut changed = false;
    if let Some(mode) = request.mode
        && metadata.mode() & 0o7777 != mode
    {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| (path.to_path_buf(), e))?;
        changed = true;
    }
    let owner = request.owner.filter(|uid| *uid != metadata.uid());
    let group = request.group.filter(|gid| *gid != metadata.gid());
    if owner.is_some() || group.is_some() {
        nix::unistd::chown(
            path,
            owner.map(nix::unistd::Uid::from_raw),
            group.map(nix::unistd::Gid::from_raw),
        )
        .map_err(|errno| (path.to_path_buf(), std::io::Error::from(errno)))?;
        changed = true;
    }
    if recursive && metadata.is_dir() {
        let entries = std::fs::read_dir(path).map_err(|e| (path.to_path_buf(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| (path.to_path_buf(), e))?;
            changed |= apply_attributes(&entry.path(), request, true)?;
        }
    }
    Ok(changed)
}
