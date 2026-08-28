//! The release evidence of `docs/ACCEPTANCE.md` §4.7: the checklist of the v0.4 tranche is held
//! against the tree, so it cannot rot.
//!
//! ADR-0137 fixed what closes a v0.4 box — a named test running un-ignored in the gate, or a
//! case running in the container — and named this file as the guard for the boxes no ordinary
//! test can reach. Everything here is a statement about *evidence*, not about the shell:
//!
//! * every test, case and file `docs/ACCEPTANCE.md` §4.7 names as a proof exists, lives where
//!   the gate or the acceptance suite runs it, and is not `#[ignore]`d — so a box whose proof
//!   was renamed away fails here rather than staying ticked;
//! * no `*.case.v04` file is left behind, because a scenario the referee does not collect is a
//!   scenario nobody runs;
//! * each of the thirteen unit areas v0.4 §43.1 requires has a test of its own;
//! * every test the spatial enumeration review (ADR-0203) names in its threat table is real;
//! * no §44 case types the name of the object it is supposed to discover, which is the house
//!   rule of `docker/acceptance/cases/README-v0.4.md` and what makes those cases evidence for
//!   §52.3's qualitative statement rather than only for §44.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repo().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{relative} is readable: {error}"))
}

/// The text of `docs/ACCEPTANCE.md` §4.7, which is the v0.4 definition of done.
fn checklist() -> String {
    let acceptance = read("docs/ACCEPTANCE.md");
    let start = acceptance
        .find("### 4.7 The v0.4 tranche")
        .expect("docs/ACCEPTANCE.md carries §4.7, the v0.4 tranche");
    let end = acceptance[start..]
        .find("\n## 5. Stopping rule")
        .map_or(acceptance.len(), |offset| start + offset);
    acceptance[start..end].to_owned()
}

/// Every `` `file.rs::test_name` `` a passage names, with the file each one belongs to.
///
/// The checklist writes a file once and then lists several of its tests as bare `::name`, the
/// way a reader reads it; the file in force is carried along.
fn named_tests(passage: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut current: Option<String> = None;
    for token in passage.split('`').skip(1).step_by(2) {
        let Some((file, name)) = token.split_once("::") else {
            if token.ends_with(".rs") && !token.contains(' ') {
                current = Some(token.to_owned());
            }
            continue;
        };
        if !file.is_empty() {
            current = Some(file.to_owned());
        }
        let name = name.trim();
        if name.is_empty() || !name.starts_with("should_") {
            continue;
        }
        let Some(file) = current.clone() else {
            panic!("`::{name}` is named before any file it could belong to");
        };
        found.push((file, name.to_owned()));
    }
    found
}

/// Where a test file named in the checklist actually lives.
///
/// The checklist names some of them by their whole path and some by their bare file name; both
/// have to resolve to exactly one file that the workspace's `cargo test` runs.
fn locate(file: &str) -> PathBuf {
    for candidate in [repo().join(file), repo().join("crates").join(file)] {
        if candidate.is_file() {
            return candidate;
        }
    }
    let mut hits: Vec<PathBuf> = Vec::new();
    for crate_dir in std::fs::read_dir(repo().join("crates"))
        .expect("the crates directory exists")
        .flatten()
    {
        let candidate = crate_dir.path().join("tests").join(file);
        if candidate.is_file() {
            hits.push(candidate);
        }
    }
    assert_eq!(
        hits.len(),
        1,
        "docs/ACCEPTANCE.md §4.7 names `{file}` as a proof; it must be exactly one file under a \
         crate's `tests/`, found {hits:?}"
    );
    hits.into_iter().next().expect("one hit")
}

/// Whether `source` declares `name` as a test, and whether that test is ignored.
fn declared(source: &str, name: &str) -> Option<bool> {
    let needle = format!("fn {name}(");
    let at = source.find(&needle)?;
    let before = &source[..at];
    let ignored = before
        .lines()
        .rev()
        .take_while(|line| {
            let line = line.trim_start();
            line.starts_with('#') || line.starts_with("//") || line.is_empty()
        })
        .any(|line| line.trim_start().starts_with("#[ignore"));
    Some(ignored)
}

