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

use support::{Streaming, processes_naming, run_bounded};

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
    // no program at all, are over long before the grace period is. Measured from the line that
    // says the shell is leaving, so the shell's start-up is not part of it (review R9).
    let home = scratch();
    let log = home.write("jobs/follow.log", "line\n");
    let programs = [Stubborn::yielding(&home, "polite")];
    let script = start_jobs(&programs).replace(
        "echo leaving\n",
        &format!("tail file {} --follow &\necho leaving\n", log.display()),
    );

    let mut shell = Streaming::start(home.path(), &script);
    assert!(shell.until("leaving"), "the script reached its end");
    let leaving = Instant::now();
    let (finished, stdout, stderr) = shell.finish();
    let took = leaving.elapsed();

    assert!(finished, "{stdout}\n{stderr}");
    assert!(
        programs[0].gone_within(Duration::from_secs(10)),
        "the job's program ended with the shell. {stdout}\n{stderr}"
    );
    assert!(
        took < Duration::from_millis(1500),
        "jobs that end on the signal do not hold the exit for the grace period: {took:?}"
    );
}

#[test]
fn should_end_a_program_backgrounded_at_the_prompt_when_the_shell_exits() {
    // Review R1: a program backgrounded directly is a job the shell owns as much as one a
    // function or a pipeline started; leaving ends it the same way.
    let home = scratch();
    let program = Stubborn::at(&home, "direct");

    let run = run_bounded(
        &home,
        &format!(
            "{} &\nsh -c 'while [ ! -e {} ]; do sleep 0.02; done'\necho leaving",
            program.path.display(),
            program.started().display()
        ),
        BUDGET,
    );

    assert!(
        run.finished && run.stdout.contains("leaving"),
        "{}",
        run.report()
    );
    assert!(
        program.gone_within(Duration::from_secs(10)),
        "the directly backgrounded program did not outlive the shell. {}",
        run.report()
    );
}

#[test]
fn should_leave_a_program_detached_with_setsid_fork_running_after_the_shell_exits() {
    // The documented way out (ADR-0959): `setsid --fork` starts the program in a session and a
    // process group of its own, which is not a job of the shell's, so leaving does not end it.
    let home = scratch();
    let program = Stubborn::at(&home, "detached");

    let run = run_bounded(
        &home,
        &format!(
            "setsid --fork {}\nsh -c 'while [ ! -e {} ]; do sleep 0.02; done'\necho leaving",
            program.path.display(),
            program.started().display()
        ),
        BUDGET,
    );

    assert!(
        run.finished && run.stdout.contains("leaving"),
        "{}",
        run.report()
    );
    assert!(
        !processes_naming(&program.path.display().to_string()).is_empty(),
        "a program detached with `setsid --fork` outlives the shell. {}",
        run.report()
    );
}

#[test]
fn should_end_an_adapted_program_a_job_runs_when_the_shell_exits() {
    // Review R5: a program whose records the shell decodes as it runs (ADR-0059) is started
    // without the terminal; it is still the job's, and leaving ends it.
    let home = scratch();
    let shims = home.path().join("shims");
    std::fs::create_dir_all(&shims).expect("the shim directory");
    let started = shims.join("journalctl.started");
    support::executable(
        &shims.join("journalctl"),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'systemd 259 (259.5)'; exit 0; fi\n\
             trap '' TERM INT\nexec 2>/dev/null\n: > '{}'\nwhile :; do sleep 1; done\n",
            started.display()
        ),
    );
    let guard = Stubborn {
        path: shims.join("journalctl"),
    };

    let run = run_bounded(
        &home,
        &format!(
            "set env PATH = \"{}:$PATH\"\n\
             fn follow() {{ journalctl -f | select message }}\n\
             follow &\n\
             sh -c 'while [ ! -e {} ]; do sleep 0.02; done'\necho leaving",
            shims.display(),
            started.display()
        ),
        BUDGET,
    );

    assert!(
        run.finished && run.stdout.contains("leaving"),
        "{}",
        run.report()
    );
    assert!(
        guard.gone_within(Duration::from_secs(10)),
        "the adapted program did not outlive the shell. {}",
        run.report()
    );
}

