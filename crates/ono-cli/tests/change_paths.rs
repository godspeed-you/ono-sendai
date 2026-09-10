//! A plan over a path is a plan over that path, whatever characters it holds (spec v0.6 §7.1,
//! §7.3).
//!
//! §7.1 freezes a file by its canonical path, and §7.3 revalidates the plan against the same path
//! at `apply`. A path is bytes the operator chose: a Btrfs subvolume is conventionally named
//! `@var`, and a revalidation that read anything but the whole path would find drift where
//! nothing changed, or miss it where something did.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{build_disk_home, one, ono_at, text};

#[test]
fn should_apply_a_plan_whose_path_holds_an_at_sign_from_another_process() {
    let home = build_disk_home("change-paths-at-sign");
    let directory = home.path().join("@var");
    std::fs::create_dir_all(&directory).expect("the directory is created");
    let source = directory.join("source");
    let target = directory.join("target");
    std::fs::write(&source, "after the change\n").expect("the source is written");
    std::fs::write(&target, "before the change\n").expect("the target is written");
    let planned = ono_at(
        home.path(),
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            target.display()
        ),
    );
    planned.assert_success();
    let plan = text(&one(&planned), "id");

    let applied = ono_at(home.path(), &format!("apply {}", &plan[..8]));

    applied.assert_success();
    assert_eq!(
        std::fs::read_to_string(&target).expect("the target is readable"),
        "after the change\n",
        "§7.3: nothing changed between the seal and the apply, so the plan ran"
    );
}
