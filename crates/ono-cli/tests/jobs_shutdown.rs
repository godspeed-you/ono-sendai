//! A shell that leaves ends the jobs it still owns (issue #303).
//!
//! On exit the shell asked each job to stop with `SIGTERM`, waited up to two seconds for it, one
//! job after another, and gave up — no `SIGKILL`. A job's programs run in process groups of their
//! own, so the terminal's hangup never reaches them either: a child that ignored `SIGTERM`
//! outlived the shell as an orphan, and every such job added its two seconds to the exit.
//!
//! Each test starts a program that ignores `SIGTERM` and `SIGINT` from a path no other process
//! names, waits for the file it writes once it runs, and lets the shell end. Whether the program
//! is gone is polled with a deadline, and a guard kills whatever is left so a failing test leaves
//! nothing behind.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ono_testkit::{Scratch, scratch};

use support::{processes_naming, run_bounded};

const BUDGET: Duration = Duration::from_secs(60);

/// A program that ignores `SIGTERM` and `SIGINT`, at a path no other process names, and the
/// guard that kills it whatever the test concluded.
struct Stubborn {
    path: PathBuf,
}

impl Stubborn {
    fn at(scratch: &Scratch, name: &str) -> Self {
        Self::written(scratch, &format!("stubborn-{name}"), "trap '' TERM INT\n")
    }

    /// The same program without the trap: it ends on the first signal.
    fn yielding(scratch: &Scratch, name: &str) -> Self {
        Self::written(scratch, &format!("yielding-{name}"), "")
    }

    fn written(scratch: &Scratch, name: &str, trap: &str) -> Self {
        let path = scratch.path().join(format!("jobs/{name}.sh"));
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
        support::executable(
            &path,
            &format!(
                "#!/bin/sh\n{trap}exec </dev/null >/dev/null 2>&1\necho $$ > '{}'\n\
                 while :; do sleep 1; done\n",
                Self::started_of(&path).display()
            ),
        );
        Self { path }
    }

    fn started_of(path: &Path) -> PathBuf {
        path.with_extension("started")
    }

    fn started(&self) -> PathBuf {
        Self::started_of(&self.path)
    }

    /// Whether every process naming the program is gone within the budget.
    fn gone_within(&self, budget: Duration) -> bool {
        let needle = self.path.display().to_string();
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if processes_naming(&needle).is_empty() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

impl Drop for Stubborn {
    fn drop(&mut self) {
        support::kill_processes_naming(&self.path.display().to_string());
    }
}

/// The lines that start `programs` as jobs, each from a function so the job has an evaluator of
/// its own, and wait until every one of them runs.
fn start_jobs(programs: &[Stubborn]) -> String {
    let mut script = String::new();
    for (index, program) in programs.iter().enumerate() {
        script.push_str(&format!(
            "fn stubborn{index}() {{ {} }}\nstubborn{index} &\n",
            program.path.display()
        ));
    }
    for program in programs {
        script.push_str(&format!(
            "sh -c 'while [ ! -e {} ]; do sleep 0.02; done'\n",
            program.started().display()
        ));
    }
    script.push_str("echo leaving\n");
    script
}

#[test]
fn should_leave_no_process_of_a_job_behind_when_the_shell_exits_and_its_child_ignores_term() {
    let home = scratch();
    let programs = [Stubborn::at(&home, "alone")];

    let run = run_bounded(&home, &start_jobs(&programs), BUDGET);

    assert!(
        run.finished && run.stdout.contains("leaving"),
        "the shell reached its end and left. {}",
        run.report()
    );
    assert!(
        programs[0].gone_within(Duration::from_secs(10)),
        "the job's program, which ignores SIGTERM, did not outlive the shell. {}",
        run.report()
    );
}

#[test]
fn should_end_several_stubborn_jobs_within_one_grace_period_rather_than_one_each() {
    // Four jobs whose programs ignore SIGTERM. Asked one after another, each cost the shell's
    // exit its own grace period of two seconds — eight in all — and still left all four running.
    // Asked together, they share one: the bound below is two grace periods with room for a
    // loaded machine, which a shell waiting one period per job cannot meet.
    let home = scratch();
    let programs = [
        Stubborn::at(&home, "one"),
        Stubborn::at(&home, "two"),
        Stubborn::at(&home, "three"),
        Stubborn::at(&home, "four"),
    ];
    let script = start_jobs(&programs);

    let begun = Instant::now();
    let run = run_bounded(&home, &script, BUDGET);
    let took = begun.elapsed();

    assert!(run.finished, "{}", run.report());
    assert!(
        took < Duration::from_secs(6),
        "the shell's exit took {took:?} for four stubborn jobs. {}",
        run.report()
    );
    for program in &programs {
        assert!(
            program.gone_within(Duration::from_secs(10)),
            "{} did not outlive the shell. {}",
            program.path.display(),
            run.report()
        );
    }
}

#[test]
fn should_end_a_job_whose_program_was_stopped_when_the_shell_exits() {
    // A program stopped by a signal cannot act on SIGTERM until it is continued. Whether the
    // job's evaluator notices the stop first (ADR-0952's review R14) or the shell's exit reaches
    // it while stopped — and continues it — the program does not outlive the shell. A guard of
    // the outcome rather than of one path to it.
    let home = scratch();
    let programs = [Stubborn::at(&home, "stopped")];
    let mut script = start_jobs(&programs);
    script = script.replace(
        "echo leaving\n",
        &format!(
            "sh -c 'kill -STOP $(cat {})'\necho leaving\n",
            programs[0].started().display()
        ),
    );

    let run = run_bounded(&home, &script, BUDGET);

    assert!(
        run.finished && run.stdout.contains("leaving"),
        "{}",
        run.report()
    );
    assert!(
        programs[0].gone_within(Duration::from_secs(10)),
        "the stopped program did not outlive the shell. {}",
        run.report()
    );
}

#[test]
fn should_leave_promptly_when_its_jobs_end_on_the_signal() {
    // The common case stays cheap: a job whose program ends on SIGTERM, and a native job with
    // no program at all, are over long before the grace period is.
    let home = scratch();
    let log = home.write("jobs/follow.log", "line\n");
    let programs = [Stubborn::yielding(&home, "polite")];
    let script = start_jobs(&programs).replace(
        "echo leaving\n",
        &format!("tail file {} --follow &\necho leaving\n", log.display()),
    );

    let begun = Instant::now();
    let run = run_bounded(&home, &script, BUDGET);
    let took = begun.elapsed();

    assert!(
        run.finished && run.stdout.contains("leaving"),
        "{}",
        run.report()
    );
    assert!(
        programs[0].gone_within(Duration::from_secs(10)),
        "the job's program ended with the shell. {}",
        run.report()
    );
    assert!(
        took < Duration::from_millis(1900),
        "jobs that end on the signal do not hold the exit for the grace period: {took:?}. {}",
        run.report()
    );
}
