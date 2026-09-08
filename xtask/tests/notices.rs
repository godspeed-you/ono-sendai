//! The notices the shipped binary owes (ADR-0608): that they exist, that they agree with the
//! lockfile, and that they leave with the packages rather than staying in the repository.
//!
//! `deny.toml` decides which licences may be shipped. These tests are about the other half —
//! what those licences ask for once the binary is in somebody else's hands.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

mod support;
use support::{read, repo};

#[test]
fn should_hold_the_notices_to_the_crates_the_lockfile_resolves() {
    // The one property that makes a generated notice file worth anything: it says what is
    // actually being distributed today, not what was at the last dependency bump somebody
    // remembered to follow up.
    let problems = xtask::notices::check_committed(&repo());
    assert!(
        problems.is_empty(),
        "run `cargo run -p xtask -- licenses --write`: {problems:?}"
    );
}

#[test]
fn should_ship_the_notices_with_both_packages() {
    // A notice file that never leaves the repository discharges nothing: the obligation is to
    // the person who receives the binary.
    let manifest = read("crates/ono-cli/Cargo.toml");
    let table = |name: &str| {
        let start = manifest
            .find(&format!("[package.metadata.{name}]"))
            .unwrap_or_else(|| panic!("Cargo.toml carries [package.metadata.{name}]"));
        let rest = &manifest[start + 1..];
        let end = rest.find("\n[").map_or(rest.len(), |offset| offset + 1);
        manifest[start..start + 1 + end].to_owned()
    };
    for tool in ["deb", "generate-rpm"] {
        let assets = table(tool);
        assert!(
            assets.contains(xtask::notices::NOTICES_FILE),
            "[package.metadata.{tool}] installs {}",
            xtask::notices::NOTICES_FILE
        );
    }
}

#[test]
fn should_reproduce_the_text_of_the_data_licence_the_root_store_brings() {
    // The licence that made the gap visible (ADR-0607's `webpki-roots`, allowed in `deny.toml`
    // as CDLA-Permissive-2.0): §2.1 asks that the agreement's own text travel with the data, and
    // this is where it travels.
    let notices = read(xtask::notices::NOTICES_FILE);
    assert!(
        notices.contains("Community Data License Agreement"),
        "the agreement's text is in the notices"
    );
    assert!(
        notices.contains("webpki-roots"),
        "and the crate that brings it is named"
    );
}

#[test]
fn should_name_a_crate_that_ships_no_licence_document_rather_than_leave_it_out() {
    // Twenty-odd crates in this graph ship no licence file at all. A notice file that simply
    // omitted them would read as complete while being short, which is the failure mode this
    // section exists to prevent.
    let notices = read(xtask::notices::NOTICES_FILE);
    assert!(
        notices.contains("2. The crates that ship no licence document"),
        "the section is there while such crates are in the graph"
    );
}