fn assert_proofs_exist(passage: &str, what: &str, least: usize) {
    let mut missing = Vec::new();
    let mut ignored = Vec::new();
    let named = named_tests(passage);
    assert!(
        named.len() >= least,
        "{what} names at least {least} tests; the harvester found {} — it has stopped reading \
         what it is meant to read",
        named.len()
    );
    for (file, name) in named {
        let path = locate(&file);
        assert!(
            path.starts_with(repo().join("crates")) || path.starts_with(repo().join("xtask")),
            "{what} names `{file}::{name}`, which is outside the suites the gate runs"
        );
        let source = std::fs::read_to_string(&path).expect("a named test file is readable");
        match declared(&source, &name) {
            None => missing.push(format!("{file}::{name}")),
            Some(true) => ignored.push(format!("{file}::{name}")),
            Some(false) => {}
        }
    }
    assert!(
        missing.is_empty(),
        "{what} names proofs that do not exist — rename them there in the increment that renames \
         the test: {missing:?}"
    );
    assert!(
        ignored.is_empty(),
        "{what} names proofs that are `#[ignore]`d, so they prove nothing: {ignored:?}"
    );
}

#[test]
fn should_find_every_test_the_v04_checklist_names_as_a_proof() {
    // ADR-0137: a box is ticked by a named test running un-ignored in the gate. A proof that was
    // renamed away, deleted or ignored leaves a box ticked by nothing, which is the one failure
    // `docs/ACCEPTANCE.md` §3 forbids.
    assert_proofs_exist(&checklist(), "docs/ACCEPTANCE.md §4.7", 100);
}

#[test]
fn should_find_every_acceptance_case_the_v04_checklist_names() {
    // §4.7 names its cases by number — "case `090`", "cases `091`, `093`". Each must be a file
    // `scripts/acceptance.sh` collects, which is to say a `*.case`.
    let passage = checklist();
    let numbers: BTreeSet<String> = passage
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|token| token.len() == 3 && token.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_owned)
        .collect();
    assert!(
        numbers.len() >= 10,
        "§4.7 names the ten §44 scenarios by number, found {numbers:?}"
    );
    let cases: Vec<String> = std::fs::read_dir(repo().join("docker/acceptance/cases"))
        .expect("the acceptance cases exist")
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "case"))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let missing: Vec<&String> = numbers
        .iter()
        .filter(|number| !cases.iter().any(|case| case.starts_with(number.as_str())))
        .collect();
    assert!(
        missing.is_empty(),
        "§4.7 names cases the referee does not collect: {missing:?}"
    );
}

#[test]
fn should_leave_no_v04_scenario_out_of_the_acceptance_suite() {
    // The rename rule of §4.7: `scripts/acceptance.sh` collects `*.case`, so a `*.case.v04` file
    // is a scenario the referee cannot see. None may remain once the tranche is delivered.
    let held_out: Vec<String> = std::fs::read_dir(repo().join("docker/acceptance/cases"))
        .expect("the acceptance cases exist")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".case.v04"))
        .collect();
    assert!(
        held_out.is_empty(),
        "these scenarios are still held out of the suite: {held_out:?}"
    );
}

/// The thirteen areas v0.4 §43.1 requires unit coverage for, and the test that covers each.
///
/// The mapping is the reviewer's; what this file guarantees is that no area is left without one
/// and that the test it names is real. An area whose test is renamed fails here.
const UNIT_AREAS: [(&str, &str, &str); 13] = [
    (
        "SpatialId stability",
        "crates/ono-spatial-core/tests/identity.rs",
        "should_resolve_two_observations_of_one_object_to_the_same_id_when_the_facts_are_the_same",
    ),
    (
        "canonical parent selection",
        "crates/ono-spatial-core/tests/hierarchy.rs",
        "should_choose_the_same_canonical_parent_whatever_order_the_edges_arrive_in",
    ),
    (
        "selector precedence",
        "crates/ono-spatial-query/tests/resolution.rs",
        "should_prefer_an_exact_index_match_over_an_approximate_visible_one",
    ),
    (
        "ambiguity detection",
        "crates/ono-spatial-query/tests/resolution.rs",
        "should_answer_ambiguous_with_the_disambiguating_context_when_two_places_share_a_name",
    ),
    (
        "neighborhood ranking",
        "crates/ono-spatial-query/tests/neighborhood.rs",
        "should_rank_a_pinned_neighbor_first_and_name_it_as_a_landmark",
    ),
    (
        "clustering",
        "crates/ono-spatial-query/tests/map.rs",
        "should_stay_inside_the_text_budget_and_cluster_the_rest_when_the_horizon_is_larger",
    ),
    (
        "landmark thresholds",
        "crates/ono-spatial-query/tests/landmarks.rs",
        "should_follow_the_configured_threshold_when_deciding_that_cpu_is_high",
    ),
    (
        "trail operations",
        "crates/ono-spatial-core/tests/trail.rs",
        "should_return_to_the_previous_place_when_going_back_from_a_place_it_entered",
    ),
    (
        "tombstone resolution",
        "crates/ono-spatial-core/tests/trail.rs",
        "should_resolve_a_removed_object_to_its_tombstone_while_one_is_held",
    ),
    (
        "relation inverse handling",
        "crates/ono-spatial-core/tests/relations.rs",
        "should_keep_an_inverted_edge_the_same_assertion",
    ),
    (
        "scope boundary detection",
        "crates/ono-spatial-core/tests/hierarchy.rs",
        "should_report_the_outermost_boundary_when_a_movement_crosses_several",
    ),
    (
        "map node/edge filtering",
        "crates/ono-spatial-query/tests/map.rs",
        "should_remove_edges_without_inventing_any_when_a_relation_filter_narrows_the_map",
    ),
    (
        "permission-state preservation",
        "crates/ono-spatial-index/tests/conformance.rs",
        "should_map_every_refusal_a_provider_can_state_to_one_of_the_six_states",
    ),
];

