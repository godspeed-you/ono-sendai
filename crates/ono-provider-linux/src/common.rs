//! Helpers every Linux provider needs: provenance, error translation and object references.

use std::io;
use std::path::Path;
use std::sync::Arc;

use jiff::Timestamp;
use nix::errno::Errno;
use ono_core::ErrorCode;
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value, ValueRef};

/// Provenance for a record this crate observed locally, naming the exact files it was read from.
///
/// Spec §25.2 makes the source the thing that lets `inspect` be trusted, which is why the
/// callers spell out `/proc/4419/stat + /proc/4419/status` rather than `procfs`.
pub(crate) fn provenance(provider: &str, schema: &SchemaId, source: &str) -> Provenance {
    Provenance::local(provider, schema.clone())
        .from_source(source)
        .observed_at(Timestamp::now())
}

/// The structured form of a failed filesystem read (spec §16.1, §43).
///
/// The errno decides, not [`io::ErrorKind`]: the kernel answers `ESRCH` for a read of
/// `/proc/<pid>/…` in the instant between a process leaving the listing and its directory
/// disappearing, and `io::ErrorKind` has no name for that condition, so a read that only says
/// "the process is gone" would otherwise be reported as the provider being unavailable
/// (ADR-0230).
pub(crate) fn io_error(error: &io::Error, what: &Path) -> ErrorValue {
    let code = match error.raw_os_error() {
        Some(raw) => condition_of(Errno::from_raw(raw)),
        None => match error.kind() {
            io::ErrorKind::NotFound => ErrorCode::IoNotFound,
            io::ErrorKind::PermissionDenied => ErrorCode::IoPermissionDenied,
            io::ErrorKind::AlreadyExists => ErrorCode::IoAlreadyExists,
            io::ErrorKind::NotADirectory => ErrorCode::IoNotDirectory,
            _ => ErrorCode::ProviderUnavailable,
        },
    };
    ErrorValue::new(code, format!("{}: {error}", what.display()))
        .with_target(ValueRef::path(what))
        .with_retryable(matches!(code, ErrorCode::ProviderUnavailable))
}

/// The structured form of a failed syscall (spec §16.1, §43).
pub(crate) fn errno_error(errno: Errno, what: &Path) -> ErrorValue {
    let code = condition_of(errno);
    ErrorValue::new(code, format!("{}: {}", what.display(), errno.desc()))
        .with_target(ValueRef::path(what))
        .with_retryable(matches!(code, ErrorCode::ProviderUnavailable))
}

/// The condition an errno names, whether it arrived from a syscall or from a read (spec §43).
fn condition_of(errno: Errno) -> ErrorCode {
    match errno {
        Errno::EACCES | Errno::EPERM => ErrorCode::IoPermissionDenied,
        Errno::ENOENT | Errno::ESRCH => ErrorCode::IoNotFound,
        Errno::EEXIST => ErrorCode::IoAlreadyExists,
        Errno::ENOTDIR => ErrorCode::IoNotDirectory,
        _ => ErrorCode::ProviderUnavailable,
    }
}

/// A `ref<ono.user/1>` that keeps the numeric identity whether or not a name resolved.
///
/// Spec §23.6 requires exactly that: "represent unresolved IDs without discarding numeric
/// identity". The reference is a whole `ono.user/1` record rather than a bare number so that
/// `get process | select user.name` works without a second lookup.
pub(crate) fn user_ref(schema: &Arc<Schema>, uid: u32, name: Option<&str>) -> Value {
    reference(schema, "uid", i128::from(uid), name)
}

/// A `ref<ono.group/1>`, on the same terms as [`user_ref`].
pub(crate) fn group_ref(schema: &Arc<Schema>, gid: u32, name: Option<&str>) -> Value {
    reference(schema, "gid", i128::from(gid), name)
}

fn reference(schema: &Arc<Schema>, id_field: &str, id: i128, name: Option<&str>) -> Value {
    match build_reference(schema, id_field, id, name) {
        Ok(record) => record.into_value(),
        // Unreachable while the schema declares the fields above; carrying the contract bug as
        // data keeps the surrounding record intact rather than losing the whole observation.
        Err(error) => error.into_value(),
    }
}

fn build_reference(
    schema: &Arc<Schema>,
    id_field: &str,
    id: i128,
    name: Option<&str>,
) -> Result<RecordValue, ErrorValue> {
    let provenance = Provenance::local("linux.nss", schema.id().clone());
    let mut builder =
        RecordValue::builder(Arc::clone(schema), provenance).set(id_field, Value::Int(id))?;
    if let Some(name) = name {
        builder = builder.set("name", Value::string(name))?;
    }
    Ok(builder.build())
}

/// A timestamp from a seconds/nanoseconds pair, or `None` when the pair is out of range.
pub(crate) fn timestamp(seconds: i64, nanoseconds: i64) -> Option<Timestamp> {
    let nanoseconds = i32::try_from(nanoseconds).ok()?;
    Timestamp::new(seconds, nanoseconds).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_esrch_as_the_object_being_gone_when_a_procfs_read_fails() {
        let gone = io::Error::from_raw_os_error(Errno::ESRCH as i32);

        let translated = io_error(&gone, Path::new("/proc/44012/stat"));

        assert_eq!(
            translated.code(),
            ErrorCode::IoNotFound,
            "`ESRCH` from a procfs read is the process having exited, not the provider being \
             unavailable: the kernel answers it for the instant between a process leaving the \
             `/proc` listing and its directory disappearing"
        );
        assert!(
            translated.retryable() != Some(true),
            "a process that exited will not come back on a retry"
        );
    }

    #[test]
    fn should_translate_a_read_failure_the_same_way_a_syscall_failure_is_translated() {
        for errno in [Errno::ENOENT, Errno::ESRCH, Errno::EACCES, Errno::ENOTDIR] {
            let path = Path::new("/proc/1/stat");
            assert_eq!(
                io_error(&io::Error::from_raw_os_error(errno as i32), path).code(),
                errno_error(errno, path).code(),
                "a failed read and a failed syscall reporting {errno:?} describe the same \
                 condition and must carry the same code"
            );
        }
    }
}
