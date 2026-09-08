//! What `xtask spec-check` must refuse about `docs/contracts/temporal/` (spec v0.5 §36.4).
//!
//! Each test takes this repository's own registries, copies them into a scratch tree, breaks one
//! thing, and asserts that the break is reported. Copying rather than hand-writing a fixture is
//! deliberate: the six registries cross-reference each other, so a hand-written minimal set would
//! be a second copy of the specification maintained beside the first, and a drift check whose
//! fixture drifts proves nothing. The first test asserts the copy is clean, so every later
//! assertion is about the break rather than about the fixture.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::path::{Path, PathBuf};

use ono_testkit::{Scratch, scratch};
use xtask::temporal::check;

/// The repository this test runs in.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ has a parent")
        .to_path_buf()
}

/// A scratch copy of everything `temporal::check` reads.
fn copied() -> Scratch {
    let repo = scratch();
    let root = repo_root();
    for relative in [
        "docs/contracts/temporal",
        "docs/contracts/providers",
        "docs/contracts/schemas",
        "crates/ono-temporal-core/src",
    ] {
        copy_tree(&root.join(relative), &repo.path().join(relative));
    }
    for relative in [
        "docs/contracts/errors.yaml",
        "docs/contracts/hardening/registries.yaml",
    ] {
        let body = std::fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"));
        repo.write(relative, body);
    }
    repo
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("scratch directory");
    for entry in std::fs::read_dir(from)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", from.display()))
        .flatten()
    {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            std::fs::copy(&path, &target).expect("copy");
        }
    }
}

fn problems(repo: &Scratch) -> Vec<String> {
    check(repo.path())
        .into_iter()
        .map(|problem| format!("{} — {}", problem.location, problem.detail))
        .collect()
}

/// Rewrites one registry, replacing `from` with `to` exactly once.
fn edit(repo: &Scratch, file: &str, from: &str, to: &str) {
    let relative = format!("docs/contracts/temporal/{file}");
    let path = repo.path().join(&relative);
    let body = std::fs::read_to_string(&path).expect("the registry was copied");
    assert!(
        body.contains(from),
        "the fixture no longer contains {from:?}; update the test with the registry"
    );
    std::fs::write(&path, body.replacen(from, to, 1)).expect("write");
}

fn assert_reports(found: &[String], needle: &str) {
    assert!(
        found.iter().any(|problem| problem.contains(needle)),
        "expected a problem mentioning {needle:?}, got {found:#?}"
    );
}

#[test]
fn should_accept_the_registries_this_repository_ships_when_checked() {
    assert_eq!(problems(&copied()), Vec::<String>::new());
}

#[test]
fn should_report_nothing_when_the_temporal_registries_have_not_arrived() {
    // AGENTS.md §14: a registry arrives with the phase that needs it, so an absent directory is
    // silence rather than a failure. This is what lets the check ship before its consumers.
    let repo = scratch();
    assert_eq!(problems(&repo), Vec::<String>::new());
}

#[test]
fn should_reject_a_required_registry_that_is_missing() {
    let repo = copied();
    std::fs::remove_file(repo.path().join("docs/contracts/temporal/causality.yaml")).expect("rm");
    assert_reports(&problems(&repo), "causality.yaml — does not exist");
}

#[test]
fn should_reject_an_event_kind_the_specification_does_not_define() {
    let repo = copied();
    edit(
        &repo,
        "events.yaml",
        "  - kind: object.observed",
        "  - kind: object.rumoured\n    subject: required\n    rule: invented\n    spec: nowhere\n  - kind: object.observed",
    );
    assert_reports(&problems(&repo), "object.rumoured");
}

#[test]
fn should_reject_a_canonical_event_kind_the_registry_omits() {
    let repo = copied();
    edit(
        &repo,
        "events.yaml",
        "  - kind: landmark.removed",
        "  - kind: landmark.added\n    subject: optional\n    rule: duplicate of the row above, so `landmark.removed` is gone\n    spec: \"§6.1\"\n  - kind: never.emitted",
    );
    let found = problems(&repo);
    assert_reports(&found, "landmark.removed");
    assert_reports(&found, "this registry omits it");
}

