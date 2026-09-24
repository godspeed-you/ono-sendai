//! The test harness is itself tested: every later test trusts it to report what a user would
//! actually see, so a harness that quietly stopped capturing stderr would make the whole suite
//! meaningless (AGENTS.md section 14).

use ono_testkit::{Shell, SkipReason, require, require_descriptors};

#[test]
fn should_report_the_version_on_standard_output_when_asked() {
    let run = Shell::new().args(["--version"]).run();
    run.assert_success();
    assert!(run.stdout().starts_with("ono "), "got {:?}", run.stdout());
    assert_eq!(run.stderr(), "");
}

#[test]
fn should_report_a_failing_status_and_keep_the_streams_apart_when_the_command_line_is_wrong() {
    let run = Shell::new().args(["--definitely-not-a-flag"]).run();
    assert_eq!(run.status().code(), 2);
    assert!(!run.status().is_success());
    assert!(
        run.stderr().contains("unrecognised"),
        "the complaint belongs on stderr, got stdout={:?} stderr={:?}",
        run.stdout(),
        run.stderr()
    );
    assert_eq!(
        run.stdout(),
        "",
        "nothing should reach stdout on a usage error"
    );
}

#[test]
fn should_run_in_a_scratch_directory_that_does_not_outlive_the_test_when_one_is_requested() {
    let scratch = ono_testkit::scratch();
    scratch.write("a.txt", "alpha\n");
    assert_eq!(scratch.read("a.txt"), "alpha\n");
    let path = scratch.path().to_path_buf();
    assert!(path.is_dir());
    drop(scratch);
    assert!(!path.exists(), "the scratch directory must be removed");
}

#[test]
fn should_pass_environment_and_working_directory_to_the_shell_when_configured() {
    // Proven against a program every system has, so the harness is verified before the shell
    // it will be used to verify exists.
    let scratch = ono_testkit::scratch();
    let run = Shell::program("/bin/sh")
        .args(["-c", "pwd; printf '%s\\n' \"$ONO_HARNESS_PROBE\""])
        .cwd(scratch.path())
        .env("ONO_HARNESS_PROBE", "alive")
        .run();
    run.assert_success();
    assert!(run.stdout().contains("alive"), "got {:?}", run.stdout());
    let reported = run.stdout().lines().next().unwrap_or_default().to_owned();
    assert_eq!(
        std::fs::canonicalize(reported).ok(),
        std::fs::canonicalize(scratch.path()).ok()
    );
}

#[test]
fn should_feed_standard_input_to_the_shell_when_given() {
    let run = Shell::program("/bin/cat").stdin("hello\nworld\n").run();
    run.assert_success();
    assert_eq!(run.stdout(), "hello\nworld\n");
}

#[test]
fn should_fail_the_test_rather_than_hang_when_a_run_exceeds_its_budget() {
    // A shell test that hangs is worse than one that fails: it stops the whole suite.
    let outcome = Shell::program("/bin/sh")
        .args(["-c", "sleep 30"])
        .timeout(std::time::Duration::from_millis(300))
        .try_run();
    assert!(
        outcome.is_err(),
        "an overrunning run must be reported, not awaited"
    );
}

#[test]
fn should_kill_and_reap_the_program_when_a_run_exceeds_its_budget() {
    // Reporting the overrun is half of it. A sweep on 2026-09-02 killed 331 leaked test
    // followers on the development machine, the oldest five days old, and a helper that walks
    // away from a child it started is how they got there — a suite that leaves processes behind
    // is not reporting its own execution truthfully, whatever its exit code says (v0.4.1 §2.4,
    // §39.3).
    let scratch = ono_testkit::scratch();
    let marker = scratch.path().join("pid");
    let outcome = Shell::program("/bin/sh")
        .args([
            "-c",
            &format!("echo $$ > {}; exec sleep 300", marker.display()),
        ])
        .timeout(std::time::Duration::from_millis(500))
        .try_run();
    assert!(outcome.is_err(), "the run overran and must say so");

    let pid: u32 = std::fs::read_to_string(&marker)
        .expect("the child recorded its pid before it slept")
        .trim()
        .parse()
        .expect("a pid is a number");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline && alive(pid) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        !alive(pid),
        "an overrunning run must leave nothing behind, and pid {pid} is still there"
    );
}

