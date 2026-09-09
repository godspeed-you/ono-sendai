//! Extended attributes, read and written without following symlinks (spec v0.6 Appendix C.7).
//!
//! Appendix C.7 lists `xattrs`, `capabilities` and `SELinux labels` separately, and on Linux all
//! three are the same mechanism seen through different namespaces: `user.*`, `security.capability`
//! and `security.selinux`. Reading them is therefore one operation and *claiming* them is three,
//! which is why [`crate::coverage`] probes each namespace on its own rather than reporting one
//! optimistic answer.
//!
//! Every call is the `l` variant. A symlink's own attributes are the symlink's, and following it
//! to read the target's would be exactly the substitution §43.5 forbids.

use std::path::Path;

use ono_change_core::error::asset_create_failed;
use ono_value::ErrorValue;

use crate::PROVIDER_ID;

/// The largest attribute list and value this provider handles, matching the kernel's own bound.
const XATTR_LIMIT: usize = 64 * 1024;

/// Every extended attribute on `path`, in the order the kernel lists them.
///
/// A filesystem that does not support extended attributes has none, which is a fact about the
/// filesystem rather than a failure: the coverage report is where that becomes visible (C.7).
///
/// # Errors
///
/// `recovery.asset_create_failed` when the object carries more attribute data than this provider
/// can hold. §15.2's rule applies to metadata too — a copy that silently dropped an attribute
/// would be a copy that does not restore the file.
pub fn read_all(path: &Path) -> Result<Vec<(String, Vec<u8>)>, ErrorValue> {
    let mut names = vec![0_u8; XATTR_LIMIT];
    let length = match rustix::fs::llistxattr(path, &mut names[..]) {
        Ok(length) => length,
        Err(rustix::io::Errno::RANGE) => return Err(too_large(path, "the attribute list")),
        Err(_) => return Ok(Vec::new()),
    };
    let mut attributes = Vec::new();
    for name in names
        .get(..length)
        .unwrap_or_default()
        .split(|byte| *byte == 0)
    {
        let Ok(name) = std::str::from_utf8(name) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let mut value = vec![0_u8; XATTR_LIMIT];
        match rustix::fs::lgetxattr(path, name, &mut value[..]) {
            Ok(read) => {
                value.truncate(read);
                attributes.push((name.to_owned(), value));
            }
            Err(rustix::io::Errno::RANGE) => return Err(too_large(path, name)),
            Err(_) => {}
        }
    }
    Ok(attributes)
}

/// Writes `attributes` onto `path`, reporting the ones the filesystem or the privilege refused.
///
/// The refused names are returned rather than raised: Appendix C.7 requires missing metadata to be
/// visible, and a restore that put the bytes back and could not set a `security.*` attribute has
/// still put the bytes back. The caller decides what to say about the gap.
pub fn write_all(path: &Path, attributes: &[(String, Vec<u8>)]) -> Vec<String> {
    attributes
        .iter()
        .filter(|(name, value)| {
            rustix::fs::lsetxattr(path, name, value, rustix::fs::XattrFlags::empty()).is_err()
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// Whether `path`'s filesystem answers questions about `name` at all.
///
/// `ENODATA` is a supported namespace with nothing in it, and `ENOTSUP` is a filesystem that does
/// not carry the namespace. Telling those two apart is the whole point of probing rather than
/// assuming (Appendix C.7).
#[must_use]
pub fn namespace_supported(path: &Path, name: &str) -> bool {
    let mut value = [0_u8; 256];
    match rustix::fs::lgetxattr(path, name, &mut value[..]) {
        Ok(_) => true,
        Err(rustix::io::Errno::RANGE) => true,
        Err(rustix::io::Errno::NODATA) => true,
        Err(_) => false,
    }
}

fn too_large(path: &Path, subject: &str) -> ErrorValue {
    asset_create_failed(
        PROVIDER_ID,
        &path.display().to_string(),
        &format!(
            "v0.6 Appendix C.7: {subject} of `{}` is larger than this provider can hold, and a \
             copy that dropped it would not restore the object",
            path.display()
        ),
    )
}
