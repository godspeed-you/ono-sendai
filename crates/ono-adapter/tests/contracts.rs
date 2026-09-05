//! The adapter pack contract (spec v0.3 §1.44, ADR-0055): every first-party pack is valid, and
//! the validator rejects what the contract forbids.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

use ono_adapter::{AdapterPack, validate};

mod support;
use support::fixtures_root;

const UTIL_LINUX: &str =
    include_str!("../../../docs/contracts/adapters/first-party/util-linux.yaml");

#[test]
fn should_load_every_first_party_pack() {
    let packs = ono_adapter::first_party();
    let ids: Vec<&str> = packs.iter().map(|pack| pack.id()).collect();
    assert!(
        ids.contains(&"org.ono.compat.util-linux"),
        "the util-linux pack is bundled (spec v0.3 §1.69 step 2), got {ids:?}"
    );
    let util_linux = packs
        .iter()
        .find(|pack| pack.id() == "org.ono.compat.util-linux")
        .unwrap();
    let adapters: Vec<&str> = util_linux.adapters().iter().map(|a| a.id()).collect();
    assert_eq!(adapters, ["lsblk", "findmnt", "lsns"]);
    assert_eq!(
        util_linux.adapters()[0].full_id(),
        "org.ono.compat.util-linux.lsblk",
        "an adapter's full id is the pack id plus its own (ADR-0055)"
    );
}

#[test]
fn should_validate_every_first_party_pack_against_the_schemas_and_fixtures() {
    for pack in ono_adapter::first_party() {
        let problems = validate(pack, ono_value::builtin_schemas(), &fixtures_root());
        assert!(
            problems.is_empty(),
            "{} must satisfy docs/contracts/adapters/schema.yaml, got {problems:#?}",
            pack.id()
        );
    }
}

fn problems_of(yaml: &str) -> Vec<String> {
    let pack = AdapterPack::parse(yaml).expect("the pack parses");
    validate(&pack, ono_value::builtin_schemas(), &fixtures_root())
        .into_iter()
        .map(|problem| problem.detail)
        .collect()
}

#[test]
fn should_reject_an_adapter_whose_schema_is_not_registered() {
    let yaml = UTIL_LINUX.replacen("schema: ono.block-device/1", "schema: ono.nonesuch/1", 1);
    let problems = problems_of(&yaml);
    assert!(
        problems.iter().any(|p| p.contains("ono.nonesuch/1")),
        "spec v0.3 §1.11: a canonical schema must exist, got {problems:?}"
    );
}

#[test]
fn should_reject_an_executable_outside_the_capability_grant() {
    let yaml = UTIL_LINUX.replacen(
        "executables: [lsblk, findmnt, lsns]",
        "executables: [findmnt, lsns]",
        1,
    );
    let problems = problems_of(&yaml);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("lsblk") && p.contains("process.exec")),
        "spec v0.3 §1.22: an adapter may only name executables its pack is granted, got {problems:?}"
    );
}

#[test]
fn should_reject_a_field_map_naming_a_field_the_schema_does_not_have() {
    let yaml = UTIL_LINUX.replacen(
        "      serial: {from: serial}",
        "      colour: {from: serial}",
        1,
    );
    let problems = problems_of(&yaml);
    assert!(
        problems.iter().any(|p| p.contains("colour")),
        "a mapped field must exist in the schema, got {problems:?}"
    );
}

#[test]
fn should_reject_a_first_party_pack_outside_its_namespace() {
    let yaml = UTIL_LINUX.replacen(
        "id: org.ono.compat.util-linux",
        "id: com.example.util-linux",
        1,
    );
    let problems = problems_of(&yaml);
    assert!(
        problems.iter().any(|p| p.contains("org.ono.compat")),
        "a first-party pack lives under org.ono.compat (ADR-0055), got {problems:?}"
    );
}

#[test]
fn should_reject_a_missing_fixture_directory() {
    let yaml = UTIL_LINUX.replacen(
        "fixtures: util-linux/lsns",
        "fixtures: util-linux/nonesuch",
        1,
    );
    let problems = problems_of(&yaml);
    assert!(
        problems.iter().any(|p| p.contains("util-linux/nonesuch")),
        "spec v0.3 §1.47: every adapter ships fixtures, got {problems:?}"
    );
}

#[test]
fn should_reject_a_tier_c_adapter_with_a_declarative_decoder() {
    let yaml = UTIL_LINUX.replacen("    tier: A\n", "    tier: C\n", 1);
    let problems = problems_of(&yaml);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("tier C") || p.contains("builtin")),
        "a version-constrained human-output parser is code, not a field map (spec v0.3 §1.9), got {problems:?}"
    );
}

#[test]
fn should_reject_a_version_probe_whose_pattern_captures_nothing() {
    let yaml = UTIL_LINUX.replacen(
        "pattern: \"util-linux ([0-9]+(?:\\\\.[0-9]+)+)\"",
        "pattern: \"util-linux\"",
        1,
    );
    let problems = problems_of(&yaml);
    assert!(
        problems.iter().any(|p| p.contains("capture")),
        "spec v0.3 §1.46: the probe must yield a version, got {problems:?}"
    );
}

#[test]
fn should_reject_a_pack_that_does_not_declare_the_adapter_role() {
    let yaml = UTIL_LINUX.replacen("roles: [adapter]", "roles: [provider]", 1);
    let problems = problems_of(&yaml);
    assert!(
        problems.iter().any(|p| p.contains("adapter")),
        "spec v0.3 §1.44: `roles` must contain `adapter`, got {problems:?}"
    );
}

#[test]
fn should_fail_closed_on_an_unknown_field() {
    let yaml = UTIL_LINUX.replacen("    tier: A\n", "    tier: A\n    colour: blue\n", 1);
    assert!(
        AdapterPack::parse(&yaml).is_err(),
        "sections are closed: an unknown field invalidates the pack"
    );
}

#[test]
fn should_bundle_the_iproute2_pack_with_its_ip_and_ss_adapters() {
    let pack = ono_adapter::first_party()
        .iter()
        .find(|pack| pack.id() == "org.ono.compat.iproute2")
        .expect("spec v0.3 §1.69 step 3: the ip family is bundled");
    let ids: Vec<&str> = pack.adapters().iter().map(|a| a.id()).collect();
    assert_eq!(
        ids,
        [
            "ip-address",
            "ip-link",
            "ip-route",
            "ip-route6",
            "ip-neigh",
            "ss-tcp",
            "ss-udp",
            "ss"
        ]
    );
}

#[test]
fn should_reject_a_template_that_names_no_placeholder() {
    let iproute2 = include_str!("../../../docs/contracts/adapters/first-party/iproute2.yaml");
    let yaml = iproute2.replacen(
        "address: {from: \"\", template: \"{local}/{prefixlen}\"}",
        "address: {from: \"\", template: \"plain\"}",
        1,
    );
    let pack = AdapterPack::parse(&yaml).expect("the pack parses");
    let problems = validate(&pack, ono_value::builtin_schemas(), &fixtures_root());
    assert!(
        problems.iter().any(|p| p.detail.contains("template")),
        "a template without `{{field}}` placeholders cannot derive anything, got {problems:?}"
    );
}
