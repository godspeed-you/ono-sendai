//! A path as bytes, on the platforms a value crosses: Unix, where the shell runs, and WASI,
//! where a KUANG/11 component runs and reads the same wire encoding (spec §31.10).

use std::ffi::OsStr;

/// The bytes of an OS string.
#[cfg(unix)]
pub(crate) fn as_bytes(text: &OsStr) -> &[u8] {
    std::os::unix::ffi::OsStrExt::as_bytes(text)
}

/// The bytes of an OS string. Off Unix a path is text — WASI strings are UTF-8 by
/// construction — and the stable API offers no other view of it.
#[cfg(not(unix))]
pub(crate) fn as_bytes(text: &OsStr) -> &[u8] {
    text.to_str().map_or(&[], str::as_bytes)
}

/// An OS string over `bytes`, whatever they hold.
#[cfg(unix)]
pub(crate) fn from_bytes(bytes: &[u8]) -> &OsStr {
    std::os::unix::ffi::OsStrExt::from_bytes(bytes)
}

/// An OS string over `bytes`. Off Unix only text can be a path, so bytes that are not text
/// become the empty path rather than a fabricated one.
#[cfg(not(unix))]
pub(crate) fn from_bytes(bytes: &[u8]) -> &OsStr {
    OsStr::new(std::str::from_utf8(bytes).unwrap_or(""))
}