#[test]
fn should_ask_a_stubborn_program_to_stop_once_rather_than_over_and_over() {
    // Review R6: `kill %N` and leaving each send SIGTERM once to a group, not every ten
    // milliseconds for as long as they wait.
    let home = scratch();
    let counted = home.path().join("jobs/terms");
    let path = home.path().join("jobs/counting.sh");
    std::fs::create_dir_all(home.path().join("jobs")).expect("the directory");
    let started = path.with_extension("started");
    support::executable(
        &path,
        &format!(
            "#!/bin/sh\ntrap 'echo term >> {}' TERM\ntrap '' INT\n: > '{}'\n\
             while :; do sleep 0.05; done\n",
            counted.display(),
            started.display()
        ),
    );
    let guard = Stubborn { path: path.clone() };

    let run = run_bounded(
        &home,
        &format!(
            "fn counting() {{ {} }}\ncounting &\n\
             sh -c 'while [ ! -e {} ]; do sleep 0.02; done'\nkill %1\necho leaving",
            path.display(),
            started.display()
        ),
        BUDGET,
    );

    assert!(
        run.finished && run.stdout.contains("leaving"),
        "{}",
        run.report()
    );
    assert!(
        guard.gone_within(Duration::from_secs(10)),
        "{}",
        run.report()
    );
    let terms = std::fs::read_to_string(&counted)
        .unwrap_or_default()
        .lines()
        .count();
    assert!(
        (1..=2).contains(&terms),
        "one SIGTERM for `kill %1` and one for leaving, not {terms}. {}",
        run.report()
    );
}

/// Starts a shell whose jobs — one a native job's program, one a program backgrounded directly,
/// one in the foreground — all ignore SIGTERM, sends it `signal` once they run, and answers
/// whether it left within the bound with `128 + signal`, leaving nothing behind (review R2).
fn ends_its_jobs_on(signal: &str, number: i32) {
    let home = scratch();
    let programs = [
        Stubborn::at(&home, &format!("{signal}-job")),
        Stubborn::at(&home, &format!("{signal}-direct")),
        Stubborn::at(&home, &format!("{signal}-foreground")),
    ];
    let script = format!(
        "fn stubborn() {{ {job} }}\nstubborn &\n{direct} &\n\
         sh -c 'while [ ! -e {job_started} ] || [ ! -e {direct_started} ]; do sleep 0.02; done'\n\
         echo ready\n{foreground}\n",
        job = programs[0].path.display(),
        direct = programs[1].path.display(),
        foreground = programs[2].path.display(),
        job_started = programs[0].started().display(),
        direct_started = programs[1].started().display(),
    );

    let mut shell = Streaming::start(home.path(), &script);
    assert!(shell.until("ready"), "the jobs started");
    let deadline = Instant::now() + BUDGET;
    while !programs[2].started().exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let killed = std::process::Command::new("kill")
        .args([&format!("-{signal}"), &shell.pid().to_string()])
        .status()
        .expect("kill runs");
    assert!(killed.success(), "the signal was sent");
    let code = shell.exit_code();
    let (finished, stdout, stderr) = shell.finish();

    assert!(
        finished,
        "the shell left on SIG{signal}. {stdout}\n{stderr}"
    );
    assert_eq!(
        code,
        Some(128 + number),
        "the shell's status says which signal ended it"
    );
    for program in &programs {
        assert!(
            program.gone_within(Duration::from_secs(10)),
            "{} did not outlive the shell. {stdout}\n{stderr}",
            program.path.display()
        );
    }
}

#[test]
fn should_end_its_jobs_when_the_shell_is_sent_sigterm() {
    ends_its_jobs_on("TERM", 15);
}

#[test]
fn should_end_its_jobs_when_the_shells_terminal_hangs_up() {
    ends_its_jobs_on("HUP", 1);
}

#[test]
fn should_end_its_jobs_when_a_script_is_interrupted() {
    ends_its_jobs_on("INT", 2);
}
