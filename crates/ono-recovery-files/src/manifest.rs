//! The archive manifest: what was protected, and what has to come back (spec v0.6 Appendix C.7).
//!
//! Everything a restore needs lives in one file beside the copied bytes. Appendix C.7 requires a
//! provider to define exactly which metadata it restores, so the manifest carries a field per
//! answer — mode, owner, group, symlink target, extended attributes — and the restore puts back
//! precisely what is written here. What the manifest cannot carry is what the coverage report
//! declares missing (§C.7's *"Missing metadata support reduces recovery coverage and MUST be
//! visible"*).
//!
//! Paths, symlink targets and extended-attribute values are hexadecimal. A path is a byte string
//! the kernel never promised to be UTF-8, and a manifest that could not represent a file name is a
//! manifest that would silently drop the file it could not spell.

use std::path::{Path, PathBuf};

use ono_change_core::error::store_corrupt;
use ono_value::ErrorValue;
use sha2::{Digest as _, Sha256};

use crate::identity::{ObjectIdentity, ObjectKind};

/// The manifest format this crate writes and reads.
pub const MANIFEST_VERSION: &str = "ono.file-archive/1";

/// The separator between manifest fields: ASCII unit separator, which no path byte can be.
const UNIT: char = '\u{1f}';

/// One protected object, and everything the restore puts back (Appendix C.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    relative: PathBuf,
    kind: ObjectKind,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    digest: String,
    link_target: Option<PathBuf>,
    identity: ObjectIdentity,
    xattrs: Vec<(String, Vec<u8>)>,
    blob: Option<String>,
}

impl ArchiveEntry {
    /// Records an entry for the object `relative` names inside the archive root.
    #[must_use]
    pub fn new(
        relative: PathBuf,
        kind: ObjectKind,
        mode: u32,
        uid: u32,
        gid: u32,
        identity: ObjectIdentity,
    ) -> Self {
        Self {
            relative,
            kind,
            mode,
            uid,
            gid,
            size: 0,
            digest: String::new(),
            link_target: None,
            identity,
            xattrs: Vec::new(),
            blob: None,
        }
    }

    /// Records the content the entry holds, its digest and the blob that carries it.
    #[must_use]
    pub fn with_content(mut self, size: u64, digest: String, blob: Option<String>) -> Self {
        self.size = size;
        self.digest = digest;
        self.blob = blob;
        self
    }

    /// Records what a symlink points at, which is restored as a link and never followed (§15.1).
    #[must_use]
    pub fn with_link_target(mut self, target: PathBuf) -> Self {
        self.link_target = Some(target);
        self
    }

    /// Records the extended attributes read from the object (Appendix C.7).
    #[must_use]
    pub fn with_xattrs(mut self, xattrs: Vec<(String, Vec<u8>)>) -> Self {
        self.xattrs = xattrs;
        self
    }

    /// The path relative to the archive root. Empty when the root *is* the object.
    #[must_use]
    pub fn relative(&self) -> &Path {
        &self.relative
    }

    /// What kind of object it is.
    #[must_use]
    pub const fn kind(&self) -> ObjectKind {
        self.kind
    }

    /// The permission bits, including the set-user, set-group and sticky bits.
    #[must_use]
    pub const fn mode(&self) -> u32 {
        self.mode
    }

    /// The owning user id.
    #[must_use]
    pub const fn uid(&self) -> u32 {
        self.uid
    }

    /// The owning group id.
    #[must_use]
    pub const fn gid(&self) -> u32 {
        self.gid
    }

    /// The size of the content in bytes.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// The SHA-256 of the content, which §11.4's identity check re-derives.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// What a symlink points at.
    #[must_use]
    pub fn link_target(&self) -> Option<&Path> {
        self.link_target.as_deref()
    }

    /// Which object this was when it was protected (§43.5).
    #[must_use]
    pub const fn identity(&self) -> &ObjectIdentity {
        &self.identity
    }

