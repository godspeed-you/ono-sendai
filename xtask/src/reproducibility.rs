//! Comparing two builds of one commit (spec §46.1, §46.5, §46.6).
//!
//! §46.5 asks release qualification to build every artifact twice in fresh clean environments and
//! compare hashes, and then asks for one thing more: *a diagnostic identifying which files or
//! archive members differ, where tooling permits.* Two hashes that disagree tell a maintainer
//! that something is wrong and nothing about what. A `.deb` is an `ar` archive of three members
//! and an `.rpm` is four concatenated sections, so "where tooling permits" is most of the way
//! down for both of them, and this module goes there.
//!
//! It compares bytes and never rebuilds. Producing the two directories is
//! `scripts/rebuild-check.sh`, which runs the same `scripts/package.sh` a release runs.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use sha2::{Digest, Sha256};

/// One way two builds of one commit disagreed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// The artifact the difference was found in.
    pub artifact: String,
    /// What differs, named as precisely as the format allows.
    pub detail: String,
}

/// The lowercase hexadecimal SHA-256 of `bytes`.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// Compares two directories of build artifacts, byte for byte.
///
/// An empty result means the two builds are identical in the sense §46.1 defines: the same set of
/// artifacts, and every one of them the same bytes.
///
/// # Errors
///
/// Returns the reason a directory could not be read. A directory that is not there is a build
/// that did not happen, which is a failure of the comparison rather than a difference in it.
pub fn compare(left: &Path, right: &Path) -> Result<Vec<Difference>, String> {
    let first = artifacts(left)?;
    let second = artifacts(right)?;
    let mut differences = Vec::new();

    for name in first.keys() {
        if !second.contains_key(name) {
            differences.push(Difference {
                artifact: name.clone(),
                detail: format!(
                    "was produced by {} and not by {}",
                    left.display(),
                    right.display()
                ),
            });
        }
    }
    for name in second.keys() {
        if !first.contains_key(name) {
            differences.push(Difference {
                artifact: name.clone(),
                detail: format!(
                    "was produced by {} and not by {}",
                    right.display(),
                    left.display()
                ),
            });
        }
    }

    for (name, artifact) in &first {
        let Some(other) = second.get(name) else {
            continue;
        };
        if artifact.bytes != other.bytes {
            differences.push(Difference {
                artifact: name.clone(),
                detail: describe(name, &artifact.bytes, &other.bytes),
            });
        }
        // §46.4 asks for deterministic modes, and the mode of the *published file* is one a
        // umask decides rather than a packaging tool. Two builds whose bytes agree and whose
        // modes do not are still two different downloads.
        if artifact.mode != other.mode {
            differences.push(Difference {
                artifact: name.clone(),
                detail: format!(
                    "was written with mode {:04o} by the first build and {:04o} by the second, \
                     so the umask of whoever built it reached the published file (spec §46.4)",
                    artifact.mode, other.mode
                ),
            });
        }
    }

    differences.sort_by(|left, right| {
        (&left.artifact, &left.detail).cmp(&(&right.artifact, &right.detail))
    });
    Ok(differences)
}

/// What a directory of build artifacts holds, by name, with each artifact's SHA-256.
///
/// This is the evidence a clean comparison leaves behind: two builds that agree are only
/// interesting if a reader can see *what* they agreed about.
///
/// # Errors
///
/// Returns the reason the directory could not be read.
pub fn inventory(directory: &Path) -> Result<Vec<(String, String)>, String> {
    Ok(artifacts(directory)?
        .into_iter()
        .map(|(name, artifact)| (name, digest(&artifact.bytes)))
        .collect())
}

/// One artifact as two builds have to agree about it: its bytes and the mode it was written with.
struct Artifact {
    bytes: Vec<u8>,
    mode: u32,
}

/// Every regular file in `directory`, by name, with its contents and its mode.
fn artifacts(directory: &Path) -> Result<BTreeMap<String, Artifact>, String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    let mut found = BTreeMap::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let mode = entry
            .metadata()
            .map_err(|error| format!("cannot stat {}: {error}", path.display()))?
            .permissions()
            .mode()
            & 0o7777;
        found.insert(
            entry.file_name().to_string_lossy().into_owned(),
            Artifact { bytes, mode },
        );
    }
    Ok(found)
}

/// Says what differs, as far into the artifact as its format allows.
fn describe(name: &str, left: &[u8], right: &[u8]) -> String {
    if let (Some(first), Some(second)) = (ar_members(left), ar_members(right)) {
        return sections("archive member", &first, &second);
    }
    if let (Some(first), Some(second)) = (rpm_sections(left), rpm_sections(right)) {
        return sections("package section", &first, &second);
    }
    let _ = name;
    generic(left, right)
}

/// The one-line verdict for a format that decomposes into named parts.
fn sections(
    kind: &str,
    left: &BTreeMap<String, Vec<u8>>,
    right: &BTreeMap<String, Vec<u8>>,
) -> String {
    let mut differing = Vec::new();
    for (name, bytes) in left {
        match right.get(name) {
            Some(other) if other == bytes => {}
            Some(other) => differing.push(format!(
                "{kind} `{name}` differs ({} bytes / {}, {} bytes / {}); {}",
                bytes.len(),
                short(&digest(bytes)),
                other.len(),
                short(&digest(other)),
                generic(bytes, other)
            )),
            None => differing.push(format!("{kind} `{name}` is missing from the second build")),
        }
    }
    for name in right.keys() {
        if !left.contains_key(name) {
            differing.push(format!("{kind} `{name}` is missing from the first build"));
        }
    }
    if differing.is_empty() {
        return format!(
            "the bytes differ although every {kind} is identical, so the difference is in the \
             container structure itself"
        );
    }
    differing.join("; ")
}

