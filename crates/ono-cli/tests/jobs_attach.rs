//! Moving a running native job to the foreground changes its attachment, not its result
//! (issue #301).
//!
//! A backgrounded line of native stages only used to run on a task whose `fg` either repainted a
//! live table its plain values never reached — and then returned 130 after the job had already
//! ended — or, without a terminal, aborted the job and called that success. What `fg` shows and
//! the status it returns are the job's: every value once, the line's real status, `130` when
//! Ctrl-C ended it, and retention bounded by v0.4.1 §23.4.
//!
//! The jobs below follow a file the test appends to. A job that needs a value the test has not
//! written yet cannot have ended, so the test writes the rest only after the line that runs `fg`
//! has announced itself, and a moment after that for `fg` to reach its wait — the pause the
//! Ctrl-C tests of `jobs_blocks.rs` take. The pause decides only whether the job is still running
//! when `fg` reaches it, never whether a test passes. No test asserts a duration.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::io::Read as _;
use std::path::Path;
use std::time::{Duration, Instant};

use ono_testkit::scratch;

use support::{read_until, run_bounded};

const BUDGET: Duration = Duration::from_secs(60);

fn append(path: &Path, line: &str) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("the followed file exists");
    writeln!(file, "{line}").expect("the line is appended");
}

/// `ono -c script` without a terminal, whose output the test reads as it comes.
struct Streaming {
    child: std::process::Child,
    out: std::sync::mpsc::Receiver<String>,
    err: std::thread::JoinHandle<String>,
    seen: String,
}

impl Streaming {
    fn start(home: &Path, script: &str) -> Self {
        let mut child = std::process::Command::new(ono_testkit::ono_binary())
            .args(["-c", script])
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join("xdg"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("ONO_CONFIG_DIR", home.join("ono"))
            .env("NO_COLOR", "1")
            .env_remove("ONO_CONFIG")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the ono binary is built");
        let mut stdout = child.stdout.take().expect("stdout was piped");
        let (sender, out) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(count) = stdout.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let _ = sender.send(String::from_utf8_lossy(&buffer[..count]).into_owned());
            }
        });
        let mut stderr = child.stderr.take().expect("stderr was piped");
        let err = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        Self {
            child,
            out,
            err,
            seen: String::new(),
        }
    }

    /// Reads standard output until `needle` has appeared, within the budget.
    fn until(&mut self, needle: &str) -> bool {
        let deadline = Instant::now() + BUDGET;
        while !self.seen.contains(needle) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.out.recv_timeout(left) {
                Ok(chunk) => self.seen.push_str(&chunk),
                Err(_) => return false,
            }
        }
        true
    }

    /// Waits for the shell to end, within the budget, and answers everything it wrote. A shell
    /// that overran is killed with what it started, so no test leaves one behind.
    fn finish(mut self) -> (bool, String, String) {
        let deadline = Instant::now() + BUDGET;
        let mut finished = false;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                finished = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !finished {
            ono_testkit::kill_tree(self.child.id());
        }
        let _ = self.child.wait();
        while let Ok(chunk) = self.out.recv_timeout(Duration::from_millis(200)) {
            self.seen.push_str(&chunk);
        }
        (finished, self.seen, self.err.join().unwrap_or_default())
    }
}

#[test]
fn should_wait_for_a_running_native_job_and_show_all_its_values_when_fg_has_no_terminal() {
    // Without a terminal `fg` aborted the job and printed what it had so far, with status 0. It
    // waits instead: the values the job produces after `fg` are shown too, each once.
    let home = scratch();
    let log = home.write("jobs/follow.log", "value-one\n");

    let mut shell = Streaming::start(
        home.path(),
        &format!(
            "tail file {} --lines 5 --follow | take 3 &\n\
             echo fg-next; fg %1\n\
             echo \"fg-status-$?\"",
            log.display()
        ),
    );
    assert!(shell.until("fg-next"), "the script reached `fg`");
    // `fg` follows `echo` on the same line; the pause lets it reach its wait, as the Ctrl-C tests
    // of `jobs_blocks.rs` do. However long the pause, the job cannot end before these lines.
    std::thread::sleep(Duration::from_millis(300));
    append(&log, "value-two");
    append(&log, "value-three");
    let (finished, stdout, stderr) = shell.finish();

    assert!(
        finished,
        "`fg` returned once the job had ended.\n{stdout}\n{stderr}"
    );
    for value in ["value-one", "value-two", "value-three"] {
        assert_eq!(
            stdout.matches(value).count(),
            1,
            "`{value}` is shown exactly once.\n{stdout}\n{stderr}"
        );
    }
    assert!(
        stdout.contains("fg-status-0"),
        "the job ended well and `fg` says so.\n{stdout}\n{stderr}"
    );
}