    /// The extended attributes.
    #[must_use]
    pub fn xattrs(&self) -> &[(String, Vec<u8>)] {
        &self.xattrs
    }

    /// The file inside the store that holds this entry's bytes.
    #[must_use]
    pub fn blob(&self) -> Option<&str> {
        self.blob.as_deref()
    }

    /// The absolute path this entry restores to, under `root`.
    #[must_use]
    pub fn destination(&self, root: &Path) -> PathBuf {
        if self.relative.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(&self.relative)
        }
    }

    fn encode(&self) -> String {
        let mut line = format!(
            "entry{UNIT}{}{UNIT}{}{UNIT}{:o}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}",
            self.kind.as_str(),
            hex(path_bytes(&self.relative)),
            self.mode,
            self.uid,
            self.gid,
            self.size,
            if self.digest.is_empty() {
                "-"
            } else {
                &self.digest
            },
            self.link_target
                .as_deref()
                .map_or_else(|| "-".to_owned(), |target| hex(path_bytes(target))),
            self.identity.device(),
            self.identity.inode(),
            self.identity
                .generation()
                .map_or_else(|| "-".to_owned(), |generation| generation.to_string()),
            self.blob.as_deref().unwrap_or("-"),
            self.xattrs.len(),
        );
        for (name, value) in &self.xattrs {
            line.push(UNIT);
            line.push_str(&hex(name.as_bytes()));
            line.push(UNIT);
            line.push_str(&hex(value));
        }
        line
    }

    fn decode(line: &str) -> Option<Self> {
        let fields: Vec<&str> = line.split(UNIT).collect();
        let kind = ObjectKind::parse(fields.get(1)?)?;
        let relative = path_of(unhex(fields.get(2)?)?);
        let mode = u32::from_str_radix(fields.get(3)?, 8).ok()?;
        let uid = fields.get(4)?.parse().ok()?;
        let gid = fields.get(5)?.parse().ok()?;
        let size = fields.get(6)?.parse().ok()?;
        let digest = match *fields.get(7)? {
            "-" => String::new(),
            text => text.to_owned(),
        };
        let link_target = match *fields.get(8)? {
            "-" => None,
            text => Some(path_of(unhex(text)?)),
        };
        let device = fields.get(9)?.parse().ok()?;
        let inode = fields.get(10)?.parse().ok()?;
        let generation = match *fields.get(11)? {
            "-" => None,
            text => Some(text.parse().ok()?),
        };
        let blob = match *fields.get(12)? {
            "-" => None,
            text => Some(text.to_owned()),
        };
        let count: usize = fields.get(13)?.parse().ok()?;
        let mut xattrs = Vec::with_capacity(count);
        for index in 0..count {
            let name = String::from_utf8(unhex(fields.get(14 + index * 2)?)?).ok()?;
            let value = unhex(fields.get(15 + index * 2)?)?;
            xattrs.push((name, value));
        }
        Some(Self {
            relative,
            kind,
            mode,
            uid,
            gid,
            size,
            digest,
            link_target,
            identity: ObjectIdentity::new(device, inode, generation),
            xattrs,
            blob,
        })
    }
}

/// Everything one file archive holds (§15.1, Appendix C.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    root: PathBuf,
    root_parent: ObjectIdentity,
    created_at_nanos: i128,
    entries: Vec<ArchiveEntry>,
}

impl Manifest {
    /// Declares an archive of `root`, whose containing directory was `root_parent` (§43.5).
    #[must_use]
    pub fn new(root: PathBuf, root_parent: ObjectIdentity, created_at_nanos: i128) -> Self {
        Self {
            root,
            root_parent,
            created_at_nanos,
            entries: Vec::new(),
        }
    }