/// Whether a pid names a process that is neither gone nor a zombie nobody waited for.
fn alive(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    stat.rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .is_some_and(|state| state != "Z")
}

#[test]
fn should_report_the_signal_that_killed_the_program_as_128_plus_it_when_it_is_signalled() {
    let run = Shell::program("/bin/sh")
        .args(["-c", "kill -TERM $$"])
        .run();
    assert_eq!(run.status().code(), 143);
    assert_eq!(run.status().signal(), Some(15));
}

#[test]
fn should_run_a_script_through_the_shell_when_asked_for_one() {
    // The helper 21 suites re-declared by hand: `ono -c <script>`, captured the way a user sees
    // it. A test that spells it itself is a test that can spell it differently.
    let run = ono_testkit::ono("let n = 41; $n | to json");
    run.assert_success();
    assert_eq!(run.stdout().trim(), "[41]", "got {:?}", run.stdout());
}

#[test]
fn should_keep_the_streams_apart_when_a_script_fails() {
    let run = ono_testkit::ono("definitely-not-a-command");
    assert!(!run.status().is_success(), "got {:?}", run.output());
    assert_eq!(run.stdout(), "", "a diagnostic belongs on stderr");
    assert!(
        run.stderr().contains("Ono-Sendai-E0101"),
        "got {:?}",
        run.stderr()
    );
}

#[test]
fn should_take_a_wider_budget_than_the_default_when_a_script_is_given_one() {
    // A suite that spawns real children needs longer than the default; asking for the budget by
    // name keeps the number in one place instead of in every file that needs it.
    let run = ono_testkit::ono_within(
        "let n = 41; $n | to json",
        std::time::Duration::from_secs(30),
    );
    run.assert_success();
    assert_eq!(run.stdout().trim(), "[41]");
}

#[test]
fn should_name_the_test_the_reason_and_the_category_when_a_skip_is_announced() {
    // The marker is what makes a skipped test countable, so its shape is a contract: the word
    // `SKIPPED`, the test that skipped, the v0.4.1 §38.4 category and the detail. Asserting it
    // here means a later change to the format has to be deliberate.
    ono_testkit::skipped(
        SkipReason::FixtureNotApplicable,
        "this host has no second mount to cross",
    );
    let name = std::thread::current().name().unwrap_or_default().to_owned();
    assert_eq!(
        name, "should_name_the_test_the_reason_and_the_category_when_a_skip_is_announced",
        "the marker takes the test's name from its thread, so cargo must still name it"
    );
    assert_eq!(
        SkipReason::FixtureNotApplicable.category(),
        "fixture_not_applicable",
        "the category token is what the expected-skip registry stores"
    );
    for reason in SkipReason::ALL {
        assert_eq!(
            SkipReason::from_category(reason.category()),
            Some(reason),
            "every §38.4 category round-trips through its token"
        );
    }
    assert_eq!(
        SkipReason::from_category("the machine was busy"),
        None,
        "the taxonomy is closed: free text is not a category"
    );
}

#[test]
fn should_offer_a_require_helper_that_records_an_unmet_prerequisite() {
    // v0.4.1 Appendix G: `require(condition, reason_category, detail) -> TestPrerequisite`. A met
    // prerequisite announces nothing and lets the test carry on; an unmet one has already emitted
    // the canonical skip signal by the time the caller returns, which is what makes the early
    // return legal under §65.10.
    let met = require(
        true,
        SkipReason::ExternalToolUnavailable,
        "this detail is never printed",
    );
    assert!(met.met(), "a satisfied prerequisite lets the test carry on");
    assert!(!met.unmet());

    let unmet = require(
        false,
        SkipReason::ExternalToolUnavailable,
        "no journal on this host",
    );
    assert!(
        unmet.unmet(),
        "an unsatisfied prerequisite tells the caller to return"
    );
    assert!(!unmet.met());
}

