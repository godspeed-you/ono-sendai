//! Tests for schema-aware completion (spec §15.1): after `where` and `select` the shell
//! offers the fields of the schema the pipeline carries at that point.
//!
//! The candidates are looked up from `docs/contracts/commands/*.yaml` (the command's output schema)
//! and `docs/contracts/schemas/*.v1.yaml` (its fields); nothing here runs a provider.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared helper in a test binary states its preconditions the same way a #[test] \
              body does (AGENTS.md section 16)"
)]

use ono_command::Candidate;

mod support;
use support::complete;

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
             `{field}` (docs/contracts/schemas/process.v1.yaml); got {names:?}"
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
             (docs/contracts/schemas/file.v1.yaml), not Process; got {names:?}"
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

// --- inside a predicate the position decides, not the word count (#133, #134, #135) ---------

fn accepts_path(line: &str) -> bool {
    ono_command::accepts_path(
        support::registry(),
        &ono_command::StageContext::from_line(line, line.len()),
    )
}

const COMPARISONS: [&str; 8] = ["!=", "<", "<=", "==", ">", ">=", "in", "not in"];

#[test]
fn should_never_accept_a_path_inside_a_predicate() {
    // Issue #133: the REPL fell back to the working directory whenever the registry had nothing,
    // and inside a predicate that was every position after the first. A path is never an
    // operator, a connector or a bare comparand there.
    for line in [
        "get process | where ",
        "get process | where cpu ",
        "get process | where cpu > ",
        "get process | where cpu > 5 ",
        "get process | where cpu > 5 and ",
        "get process | where nosuch ",
        "get process | select name ",
        "get process | sort ",
    ] {
        assert!(
            !accepts_path(line),
            "`{line}`: an expression-mode argument is not a path position (v0.6.1 §16)"
        );
    }
}

#[test]
fn should_accept_a_path_where_a_path_can_stand() {
    // `get file` takes a path; an external program's arguments may be anything.
    for line in ["get file sr", "cat fo", "cd ", "cat "] {
        assert!(accepts_path(line), "`{line}` is a path position");
    }
}

#[test]
fn should_offer_the_operators_of_a_bytesize_field_after_it() {
    // Issue #134, second row: after a field path, the comparisons its declared type supports.
    assert_eq!(
        texts(&complete("get filesystem | where size ")),
        COMPARISONS,
        "`size` is a bytesize: equality, ordering and membership (ADR-0860)"
    );
}

#[test]
fn should_offer_the_operators_that_continue_what_is_typed_when_the_field_has_no_space() {
    // Compact syntax: `size>` is a field and the start of an operator (v0.6.1 §11).
    assert_eq!(
        texts(&complete("get filesystem | where size>")),
        [">", ">="]
    );
    assert_eq!(
        texts(&complete("get process | where name !")),
        ["!=", "!~="]
    );
}

#[test]
fn should_offer_the_regex_operators_for_a_text_field_and_no_ordering() {
    assert_eq!(
        texts(&complete("get process | where name ")),
        ["!=", "!~=", "==", "in", "not in", "~="]
    );
}

#[test]
fn should_offer_equality_and_the_connectors_after_a_bool_field() {
    // `where read_only` is a whole predicate already, so it may also be continued.
    assert_eq!(
        texts(&complete("get filesystem | where read_only ")),
        ["!=", "==", "and", "or"]
    );
}

#[test]
fn should_offer_ordering_for_an_enum_field_because_its_variants_are_ordered() {
    // ADR-0222: an enum field orders by its declared variants.
    assert_eq!(texts(&complete("get socket | where state ")), COMPARISONS);
}

#[test]
fn should_carry_the_language_doc_on_an_operator_candidate() {
    let candidates = complete("get process | where name ~");
    let regex = candidates
        .iter()
        .find(|candidate| candidate.text() == "~=")
        .unwrap_or_else(|| panic!("`~=` is offered; got {:?}", texts(&candidates)));
    assert_eq!(regex.kind(), ono_command::CandidateKind::Operator);
    assert!(
        regex
            .doc()
            .is_some_and(|doc| doc.starts_with("Regex match")),
        "the operator's doc is the one docs/contracts/language.yaml gives it (issue #136), got \
         {:?}",
        regex.doc()
    );
}

#[test]
fn should_offer_the_connectors_after_a_complete_comparison() {
    // Issue #134, third row.
    for line in [
        "get filesystem | where size > 1GB ",
        "get filesystem | where size>1GB ",
        "get socket | where state == listen ",
        "get process | where name == \"ono\" ",
    ] {
        assert_eq!(texts(&complete(line)), ["and", "or"], "after `{line}`");
    }
    assert_eq!(
        texts(&complete("get filesystem | where size > 1GB a")),
        ["and"]
    );
}

