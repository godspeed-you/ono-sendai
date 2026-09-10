//! A copy never writes through a symlink at its destination (v0.6 §43.5, §15.4).
//!
//! §43.5: "Protection of one object followed by mutation of a replaced symlink target is
//! unacceptable." A destination that is a link is refused rather than followed: the object a plan
//! protected and revalidated is the link's own path, and the file it happens to point at is a
//! different object nobody protected.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod change_support;

use std::os::unix::fs::symlink;

use change_support::{home, ono_at};

#[test]
fn should_refuse_to_copy_through_a_symlink_at_the_destination() {
    let home = home();
    let root = home.path();
    let source = root.join("source");
    let victim = root.join("victim");
    let link = root.join("link");
    std::fs::write(&source, "new bytes\n").expect("written");
    std::fs::write(&victim, "the victim's own bytes\n").expect("written");
    symlink(&victim, &link).expect("a link");

    let run = ono_at(
        root,
        &format!(
            "copy file {} {} --overwrite",
            source.display(),
            link.display()
        ),
    );

    assert_eq!(
        std::fs::read_to_string(&victim).expect("readable"),
        "the victim's own bytes\n",
        "§43.5: the file the link points at is not the object the copy was about"
    );
    assert!(
        !run.status().is_success(),
        "the refusal is a failure a script can see, got {:?}",
        run.stdout()
    );
    assert!(
        std::fs::symlink_metadata(&link)
            .expect("the link is still there")
            .file_type()
            .is_symlink(),
        "and the link itself is left as it was"
    );
}
