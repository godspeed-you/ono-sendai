//! What a package artifact is made of (spec §31.36).
//!
//! One definition, used by everything that needs to answer "these bytes": the host that records
//! an integrity hash at install and re-checks it at load, and the tool an author signs with. Two
//! walks of the same directory that disagreed about which files count would be a signature that
//! verifies on one side and not the other, so there is one walk and it lives beside the format
//! it feeds (ADR-0311, ADR-0312).

use std::path::Path;

use crate::signature::{FileDigest, SIGNATURE_FILE};

/// Where a package carries a keyless signature, beside `manifest.yaml` (ADR-0609).
///
/// The name lives here, with the rest of the package layout: what a package may contain is the
/// protocol's business, and verifying what it contains is the host's.
pub const BUNDLE_FILE: &str = "signature.sigstore.json";

/// Every file a package artifact is made of, with its digest, sorted by path.
///
/// Everything under the package directory except the signature. What a manifest *declares* is
/// what the package contributes, not what it consists of: a fixture an adapter pack reads at
/// run time, a data table a command answers from, a second binary the entry point execs are all
/// part of the artifact and none of them is named by a manifest field. A hash that covered only
/// the declared files would answer "these are the exact bytes referenced" while most of the
/// bytes went unlooked at (spec §31.36, ADR-0311).
///
/// `signature.yaml` and `signature.sigstore.json` are excluded because each is a statement
/// *about* the artifact: a signature cannot cover itself.
#[must_use]
pub fn artifact_files(directory: &Path) -> Vec<FileDigest> {
    let mut files = Vec::new();
    collect_artifact_files(directory, "", &mut files);
    files.sort();
    files
}

/// Every file that travels in a `.kuang` archive: the artifact, plus whichever signatures the
/// package carries.
///
/// A signature is not one of the files it covers, so `artifact_files` leaves both out — and an
/// archive built from that list alone would arrive without the very statement that makes it a
/// signed release, installing under local-development semantics instead. The two lists differ by
/// exactly the signatures, and they are written next to each other so they cannot drift: the
/// keyless bundle went missing from `.kuang` archives for the length of one release because the
/// packer knew about `signature.yaml` and nothing told it about the second form (ADR-0609).
#[must_use]
pub fn packed_files(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = artifact_files(directory)
        .into_iter()
        .map(|entry| entry.path)
        .collect();
    names.extend(
        [SIGNATURE_FILE, BUNDLE_FILE]
            .into_iter()
            .filter(|name| directory.join(name).is_file())
            .map(str::to_owned),
    );
    names
}

/// Walks `directory`, adding one entry per regular file and per symbolic link.
///
/// A symbolic link is recorded by its target rather than by what it points at: following it
/// would let a package's hash depend on a file outside the package, and skipping it would let
/// the link be repointed unnoticed.
fn collect_artifact_files(directory: &Path, prefix: &str, files: &mut Vec<FileDigest>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        // Neither signature covers itself: the ed25519 document, and the keyless bundle beside
        // it (ADR-0311, ADR-0609 §1).
        if relative == SIGNATURE_FILE || relative == BUNDLE_FILE {
            continue;
        }
        let Ok(metadata) = entry.path().symlink_metadata() else {
            continue;
        };
        if metadata.is_symlink() {
            let target = std::fs::read_link(entry.path()).unwrap_or_default();
            files.push(FileDigest::of_bytes(
                &relative,
                target.as_os_str().as_encoded_bytes(),
            ));
        } else if metadata.is_dir() {
            collect_artifact_files(&entry.path(), &relative, files);
        } else if let Ok(bytes) = std::fs::read(entry.path()) {
            files.push(FileDigest::of_bytes(&relative, &bytes));
        } else {
            // A file that is there and cannot be read is a fact about the artifact, and one
            // that must change the answer rather than be passed over.
            files.push(FileDigest {
                path: relative,
                sha256: "unreadable".to_owned(),
            });
        }
    }
}