#[test]
fn should_report_a_native_jobs_failure_status_when_fg_collects_it() {
    // A job whose line failed is not a success when it is collected: `fg` returns the status the
    // same line returns in the foreground, and reports the failure structured.
    let home = scratch();
    let missing = home.path().join("not-there.txt");

    let run = run_bounded(
        &home,
        &format!(
            "get file {missing}\n\
             echo \"foreground-status-$?\"\n\
             get file {missing} &\n\
             fg %1\n\
             echo \"fg-status-$?\"",
            missing = missing.display()
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    let foreground = run
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("foreground-status-"))
        .expect("the foreground status was printed")
        .to_owned();
    assert_ne!(
        foreground,
        "0",
        "the line fails in the foreground. {}",
        run.report()
    );
    assert!(
        run.stdout.contains(&format!("fg-status-{foreground}")),
        "`fg` returns the status the line has in the foreground. {}",
        run.report()
    );
    assert_eq!(
        run.stderr.matches("io.not_found").count(),
        2,
        "the job's failure is reported when it is collected, as the foreground's was. {}",
        run.report()
    );
}

#[test]
fn should_refuse_structured_past_the_capture_ceiling_when_a_native_job_retains_too_much() {
    // What a job holds for `fg` is a capture, bounded by v0.4.1 §23.4's ceiling like every
    // other: past it the job ends with the structured refusal, and it does not keep it all.
    let home = scratch();
    let items: Vec<String> = (1..=400)
        .map(|line| format!("\"line-{line:04}-{}\"", "x".repeat(80)))
        .collect();
    let document = home.write("jobs/big.json", format!("[{}]", items.join(",")));

    let run = run_bounded(
        &home,
        &format!(
            "set config limits.command_capture_bytes 8KiB\n\
             read file {} | from json &\n\
             fg %1\n\
             echo \"fg-status-$?\"",
            document.display()
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        run.stderr.contains("limits.command_capture_bytes"),
        "the refusal names the ceiling it hit and the setting that moves it. {}",
        run.report()
    );
    assert!(
        !run.stdout.contains("line-0400"),
        "the job did not retain past the ceiling. {}",
        run.report()
    );
    assert!(
        !run.stdout.contains("fg-status-0"),
        "a job refused at its ceiling did not succeed. {}",
        run.report()
    );
}

#[test]
fn should_end_a_live_native_job_with_the_structured_refusal_when_it_reaches_the_ceiling() {
    // A followed file never ends, so a job following it holds what it reads until `fg` — and
    // that, too, is a capture with a ceiling. The job ends at the ceiling instead of growing.
    let home = scratch();
    let lines: String = (1..=400)
        .map(|line| format!("line-{line:04}-{}\n", "x".repeat(80)))
        .collect();
    let log = home.write("jobs/big.log", lines);

    let run = run_bounded(
        &home,
        &format!(
            "set config limits.command_capture_bytes 8KiB\n\
             tail file {} --lines 400 --follow &\n\
             fg %1\n\
             echo \"fg-status-$?\"",
            log.display()
        ),
        BUDGET,
    );

    assert!(
        run.finished,
        "the job ended at its ceiling, so `fg` returned. {}",
        run.report()
    );
    assert!(
        run.stderr.contains("limits.command_capture_bytes"),
        "the refusal is structured and names the setting. {}",
        run.report()
    );
    assert!(
        !run.stdout.contains("fg-status-0"),
        "the refused job did not succeed. {}",
        run.report()
    );
}

#[test]
fn should_collect_a_finished_native_job_once_with_its_status() {
    // `fg` after the job has ended hands over what it made, once, and the job is gone.
    let home = scratch();
    let document = home.write("jobs/done.json", "[\"only-value\"]");

    let run = run_bounded(
        &home,
        &format!(
            "read file {} | from json &\n\
             while (get job | where state == running | count) > 0 {{ sh -c 'sleep 0.02' }}\n\
             fg %1\n\
             echo \"fg-status-$?\"\n\
             fg %1\n\
             echo \"again-status-$?\"",
            document.display()
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert_eq!(
        run.stdout.matches("only-value").count(),
        1,
        "the value is shown once. {}",
        run.report()
    );
    assert!(run.stdout.contains("fg-status-0"), "{}", run.report());
    assert!(
        !run.stdout.contains("again-status-0") && run.stderr.contains("no job %1"),
        "a collected job is gone. {}",
        run.report()
    );
}

#[test]
fn should_show_what_a_jobs_serializer_and_programs_wrote_as_the_foreground_would() {
    // `fg` shows a job's result in place of the terminal the job never had. A serializer's
    // document and a program's output are bytes the foreground writes as they are; `fg` used to
    // render them as a one-column table, cut to the width of a cell.
    let home = scratch();
    let long = "x".repeat(300);
    let document = home.write("jobs/one.json", format!("[{{\"name\":\"{long}\"}}]"));

    let run = run_bounded(
        &home,
        &format!(
            "read file {path} | from json | to json\n\
             echo foreground-done\n\
             read file {path} | from json | to json &\n\
             fg %1\n\
             fn say() {{ sh -c 'echo program-{long}' }}\n\
             say &\n\
             fg %1\n\
             echo \"fg-status-$?\"",
            path = document.display(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    let (foreground, collected) = run
        .stdout
        .split_once("foreground-done\n")
        .expect("the foreground's output came first");
    assert!(
        collected.starts_with(foreground),
        "`fg` wrote the job's document exactly as the foreground wrote the same line's. {}",
        run.report()
    );
    assert!(
        collected.contains(&format!("program-{long}\n")),
        "`fg` wrote the job's program output whole. {}",
        run.report()
    );
    assert!(
        !collected.contains("VALUE") && !collected.contains("..."),
        "nothing was rendered as a table or cut short. {}",
        run.report()
    );
    assert!(run.stdout.contains("fg-status-0"), "{}", run.report());
}

#[test]
fn should_write_a_live_jobs_lines_to_its_redirection_as_they_come() {
    // A job that ends in a streaming serializer redirected to a file writes each line as it
    // comes, like the foreground does (ADR-0954), rather than holding them for an end a followed
    // file never reaches.
    let home = scratch();
    let log = home.write("jobs/follow.log", "first-line\n");
    let out = home.path().join("jobs/out.jsonl");

    let run = run_bounded(
        &home,
        &format!(
            "tail file {log} --lines 5 --follow | to jsonl > {out} &\n\
             sh -c 'while ! grep -q first-line {out} 2>/dev/null; do sleep 0.02; done'\n\
             kill %1\n\
             echo \"written-$?\"",
            log = log.display(),
            out = out.display(),
        ),
        BUDGET,
    );

    assert!(
        run.finished && run.stdout.contains("written-0"),
        "the line reached the file while the job ran. {}",
        run.report()
    );
}

// --- at a terminal -----------------------------------------------------------------------------

#[test]
fn should_show_every_value_of_a_running_native_job_once_when_it_is_foregrounded_at_a_terminal() {
    // At a terminal `fg` used to repaint a live table that plain values never reach, and return
    // 130 once the job had ended without printing what it made. It waits for the job and shows
    // every value, those that arrived before `fg` and those after, each once.
    let home = scratch();
    let log = home.write("jobs/follow.log", "value-one\n");
    let mut shell = support::interactive_shell_in(&home);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(
            format!(
                "tail file {} --lines 5 --follow | take 3 &\n",
                log.display()
            )
            .as_bytes(),
        )
        .expect("the terminal accepts input");
    read_until(&mut shell, "[%1]", Duration::from_secs(10));
    shell
        .write_all(b"echo \"foregrounding-$?\"; fg %1\n")
        .expect("the terminal accepts input");
    let mut seen = read_until(&mut shell, "foregrounding-0", Duration::from_secs(20));
    // A pause for `fg` to reach its wait; the job cannot end before the lines below exist.
    std::thread::sleep(Duration::from_millis(300));
    append(&log, "value-two");
    append(&log, "value-three");
    seen.push_str(&read_until(
        &mut shell,
        "value-three",
        Duration::from_secs(20),
    ));
    shell
        .write_all(b"echo \"fg-status-$?\"\n")
        .expect("the terminal accepts input");
    seen.push_str(&read_until(
        &mut shell,
        "fg-status-0",
        Duration::from_secs(20),
    ));

    let after = seen
        .split_once("foregrounding-0")
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    for value in ["value-one", "value-two", "value-three"] {
        assert_eq!(
            after.matches(value).count(),
            1,
            "`{value}` is shown exactly once after `fg`; saw:\n{seen}"
        );
    }
    assert!(
        seen.contains("fg-status-0"),
        "`fg` returned the job's own status; saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

#[test]
fn should_end_a_foregrounded_native_job_with_ctrl_c_and_show_what_it_had() {
    // Ctrl-C under `fg` ends the job, the shell reports the status of an interrupted job, and
    // what the job had produced so far is not thrown away.
    let home = scratch();
    let log = home.write("jobs/follow.log", "value-before\n");
    let mut shell = support::interactive_shell_in(&home);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(format!("tail file {} --lines 5 --follow &\n", log.display()).as_bytes())
        .expect("the terminal accepts input");
    read_until(&mut shell, "[%1]", Duration::from_secs(10));
    // As in `jobs_blocks.rs`: a byte typed while the line editor holds the terminal is a
    // keystroke, so the line announces itself and the Ctrl-C follows, with a pause for `fg` to
    // reach its wait. The job never ends by itself.
    shell
        .write_all(b"echo \"foregrounding-$?\"; fg %1\n")
        .expect("the terminal accepts input");
    let mut seen = read_until(&mut shell, "foregrounding-0", Duration::from_secs(20));
    std::thread::sleep(Duration::from_millis(300));
    shell
        .write_all(&[0x03])
        .expect("the terminal accepts Ctrl-C");
    shell
        .write_all(b"echo \"after-fg-$?\"; jobs; echo \"listed-$?\"\n")
        .expect("the terminal accepts input");
    // A needle the typed line does not hold itself, since the terminal echoes what is typed.
    seen.push_str(&read_until(&mut shell, "listed-0", Duration::from_secs(20)));

    assert!(
        seen.contains("after-fg-130"),
        "Ctrl-C ended the foregrounded job with an interrupted job's status; saw:\n{seen}"
    );
    let after = seen
        .split_once("foregrounding-0")
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    assert!(
        after.contains("value-before"),
        "what the job had produced was shown; saw:\n{seen}"
    );
    assert!(
        !after.contains("running tail"),
        "the ended job left the table; saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}