/// The fallback for bytes with no structure this module reads.
fn generic(left: &[u8], right: &[u8]) -> String {
    let at = left
        .iter()
        .zip(right)
        .position(|(first, second)| first != second);
    match at {
        Some(offset) => format!(
            "first differing byte at offset {offset} ({:#04x} against {:#04x})",
            left[offset], right[offset]
        ),
        None => format!(
            "one is a prefix of the other: {} bytes against {} bytes",
            left.len(),
            right.len()
        ),
    }
}

/// The first eight characters of a digest, which is enough to tell two apart in a log line.
fn short(digest: &str) -> &str {
    &digest[..8.min(digest.len())]
}

/// The members of a `.deb`, which is a plain `ar` archive of three of them.
fn ar_members(bytes: &[u8]) -> Option<BTreeMap<String, Vec<u8>>> {
    if !bytes.starts_with(b"!<arch>\n") {
        return None;
    }
    let mut members = BTreeMap::new();
    let mut cursor = 8;
    while cursor + 60 <= bytes.len() {
        let header = &bytes[cursor..cursor + 60];
        let name = String::from_utf8_lossy(&header[0..16])
            .trim()
            .trim_end_matches('/')
            .to_owned();
        let size: usize = String::from_utf8_lossy(&header[48..58])
            .trim()
            .parse()
            .ok()?;
        let body = cursor + 60;
        let end = body.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        members.insert(name, bytes[body..end].to_vec());
        cursor = end + end % 2;
    }
    (!members.is_empty()).then_some(members)
}

/// The four concatenated sections of an `.rpm`: lead, signature header, header, payload.
fn rpm_sections(bytes: &[u8]) -> Option<BTreeMap<String, Vec<u8>>> {
    const LEAD: usize = 96;
    const MAGIC: [u8; 4] = [0x8e, 0xad, 0xe8, 0x01];
    if bytes.len() < LEAD || bytes[..4] != [0xed, 0xab, 0xee, 0xdb] {
        return None;
    }
    let section_end = |at: usize| -> Option<usize> {
        if bytes.len() < at + 16 || bytes[at..at + 4] != MAGIC {
            return None;
        }
        let count = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().ok()?) as usize;
        let length = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().ok()?) as usize;
        at.checked_add(16)?
            .checked_add(count.checked_mul(16)?)?
            .checked_add(length)
    };
    let after_signature = section_end(LEAD)?;
    let header_at = after_signature.div_ceil(8) * 8;
    let after_header = section_end(header_at)?;
    if after_header > bytes.len() {
        return None;
    }
    let mut sections = BTreeMap::new();
    sections.insert("lead".to_owned(), bytes[..LEAD].to_vec());
    sections.insert(
        "signature header".to_owned(),
        bytes[LEAD..after_signature].to_vec(),
    );
    sections.insert("header".to_owned(), bytes[header_at..after_header].to_vec());
    sections.insert("payload".to_owned(), bytes[after_header..].to_vec());
    Some(sections)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::{ar_members, compare, digest, generic, rpm_sections};

    #[test]
    fn reads_the_members_of_an_ar_archive() {
        let mut archive = b"!<arch>\n".to_vec();
        archive.extend_from_slice(b"debian-binary   0           0     0     100644  4         `\n");
        archive.extend_from_slice(b"2.0\n");
        let members = ar_members(&archive).expect("an ar archive");
        assert_eq!(
            members.get("debian-binary").map(Vec::as_slice),
            Some(&b"2.0\n"[..])
        );
    }

    #[test]
    fn declines_bytes_that_are_neither_format() {
        assert!(ar_members(b"not an archive").is_none());
        assert!(rpm_sections(b"not a package").is_none());
    }

    #[test]
    fn names_the_first_differing_byte() {
        assert!(generic(b"abcd", b"abZd").contains("offset 2"));
        assert!(generic(b"abc", b"abcd").contains("prefix"));
    }

    #[test]
    fn notices_when_two_builds_agree_about_the_bytes_and_not_about_the_mode() {
        // Found by probing: with the determinism block removed from `scripts/package.sh`, both
        // packages still came out byte-identical and the second build's files were `0600`
        // because its umask was `077`. Identical bytes behind a different mode are still two
        // different downloads (§46.4).
        let scratch = ono_testkit::scratch();
        let (left, right) = (scratch.path().join("a"), scratch.path().join("b"));
        for directory in [&left, &right] {
            std::fs::create_dir_all(directory).expect("a scratch directory");
            std::fs::write(directory.join("ono_0.0.0_amd64.deb"), b"same bytes")
                .expect("an artifact");
        }
        std::fs::set_permissions(
            right.join("ono_0.0.0_amd64.deb"),
            std::fs::Permissions::from_mode(0o600),
        )
        .expect("the second build's umask");
        std::fs::set_permissions(
            left.join("ono_0.0.0_amd64.deb"),
            std::fs::Permissions::from_mode(0o644),
        )
        .expect("the first build's umask");

        let differences = compare(&left, &right).expect("both directories are readable");
        assert_eq!(differences.len(), 1, "{differences:?}");
        assert!(differences[0].detail.contains("0644"));
        assert!(differences[0].detail.contains("0600"));
    }

    #[test]
    fn hashes_the_way_sha256sum_does() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
