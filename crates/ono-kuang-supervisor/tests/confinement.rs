//! What a native plugin runs inside, and what happens when it cannot be made to (v0.4.1 spec
//! §0.5.3, §2.3, §16.1–§16.3, §59.7, Appendix D; issues #59 and #60).
//!
//! `crates/ono-kuang-supervisor/src/sandbox.rs` says of the controls it installs between `fork`
//! and `exec`: *"so the artifact never executes an instruction outside it"*. Appendix D says the
//! same in a table — for `no_new_privs`, resource limits and session separation the Failure
//! column reads `spawn fails` — and §2.3 makes it a release-wide invariant: *"If Ono claims that
//! a safety control is applied before an operation, failure to apply that control MUST prevent
//! the operation from starting."*
//!
//! At the time this suite was written none of that was true. Every confinement syscall in the
//! pre-exec closure discards its return value and the closure ends in an unconditional `Ok(())`,
//! so a control that the kernel refused to install is a control nobody hears about, and the
//! artifact execs anyway — inside whatever confinement happened to survive.
//!
//! # How the failure is arranged
//!
//! §59.7 asks for "an injectable platform layer/test hook" that makes a mandatory control fail.
//! This suite needs no such layer, because one mandatory control can be made to fail from
//! outside the process with nothing but the standard library, deterministically and without
//! privileges: `setsid` returns `EPERM` when the calling process is already a process-group
//! leader, and `Command::process_group(0)` makes the child exactly that before the pre-exec
//! closure runs. Session separation is mandatory — §16.4 lists it as `mandatory for the native
//! supervised tier`, Appendix D as `required`, failure `spawn fails` — so a child that reaches
//! `exec` after that call failed is the defect of §0.5.3, observed rather than simulated.
//!
//! The test below is the failure proof required by §57 phase H0 before the fixes in issues #59
//! and #60 may land. It is `#[ignore]`d because it fails at HEAD, and the increment that makes
//! pre-exec failures fatal removes the attribute rather than the test.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::PathBuf;

use ono_kuang_protocol::CpuBudget;
use ono_kuang_supervisor::{apply, native_process, working_directory};

/// The session id in a `/proc/<pid>/stat` line.
///
/// The command name is parenthesised and may itself contain spaces, so everything after the last
/// `)` is positional: `state`, `ppid`, `pgrp`, `session`.
fn session_of(stat: &str) -> Option<&str> {
    stat.rsplit_once(')')?.1.split_whitespace().nth(3)
}

/// The session this test process runs in, which a child that separated itself must not share.
fn our_session() -> String {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("this process has a stat line");
    session_of(&stat)
        .expect("a stat line carries the session id")
        .to_owned()
}

// REASON: the failure proof of v0.4.1 §57 phase H0 for §0.5.3 — the pre-exec closure in
// `sandbox.rs` discards every confinement syscall result and returns `Ok(())`, so the artifact
// execs after a mandatory control was refused. Issues #59 and #60 make it fatal. ADR-0430.
#[tokio::test]
#[ignore = "RED proof for issues #59 and #60: the plugin execs after a mandatory control failed"]
async fn should_not_exec_the_plugin_when_a_mandatory_confinement_control_fails() {
    let root = std::env::temp_dir().join(format!("ono-confinement-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a private directory for the fixture plugin");
    let marker = root.join("the-plugin-ran");

    // Stands in for the plugin's runtime artifact: the first thing it does on startup is record
    // that it started, which is the marker §59.7 requires to remain absent.
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(format!("cat /proc/self/stat > {}", marker.display()))
        // Makes the child its own process-group leader before the pre-exec closure runs, so the
        // mandatory `setsid` in that closure is refused with EPERM.
        .process_group(0);
    let sandbox = native_process(
        512 * 1024 * 1024,
        CpuBudget::Interactive,
        working_directory(Some(&root), &PathBuf::from("/bin/sh")),
    );
    apply(&mut command, &sandbox);

    let spawned = command.spawn();
    if let Ok(mut child) = spawned {
        let _ = child.wait().await;
    }
    let ran = std::fs::read_to_string(&marker).ok();
    let ran_in_session = ran.as_deref().and_then(session_of).map(str::to_owned);
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(
        ran_in_session,
        None,
        "v0.4.1 §2.3 and §16.3: a mandatory control that could not be installed MUST prevent the \
         plugin from starting, and Appendix D spells session separation out — `required`, \
         failure `spawn fails`. `setsid` was refused here because the child was already a \
         process-group leader, and the artifact ran regardless — in the supervisor's own session \
         {}, which is the confinement it was supposed to have left. A confinement report calling \
         this spawn confined would be claiming it of nothing (§16.5).",
        our_session()
    );
}
