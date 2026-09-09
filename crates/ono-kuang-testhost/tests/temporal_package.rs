//! The test host validates a package's temporal contributions before it is loaded (v0.5 §37.4,
//! §37.5, §30.7; §48.9).
//!
//! §37.5 lets a package expose the past of an external system and requires it to "map data into
//! canonical Ono objects/events and expose coverage/provenance". Everything that requirement
//! implies is checkable on disk, so it is checked on disk: a source that maps into nothing
//! canonical, produces a kind Ono does not have, or states no coverage is a problem a publisher
//! meets before a user does.

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_kuang_testhost::check_temporal_package;

const SOURCES: &str = "\
temporal_sources:
  - id: dev.example.packet-eye.temporal-source.flows
    summary: Flow records from the collector's archive.
    schema: dev.example.packet-eye.flow/1
    kinds: [object.changed]
    answer: bounded
    coverage: Whatever the collector retains, stated per query.
    retained_history: 24h
";

const RULES: &str = "\
causal_rules:
  - rule_id: dev.example.packet-eye.retransmit-to-drop
    relation: correlated_with
    strength: correlated
    summary: Retransmissions cluster around interface drops.
    inputs: [object.changed]
    identity_constraints: The same interface identity on both sides.
";

const TARGETS: &str = "\
targets:
  - name: flow
    schema: dev.example.packet-eye.flow/1
    summary: One observed flow.
    identity_doc: The five-tuple and the observation window.
";

/// A package directory whose manifest points at the documents given.
fn package(sources: Option<&str>, rules: Option<&str>, capabilities: &str) -> ono_testkit::Scratch {
    let scratch = ono_testkit::scratch();
    scratch.write("contributions/targets.yaml", TARGETS);
    let mut declared = String::from("  targets: [contributions/targets.yaml]\n");
    if let Some(document) = sources {
        scratch.write("contributions/sources.yaml", document);
        declared.push_str("  temporal_sources: [contributions/sources.yaml]\n");
    }
    if let Some(document) = rules {
        scratch.write("contributions/rules.yaml", document);
        declared.push_str("  causal_rules: [contributions/rules.yaml]\n");
    }
    scratch.write(
        "manifest.yaml",
        format!(
            "format: kuang-package/1\n\
             package:\n  \
               id: dev.example.packet-eye\n  \
               name: packet-eye\n  \
               version: 0.1.0\n  \
               description: Watches packets.\n  \
               publisher: dev.example\n  \
               license: MIT\n\
             compatibility:\n  \
               kuang_api: \">=11.1 <12\"\n  \
               ono_language: \">=0.2\"\n  \
               platforms: [linux-amd64, linux-arm64]\n\
             runtime:\n  \
               kind: native-process\n  \
               entry: runtime/eye\n  \
               memory_max: 64MiB\n  \
               cpu_budget: interactive\n  \
               startup: lazy\n\
             roles: [provider]\n\
             contributions:\n\
             {declared}\
             {capabilities}\
             network:\n  \
               outbound: none\n"
        ),
    );
    scratch
}

const BOTH_GRANTS: &str = "capabilities:\n  optional:\n    - temporal.contribute.events\n    \
                           - temporal.contribute.causality\n";

#[test]
fn should_report_a_contributed_history_provider_with_its_coverage_and_its_boundedness() {
    let scratch = package(Some(SOURCES), Some(RULES), BOTH_GRANTS);

    let report = check_temporal_package(scratch.path());

    assert_eq!(report.problems, Vec::<String>::new());
    assert_eq!(
        report.sources,
        ["dev.example.packet-eye.temporal-source.flows"],
        "§37.5: the source the package would contribute, named before it runs"
    );
    assert_eq!(
        report.bounded_sources,
        ["dev.example.packet-eye.temporal-source.flows"],
        "a historical query provider is bounded, and the host knows before the first record"
    );
    assert_eq!(report.rules, ["dev.example.packet-eye.retransmit-to-drop"]);
    assert!(report.contributes_when_granted);
}

#[test]
fn should_report_that_history_never_reaches_a_package_by_default() {
    let scratch = package(Some(SOURCES), None, BOTH_GRANTS);

    let report = check_temporal_package(scratch.path());

    assert!(
        !report.history_by_default,
        "§30.7 and §31.19: nothing about the past is granted by default"
    );
}

#[test]
fn should_refuse_a_source_that_maps_into_nothing_canonical() {
    let scratch = package(
        Some(&SOURCES.replace(
            "schema: dev.example.packet-eye.flow/1",
            "schema: some.foreign.thing/1",
        )),
        None,
        BOTH_GRANTS,
    );

    let report = check_temporal_package(scratch.path());

    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("canonical Ono objects")),
        "§37.5: the provider MUST map data into canonical Ono objects: {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_source_that_produces_an_event_kind_ono_does_not_have() {
    let scratch = package(
        Some(&SOURCES.replace("kinds: [object.changed]", "kinds: [flow.retransmitted]")),
        None,
        BOTH_GRANTS,
    );

    let report = check_temporal_package(scratch.path());

    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("subtype")),
        "§37.1: core retains the event vocabulary, and the refusal says where a refinement goes: \
         {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_source_that_states_no_coverage() {
    let scratch = package(
        Some(&SOURCES.replace(
            "coverage: Whatever the collector retains, stated per query.",
            "coverage: \"\"",
        )),
        None,
        BOTH_GRANTS,
    );

    let report = check_temporal_package(scratch.path());

    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("coverage")),
        "§37.5: a contributed source exposes coverage and provenance: {:?}",
        report.problems
    );
}

#[test]
fn should_tell_a_package_that_its_causal_claim_will_be_capped() {
    let scratch = package(
        None,
        Some(&RULES.replace("strength: correlated", "strength: authoritative")),
        BOTH_GRANTS,
    );

    let report = check_temporal_package(scratch.path());

    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("will carry `asserted`")),
        "§37.4: a package learns before it ships that its `authoritative` will be `asserted`: \
         {:?}",
        report.problems
    );
    assert_eq!(report.strength_ceiling.as_deref(), Some("asserted"));
}

#[test]
fn should_refuse_a_causal_rule_outside_the_packages_namespace() {
    let scratch = package(
        None,
        Some(&RULES.replace(
            "rule_id: dev.example.packet-eye.retransmit-to-drop",
            "rule_id: ono.action-to-transaction",
        )),
        BOTH_GRANTS,
    );

    let report = check_temporal_package(scratch.path());

    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("namespaced")),
        "§37.4: a third-party rule is namespaced to its publisher: {:?}",
        report.problems
    );
    assert!(report.rules.is_empty());
}

#[test]
fn should_say_a_contribution_can_never_land_when_the_package_asks_for_no_capability() {
    let scratch = package(Some(SOURCES), Some(RULES), "");

    let report = check_temporal_package(scratch.path());

    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("temporal.contribute.events")),
        "a source with no grant is a source that would contribute nothing: {:?}",
        report.problems
    );
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("temporal.contribute.causality")),
        "and so is a rule: {:?}",
        report.problems
    );
    assert!(!report.contributes_when_granted);
}

#[test]
fn should_say_so_when_a_package_contributes_nothing_about_the_past() {
    let scratch = package(None, None, BOTH_GRANTS);

    let report = check_temporal_package(scratch.path());

    assert_eq!(
        report.problems.len(),
        1,
        "one problem, and it is that there is nothing to check: {:?}",
        report.problems
    );
    assert!(report.sources.is_empty());
    assert!(report.rules.is_empty());
}
