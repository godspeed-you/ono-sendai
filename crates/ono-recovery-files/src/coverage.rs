//! What this provider can actually put back, measured on the spot (spec v0.6 Appendix C.7).
//!
//! Appendix C.7 requires recovery to define whether it restores content, mode, owner, ACLs,
//! xattrs, capabilities, SELinux labels and hard-link relationships, and it ends with the rule
//! that makes the list worth having: *"Missing metadata support reduces recovery coverage and MUST
//! be visible."*
//!
//! So the answers that depend on the host are asked of the host. Whether a filesystem carries
//! extended attributes, POSIX ACLs, file capabilities or SELinux labels is a question `lgetxattr`
//! answers about the destination directory itself, and it answers it without writing anything:
//! `ENODATA` is a namespace that exists and is empty, `EOPNOTSUPP` is a namespace the filesystem
//! does not have. Asserting `xattrs: true` because Linux has extended attributes produces a
//! provider that claims coverage on a filesystem mounted `nouser_xattr`, and §10.1's whole point
//! is that protection is what a mechanism can deliver rather than what it usually delivers.
//!
//! Whether *this process* may give a file to another user is a question about privilege rather
//! than about the destination, and it is asked with a probe file inside Ono's own recovery store —
//! never inside the tree being protected, because §5.5 keeps discovery read-only over the system.
//!
//! Two answers are constants, and both are properties of the mechanism. Content and mode are
//! always restored: the copy holds the bytes and the manifest holds the permission bits.
//! Hard-link relationships are never restored: an archive writes one file per name, so a file with
//! two names comes back as two files, and [`crate::capture::scan`] records that as an exclusion on
//! every asset where it applies.

use std::path::Path;

use ono_change_core::MetadataCoverage;
use ono_change_core::error::store_unavailable;
use ono_value::ErrorValue;

use crate::store::write_private_file;
use crate::xattr;

/// The extended attribute the writability probe uses.
const PROBE_ATTRIBUTE: &str = "user.ono.recovery-probe";

/// The user id the ownership probe tries to hand a file to.
///
/// `nobody` on nearly every distribution, and the number is what matters: only a process holding
/// `CAP_CHOWN` may give a file it owns away, which is the question §43.4 asks — can this process
/// restore ownership, rather than is this process root.
const PROBE_UID: u32 = 65534;

/// Measures what a restore into `destination` would put back, probing from `store_root`.
///
/// `destination` is the directory the restore writes into, because coverage is a property of the
/// filesystem that receives the file. `store_root` is Ono's own private store, which is where the
/// privilege probe writes: §5.5 keeps discovery read-only over the system being protected.
///
/// # Errors
///
/// `change.store_unavailable` when the probe file cannot be created inside the store, which means
/// the ownership question cannot be answered — and §56.3 makes an unestablished recovery fact a
/// refusal rather than an assumption.
pub fn measure(destination: &Path, store_root: &Path) -> Result<MetadataCoverage, ErrorValue> {
    let privileged = rustix::process::geteuid().is_root();
    Ok(MetadataCoverage {
        content: true,
        mode: true,
        owner: owner_restorable(store_root)?,
        acl: xattr::namespace_supported(destination, "system.posix_acl_access"),
        xattrs: xattr::namespace_supported(destination, PROBE_ATTRIBUTE),
        capabilities: privileged && xattr::namespace_supported(destination, "security.capability"),
        selinux: privileged && xattr::namespace_supported(destination, "security.selinux"),
        hardlinks: false,
    })
}

/// Measures the coverage of a restore of `path` itself, which lands in the directory holding it.
///
/// # Errors
///
/// As [`measure`].
pub fn measure_for(path: &Path, store_root: &Path) -> Result<MetadataCoverage, ErrorValue> {
    measure(path.parent().unwrap_or(Path::new("/")), store_root)
}

/// Whether this process may hand a file it owns to another user (§43.4, Appendix C.7).
fn owner_restorable(store_root: &Path) -> Result<bool, ErrorValue> {
    let probe = store_root.join(".ono-recovery-owner-probe");
    let _ = std::fs::remove_file(&probe);
    write_private_file(&probe, b"").map_err(|error| {
        store_unavailable(&format!(
            "whether this process can restore ownership could not be established: {error}"
        ))
    })?;
    let restorable = rustix::fs::chownat(
        rustix::fs::CWD,
        &probe,
        Some(rustix::fs::Uid::from_raw(PROBE_UID)),
        None,
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
    )
    .is_ok();
    let _ = std::fs::remove_file(&probe);
    Ok(restorable)
}