#[test]
fn should_report_a_descriptor_limit_the_host_cannot_reach_rather_than_failing() {
    // A machine that cannot supply the descriptors a fixture needs has not found a defect in the
    // product. v0.4.1 §38.1 says a test reports execution truth, and a red result meaning "this
    // runner has a lower rlimit" is §65.10's skip-as-pass inverted — it is fail-as-defect
    // (ADR-0517).
    let plenty = require_descriptors(64);
    assert!(
        plenty.is_ok(),
        "every host allows sixty-four open descriptors, got {plenty:?}"
    );

    let impossible =
        require_descriptors(u64::MAX).expect_err("no host allows every descriptor a u64 can count");
    assert!(
        impossible.needed > impossible.hard,
        "the shortfall names what was needed and what the host would give, got {impossible:?}"
    );
    let told = impossible.to_string();
    assert!(
        told.contains(&impossible.hard.to_string()) && told.contains("hard limit"),
        "the reason names the hard limit an unprivileged process cannot raise, got {told:?}"
    );
}

#[test]
fn should_raise_its_own_soft_descriptor_limit_before_reporting_a_shortfall() {
    // Raising the soft limit toward the hard one changes nothing about what a fixture measures —
    // the descriptors were always allowed and the process was simply not asking for them — so it
    // happens before anything is reported (ADR-0517).
    //
    // The room to raise it is made rather than hoped for. A sibling test in this binary asks for
    // more descriptors than any host has, which leaves the soft limit at the hard one; a test
    // that skipped whenever that had already run would be a test that almost never ran.
    use nix::sys::resource::{Resource, getrlimit, setrlimit};

    let (soft, hard) = getrlimit(Resource::RLIMIT_NOFILE).expect("the limit is readable");
    let lowered = 1024.min(hard);
    setrlimit(Resource::RLIMIT_NOFILE, lowered, hard).expect("a soft limit may always be lowered");

    let asked = lowered + 1;
    let reached = require_descriptors(asked);
    let (raised, _) = getrlimit(Resource::RLIMIT_NOFILE).expect("the limit is readable");
    setrlimit(Resource::RLIMIT_NOFILE, soft.max(raised), hard)
        .expect("the limit this test found is restored");

    reached.expect("a limit below the hard one is reachable without privilege");
    assert!(
        raised >= asked,
        "the soft limit was raised from {lowered} to at least {asked}, got {raised}"
    );
}

#[test]
fn should_stretch_a_watchdog_for_the_load_the_test_does_not_control() {
    // A `Shell` budget is a watchdog, not an assertion: nothing asserts that a command answered
    // within twenty seconds, and the number exists so a hung test fails instead of stalling the
    // suite. Measuring a fixed wall clock against a machine whose load the test does not control
    // is not measuring the product — on 2026-09-02 two such failures were each first reported as
    // a product hang (ADR-0517).
    let run = Shell::program("/bin/sh")
        .args(["-c", "sleep 30"])
        .timeout(std::time::Duration::from_millis(200))
        .try_run();
    let error = run.expect_err("a thirty-second sleep overruns a two-hundred-millisecond watchdog");
    let told = error.to_string();
    assert!(
        told.contains("load average of") && told.contains("200ms"),
        "an overrun says what it was scaled against and what the caller asked for, so a reader \
         can tell a busy machine from a hang, got {told:?}"
    );
}