#[test]
fn should_reject_a_disappearance_contract_that_permits_a_polling_gap() {
    let repo = copied();
    edit(
        &repo,
        "events.yaml",
        "    - id: polling_gap",
        "    - id: sampling_interval_elapsed",
    );
    assert_reports(&problems(&repo), "polling_gap");
}

#[test]
fn should_reject_a_causal_relation_class_with_no_inverse_label() {
    let repo = copied();
    edit(
        &repo,
        "causality.yaml",
        "    inverse_label: triggered\n",
        "    inverse_label: \"\"\n",
    );
    assert_reports(&problems(&repo), "declares no `inverse_label`");
}

#[test]
fn should_reject_a_causal_rule_emitting_a_relation_no_class_declares() {
    let repo = copied();
    edit(
        &repo,
        "causality.yaml",
        "    output_relation: triggered_by",
        "    output_relation: made_happen",
    );
    assert_reports(&problems(&repo), "made_happen");
}

#[test]
fn should_reject_a_correlation_rule_emitting_a_causal_relation() {
    let repo = copied();
    edit(
        &repo,
        "causality.yaml",
        "    window: 5m\n    output_relation: correlated_with",
        "    window: 5m\n    output_relation: caused_by",
    );
    assert_reports(&problems(&repo), "may emit `correlated_with` only");
}

#[test]
fn should_reject_a_causal_rule_requiring_an_evidence_strength_nobody_declares() {
    let repo = copied();
    edit(
        &repo,
        "causality.yaml",
        "      action.executed: authoritative",
        "      action.executed: conclusive",
    );
    assert_reports(&problems(&repo), "conclusive");
}

#[test]
fn should_reject_a_built_in_causal_rule_that_is_not_registered() {
    let repo = copied();
    edit(
        &repo,
        "causality.yaml",
        "  - rule_id: ono.process-parent",
        "  - rule_id: ono.process-ancestry",
    );
    let found = problems(&repo);
    assert_reports(&found, "ono.process-parent");
    assert_reports(&found, "is not registered");
}

#[test]
fn should_reject_a_renderer_wording_rule_that_permits_causal_language_for_a_correlation() {
    let repo = copied();
    edit(&repo, "causality.yaml", "    - because\n", "");
    assert_reports(&problems(&repo), "because");
}

#[test]
fn should_reject_evidence_strengths_written_out_of_strength_order() {
    let repo = copied();
    edit(
        &repo,
        "evidence.yaml",
        "  - id: authoritative\n    rank: 1",
        "  - id: asserted\n    rank: 1",
    );
    assert_reports(&problems(&repo), "strength order");
}

#[test]
fn should_reject_a_gap_reason_the_specification_does_not_define() {
    let repo = copied();
    edit(
        &repo,
        "evidence.yaml",
        "  - id: corrupt_segment",
        "  - id: probably_fine",
    );
    assert_reports(&problems(&repo), "probably_fine");
}

#[test]
fn should_reject_a_source_naming_an_evidence_class_that_does_not_exist() {
    let repo = copied();
    edit(
        &repo,
        "sources.yaml",
        "    evidence_class: linux.journald",
        "    evidence_class: linux.syslog",
    );
    assert_reports(&problems(&repo), "linux.syslog");
}

#[test]
fn should_reject_procfs_claiming_native_historical_process_coverage() {
    let repo = copied();
    edit(
        &repo,
        "sources.yaml",
        "      current_snapshot: true\n      live_events: false\n      historical_query: false",
        "      current_snapshot: true\n      live_events: false\n      historical_query: true",
    );
    assert_reports(&problems(&repo), "§22.1");
}

#[test]
fn should_reject_a_polled_source_claiming_exhaustive_events() {
    let repo = copied();
    edit(
        &repo,
        "sources.yaml",
        "      exhaustive_events: false\n      causal_tokens: false\n      checkpointable: true\n      retained_history: null\n    coverage:\n      completeness: point_sample\n      sampled: true",
        "      exhaustive_events: true\n      causal_tokens: false\n      checkpointable: true\n      retained_history: null\n    coverage:\n      completeness: point_sample\n      sampled: true",
    );
    assert_reports(&problems(&repo), "§21.5");
}

