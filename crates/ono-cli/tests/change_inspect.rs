//! `inspect plan --resolution`, the view that explains why several recovery assets are planned
//! (spec v0.6 Appendix B.10, §20).
//!
//! Appendix B.10 asks for an expansion per target — mount, filesystem, dataset or subvolume,
//! recovery boundary — because a plan whose targets sit in two recovery boundaries needs two
//! assets, and the only way an operator can see why is to see where each target's state lives.
//! Each test drives the real binary, reads the drawing, and checks `| to json` beside it so the
//! structured value carries the same facts.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use std::path::Path;

use change_support::{home, one, ono_at, plan, text};
use serde_yaml_ng::Value;

/// A plan over two copies in `home`: its short reference and the two paths it overwrites.
fn two_copy_plan(home: &Path) -> (String, [String; 2]) {
    let mut destinations = Vec::new();
    let mut actions = Vec::new();
    for name in ["first", "second"] {
        let source = home.join(format!("{name}.new"));
        let destination = home.join(format!("{name}.conf"));
        std::fs::write(&source, b"new\n").expect("the source is written");
        std::fs::write(&destination, b"old\n").expect("the destination is written");
        actions.push(format!(
            "copy file {} {} --overwrite",
            source.display(),
            destination.display()
        ));
        destinations.push(destination.display().to_string());
    }
    let reference = plan(home, &format!("{{ {} }}", actions.join("; ")));
    (
        reference,
        [destinations[0].clone(), destinations[1].clone()],
    )
}

/// The resolution rows `inspect plan --resolution | to json` carried.
fn resolution_rows(record: &Value) -> Vec<Value> {
    record
        .get("ono.change/resolution")
        .and_then(Value::as_sequence)
        .unwrap_or_else(|| {
            panic!(
                "Appendix B.10: `inspect plan --resolution` carries the expansion in the \
                 namespaced extension `ono.change/resolution`. Got {record:?}"
            )
        })
        .clone()
}

#[test]
fn should_resolve_every_target_to_where_its_state_lives_when_the_resolution_is_asked_for() {
    let home = home();
    let (reference, destinations) = two_copy_plan(home.path());

    let record = one(&ono_at(
        home.path(),
        &format!("inspect plan {reference} --resolution | to json"),
    ));

    let rows = resolution_rows(&record);
    for destination in &destinations {
        let row = rows
            .iter()
            .find(|row| text(row, "path") == *destination)
            .unwrap_or_else(|| {
                panic!("Appendix B.10: every target has its own resolution; {destination} has none in {rows:?}")
            });
        for field in ["mount_point", "filesystem"] {
            assert!(
                !text(row, field).is_empty(),
                "Appendix B.10: the expansion names the {field} behind {destination}. Got {row:?}"
            );
        }
        // Appendix B.1: a filesystem with no snapshot boundary of its own has none to name, and
        // a volatile one is no persistence domain at all (B.7). Either way the row answers the
        // question rather than leaving it out, and says why where the answer is "none".
        let boundary = row.get("recovery_boundary").unwrap_or_else(|| {
            panic!("Appendix B.10: the expansion states the recovery boundary of {destination}. Got {row:?}")
        });
        assert!(
            boundary.as_str().is_some_and(|name| !name.is_empty())
                || (boundary.is_null()
                    && (row["not_persistent"].as_str().is_some()
                        || row["object_kind"].as_str().is_some())),
            "Appendix B.10: a target without a recovery boundary says why. Got {row:?}"
        );
    }
}

#[test]
fn should_draw_the_resolution_of_each_target_under_its_own_heading() {
    let home = home();
    let (reference, destinations) = two_copy_plan(home.path());

    let run = ono_at(
        home.path(),
        &format!("inspect plan {reference} --resolution"),
    );

    run.assert_success();
    let drawn = run.stdout();
    assert!(
        drawn.contains("PERSISTENCE RESOLUTION"),
        "Appendix B.10: the expansion is drawn as its own section. Got {drawn:?}"
    );
    for destination in &destinations {
        assert!(
            drawn.contains(destination.as_str()),
            "Appendix B.10: the drawing names {destination}. Got {drawn:?}"
        );
    }
    assert_eq!(
        drawn.matches("recovery boundary").count(),
        2,
        "Appendix B.10: each of the two targets states its own recovery boundary, which is what \
         says whether one asset covers both. Got {drawn:?}"
    );
}

#[test]
fn should_leave_the_resolution_out_when_it_is_not_asked_for() {
    let home = home();
    let (reference, _) = two_copy_plan(home.path());

    let drawn = ono_at(home.path(), &format!("inspect plan {reference}"));
    let record = one(&ono_at(
        home.path(),
        &format!("inspect plan {reference} | to json"),
    ));

    drawn.assert_success();
    assert!(
        !drawn.stdout().contains("PERSISTENCE RESOLUTION"),
        "§20: the expansion is a section a reader asks for. Got {:?}",
        drawn.stdout()
    );
    assert!(
        record.get("ono.change/resolution").is_none(),
        "v0.2 §10.4: the extension travels only when the view was asked for. Got {record:?}"
    );
}
