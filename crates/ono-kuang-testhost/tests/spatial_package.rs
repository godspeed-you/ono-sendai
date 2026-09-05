//! The test host validates a package's spatial contributions before it is loaded (spec v0.4
//! §36.1, §35.5; v0.2 §31.7, ADR-0194): the `<from>-><to>` shapes, the relations they would
//! register, and the capability that decides whether any of them ever reaches a map.

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;

/// A package directory holding a manifest, and the target declarations of `targets` where a
/// suite needs them.
fn package(shapes: &str, capabilities: &str) -> ono_testkit::Scratch {
    package_declaring(shapes, capabilities, None)
}

fn package_declaring(
    shapes: &str,
    capabilities: &str,
    targets: Option<&str>,
) -> ono_testkit::Scratch {
    let scratch = ono_testkit::scratch();
    let declared = if targets.is_some() {
        "  targets: [contributions/targets.yaml]\n"
    } else {
        ""
    };
    if let Some(document) = targets {
        scratch.write("contributions/targets.yaml", document);
    }
    scratch.write(
        "manifest.yaml",
        format!(
            "format: kuang-package/1\n\
             package:\n  \
               id: dev.example.topology\n  \
               name: topology\n  \
               version: 0.1.0\n  \
               description: Contributes one relation.\n  \
               publisher: dev.example\n  \
               license: MIT\n\
             compatibility:\n  \
               kuang_api: \">=11.1 <12\"\n  \
               ono_language: \">=0.2\"\n  \
               platforms: [linux-amd64, linux-arm64]\n\
             runtime:\n  \
               kind: native-process\n  \
               entry: runtime/topology\n  \
               memory_max: 64MiB\n  \
               cpu_budget: interactive\n  \
               startup: lazy\n\
             roles: [provider]\n\
             contributions:\n\
             {declared}  \
               relations: [{shapes}]\n\
             {capabilities}\
             network:\n  \
               outbound: none\n"
        ),
    );
    scratch
}

const GRANTABLE: &str = "capabilities:\n  optional:\n    - relation.write\n";

#[test]
fn should_report_the_relations_a_contributing_package_would_register() {
    let scratch = package("\"process->process\", \"process->socket\"", GRANTABLE);
    let report = ono_kuang_testhost::check_spatial_package(Path::new(scratch.path()));
    assert!(report.problems.is_empty(), "got {:#?}", report.problems);
    assert_eq!(
        report.relations,
        vec![
            "dev.example.topology.process_to_process".to_owned(),
            "dev.example.topology.process_to_socket".to_owned(),
        ],
        "spec §31.5, ADR-0194: a contributed relation lives in the package's own namespace, one \
         per declared shape"
    );
}

#[test]
fn should_report_that_no_contribution_reaches_a_map_until_the_capability_is_granted() {
    let scratch = package("\"process->process\"", GRANTABLE);
    let report = ono_kuang_testhost::check_spatial_package(Path::new(scratch.path()));
    assert!(
        !report.enabled_by_default,
        "spec §35.5: the capability filter runs before the merge, and the default policy grants \
         nothing"
    );
    assert!(
        report.enabled_when_granted,
        "spec §36.1: with `relation.write` granted the package's relations do reach a map"
    );
}

#[test]
fn should_refuse_a_package_that_declares_relations_without_asking_for_the_capability() {
    let scratch = package("\"process->process\"", "");
    let report = ono_kuang_testhost::check_spatial_package(Path::new(scratch.path()));
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("relation.write")),
        "spec §35.5: a package whose edges could never reach a map is told so before it is \
         loaded, got {:#?}",
        report.problems
    );
    assert!(!report.enabled_when_granted);
}

#[test]
fn should_refuse_a_shape_that_names_something_the_geography_does_not_place() {
    let scratch = package("\"process->unicorn\"", GRANTABLE);
    let report = ono_kuang_testhost::check_spatial_package(Path::new(scratch.path()));
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("unicorn")),
        "spec §3.3: a contribution shape names a kind of place the geography defines, got {:#?}",
        report.problems
    );
    assert!(
        report.relations.is_empty(),
        "a shape that names nothing registers nothing"
    );
}

#[test]
fn should_report_a_relation_between_two_kinds_of_place_the_package_contributes() {
    // ADR-0585: a shape endpoint may name the id of a schema one of this package's own targets
    // answers with, which is what lets a package relate the kinds of place it contributes. The
    // check reads both halves off the disk — the shape from the manifest, the schema ids from the
    // `contributions.targets` documents §31.68 already reads — so a package author is told the
    // relation this would register before anything has run.
    let scratch = package_declaring(
        "\"dev.example.topology.node/1->dev.example.topology.link/1\"",
        GRANTABLE,
        Some(
            "targets:\n  \
             - name: node\n    \
               schema: dev.example.topology.node/1\n    \
               summary: Nodes.\n    \
               identity_doc: The uid.\n  \
             - name: link\n    \
               schema: dev.example.topology.link/1\n    \
               summary: Links.\n    \
               identity_doc: The uid.\n",
        ),
    );
    let report = ono_kuang_testhost::check_spatial_package(Path::new(scratch.path()));
    assert!(report.problems.is_empty(), "got {:#?}", report.problems);
    assert_eq!(
        report.relations,
        vec!["dev.example.topology.node_to_link".to_owned()],
        "the relation is named for the two kinds of place it runs between, inside the package's \
         own namespace (spec §31.5)"
    );
}

#[test]
fn should_refuse_a_shape_naming_a_schema_no_target_of_this_package_answers_with() {
    // The other half of the same rule: a schema id is not a licence to name anything. A package
    // that declares no target answering with `dev.example.topology.node/1` has not contributed
    // that kind of place, so a shape naming it names nothing — and says so before the package
    // runs rather than when somebody types `follow`.
    let scratch = package("\"dev.example.topology.node/1->process\"", GRANTABLE);
    let report = ono_kuang_testhost::check_spatial_package(Path::new(scratch.path()));
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("dev.example.topology.node/1")),
        "the refusal names the endpoint nothing defines, got {:#?}",
        report.problems
    );
    assert!(
        report.relations.is_empty(),
        "a shape that names nothing registers nothing"
    );
}
