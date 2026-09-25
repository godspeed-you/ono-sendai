#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

//! Reading a file archive manifest back (spec v0.6 §15.1, §56.3). The manifest is a file on disk,
//! so what it says is input: a damaged one is refused, never trusted.

use ono_recovery_files::Manifest;

/// A manifest with one file entry whose extended-attribute count is `count`, and no attribute
/// fields behind it.
fn manifest_claiming_xattrs(count: &str) -> String {
    let header = ["ono.file-archive/1", "2f657463", "0", "64", "2", "-"].join("\u{1f}");
    let entry = [
        "entry",
        "file",
        "6e67696e782e636f6e66",
        "644",
        "0",
        "0",
        "5",
        "abc123",
        "-",
        "64",
        "3001",
        "-",
        "-",
        count,
    ]
    .join("\u{1f}");
    format!("{header}\n{entry}\n")
}

#[test]
fn should_refuse_an_entry_that_claims_more_xattrs_than_it_carries() {
    // The count is a field of the line, not a promise the line keeps. Sizing an allocation by it
    // lets eleven bytes of a damaged manifest ask for more memory than the machine has.
    for count in ["1", "4096", &usize::MAX.to_string()] {
        let error = Manifest::decode(&manifest_claiming_xattrs(count))
            .expect_err("an entry missing the attributes it counts is not a manifest");
        assert_eq!(
            error.code().name(),
            "change.plan_store_corrupt",
            "§56.3: an unreadable archive is a refusal (count {count})"
        );
    }
}

#[test]
fn should_read_an_entry_whose_xattr_count_matches_what_it_carries() {
    let manifest = Manifest::decode(&manifest_claiming_xattrs("0"))
        .expect("an entry with no attributes is readable");
    assert_eq!(
        Manifest::decode(&manifest.encode()).as_ref().ok(),
        Some(&manifest),
        "§15.1: a manifest reads back as itself"
    );
}
