//! A preparation that cannot happen stops the mutation (spec v0.6 §2.3, §4.5, §55.7 case 31,
//! ADR-0825).
//!
//! §2.3: if a required recovery asset cannot be created, mutation MUST NOT begin. ADR-0825 makes
//! "required" mean what the operator approved — a plan sealed showing its target protected keeps
//! that protection or refuses — so the default `prefer` mode cannot turn a failed preparation into
//! an unprotected change. These tests take the recovery store away between seal and apply.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use std::path::Path;

use change_support::{build_disk_home, one, ono_at, text};

/// Seals a protected copy of `source` over `target`, answering with the plan's id.
fn sealed_copy(home: &Path, source: &Path, target: &Path) -> String {
    let planned = ono_at(
        home,
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            target.display()
        ),
    );
    planned.assert_success();
    let plan = one(&planned);
    assert_eq!(
        text(&plan, "protection_level"),
        "protected",
        "the fixture's plan is sealed protected, so its preparation is one the operator approved"
    );
    text(&plan, "id")
}

#[test]
fn should_refuse_to_mutate_when_the_protection_the_plan_was_sealed_with_cannot_be_created() {
    let home = build_disk_home("prepare-failure");
    let source = home.path().join("new.conf");
    let first = home.path().join("first.conf");
    let locked = home.path().join("locked.conf");
    std::fs::write(&source, "worker_processes 8;\n").expect("the source is written");
    std::fs::write(&first, "one\n").expect("a first target is written");
    std::fs::write(&locked, "before\n").expect("the target is written");
    // A first protected apply makes the recovery store the second one will not be able to use.
    let warm = sealed_copy(home.path(), &source, &first);
    ono_at(home.path(), &format!("apply {}", &warm[..8])).assert_success();
    let plan = sealed_copy(home.path(), &source, &locked);
    let store = home.path().join("data/ono/change/recovery");
    assert!(store.is_dir(), "the first apply made the recovery store");
    let aside = store.with_extension("aside");
    std::fs::rename(&store, &aside).expect("the store is moved aside");
    std::fs::write(&store, "not a directory\n").expect("a file stands where the store was");

    let applied = ono_at(home.path(), &format!("apply {}", &plan[..8]));

    std::fs::remove_file(&store).expect("the stand-in is removed");
    std::fs::rename(&aside, &store).expect("the store is put back");
    assert!(
        !applied.status().is_success(),
        "§2.3: an apply whose protection could not be created fails. Got {:?}",
        applied.output()
    );
    assert!(
        applied.stderr().contains("change.prepare_failed")
            || applied.stderr().contains("recovery.asset_create_failed"),
        "§45: the refusal is the prepare failure. Got {:?}",
        applied.stderr()
    );
    assert_eq!(
        std::fs::read_to_string(&locked).expect("the target is readable"),
        "before\n",
        "§2.3: the target is byte-for-byte as it was"
    );
    assert_eq!(
        text(
            &one(&ono_at(
                home.path(),
                &format!("get plan {} | to json", &plan[..8])
            )),
            "state"
        ),
        "prepare-failed",
        "§4.1 and Appendix F: the plan is prepare-failed, a state with no edge to APPLYING"
    );
}