    /// Adds an entry.
    #[must_use]
    pub fn with_entry(mut self, entry: ArchiveEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// The path that was protected.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory that held the root when it was protected (§43.5).
    #[must_use]
    pub const fn root_parent(&self) -> &ObjectIdentity {
        &self.root_parent
    }

    /// When the archive was written, in nanoseconds since the epoch.
    #[must_use]
    pub const fn created_at_nanos(&self) -> i128 {
        self.created_at_nanos
    }

    /// The objects it holds, in the order they were walked.
    #[must_use]
    pub fn entries(&self) -> &[ArchiveEntry] {
        &self.entries
    }

    /// The entry for the absolute path `object`, where the archive holds it.
    #[must_use]
    pub fn entry_for(&self, object: &Path) -> Option<&ArchiveEntry> {
        self.entries
            .iter()
            .find(|entry| entry.destination(&self.root) == object)
    }

    /// Every object the archive covers, as absolute paths, for [`ono_change_core::RecoveryScope`].
    #[must_use]
    pub fn covered_objects(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| entry.destination(&self.root).display().to_string())
            .collect()
    }

    /// The total size of the content the archive holds.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.entries.iter().map(ArchiveEntry::size).sum()
    }

    /// The manifest as it is written to the store.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut text = format!(
            "{MANIFEST_VERSION}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}\n",
            hex(path_bytes(&self.root)),
            self.created_at_nanos,
            self.root_parent.device(),
            self.root_parent.inode(),
            self.root_parent
                .generation()
                .map_or_else(|| "-".to_owned(), |generation| generation.to_string()),
        );
        for entry in &self.entries {
            text.push_str(&entry.encode());
            text.push('\n');
        }
        text
    }

    /// Reads a manifest back.
    ///
    /// # Errors
    ///
    /// `change.store_corrupt` when the text is not a manifest this version wrote. §56.3 makes an
    /// unreadable archive a refusal: an asset whose contents cannot be understood cannot be used
    /// to restore anything, and pretending otherwise is what §62.1 calls snapshot theatre.
    pub fn decode(text: &str) -> Result<Self, ErrorValue> {
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().unwrap_or_default().split(UNIT).collect();
        let corrupt = || store_corrupt("the file archive manifest is not readable at this version");
        if header.first().copied() != Some(MANIFEST_VERSION) {
            return Err(corrupt());
        }
        let root = header
            .get(1)
            .and_then(|field| unhex(field))
            .map(path_of)
            .ok_or_else(corrupt)?;
        let created_at_nanos = header
            .get(2)
            .and_then(|field| field.parse().ok())
            .ok_or_else(corrupt)?;
        let device = header
            .get(3)
            .and_then(|field| field.parse().ok())
            .ok_or_else(corrupt)?;
        let inode = header
            .get(4)
            .and_then(|field| field.parse().ok())
            .ok_or_else(corrupt)?;
        let generation = match header.get(5).copied() {
            Some("-") => None,
            Some(text) => Some(text.parse().map_err(|_| corrupt())?),
            None => return Err(corrupt()),
        };
        let mut manifest = Self::new(
            root,
            ObjectIdentity::new(device, inode, generation),
            created_at_nanos,
        );
        for line in lines {
            if line.is_empty() {
                continue;
            }
            manifest
                .entries
                .push(ArchiveEntry::decode(line).ok_or_else(corrupt)?);
        }
        Ok(manifest)
    }
}

/// The SHA-256 of `bytes`, lowercase hexadecimal.
#[must_use]
pub fn digest_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

/// `bytes` as lowercase hexadecimal.
fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// The bytes `text` spells, or `None` when it is not even hexadecimal.
fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let text = std::str::from_utf8(pair).ok()?;
        out.push(u8::from_str_radix(text, 16).ok()?);
    }
    Some(out)
}

/// A path's bytes, which are not required to be UTF-8.
fn path_bytes(path: &Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes()
}

/// The path `bytes` spell, whether or not they are UTF-8.
fn path_of(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}
