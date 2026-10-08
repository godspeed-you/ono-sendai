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

use std::path::Path;
use std::time::{Duration, Instant};

use ono_testkit::scratch;

use support::{Streaming, read_until, run_bounded};

const BUDGET: Duration = Duration::from_secs(60);

fn append(path: &Path, line: &str) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("the followed file exists");
    writeln!(file, "{line}").expect("the line is appended");
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

#[test]
fn should_report_a_failed_job_nobody_collected_before_the_shell_ends() {
    // Review R8: a script that leaves a failed job behind says so before it ends — the failure
    // as the foreground reports it, and the job's status.
    let home = scratch();
    let missing = home.path().join("not-there.txt");

    let run = run_bounded(
        &home,
        &format!(
            "get file {} &\n\
             while (get job | where state == running | count) > 0 {{ sh -c 'sleep 0.02' }}\n\
             echo leaving",
            missing.display()
        ),
        BUDGET,
    );

    assert!(
        run.finished && run.stdout.contains("leaving"),
        "{}",
        run.report()
    );
    assert!(
        run.stderr.contains("io.not_found") && run.stderr.contains("job %1"),
        "the uncollected job's failure was reported. {}",
        run.report()
    );
}

#[test]
#[cfg(feature = "remote")]
fn should_run_a_backgrounded_native_line_inside_a_link_frame_against_the_link() {
    // Review R3: a native line backgrounded inside a link frame ran on the link before ADR-0958
    // and must still: the job answers from the far side.
    let home = scratch();

    let run = run_bounded(
        &home,
        "link host far --transport local\n\
         enter link far\n\
         get process | where pid == 1 | inspect | select provenance | to json &\n\
         fg %1\n\
         echo \"fg-status-$?\"",
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        run.stdout.contains("\"link\":\"far\"") && run.stdout.contains("fg-status-0"),
        "the job's records were answered by the link, and `fg` collected them. {}",
        run.report()
    );
}

#[test]
fn should_refuse_past_the_ceiling_when_a_jobs_program_writes_without_end() {
    // Review R4: a job's program output is its capture too, charged as it is read: a program
    // that never stops writing ends at the ceiling instead of growing the shell.
    let home = scratch();

    let run = run_bounded(
        &home,
        "set config limits.command_capture_bytes 64KiB\n\
         fn chatter() { sh -c 'while :; do echo 0123456789012345678901234567890123456789; done' }\n\
         chatter &\n\
         fg %1\n\
         echo \"fg-status-$?\"",
        Duration::from_secs(30),
    );

    assert!(
        run.finished,
        "the job ended at its ceiling, so `fg` returned. {}",
        run.report()
    );
    assert!(
        run.stderr.contains("limits.command_capture_bytes") && !run.stdout.contains("fg-status-0"),
        "the refusal is structured and the job did not succeed. {}",
        run.report()
    );
}

#[test]
fn should_refuse_a_live_stream_a_job_would_hand_to_a_program_whole() {
    // Review R4: a live stream that a job would collect whole for the program after it never
    // ends; it is refused, structured, instead of draining without bound.
    let home = scratch();
    let log = home.write("jobs/follow.log", "x-line\n");

    let run = run_bounded(
        &home,
        &format!(
            "tail file {} --lines 5 --follow | to jsonl | grep x &\n\
             fg %1\n\
             echo \"fg-status-$?\"",
            log.display()
        ),
        Duration::from_secs(30),
    );

    assert!(
        run.finished,
        "the job ended, so `fg` returned. {}",
        run.report()
    );
    assert!(
        run.stderr.contains("stream.unbounded_operation") && !run.stdout.contains("fg-status-0"),
        "the refusal is structured. {}",
        run.report()
    );
}

#[test]
fn should_refuse_past_the_ceiling_when_a_live_job_keeps_plain_records() {
    // Review R7: a live stream of plain records — a projection of events — is the job's result,
    // charged to the ceiling; it used to be cut silently to a screenful.
    let home = scratch();

    let run = run_bounded(
        &home,
        "set config limits.command_capture_bytes 8KiB\n\
         watch process --every 50ms | select kind &\n\
         fg %1\n\
         echo \"fg-status-$?\"",
        Duration::from_secs(30),
    );

    assert!(
        run.finished,
        "the job ended at its ceiling. {}",
        run.report()
    );
    assert!(
        run.stderr.contains("limits.command_capture_bytes") && !run.stdout.contains("fg-status-0"),
        "the refusal is structured. {}",
        run.report()
    );
}

