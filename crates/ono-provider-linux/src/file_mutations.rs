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
    let _ = provider;
    let path = path_of(action)?;
    match action.operation() {
        "write" => Ok(write(action, &path)),
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
