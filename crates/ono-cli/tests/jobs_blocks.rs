//! A backgrounded pipeline that runs a block is a job (spec §18.4, ADR-0024, ADR-0952).
//!
//! `… | each { … } &` used to fail with `type.mismatch` — "a background job cannot mix native
//! stages with external programs yet" — or, with only native stages, ran the block as nothing at
//! all (issue #193). A block holds statements, and only an evaluator runs statements, so a job
//! that runs one has an evaluator of its own: listed by `jobs` and `get job`, collected by `fg`,
//! stopped by `kill %N` with its children, and never reading the terminal.
//!
//! No test here asserts a duration. Where a job has to have started a child before it is killed,
//! the script waits for a file the child writes, and the shell's watchdog bounds the wait.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use ono_testkit::{Scratch, scratch};

use support::{processes_naming, read_until, run_bounded};

const BUDGET: Duration = Duration::from_secs(60);

#[test]
fn should_run_a_backgrounded_block_as_a_job_that_fg_collects() {
    // Issue #193's exit test: "`each { … } &` runs". The job is listed while the shell goes on,
    // and `fg` hands over what its block produced.
    let scratch = scratch();

    let run = run_bounded(
        &scratch,
        "echo \"[1,2,3]\" | from json | each { @ * 2 } | to json &\njobs\nfg %1",
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert_eq!(run.code, Some(0), "{}", run.report());
    assert!(
        !run.stderr.contains("E0201"),
        "a backgrounded block is a job, not a refusal. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("[%1]") && run.stdout.contains("each { @ * 2 }"),
        "`jobs` lists it under its number and as it was typed (spec §18.4). {}",
        run.report()
    );
    assert!(
        run.stdout.contains("[2,4,6]"),
        "`fg` collects what the block produced for every item. {}",
        run.report()
    );
}

#[test]
fn should_keep_what_a_background_block_rebinds_inside_the_job() {
    // ADR-0952: a job's evaluator starts from a copy of the session's scopes. A block that
    // advances a counter advances the job's copy, item after item, and the session the job was
    // started from still holds the value it had.
    let scratch = scratch();

    let run = run_bounded(
        &scratch,
        "let seen = 0\n\
         echo \"[10,20,30]\" | from json | each { let seen = $seen + 1; $seen } | to json &\n\
         fg %1\n\
         echo \"seen-in-the-shell-$seen\"",
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        run.stdout.contains("[1,2,3]"),
        "inside the job the rebinding carries from item to item. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("seen-in-the-shell-0"),
        "and it does not leak into the session the job was started from. {}",
        run.report()
    );
}

#[test]
fn should_report_a_background_blocks_failure_structured_with_its_status() {
    // §43: a job's failure is a structured error like any other, reported when the job is
    // collected, and its status is the one the line would have had in the foreground (ADR-0008).
    let scratch = scratch();

    let run = run_bounded(
        &scratch,
        "echo \"[1]\" | from json | each { a-command-no-host-has } &\n\
         fg %1\n\
         echo \"status-was-$?\"",
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        run.stderr.contains("resolve.command_not_found"),
        "the block's own error is what is reported. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("status-was-127"),
        "and `fg` ends with the status the line ended with. {}",
        run.report()
    );
}

#[test]
fn should_publish_a_finished_background_block_in_get_job_with_its_status() {
    let scratch = scratch();

    let run = run_bounded(
        &scratch,
        "echo \"[1,2]\" | from json | each { @ } &\n\
         while (get job | where state == \"running\" | count) > 0 { sleep 0.05 }\n\
         get job | select id kind state exit_status | to json",
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    let jobs: serde_json::Value =
        serde_json::from_str(run.stdout.trim()).expect("`to json` writes one document");
    assert_eq!(
        jobs,
        serde_json::json!([{"id": 1, "kind": "native", "state": "done", "exit_status": 0}]),
        "the job table holds the finished job and its status (spec §18.4). {}",
        run.report()
    );
}

#[test]
fn should_stop_a_background_block_and_its_child_when_the_job_is_killed() {
    // `kill %N` stops the job: its source is read no further, and the program its block is
    // running is signalled and reaped rather than left behind (v0.4.1 §28.4). The block's child
    // writes a file once it runs, and the script waits for that file before it kills the job, so
    // there is a child to stop.
    let scratch = scratch();
    let source = scratch.write("jobs/source.log", "first\n");
    let child = long_running_child(&scratch, "killed");
    let started = started_marker(&child);

    let run = run_bounded(
        &scratch,
        &format!(
            "tail file {} --lines 1 --follow | each {{ {} }} &\n\
             sh -c 'while [ ! -e {} ]; do sleep 0.05; done'\n\
             kill %1\n\
             jobs\n\
             echo jobs-listed",
            source.display(),
            child.display(),
            started.display()
        ),
        BUDGET,
    );

    assert!(
        run.finished,
        "the job was stopped, so nothing keeps the shell from ending. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("jobs-listed") && !run.stdout.contains("[%1]"),
        "a killed job leaves the table. {}",
        run.report()
    );
    assert!(
        gone_within(&child, BUDGET),
        "the program the block was running was stopped with the job. {}",
        run.report()
    );
}

#[test]
fn should_leave_the_shells_working_directory_where_it_was_when_a_job_tries_to_move() {
    // Review R2: a job's evaluator is a thread of the shell's own process, and the kernel keeps
    // one working directory per process. A `cd` in a job moved the foreground with it, so the
    // next relative path the shell resolved — `find file .` — was read in the job's directory.
    // A job cannot have a working directory of its own, so moving it is refused in the job, and
    // the shell stays where it stands.
    for movement in ["cd elsewhere", "enter dir elsewhere"] {
        let scratch = scratch();
        scratch.write("start/here-marker", "");
        scratch.write("start/elsewhere/away-marker", "");
        let start = scratch.path().join("start");

        let run = run_bounded(
            &scratch,
            &format!(
                "cd {}\n\
                 fn wander() {{ {movement}; echo moved }}\n\
                 wander &\n\
                 fg %1\n\
                 echo \"status-was-$?\"\n\
                 find file . --depth 1 | to json",
                start.display()
            ),
            BUDGET,
        );

        assert!(run.finished, "{}", run.report());
        assert!(
            run.stdout.contains("here-marker") && !run.stdout.contains("away-marker"),
            "`{movement}` in a job: the shell resolves its relative paths where it stands. {}",
            run.report()
        );
        assert!(
            run.stderr
                .contains("a background job cannot change the working directory")
                && run.stdout.contains("status-was-1")
                && !run.stdout.contains("moved"),
            "`{movement}` in a job is refused, structured, and the job ends there. {}",
            run.report()
        );
    }
}

// --- the same job, at a terminal ---------------------------------------------------------------

#[test]
fn should_not_let_a_background_block_read_the_terminal() {
    // Spec §18.4: a background job does not read the terminal. The block runs `cat`, which reads
    // its standard input; a job that handed it the terminal would leave it stopped by the kernel
    // — or, worse, reading what the user types next. The line typed after the job is the shell's.
    let directory = scratch();
    let mut shell = support::interactive_shell_in(&directory);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(b"echo \"[1]\" | from json | each { cat } | to json &\n")
        .expect("the terminal accepts input");
    read_until(&mut shell, "[%1]", Duration::from_secs(10));
    // `$?` is typed and `0` comes back: only the shell's own evaluation of the line can write
    // `read-by-the-shell-0`, so the line did not go to the job's `cat`.
    shell
        .write_all(b"echo \"read-by-the-shell-$?\"\n")
        .expect("the terminal accepts input");
    let seen = read_until(&mut shell, "read-by-the-shell-0", Duration::from_secs(20));
    assert!(
        seen.contains("read-by-the-shell-0"),
        "the line typed after the job reached the shell, not the job's `cat`; saw:\n{seen}"
    );

    shell
        .write_all(b"fg %1\necho \"fg-ended-$?\"\n")
        .expect("the terminal accepts input");
    let seen = read_until(&mut shell, "fg-ended-0", Duration::from_secs(20));
    assert!(
        seen.contains("fg-ended-0"),
        "the job ended by itself — its `cat` read an empty input — and `fg` collected it; \
         saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

#[test]
fn should_interrupt_a_foregrounded_block_job_with_ctrl_c_and_reap_its_child() {
    // `fg %1` gives the job the foreground: Ctrl-C then ends it — its child included — and the
    // shell stays standing with the status every shell reports for an interrupted job (ADR-0008).
    let directory = scratch();
    let source = directory.write("jobs/source.log", "first\n");
    let child = long_running_child(&directory, "interrupted");
    let started = started_marker(&child);
    let mut shell = support::interactive_shell_in(&directory);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(
            format!(
                "tail file {} --lines 1 --follow | each {{ {} }} &\n",
                source.display(),
                child.display()
            )
            .as_bytes(),
        )
        .expect("the terminal accepts input");
    read_until(&mut shell, "[%1]", Duration::from_secs(10));
    assert!(
        appears_within(&started, BUDGET),
        "the job's block started its child"
    );

    // The interrupt has to arrive while `fg` is waiting. A byte typed while the line editor still
    // holds the terminal is a keystroke, not a signal, so the line announces that it is running
    // — `$?` is typed and `0` comes back — and the Ctrl-C follows that, with a pause for `fg` to
    // reach its wait. The job never ends by itself, so there is no other state to be in.
    shell
        .write_all(b"echo \"foregrounding-$?\"; fg %1\n")
        .expect("the terminal accepts input");
    read_until(&mut shell, "foregrounding-0", Duration::from_secs(20));
    std::thread::sleep(Duration::from_millis(300));
    shell
        .write_all(&[0x03])
        .expect("the terminal accepts Ctrl-C");
    shell
        .write_all(b"echo \"after-fg-$?\"\n")
        .expect("the terminal accepts input");
    let seen = read_until(&mut shell, "after-fg-130", Duration::from_secs(20));

    assert!(
        seen.contains("after-fg-130"),
        "Ctrl-C ended the foregrounded job and the shell survived it; saw:\n{seen}"
    );
    assert!(
        gone_within(&child, BUDGET),
        "the job's child was stopped with it; saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

/// A script that announces it has started and then waits far longer than any test runs, at a
/// path no other process names.
fn long_running_child(scratch: &Scratch, name: &str) -> std::path::PathBuf {
    let path = scratch.path().join(format!("jobs/child-{name}.sh"));
    std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory exists");
    let started = started_marker(&path);
    support::executable(
        &path,
        &format!("#!/bin/sh\n: > '{}'\nsleep 3600\n", started.display()),
    );
    path
}

fn started_marker(child: &Path) -> std::path::PathBuf {
    child.with_extension("started")
}

fn appears_within(path: &Path, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Whether every process whose command line names `path` is gone within `budget`.
fn gone_within(path: &Path, budget: Duration) -> bool {
    let needle = path.display().to_string();
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if processes_naming(&needle).is_empty() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}
