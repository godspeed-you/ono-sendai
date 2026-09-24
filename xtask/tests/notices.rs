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

// --- each shipped binary's graph, resolved on its own ------------------------------------------

/// A workspace of two shipped members over one engine whose compiler is a feature: `shell` uses
/// the engine without it, `tool` turns it on. Cargo unifies features across a workspace's
/// resolution, so a graph read from the workspace gives `shell` the compiler it never links.
fn two_binaries_one_feature() -> ono_testkit::Scratch {
    let workspace = ono_testkit::scratch();
    let package = |dir: &str, extra: &str| {
        workspace.write(
            format!("{dir}/Cargo.toml"),
            format!(
                "[package]\nname = \"{dir}\"\nversion = \"1.0.0\"\nedition = \"2021\"\n\
                 license = \"MIT\"\n{extra}"
            ),
        );
        workspace.write(format!("{dir}/src/lib.rs"), "");
        workspace.write(format!("{dir}/LICENSE"), format!("the {dir} licence\n"));
    };
    workspace.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"shell\", \"tool\"]\n\
         exclude = [\"engine\", \"heavy\", \"built\", \"tested\"]\n",
    );
    package(
        "shell",
        "[dependencies]\nengine = { path = \"../engine\" }\n\
         [build-dependencies]\nbuilt = { path = \"../built\" }\n\
         [dev-dependencies]\ntested = { path = \"../tested\" }\n",
    );
    package(
        "tool",
        "[dependencies]\nengine = { path = \"../engine\", features = [\"compiler\"] }\n",
    );
    package(
        "engine",
        "[dependencies]\nheavy = { path = \"../heavy\", optional = true }\n\
         [features]\ncompiler = [\"dep:heavy\"]\n",
    );
    for leaf in ["heavy", "built", "tested"] {
        package(leaf, "");
    }
    let locked = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(workspace.path())
        .output()
        .expect("cargo must be runnable in the gate");
    assert!(
        locked.status.success(),
        "{}",
        String::from_utf8_lossy(&locked.stderr)
    );
    workspace
}

fn names(dependencies: &[xtask::notices::Dependency]) -> Vec<&str> {
    dependencies.iter().map(|d| d.name.as_str()).collect()
}

#[test]
fn should_owe_exactly_the_crates_each_shipped_binary_links_resolved_on_its_own() {
    // ADR-0870 moved Cranelift out of `ono` and into `kuang-compile`; the notices went on listing
    // it for `ono`, because they were read from the workspace's feature-unified graph. The file
    // owes the union of what the shipped binaries link, each built on its own, and nothing else.
    let workspace = two_binaries_one_feature();

    let shell =
        xtask::notices::dependencies_of(workspace.path(), &["shell"]).expect("the graph reads");
    assert_eq!(
        names(&shell),
        vec!["built", "engine"],
        "`shell` links the engine without its compiler, a build dependency's output and no test \
         harness; a crate only another member's feature brings in is not shipped with it"
    );

    let both = xtask::notices::dependencies_of(workspace.path(), &["shell", "tool"])
        .expect("the graph reads");
    assert_eq!(
        names(&both),
        vec!["built", "engine", "heavy"],
        "shipping `tool` too ships the compiler it links, once"
    );
    assert!(
        both.iter().find(|d| d.name == "heavy").is_some_and(|d| d
            .texts
            .iter()
            .any(|(_, text)| text.contains("the heavy licence"))),
        "the licence text of a crate reached only through `tool` is read: {both:?}"
    );
}