#[test]
fn should_refuse_past_the_ceiling_when_a_programs_endless_output_feeds_a_native_stage() {
    // Review S2: program output collected for the native stage after it was read to its end, and
    // `yes | from lines | take 3` grew the shell until the allocator aborted it — in the
    // foreground and in a job. It is bounded by the capture ceiling and refused, structured, and
    // the shell carries on.
    let home = scratch();

    let run = run_bounded(
        &home,
        "set config limits.command_capture_bytes 64KiB\n\
         yes | from lines | take 3 | to json\n\
         echo \"foreground-$?\"\n\
         fn many() { yes | from lines | take 3 | to json }\n\
         many &\n\
         fg %1\n\
         echo \"job-$?\"",
        Duration::from_secs(30),
    );

    assert!(run.finished, "the shell survived both. {}", run.report());
    assert_eq!(
        run.stderr.matches("resource.byte_limit").count(),
        2,
        "both were refused at the ceiling, structured. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("foreground-1") && run.stdout.contains("job-1"),
        "the refusal is the status of each. {}",
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

/// Sends Ctrl-C until `done` holds, reading what the terminal says meanwhile.
///
/// A Ctrl-C reaches `fg` only once the line running it has begun — one typed a moment earlier
/// belongs to nothing and is dropped by the line as it starts (ADR-0782). The test cannot see
/// that moment, so it repeats the keystroke until the outcome it waits for has happened; a Ctrl-C
/// that lands on the prompt afterwards only clears an empty line.
fn interrupt_until(
    shell: &mut ono_testkit::Guarded<ono_process::PtySession>,
    seen: &mut String,
    done: impl Fn(&str) -> bool,
) -> bool {
    let deadline = Instant::now() + BUDGET;
    while Instant::now() < deadline {
        shell
            .write_all(&[0x03])
            .expect("the terminal accepts Ctrl-C");
        seen.push_str(&read_until(shell, "\u{0}never", Duration::from_millis(300)));
        if done(seen) {
            return true;
        }
    }
    false
}

#[test]
fn should_end_a_foregrounded_native_job_with_ctrl_c_and_show_what_it_had() {
    // Ctrl-C under `fg` ends the job, the shell reports the status of an interrupted job, and
    // what the job had produced so far is not thrown away.
    let home = scratch();
    let log = home.write("jobs/follow.log", "value-before\n");
    // The job marks each line once it has kept it, so the test knows there is something to show.
    let kept = home.path().join("jobs/kept");
    let mut shell = support::interactive_shell_in(&home);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(
            format!(
                "tail file {} --lines 5 --follow | each {{ @; sh -c ': > {}' }} &\n",
                log.display(),
                kept.display()
            )
            .as_bytes(),
        )
        .expect("the terminal accepts input");
    read_until(&mut shell, "[%1]", Duration::from_secs(10));
    let deadline = Instant::now() + BUDGET;
    while !kept.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    shell
        .write_all(b"fg %1\n")
        .expect("the terminal accepts input");
    let mut seen = read_until(&mut shell, "fg %1", Duration::from_secs(20));
    let shown = interrupt_until(&mut shell, &mut seen, |seen: &str| {
        seen.split_once("fg %1")
            .is_some_and(|(_, after)| after.contains("value-before"))
    });
    assert!(
        shown,
        "Ctrl-C ended the job and `fg` showed what it had; saw:\n{seen}"
    );
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
        .split_once("fg %1")
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    assert!(
        !after.contains("running tail"),
        "the ended job left the table; saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

#[test]
fn should_keep_a_live_jobs_painted_table_as_the_last_result_when_ctrl_c_ends_it() {
    // Review R10: at a terminal `fg` repaints a live job's table; when Ctrl-C ends the job the
    // table is its result, retained for `@-1` exactly as it is without a terminal. The repaint is
    // also the proof that `fg` is waiting, so the one Ctrl-C needs no retry.
    let home = scratch();
    let mut shell = support::interactive_shell_in(&home);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(b"watch process --every 200ms &\n")
        .expect("the terminal accepts input");
    read_until(&mut shell, "[%1]", Duration::from_secs(10));
    shell
        .write_all(b"fg %1\n")
        .expect("the terminal accepts input");
    let mut seen = read_until(&mut shell, "fg %1", Duration::from_secs(20));
    let painted = Instant::now() + BUDGET;
    while Instant::now() < painted
        && !seen
            .split_once("fg %1")
            .is_some_and(|(_, after)| after.contains("PID"))
    {
        seen.push_str(&read_until(&mut shell, "PID", Duration::from_millis(300)));
    }
    shell
        .write_all(&[0x03])
        .expect("the terminal accepts Ctrl-C");
    shell
        .write_all(b"echo \"after-fg-$?\"; @-1 | count | to json; echo \"counted-$?\"\n")
        .expect("the terminal accepts input");
    // The typed line holds `counted-` too; the shell's answer starts a line of its own.
    seen.push_str(&read_until(
        &mut shell,
        "\ncounted-",
        Duration::from_secs(20),
    ));
    seen.push_str(&read_until(&mut shell, "\n", Duration::from_secs(2)));

    assert!(
        seen.contains("after-fg-130"),
        "Ctrl-C ended the job; saw:\n{seen}"
    );
    let counted = seen
        .rsplit_once("after-fg-130")
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    let rows: u64 = counted
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix('[')?
                .strip_suffix(']')?
                .parse()
                .ok()
        })
        .unwrap_or(0);
    assert!(
        counted.contains("counted-0") && rows > 1,
        "the painted table — every process — is the last result `@-1` names; saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

#[test]
fn should_return_the_jobs_own_status_when_it_ends_on_the_ctrl_c_with_one_of_its_own() {
    // Review R10: a job whose program answers Ctrl-C by exiting 3 has ended with status 3, and
    // that is what `fg` returns — 130 is for a job Ctrl-C cut short.
    let home = scratch();
    let started = home.path().join("jobs/started");
    let exited = home.path().join("jobs/exited");
    std::fs::create_dir_all(home.path().join("jobs")).expect("the directory");
    let mut shell = support::interactive_shell_in(&home);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(
            format!(
                "fn polite() {{ sh -c 'trap \"echo > {exited}; exit 3\" INT; : > {started}; while :; do sleep 0.1; done' }}\n\
                 polite &\n",
                exited = exited.display(),
                started = started.display()
            )
            .as_bytes(),
        )
        .expect("the terminal accepts input");
    read_until(&mut shell, "[%1]", Duration::from_secs(10));
    let deadline = Instant::now() + BUDGET;
    while !started.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    shell
        .write_all(b"fg %1\n")
        .expect("the terminal accepts input");
    let mut seen = read_until(&mut shell, "fg %1", Duration::from_secs(20));
    let ended = interrupt_until(&mut shell, &mut seen, |_: &str| exited.exists());
    assert!(ended, "the program answered Ctrl-C; saw:\n{seen}");
    shell
        .write_all(b"echo \"after-fg-$?\"\n")
        .expect("the terminal accepts input");
    seen.push_str(&read_until(
        &mut shell,
        "after-fg-3",
        Duration::from_secs(20),
    ));
    let reported = seen
        .rsplit_once("after-fg-")
        .map(|(_, rest)| {
            rest.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .unwrap_or_default();
    assert_eq!(
        reported, "3",
        "`fg` returned the job's own status; saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

#[test]
fn should_report_a_job_that_failed_unattended_when_the_prompt_returns() {
    // Review R8: a job's failure used to be silent until `fg`. When it has ended with one, the
    // next prompt says so, once, and `fg` still shows why.
    let home = scratch();
    let missing = home.path().join("not-there.txt");
    let mut shell = support::interactive_shell_in(&home);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(format!("get file {} &\n", missing.display()).as_bytes())
        .expect("the terminal accepts input");
    read_until(&mut shell, "[%1]", Duration::from_secs(10));
    shell
        .write_all(
            b"while (get job | where state == running | count) > 0 { sh -c 'sleep 0.02' }; echo \"waited-$?\"\n",
        )
        .expect("the terminal accepts input");
    let mut seen = read_until(&mut shell, "waited-0", Duration::from_secs(20));
    seen.push_str(&read_until(&mut shell, "job %1", Duration::from_secs(10)));

    let after = seen
        .split_once("waited-0")
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    assert!(
        after.contains("job %1") && after.contains("status 1"),
        "the prompt reported the failed job; saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}
