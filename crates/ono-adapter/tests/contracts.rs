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

// --- the historical query plan (spec v0.5 §23.2) ----------------------------------------------

const SYSTEMD: &str = include_str!("../../../docs/contracts/adapters/first-party/systemd.yaml");

/// The `journalctl` adapter of the bundled systemd pack.
fn journalctl() -> &'static ono_adapter::Adapter {
    ono_adapter::first_party()
        .iter()
        .flat_map(ono_adapter::AdapterPack::adapters)
        .find(|adapter| adapter.full_id() == "org.ono.compat.systemd.journalctl")
        .expect("the bundled systemd pack carries the journalctl adapter")
}

#[test]
fn should_declare_the_five_things_a_historical_plan_must_state() {
    // v0.5 §23.2: "A historical adapter plan MUST declare: historical_query: true, coverage
    // semantics, source timestamp mapping, identity mapping, deduplication key if available."
    // journalctl is the section's own example, and it is the only adapter in this tree that
    // declares one — §23.3 keeps `ps`, `ss`, `ip address` and `lsblk` current observations.
    let plan = journalctl()
        .temporal()
        .expect("`journalctl --since ... --until ...` has explicit historical semantics");
    assert!(plan.historical_query());
    assert_eq!(
        plan.coverage(),
        ono_adapter::HistoricalCoverage::RetainedWindow,
        "the journal's window is journald's own, which Ono neither controls nor reads"
    );
    assert_eq!(
        plan.source_time(),
        "timestamp",
        "`__REALTIME_TIMESTAMP` is the source's own instant"
    );
    assert_eq!(plan.identity(), ["boot_id", "unit"]);
    assert_eq!(
        plan.deduplication_key(),
        Some("cursor"),
        "`__CURSOR` is journald's own unique handle for one entry (v0.5 §6.8)"
    );
}

#[test]
fn should_spell_a_time_bound_the_way_the_contract_declares_rather_than_the_caller_guessing() {
    // §23.2's plan is what makes the declaration executable: a caller asking about a window does
    // not have to know journalctl's flags or its time formats.
    let plan = journalctl().temporal().expect("a declared plan");
    let at: jiff::Timestamp = "2026-08-31T12:17:00Z".parse().expect("a fixed instant");
    assert_eq!(plan.invocation(), "query");
    assert_eq!(plan.since_argument(at), "--since=@1788178620");
    assert_eq!(plan.until_argument(at), "--until=@1788178620");
}

#[test]
fn should_refuse_a_historical_plan_whose_deduplication_key_is_not_a_field_it_produces() {
    let yaml = SYSTEMD.replacen(
        "deduplication_key: cursor",
        "deduplication_key: nonesuch",
        1,
    );
    let problems = problems_of(&yaml);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("nonesuch") && problem.contains("deduplication")),
        "a mapping onto a field the adapter does not produce is a declaration nothing can act \
         on, got {problems:?}"
    );
}

#[test]
fn should_refuse_a_historical_plan_that_names_an_invocation_the_adapter_does_not_declare() {
    let yaml = SYSTEMD.replacen(
        "        invocation: query",
        "        invocation: nonesuch",
        1,
    );
    let problems = problems_of(&yaml);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("nonesuch") && problem.contains("invocation")),
        "got {problems:?}"
    );
}

#[test]
fn should_refuse_a_time_bound_with_nowhere_for_the_instant_to_go() {
    let yaml = SYSTEMD.replacen(
        r#"        since: "--since=@{seconds}""#,
        r#"        since: "--since=yesterday""#,
        1,
    );
    let problems = problems_of(&yaml);
    assert!(
        problems.iter().any(|problem| problem.contains("{seconds}")),
        "got {problems:?}"
    );
}

#[test]
fn should_refuse_a_temporal_block_that_claims_nothing() {
    // The block's presence is the claim (§23.1), so declaring one and setting it to false is a
    // contradiction rather than a way to opt out: an adapter with no history declares no block.
    let yaml = SYSTEMD.replacen(
        "      historical_query: true",
        "      historical_query: false",
        1,
    );
    let problems = problems_of(&yaml);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("historical_query: false")),
        "got {problems:?}"
    );
}

#[test]
fn should_leave_a_current_state_tool_declaring_no_history_at_all() {
    // §23.3: "Adapters for current-state tools such as `ps`, ordinary `ss`, `ip address` or
    // `lsblk` remain current observations unless their underlying tools expose history."
    let with_plans: Vec<String> = ono_adapter::first_party()
        .iter()
        .flat_map(ono_adapter::AdapterPack::adapters)
        .filter(|adapter| adapter.temporal().is_some())
        .map(ono_adapter::Adapter::full_id)
        .collect();
    assert_eq!(
        with_plans,
        ["org.ono.compat.systemd.journalctl"],
        "one adapter in this tree has explicit historical semantics, and it is the journal"
    );
}
