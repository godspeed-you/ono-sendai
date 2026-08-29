//! Where the seeds are, and where a finding goes.
//!
//! A corpus is data, not code: `corpus/<target>/` holds the inputs that reach past each
//! decoder's first rejection, and `artifacts/<target>/` holds every input that ever caused a
//! finding. Both are committed, so a crash found once is a regression test for ever after —
//! `tests/corpus.rs` replays all of it on every `cargo test`.

use std::io;
use std::path::{Path, PathBuf};

/// The fuzz crate's own directory, whatever the working directory is.
#[must_use]
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Where the seeds of `target` live.
#[must_use]
pub fn corpus_dir(target: &str) -> PathBuf {
    root().join("corpus").join(target)
}

/// Where the findings of `target` are kept.
#[must_use]
pub fn artifacts_dir(target: &str) -> PathBuf {
    root().join("artifacts").join(target)
}

/// Every input in `directory`, in name order. A directory that does not exist is an empty
/// corpus, not a failure: a target may legitimately have no artifacts.
#[must_use]
pub fn load(directory: &Path) -> Vec<Vec<u8>> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .collect()
}

/// The seeds and the past findings of `target`, which is what a run starts from.
#[must_use]
pub fn load_for(target: &str) -> Vec<Vec<u8>> {
    let mut inputs = load(&corpus_dir(target));
    inputs.extend(load(&artifacts_dir(target)));
    inputs
}

/// Writes `input` under `artifacts/<target>/`, named by its digest so the same finding is
/// written once. Answers where it went.
///
/// # Errors
///
/// The I/O failure, when the artifact cannot be written.
pub fn record(target: &str, input: &[u8]) -> io::Result<PathBuf> {
    let directory = artifacts_dir(target);
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{}.bin", digest(input)));
    std::fs::write(&path, input)?;
    Ok(path)
}

/// The lowercase hex SHA-256 of `input`, which is how an artifact is named.
#[must_use]
pub fn digest(input: &[u8]) -> String {
    use std::fmt::Write as _;

    use sha2::Digest as _;

    sha2::Sha256::digest(input)
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}