#[test]
fn should_run_a_script_again_while_another_thread_still_holds_it_open() {
    // `cargo test` runs a crate's tests in threads of one process, and a thread that forks between
    // another thread's `open` and `close` of a file inherits the write descriptor. Until that
    // child execs, `execve` on the file answers ETXTBSY, which POSIX and this shell both report as
    // exit 126 — "found and not executable" about a file that is executable. Issue #27 saw it
    // once; issue #7 is the same race one crate over (ADR-0520).
    let directory = ono_testkit::scratch();
    let script = ono_testkit::executable_script(directory.path(), "greet", "#!/bin/sh\nexit 7\n");

    let holder = std::fs::OpenOptions::new()
        .write(true)
        .open(&script)
        .expect("the script this test wrote is writable");
    let refused = std::process::Command::new(&script).output();
    assert_eq!(
        refused
            .as_ref()
            .err()
            .and_then(std::io::Error::raw_os_error),
        Some(26),
        "a writer holding the file makes `execve` answer ETXTBSY, got {refused:?}"
    );

    let released = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        drop(holder);
    });
    let outcome = ono_testkit::while_text_file_busy(
        |answer: &std::io::Result<std::process::Output>| {
            answer.as_ref().err().and_then(std::io::Error::raw_os_error) == Some(26)
        },
        || std::process::Command::new(&script).output(),
    );
    released.join().expect("the holder thread finishes");
    assert_eq!(
        outcome
            .expect("the script runs once nobody is writing it")
            .status
            .code(),
        Some(7),
        "the retry waits out the writer instead of reporting the machine's state as the script's"
    );
}

#[test]
fn should_answer_a_failure_that_is_not_a_busy_file_on_the_first_attempt() {
    // The retry is not a blanket one: every other failure is returned unretried, so a script that
    // really cannot be run fails at once rather than a second later.
    let attempts = std::cell::Cell::new(0u32);
    let outcome = ono_testkit::while_text_file_busy(
        |_: &u32| false,
        || {
            attempts.set(attempts.get() + 1);
            attempts.get()
        },
    );
    assert_eq!(outcome, 1, "an answer that is not `busy` is the answer");
    assert_eq!(attempts.get(), 1, "and it was asked for exactly once");
}

#[test]
fn should_run_a_script_it_has_just_written_while_other_threads_are_starting_processes() {
    // Issue #188: `cargo test` runs a crate's tests as threads of one process, and every thread
    // that starts a process forks. A fork taken while this thread still has the script open for
    // writing hands the child a copy of that descriptor, and until the child execs, `execve` of
    // the script answers ETXTBSY — for a file that is executable and that nobody is writing any
    // more. The shared helper must leave no such descriptor anywhere, so the script runs at once,
    // however busy the neighbours are (ADR-0891).
    let busy = busy_among_fresh_executables(|directory, attempt| {
        ono_testkit::executable_script(
            directory,
            &format!("script-{attempt}"),
            "#!/bin/sh\nexit 0\n",
        )
    });
    assert_eq!(
        busy, 0,
        "a script the helper wrote is never busy when it is run, but {busy} of \
         {FRESH_EXECUTABLES} runs answered ETXTBSY"
    );
}

#[test]
fn should_run_a_program_it_has_just_copied_while_other_threads_are_starting_processes() {
    // The same race with a copied program — a plugin binary put where the shell will load it, or a
    // renamed `sleep` whose name a test selects on — because `std::fs::copy` holds the copy open
    // for writing in this process exactly as `std::fs::write` does (issue #188, ADR-0891).
    let busy = busy_among_fresh_executables(|directory, attempt| {
        ono_testkit::executable_copy("/bin/true", &directory.join(format!("true-{attempt}")))
    });
    assert_eq!(
        busy, 0,
        "a program the helper copied is never busy when it is run, but {busy} of \
         {FRESH_EXECUTABLES} runs answered ETXTBSY"
    );
}

/// How many executables each race test makes and runs. Before the fix, three hundred runs
/// answered ETXTBSY between five and sixteen times on the development host and a hundred about
/// once, so two hundred reproduce the race reliably, and a fixed helper passes whatever the count.
const FRESH_EXECUTABLES: u32 = 200;

/// Makes [`FRESH_EXECUTABLES`] executables with `make` and runs each at once, while six threads
/// start processes as fast as they can, and answers how many runs the kernel refused as busy.
#[allow(
    clippy::panic,
    clippy::expect_used,
    reason = "a helper of two tests states their preconditions the way a #[test] body does"
)]
fn busy_among_fresh_executables(make: impl Fn(&std::path::Path, u32) -> std::path::PathBuf) -> u32 {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let spawners: Vec<_> = (0..6)
        .map(|_| {
            let stop = std::sync::Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = std::process::Command::new("/bin/true").status();
                }
            })
        })
        .collect();

    let directory = ono_testkit::scratch();
    let mut busy = 0;
    for attempt in 0..FRESH_EXECUTABLES {
        let program = make(directory.path(), attempt);
        match std::process::Command::new(&program).status() {
            Err(error) if error.raw_os_error() == Some(26) => busy += 1,
            Err(error) => panic!("{} could not be run: {error}", program.display()),
            Ok(status) => assert!(
                status.success(),
                "{} exits 0, got {status}",
                program.display()
            ),
        }
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for spawner in spawners {
        spawner.join().expect("a spawner thread finishes");
    }
    busy
}