#[test]
fn should_cover_every_unit_area_the_test_strategy_requires() {
    // v0.4 §43.1 lists thirteen areas that "required unit coverage includes"; §4.7.2's unit box
    // is ticked on the strength of this test, so an area without a test fails the gate.
    let mut without = Vec::new();
    for (area, file, name) in UNIT_AREAS {
        let source = read(file);
        match declared(&source, name) {
            Some(false) => {}
            Some(true) => without.push(format!("{area}: {file}::{name} is ignored")),
            None => without.push(format!("{area}: {file}::{name} does not exist")),
        }
    }
    assert!(
        without.is_empty(),
        "v0.4 §43.1 areas without a test that runs: {without:?}"
    );
}

#[test]
fn should_find_every_test_the_spatial_enumeration_review_names() {
    // ADR-0137: the security review of §52.2 is closed by an ADR whose threat table names a
    // passing test per §35 boundary, and by this test asserting that those tests are real.
    let review = read("docs/decisions/ADR-0203-the-spatial-enumeration-review.md");
    let rows: String = review
        .lines()
        .filter(|line| line.starts_with("| T"))
        .collect::<Vec<&str>>()
        .join("\n");
    assert!(
        rows.lines().count() >= 5,
        "§35 has five boundaries; the review names {} rows",
        rows.lines().count()
    );
    assert_proofs_exist(&rows, "the spatial enumeration review (ADR-0203)", 12);

    let cases_named: Vec<&str> = rows
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|token| token.ends_with(".case"))
        .collect();
    for case in cases_named {
        assert!(
            repo().join(case).is_file(),
            "the review names `{case}`, which the referee does not collect"
        );
    }
}

/// What the §43.3 fixtures call the objects the §44 scenarios are supposed to *discover*.
const DISCOVERED_NAMES: [&str; 3] = [
    "fixture-web.service",
    "fixture-backup.service",
    "fixture-web-wor",
];

#[test]
fn should_keep_every_scenario_from_typing_the_name_of_what_it_discovers() {
    // The house rule of `docker/acceptance/cases/README-v0.4.md`, and, per ADR-0137, what makes
    // cases 090–099 evidence for §52.3's qualitative statement: "a technically experienced user
    // … without needing to know the object names … in advance". A name may be asserted on; it
    // may never be typed as input. Mechanically: every occurrence of a discovered name is inside
    // a pattern being matched — a `grep` argument or a comparison — never earlier on its line
    // than the thing that matches it.
    let mut typed = Vec::new();
    for entry in std::fs::read_dir(repo().join("docker/acceptance/cases"))
        .expect("the acceptance cases exist")
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("09") || !name.ends_with(".case") {
            continue;
        }
        let case = std::fs::read_to_string(entry.path()).expect("a case is readable");
        for (number, line) in case.lines().enumerate() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            for discovered in DISCOVERED_NAMES {
                let Some(at) = line.find(discovered) else {
                    continue;
                };
                let before = &line[..at];
                if before.contains("grep") || before.contains("= \"") || before.contains("-qF") {
                    continue;
                }
                typed.push(format!("{name}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(
        typed.is_empty(),
        "a §44 scenario types the name of the object it is supposed to discover: {typed:#?}"
    );
}

#[test]
fn should_report_a_checklist_proof_that_no_longer_exists() {
    // The guard's own guard: a renamed proof must be found, not silently accepted.
    let passage =
        "- [ ] **A box** — `spatial_navigation_missing.rs::should_never_have_been_written`.";
    let found = named_tests(passage);
    assert_eq!(
        found,
        vec![(
            "spatial_navigation_missing.rs".to_owned(),
            "should_never_have_been_written".to_owned()
        )],
        "the harvester reads a box's named proof"
    );
    let source = std::fs::read_to_string(locate(&found[0].0)).expect("the suite is readable");
    assert!(
        declared(&source, &found[0].1).is_none(),
        "a test nobody wrote must not be reported as declared"
    );
}
