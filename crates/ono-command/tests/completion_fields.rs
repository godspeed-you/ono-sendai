//! Tests for schema-aware completion (spec §15.1): after `where` and `select` the shell
//! offers the fields of the schema the pipeline carries at that point.
//!
//! The candidates are looked up from `docs/spec/commands/*.yaml` (the command's output schema)
//! and `docs/spec/schemas/*.v1.yaml` (its fields); nothing here runs a provider.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared helper in a test binary states its preconditions the same way a #[test] \
              body does (AGENTS.md section 16)"
)]

use ono_command::{Candidate, StageContext};

mod support;
use support::registry;

fn complete(line: &str) -> Vec<Candidate> {
    let cursor = line.len();
    ono_command::complete(registry(), &StageContext::from_line(line, cursor), None)
}

fn texts(candidates: &[Candidate]) -> Vec<String> {
    candidates
        .iter()
        .map(|candidate| candidate.text().to_owned())
        .collect()
}

fn offers(names: &[String], field: &str) -> bool {
    names.iter().any(|name| name == field)
}

#[test]
fn should_offer_the_process_fields_after_where() {
    let names = texts(&complete("get process | where "));

    // Spec §15.1's own example: `get process | where <tab>` shows Process fields.
    for field in ["pid", "ppid", "name", "user", "cpu", "memory", "state"] {
        assert!(
            offers(&names, field),
            "spec §15.1: `where` after `get process` completes the `ono.process/1` field \
             `{field}` (docs/spec/schemas/process.v1.yaml); got {names:?}"
        );
    }
}

#[test]
fn should_narrow_the_field_candidates_by_the_typed_prefix() {
    let narrowed = texts(&complete("get process | where cp"));
    assert_eq!(
        narrowed,
        ["cpu", "cpu_window"],
        "spec §15.1: the typed prefix keeps only the fields that start with it — both of \
         `ono.process/1`'s CPU fields do (ADR-0232), and nothing else does"
    );
}

#[test]
fn should_offer_the_fields_after_select() {
    let names = texts(&complete("get process | select "));

    for field in ["pid", "name", "memory"] {
        assert!(
            offers(&names, field),
            "spec §15.1: `select` projects schema fields, so it completes them; got {names:?}"
        );
    }
}

#[test]
fn should_offer_the_file_fields_after_where_on_a_file_listing() {
    let names = texts(&complete("get file /etc | where "));

    for field in ["name", "kind", "size", "modified"] {
        assert!(
            offers(&names, field),
            "spec §15.1: the schema is the one the head command emits — `ono.file/1` here \
             (docs/spec/schemas/file.v1.yaml), not Process; got {names:?}"
        );
    }
    assert!(
        !offers(&names, "cpu"),
        "a File has no `cpu`; completion is metadata lookup, not a union of every schema"
    );
}

#[test]
fn should_keep_offering_the_fields_after_an_earlier_streaming_transform() {
    // `where` passes records through unchanged, so the schema is still Process two stages in.
    let names = texts(&complete("get process | where cpu > 1 | select "));

    assert!(
        offers(&names, "pid") && offers(&names, "name"),
        "spec §15.1: the schema flows through a filter to the next stage; got {names:?}"
    );
}

#[test]
fn should_carry_the_field_documentation_on_a_field_candidate() {
    let candidates = complete("get process | where pi");
    let pid = candidates
        .iter()
        .find(|candidate| candidate.text() == "pid")
        .unwrap_or_else(|| {
            panic!(
                "`pid` is offered (spec §15.1); got {:?}",
                texts(&candidates)
            )
        });

    assert_eq!(
        pid.doc(),
        Some("The process id."),
        "spec §15.2: help derives from metadata — a field candidate carries the schema's doc"
    );
}