#[test]
fn should_kill_what_the_program_started_when_a_run_exceeds_its_budget() {
    // Issue #204: killing the overrunning program is not enough when the program is a shell.
    // What it started survives it — reparented, still running, and invisible to the test that
    // caused it. One grandchild here stays in the program's session; the other leads a session of
    // its own, as the jobs `ono` starts lead process groups of their own, so neither the pid nor
    // the process group of the program reaches it (ADR-0892).
    let scratch = ono_testkit::scratch();
    let marker = scratch.path().join("pids");
    let outcome = Shell::program("/bin/sh")
        .args([
            "-c",
            &format!(
                "sleep 300 & echo $! >> {0}; setsid sleep 300 & echo $! >> {0}; wait",
                marker.display()
            ),
        ])
        .timeout(std::time::Duration::from_millis(500))
        .try_run();
    assert!(outcome.is_err(), "the run overran and must say so");
    assert_all_gone(&recorded_pids(&marker, 2));
}

#[test]
fn should_kill_the_jobs_the_shell_started_when_a_bounded_run_is_cut_off() {
    // Issue #204, through the real shell: `ono` runs an external command in a process group of its
    // own, so a bounded run that kills only `ono` at its deadline leaves the command running.
    let scratch = ono_testkit::scratch();
    let marker = scratch.path().join("pids");
    let bounded = ono_testkit::run_bounded(
        &scratch,
        // The job lets go of the run's pipes, so a surviving job shows as a survivor here rather
        // than as a run that cannot finish draining output it will never close.
        &format!(
            "sh -c 'echo $$ > {}; exec sleep 300 > /dev/null 2>&1'",
            marker.display()
        ),
        std::time::Duration::from_secs(3),
    );
    assert!(
        !bounded.finished,
        "the job sleeps past the budget, so the run is cut off: {}",
        bounded.report()
    );
    assert_all_gone(&recorded_pids(&marker, 1));
}

/// The pids a fixture recorded, one per line, once `count` of them are there.
fn recorded_pids(marker: &std::path::Path, count: usize) -> Vec<u32> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let pids: Vec<u32> = std::fs::read_to_string(marker)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect();
        if pids.len() >= count || std::time::Instant::now() >= deadline {
            assert_eq!(
                pids.len(),
                count,
                "the fixture records {count} pids before its budget runs out, got {pids:?}"
            );
            return pids;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Asserts that none of `pids` is still running, allowing the kernel a moment to deliver.
fn assert_all_gone(pids: &[u32]) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline && pids.iter().any(|pid| alive(*pid)) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let survivors: Vec<u32> = pids.iter().copied().filter(|pid| alive(*pid)).collect();
    // A survivor must not outlive the proof that it survived.
    for pid in &survivors {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
    assert!(
        survivors.is_empty(),
        "a process the test started is still running after its owner was done with it: \
         {survivors:?} of {pids:?}"
    );
}

#[test]
fn should_leave_no_process_behind_when_a_test_holding_one_panics() {
    // Issue #162: a test that spawns a process, asserts, and only then kills it leaves the process
    // running whenever the assertion fails — `std::process::Child` neither kills nor reaps on
    // drop. The guarded child dies with everything it started, however the test ends (ADR-0892).
    let scratch = ono_testkit::scratch();
    let marker = scratch.path().join("pids");
    let script = format!(
        "echo $$ >> {0}; sleep 300 & echo $! >> {0}; setsid sleep 300 & echo $! >> {0}; wait",
        marker.display()
    );
    let failed = std::panic::catch_unwind(|| {
        let _child = ono_testkit::OwnedChild::new(
            std::process::Command::new("/bin/sh")
                .args(["-c", &script])
                .spawn()
                .expect("/bin/sh starts"),
        );
        recorded_pids(&marker, 3);
        panic!("an assertion of the test fails while it holds the child");
    });
    assert!(failed.is_err(), "the guarded scope panicked");
    assert_all_gone(&recorded_pids(&marker, 3));
}

#[test]
fn should_leave_no_process_under_a_session_behind_when_a_test_holding_it_panics() {
    // The same for a process whose handle is not a `Child`, such as a pseudo-terminal session: the
    // guard kills the tree under it and leaves reaping the leader to the handle.
    let scratch = ono_testkit::scratch();
    let marker = scratch.path().join("pids");
    let mut leader = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            &format!("setsid sleep 300 & echo $! >> {0}; wait", marker.display()),
        ])
        .spawn()
        .expect("/bin/sh starts");
    let failed = std::panic::catch_unwind(|| {
        let _tree = ono_testkit::OwnedTree::of(leader.id());
        recorded_pids(&marker, 1);
        panic!("an assertion of the test fails while it holds the session");
    });
    assert!(failed.is_err(), "the guarded scope panicked");
    let status = leader.wait().expect("the leader can be reaped");
    assert!(
        !status.success(),
        "the leader was killed, not left to finish: {status}"
    );
    assert_all_gone(&recorded_pids(&marker, 1));
}