#[test]
fn should_offer_the_fields_again_after_a_connector() {
    // Issue #134, fourth row, spaced and compact.
    for line in [
        "get filesystem | where size > 1GB and ",
        "get filesystem | where size>1GB and ",
        "get filesystem | where not ",
        "get filesystem | where (size > 1GB or ",
    ] {
        let names = texts(&complete(line));
        assert!(
            offers(&names, "size") && offers(&names, "read_only"),
            "after `{line}` a new operand starts; got {names:?}"
        );
    }
    assert_eq!(
        texts(&complete("get filesystem | where size>1GB and re")),
        ["read_only"]
    );
}

#[test]
fn should_offer_the_connectors_after_a_closed_group() {
    assert_eq!(
        texts(&complete(
            "get filesystem | where (size > 1GB or read_only) "
        )),
        ["and", "or"]
    );
}

#[test]
fn should_offer_nothing_where_the_line_cannot_be_read_as_a_predicate() {
    // v0.6.1 §20: incomplete is normal, unreadable fails conservatively — no guess.
    for line in [
        "get process | where nosuch ",
        "get process | where cpu > > ",
        "get process | where name == \"ab",
        "get process | where cpu + ",
        "get filesystem | where size > 1GB size ",
        "get process | where name == foo bar ",
    ] {
        assert_eq!(
            texts(&complete(line)),
            Vec::<String>::new(),
            "after `{line}`"
        );
    }
}

#[test]
fn should_offer_the_declared_variants_after_an_enum_comparison() {
    // Issue #135: the check already reads a variant as a bare word (ADR-0096).
    let names = texts(&complete("get socket | where state == "));
    for variant in ["established", "listen", "time-wait"] {
        assert!(offers(&names, variant), "got {names:?}");
    }
    assert_eq!(
        texts(&complete("get socket | where state == ESTA")),
        ["established"],
        "a variant typed in the wrong case still finds its spelling (v0.6.1 §14)"
    );
    assert_eq!(
        texts(&complete("get socket | where state==lis")),
        ["listen"]
    );
}

#[test]
fn should_offer_true_and_false_after_a_bool_comparison() {
    assert_eq!(
        texts(&complete("get filesystem | where read_only == ")),
        ["false", "true"]
    );
    assert_eq!(
        texts(&complete("get filesystem | where read_only != tr")),
        ["true"]
    );
}

#[test]
fn should_offer_the_byte_units_after_a_number_compared_with_a_bytesize() {
    // Spec §15.1: "`memory > 5<tab>` MAY suggest byte units rather than unrelated filenames".
    let names = texts(&complete("get filesystem | where size > 5"));
    for unit in ["5B", "5KiB", "5GiB", "5GB", "5PB"] {
        assert!(offers(&names, unit), "got {names:?}");
    }
    assert!(
        !offers(&names, "5s"),
        "a time unit is not a byte unit; got {names:?}"
    );
    assert_eq!(
        texts(&complete("get filesystem | where size>5G")),
        ["5GB", "5GiB"]
    );
    let gib = complete("get filesystem | where size > 5Gi");
    assert_eq!(texts(&gib), ["5GiB"]);
    assert_eq!(
        gib[0].doc(),
        Some("gibibyte"),
        "the unit's doc from language.yaml"
    );
}

#[test]
fn should_offer_the_time_units_after_a_number_compared_with_a_duration() {
    let names = texts(&complete("get process | where cpu_window < 5"));
    for unit in ["5ns", "5ms", "5s", "5m", "5h", "5d", "5w"] {
        assert!(offers(&names, unit), "got {names:?}");
    }
    assert!(!offers(&names, "5GiB"), "got {names:?}");
}

#[test]
fn should_offer_a_list_after_in_and_the_variants_inside_it() {
    assert_eq!(texts(&complete("get socket | where state in ")), ["["]);
    let names = texts(&complete("get socket | where state in ["));
    assert!(offers(&names, "listen"), "got {names:?}");
    let names = texts(&complete("get socket | where state in [listen, "));
    assert!(offers(&names, "established"), "got {names:?}");
    assert_eq!(
        texts(&complete(
            "get socket | where state in [listen, established] "
        )),
        ["and", "or"]
    );
}

#[test]
fn should_offer_no_value_for_an_open_text_field() {
    // Offering values would mean reading a provider, which #135 does not ask for.
    assert_eq!(
        texts(&complete("get process | where name == ")),
        Vec::<String>::new()
    );
}

#[test]
fn should_offer_another_field_after_a_selected_one() {
    // `select` takes field paths, one after another; an operator would be a wrong answer there.
    let names = texts(&complete("get process | select name "));
    assert!(offers(&names, "pid"), "got {names:?}");
    assert!(!offers(&names, "=="), "got {names:?}");
}
