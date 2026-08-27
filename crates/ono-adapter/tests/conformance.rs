//! The fixture conformance harness (spec v0.3 §1.47, ADAPT-010): fixture bytes → decoder →
//! canonical value → schema conformance → provenance, for every fixture of every first-party
//! adapter, through the decoder the shell itself uses.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

use std::path::PathBuf;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/adapters/fixtures")
        .canonicalize()
        .expect("the fixtures directory exists")
}

#[test]
fn should_decode_every_first_party_fixture_to_what_its_sidecar_promises() {
    for pack in ono_adapter::first_party() {
        let problems = ono_adapter::conformance::check_pack(pack, &fixtures_root());
        assert!(
            problems.is_empty(),
            "{} fixtures must decode as their sidecars say, got {problems:#?}",
            pack.id()
        );
    }
}

#[test]
fn should_report_a_fixture_whose_sidecar_disagrees_with_the_decoder() {
    // A harness that cannot fail proves nothing: copy one fixture set, change an expectation,
    // and require the disagreement to be named.
    let scratch = ono_testkit::scratch();
    let source = fixtures_root().join("util-linux/lsblk");
    for entry in std::fs::read_dir(&source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let mut text = std::fs::read_to_string(entry.path()).unwrap();
        if name == "disk-with-partitions.yaml" {
            text = text.replacen("type: disk", "type: floppy", 1);
        }
        scratch.write(format!("util-linux/lsblk/{name}"), &text);
    }
    let pack = ono_adapter::first_party()
        .iter()
        .find(|pack| pack.id() == "org.ono.compat.util-linux")
        .unwrap();
    let problems = ono_adapter::conformance::check_pack(pack, scratch.path());
    assert!(
        problems
            .iter()
            .any(|p| p.detail.contains("disk-with-partitions")
                && p.detail.contains("type")
                && p.detail.contains("floppy")),
        "the fixture, the field and the expectation are named, got {problems:#?}"
    );
}