#[test]
fn should_kill_the_tree_of_the_process_it_was_made_for_even_when_that_process_changed_its_name() {
    // An `OwnedTree` holds on to its process from the moment it is made (ADR-0894): the tree it
    // kills is the one under that process, found through the process, whatever happens to its
    // number later. Here the leader execs a new program in place, which keeps the process, and
    // the tree under it still dies.
    let scratch = ono_testkit::scratch();
    let marker = scratch.path().join("pids");
    let mut leader = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            &format!(
                "sleep 300 & echo $! >> {0}; exec sh -c 'setsid sleep 300 & echo $! >> {0}; wait'",
                marker.display()
            ),
        ])
        .spawn()
        .expect("/bin/sh starts");
    let tree = ono_testkit::OwnedTree::of(leader.id());
    let pids = recorded_pids(&marker, 2);
    drop(tree);
    let _ = leader.wait();
    assert_all_gone(&pids);
}

#[test]
fn should_do_nothing_when_the_process_it_was_made_for_has_already_been_reaped() {
    // The handle an `OwnedTree` guards may have waited for its leader before the guard is dropped
    // — a session that ran to its end. The number is free for anyone then, and the guard must not
    // reach whatever took it (ADR-0894).
    let mut leader = std::process::Command::new("/bin/true")
        .spawn()
        .expect("/bin/true starts");
    let tree = ono_testkit::OwnedTree::of(leader.id());
    leader.wait().expect("the leader is reaped");
    drop(tree);
}

#[test]
fn should_leave_nothing_under_a_guarded_handle_behind_when_a_test_holding_it_panics() {
    // A shared helper that hands a test a process handle of its own kind — a pseudo-terminal
    // session — wraps it once, and every test that uses the helper is covered: when the test
    // fails, the tree under the handle dies before the handle is dropped (issue #162, ADR-0894).
    let scratch = ono_testkit::scratch();
    let marker = scratch.path().join("pids");
    let script = format!(
        "sleep 300 & echo $! >> {0}; setsid sleep 300 & echo $! >> {0}; wait",
        marker.display()
    );
    let failed = std::panic::catch_unwind(|| {
        let handle = ono_testkit::Guarded::new(
            std::process::Command::new("/bin/sh")
                .args(["-c", &script])
                .spawn()
                .expect("/bin/sh starts"),
            std::process::Child::id,
        );
        recorded_pids(&marker, 2);
        assert!(handle.id() > 0, "the handle is used through the guard");
        panic!("an assertion of the test fails while it holds the handle");
    });
    assert!(failed.is_err(), "the guarded scope panicked");
    assert_all_gone(&recorded_pids(&marker, 2));
}