#[test]
fn should_reject_a_source_fronted_by_a_provider_no_contract_declares() {
    let repo = copied();
    edit(
        &repo,
        "sources.yaml",
        "    provider: systemd-journal",
        "    provider: syslogd",
    );
    assert_reports(&problems(&repo), "syslogd");
}

#[test]
fn should_reject_a_source_claiming_a_capability_its_provider_does_not_advertise() {
    let repo = copied();
    edit(
        &repo,
        "sources.yaml",
        "  - id: linux.journald\n    evidence_class: linux.journald\n    provider: systemd-journal\n    capabilities:\n      current_snapshot: true\n      live_events: false",
        "  - id: linux.journald\n    evidence_class: linux.journald\n    provider: systemd-journal\n    capabilities:\n      current_snapshot: true\n      live_events: true",
    );
    assert_reports(&problems(&repo), "advertises false");
}

#[test]
fn should_reject_a_recorder_policy_that_omits_something_it_must_not_persist() {
    let repo = copied();
    edit(
        &repo,
        "recorder.yaml",
        "  - id: packet_payloads",
        "  - id: packet_headers",
    );
    assert_reports(&problems(&repo), "packet_payloads");
}

#[test]
fn should_reject_a_recorder_that_records_by_default() {
    let repo = copied();
    edit(
        &repo,
        "recorder.yaml",
        "defaults:\n  enabled: false",
        "defaults:\n  enabled: true",
    );
    assert_reports(&problems(&repo), "§10.2");
}

#[test]
fn should_reject_a_ledger_directory_that_is_not_user_private() {
    let repo = copied();
    edit(
        &repo,
        "recorder.yaml",
        "directory_mode: \"0700\"",
        "directory_mode: \"0755\"",
    );
    assert_reports(&problems(&repo), "§30.2");
}

#[test]
fn should_reject_a_restart_procedure_that_skips_marking_downtime_as_a_gap() {
    let repo = copied();
    edit(
        &repo,
        "recorder.yaml",
        "    id: mark_downtime_as_gap",
        "    id: assume_continuity",
    );
    assert_reports(&problems(&repo), "§44.1");
}

#[test]
fn should_reject_a_setting_default_that_differs_from_the_shell() {
    let repo = copied();
    edit(
        &repo,
        "temporal.yaml",
        "    default: 24h",
        "    default: 48h",
    );
    assert_reports(&problems(&repo), "temporal.retention.max_age");
}

#[test]
fn should_reject_a_setting_declared_with_a_type_the_shell_does_not_use() {
    let repo = copied();
    edit(
        &repo,
        "temporal.yaml",
        "  - key: temporal.session.max_events\n    type: int",
        "  - key: temporal.session.max_events\n    type: bytesize",
    );
    assert_reports(&problems(&repo), "temporal.session.max_events");
}

#[test]
fn should_reject_a_settings_block_that_omits_a_key_the_shell_declares() {
    let repo = copied();
    edit(
        &repo,
        "temporal.yaml",
        "  - key: temporal.ui.show_source_tags",
        "  - key: temporal.ui.show_tags",
    );
    let found = problems(&repo);
    assert_reports(&found, "temporal.ui.show_source_tags");
    assert_reports(&found, "temporal.ui.show_tags");
}

#[test]
fn should_reject_a_prompt_marker_that_does_not_require_supported_coverage() {
    let repo = copied();
    edit(
        &repo,
        "temporal.yaml",
        "    requires: supported_coverage",
        "    requires: any_nearby_event",
    );
    assert_reports(&problems(&repo), "§8.6");
}

#[test]
fn should_reject_a_read_only_policy_that_omits_one_of_the_four_covered_classes() {
    let repo = copied();
    edit(
        &repo,
        "temporal.yaml",
        "    - id: remote_mutation",
        "    - id: link_mutation",
    );
    assert_reports(&problems(&repo), "remote_mutation");
}

