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

#[test]
fn should_keep_what_a_job_binds_in_the_environment_out_of_the_shells() {
    // Review R7: `get env` answers for the session that asks (ADR-0952 copies the environment
    // into the job). A job that binds a variable item after item must never be what the shell's
    // own `get env` reports, however the two interleave.
    let scratch = scratch();
    let lines: String = (1..=3000).map(|line| format!("{line}\n")).collect();
    let source = scratch.write("jobs/lines.log", lines);

    let run = run_bounded(
        &scratch,
        &format!(
            "tail file {} --lines 3000 | each {{ set env LEAKED job; get env LEAKED | count }} &\n\
             let i = 0\n\
             while $i < 300 {{ get env LEAKED | count | to json; let i = $i + 1 }}\n\
             kill %1",
            source.display()
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    let reads: Vec<&str> = run
        .stdout
        .lines()
        .filter(|line| line.starts_with('[') && !line.starts_with("[%"))
        .collect();
    assert!(
        reads.len() >= 300,
        "every read of the shell's environment answered. {}",
        run.report()
    );
    assert!(
        reads[..300].iter().all(|read| *read == "[0]"),
        "the shell never sees the job's binding. {}",
        run.report()
    );
}

#[test]
fn should_keep_a_killed_job_listed_while_its_child_has_not_stopped() {
    // Review C2: `kill %N` signals the job and waits a bounded moment. A child that ignores the
    // signal keeps the job's evaluator waiting on it, so the job has not ended — and a job that
    // has not ended stays in the table, where `jobs` shows it and the shell can still reach it,
    // rather than leaving a thread and a process nobody tracks.
    let scratch = scratch();
    let source = scratch.write("jobs/source.log", "first\n");
    let child = Stubborn::at(&scratch, "killed");

    let run = run_bounded(
        &scratch,
        &format!(
            "tail file {} --lines 1 --follow | each {{ {} }} &\n\
             sh -c 'while [ ! -e {} ]; do sleep 0.05; done'\n\
             kill %1\n\
             echo \"killed-$?\"\n\
             jobs",
            source.display(),
            child.path.display(),
            child.started().display()
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        run.stdout.contains("killed-0"),
        "`kill %1` delivered its signal. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("[%1]"),
        "the job whose child ignored the signal is still listed. {}",
        run.report()
    );
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

#[test]
fn should_keep_a_foregrounded_job_listed_when_ctrl_c_does_not_stop_its_child() {
    // Review C2, under `fg`: Ctrl-C ends the wait and gives the prompt back with 130, and a job
    // whose child ignored the interrupt is still a job — listed, not dropped.
    let directory = scratch();
    let source = directory.write("jobs/source.log", "first\n");
    let child = Stubborn::at(&directory, "interrupted");
    let mut shell = support::interactive_shell_in(&directory);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(
            format!(
                "tail file {} --lines 1 --follow | each {{ {} }} &\n",
                source.display(),
                child.path.display()
            )
            .as_bytes(),
        )
        .expect("the terminal accepts input");
    read_until(&mut shell, "[%1]", Duration::from_secs(10));
    assert!(
        appears_within(&child.started(), BUDGET),
        "the job's block started its child"
    );

    shell
        .write_all(b"echo \"foregrounding-$?\"; fg %1\n")
        .expect("the terminal accepts input");
    read_until(&mut shell, "foregrounding-0", Duration::from_secs(20));
    std::thread::sleep(Duration::from_millis(300));
    shell
        .write_all(&[0x03])
        .expect("the terminal accepts Ctrl-C");
    shell
        .write_all(b"echo \"after-fg-$?\"; jobs; echo \"listed-$?\"\n")
        .expect("the terminal accepts input");
    let seen = read_until(&mut shell, "listed-0", Duration::from_secs(30));

    assert!(
        seen.contains("after-fg-130"),
        "Ctrl-C ended the wait with the interrupt's status; saw:\n{seen}"
    );
    let after = seen.split("after-fg-130").nth(1).unwrap_or_default();
    assert!(
        after.contains("[%1]"),
        "the job whose child ignored Ctrl-C is still listed; saw:\n{seen}"
    );

    drop(child);
    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

#[test]
fn should_end_a_job_program_that_stopped_for_the_terminal() {
    // Review R14: a job's programs never get the terminal (ADR-0952 §3). One that tries to set
    // the terminal is stopped by `SIGTTOU`, and nobody can continue it — the job's own job table
    // is nobody's to `fg`. It was left stopped behind a job that said `done`; it is ended, and
    // the job says why.
    let directory = scratch();
    let child = directory.path().join("jobs/wants-the-terminal.sh");
    std::fs::create_dir_all(child.parent().expect("a directory")).expect("the directory exists");
    support::executable(
        &child,
        "#!/bin/sh\nstty -echo < /dev/tty\necho never-reached\n",
    );
    let mut shell = support::interactive_shell_in(&directory);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(
            format!(
                "echo \"[1]\" | from json | each {{ {} }} &\n",
                child.display()
            )
            .as_bytes(),
        )
        .expect("the terminal accepts input");
    read_until(&mut shell, "[%1]", Duration::from_secs(10));
    // `fg` waits for the job, which ends once its program is ended, and reports why.
    shell
        .write_all(b"fg %1\n")
        .expect("the terminal accepts input");
    let seen = read_until(&mut shell, "was ended", Duration::from_secs(30));

    assert!(
        gone_within(&child, Duration::from_secs(10)),
        "the program that stopped for the terminal was ended with its job; saw:\n{seen}"
    );
    assert!(
        seen.contains("external.signal") && seen.contains("was ended"),
        "and the job reports why when it is collected; saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
    for pid in processes_naming(&child.display().to_string()) {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
}

/// A child that ignores `SIGTERM` and `SIGINT`, announces it has started, and runs until this
/// test kills it — at a path no other process names.
struct Stubborn {
    path: std::path::PathBuf,
}

impl Stubborn {
    fn at(scratch: &Scratch, name: &str) -> Self {
        let path = scratch.path().join(format!("jobs/stubborn-{name}.sh"));
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory exists");
        let started = started_marker(&path);
        support::executable(
            &path,
            &format!(
                "#!/bin/sh\ntrap '' TERM INT\nexec </dev/null >/dev/null 2>&1\n: > '{}'\nwhile :; do sleep 1; done\n",
                started.display()
            ),
        );
        Self { path }
    }

    fn started(&self) -> std::path::PathBuf {
        started_marker(&self.path)
    }
}

impl Drop for Stubborn {
    fn drop(&mut self) {
        support::kill_processes_naming(&self.path.display().to_string());
    }
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