#[test]
fn should_reject_a_temporal_capability_key_the_specification_does_not_define() {
    let repo = copied();
    edit(
        &repo,
        "temporal.yaml",
        "  - key: causal_tokens",
        "  - key: causal_hints",
    );
    assert_reports(&problems(&repo), "§21.1");
}

#[test]
fn should_reject_an_error_a_temporal_registry_names_and_the_error_registry_does_not() {
    let repo = copied();
    edit(
        &repo,
        "temporal.yaml",
        "  - temporal.read_only",
        "  - temporal.frozen",
    );
    assert_reports(&problems(&repo), "temporal.frozen");
}

#[test]
fn should_reject_a_schema_a_temporal_registry_names_and_the_schema_directory_does_not_hold() {
    let repo = copied();
    edit(
        &repo,
        "recorder.yaml",
        "      returns: ono.recorder-status/1",
        "      returns: ono.recorder-report/1",
    );
    assert_reports(&problems(&repo), "recorder-report.v1.yaml");
}

#[test]
fn should_reject_a_stable_temporal_command_the_registry_inventory_omits() {
    // The command family file arrives with the implementations that bind it, so the check is
    // silent today. This proves it bites the moment the file lands.
    let repo = copied();
    repo.write(
        "docs/contracts/commands/temporal.yaml",
        "version: 1\nfamily: temporal\ncommands:\n  - id: ono.temporal.rewind\n    stability: stable\n",
    );
    assert_reports(&problems(&repo), "ono.temporal.rewind");
}

#[test]
fn should_accept_a_planned_temporal_command_that_is_not_yet_in_the_inventory() {
    // ADR-0012: a registry describes the whole product, so a `planned` command may point ahead
    // of the inventory. Only `stable` is held to it.
    let repo = copied();
    repo.write(
        "docs/contracts/commands/temporal.yaml",
        "version: 1\nfamily: temporal\ncommands:\n  - id: ono.temporal.rewind\n    stability: planned\n",
    );
    assert_eq!(problems(&repo), Vec::<String>::new());
}

#[test]
fn should_reject_an_event_kind_the_registry_declares_and_the_core_crate_never_names() {
    // The comparison is on the string literals `ono-temporal-core` carries, because `xtask` has
    // to keep compiling while that crate is being written: a `use` of a type that does not exist
    // yet breaks the whole gate rather than reporting one problem. The fixture writes an
    // `EventKind` that has lost one canonical name, which is the drift §36.4 is about.
    let repo = copied();
    let mut source =
        String::from("pub enum EventKind {}\nfn names() -> &'static [&'static str] {\n    &[\n");
    for kind in xtask::temporal::CANONICAL_EVENT_KINDS {
        if kind != "landmark.removed" {
            source.push_str(&format!("        \"{kind}\",\n"));
        }
    }
    source.push_str("    ]\n}\n");
    repo.write("crates/ono-temporal-core/src/event.rs", source);
    assert_reports(&problems(&repo), "landmark.removed");
}

#[test]
fn should_report_nothing_about_the_core_crate_when_it_has_not_declared_its_event_kinds() {
    // AGENTS.md §14 again, one level down: the vocabulary arrives with the crate that owns it,
    // and a check that failed on its absence would turn every other package's gate red.
    let repo = copied();
    let source = repo.path().join("crates/ono-temporal-core/src");
    std::fs::remove_dir_all(&source).expect("rm");
    std::fs::create_dir_all(&source).expect("mkdir");
    assert_eq!(problems(&repo), Vec::<String>::new());
}

#[test]
fn should_reject_a_temporal_registry_the_hardening_inventory_does_not_index() {
    let repo = copied();
    let path = repo.path().join("docs/contracts/hardening/registries.yaml");
    let body = std::fs::read_to_string(&path).expect("the inventory was copied");
    std::fs::write(
        &path,
        body.replacen(
            "  - file: ../temporal/sources.yaml",
            "  - file: ../temporal/source.yaml",
            1,
        ),
    )
    .expect("write");
    assert_reports(&problems(&repo), "docs/contracts/temporal/sources.yaml");
}
