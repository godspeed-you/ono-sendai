//! `to jsonl`: the streaming serializer (issue #214, ADR-0954).
//!
//! `to json` is one array, so it is written when the stream ends, and a live stream cut off before
//! its end prints nothing. JSON Lines writes each value as one compact document on a line of its
//! own, the moment the value arrives — which is what lets a live view be scripted. Every claim here
//! is about *when* a line exists, so the proofs read lines while the source is still open: the
//! first line is read before the second value has been written to the source at all.
//!
//! No test here asserts a duration: the source is a followed file, which produces nothing until
//! the test writes to it, and every wait is bounded by a budget nothing is measured against
//! (ADR-0431).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use ono_testkit::{Scratch, scratch};

use support::{read_until, run_bounded};

const BUDGET: Duration = Duration::from_secs(60);

/// A shell running `script`, with its standard output read line by line as it arrives.
struct Reading {
    child: Child,
    lines: Receiver<String>,
}

impl Reading {
    fn start(home: &Scratch, script: &str) -> Self {
        let mut child = Command::new(ono_testkit::ono_binary())
            .args(["-c", script])
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("xdg"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the ono binary is built before its tests run");
        let stdout = child.stdout.take().expect("stdout was piped");
        let (sender, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        Self { child, lines }
    }

    /// The next line the shell wrote, or `None` if none arrived within the budget.
    fn next_line(&self) -> Option<String> {
        self.lines.recv_timeout(BUDGET).ok()
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        ono_testkit::kill_tree(self.child.id());
        let _ = self.child.wait();
    }
}

fn append(path: &Path, line: &str) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("the source file exists");
    writeln!(file, "{line}").expect("the source file is writable");
}

fn lines_of(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .count()
}

#[test]
fn should_write_the_first_line_before_the_second_value_exists() {
    // Issue #214's exit test: the first value is printed before the second arrives. The source
    // holds one line when the shell starts; the test reads the first JSON line, checks that the
    // source still holds one line — so no second value can exist yet — and only then writes the
    // second, which is the next line out.
    let home = scratch();
    let source = home.write("jsonl/source.log", "first\n");
    let reading = Reading::start(
        &home,
        &format!(
            "tail file {} --lines 1 --follow | to jsonl",
            source.display()
        ),
    );

    let first = reading.next_line();
    assert_eq!(
        first.as_deref(),
        Some("\"first\""),
        "the first value is written as a line of its own while the source is open"
    );
    assert_eq!(
        lines_of(&source),
        1,
        "and it was written before the second value existed anywhere"
    );

    append(&source, "second");
    assert_eq!(
        reading.next_line().as_deref(),
        Some("\"second\""),
        "the next value is the next line, written as it arrives"
    );
}

#[test]
fn should_encode_each_value_as_to_json_encodes_it_in_its_array() {
    // One serializer semantics (ADR-0954): a line of `to jsonl` is the element `to json` writes
    // for the same value — typed values in their canonical data form, records as plain objects.
    let scratch = scratch();
    let values = "echo \"[1,2]\" | from json | each { ({size: (@ * 1KiB), name: \"x\"}) }";

    let array = run_bounded(&scratch, &format!("{values} | to json"), BUDGET);
    let lines = run_bounded(&scratch, &format!("{values} | to jsonl"), BUDGET);

    assert_eq!(lines.code, Some(0), "{}", lines.report());
    let expected: serde_json::Value =
        serde_json::from_str(array.stdout.trim()).expect("`to json` writes one array");
    let written: Vec<serde_json::Value> = lines
        .stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("every line is one JSON document"))
        .collect();
    assert_eq!(
        serde_json::Value::Array(written),
        expected,
        "{}",
        lines.report()
    );
    assert_eq!(
        lines.stdout,
        "{\"size\":1024,\"name\":\"x\"}\n{\"size\":2048,\"name\":\"x\"}\n",
        "one compact document per line, each line ended. {}",
        lines.report()
    );
}

#[test]
fn should_end_when_take_bounds_the_stream_before_or_after_the_serializer() {
    let source_home = scratch();
    let source = source_home.write("jsonl/source.log", "a\nb\nc\n");
    for script in [
        format!(
            "tail file {} --lines 3 --follow | take 2 | to jsonl",
            source.display()
        ),
        format!(
            "tail file {} --lines 3 --follow | to jsonl | take 2",
            source.display()
        ),
    ] {
        let run = run_bounded(&source_home, &script, BUDGET);

        assert!(
            run.finished,
            "`take 2` ends a stream that would never have ended. {}",
            run.report()
        );
        assert_eq!(run.code, Some(0), "{}", run.report());
        assert_eq!(run.stdout, "\"a\"\n\"b\"\n", "{}", run.report());
    }
}

#[test]
fn should_accept_an_unbounded_stream_that_to_json_could_not_finish() {
    // `to jsonl` holds nothing, so a stream declared unbounded is a legal input — no
    // `stream.unbounded_operation` — and nothing about it waits for an end.
    let home = scratch();
    let source = home.write("jsonl/source.log", "only\n");
    let reading = Reading::start(
        &home,
        &format!(
            "tail file {} --lines 1 --follow | to jsonl --human",
            source.display()
        ),
    );

    assert_eq!(reading.next_line().as_deref(), Some("\"only\""));
}

#[test]
fn should_name_the_streaming_serializer_when_a_live_stream_has_no_representation() {
    // §18.3: an unbounded stream nobody is watching needs a representation. The refusal's advice
    // is the one serializer that can actually write such a stream — a single JSON array never
    // ends, so suggesting `to json` sent the user from a refusal into a run that never answers.
    let home = scratch();
    let source = home.write("jsonl/source.log", "only\n");

    let run = run_bounded(
        &home,
        &format!("tail file {} --lines 1 --follow", source.display()),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        run.stderr.contains("stream.unbounded_operation") && run.stderr.contains("to jsonl"),
        "the refusal names `to jsonl`. {}",
        run.report()
    );
}

#[test]
fn should_stop_without_a_panic_when_the_reader_goes_away() {
    // EPIPE: `head` takes its line and leaves. The shell writing to it ends the way a program
    // killed by `SIGPIPE` does — 141 — never with a panic's 101, and it ends without the source
    // ever producing another value: the reader leaving is noticed by itself.
    let home = scratch();
    let source = home.write("jsonl/source.log", "first\n");
    let script = format!(
        "{} -c 'tail file {} --lines 1 --follow | to jsonl' | head -n 1; echo \"ono=${{PIPESTATUS[0]}}\"",
        ono_testkit::ono_binary().display(),
        source.display()
    );
    let reading = BashReading::start(&home, &script);

    assert_eq!(reading.next_line().as_deref(), Some("\"first\""));
    assert_eq!(
        reading.next_line().as_deref(),
        Some("ono=141"),
        "the shell ended as a program whose reader left ends, and the source never grew"
    );
    assert_eq!(lines_of(&source), 1);
}

#[test]
fn should_stop_writing_into_a_program_that_has_read_enough() {
    // Inside the shell, too: `to jsonl` feeds a program while the source is open, the program's
    // own status is the pipeline's, and its leaving ends the stream — the source here never ends.
    let home = scratch();
    let source = home.write("jsonl/source.log", "first\nsecond\n");

    let run = run_bounded(
        &home,
        &format!(
            "tail file {} --lines 2 --follow | to jsonl | head -n 1\necho \"status-$?\"",
            source.display()
        ),
        BUDGET,
    );

    assert!(
        run.finished,
        "the program read its line and left, and the stream stopped with it. {}",
        run.report()
    );
    assert!(
        run.stdout.starts_with("\"first\"\n") && run.stdout.contains("status-0"),
        "the program got the first line as it was written, and its status is the pipeline's. {}",
        run.report()
    );
}

/// A named pipe at `path`, and a reader that opens it, reads `bytes` bytes and leaves.
fn fifo_read_once(path: &Path, bytes: usize) -> std::thread::JoinHandle<Vec<u8>> {
    let status = Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("mkfifo runs");
    assert!(status.success(), "the named pipe was made");
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let mut file = std::fs::File::open(&path).expect("the named pipe opens for reading");
        let mut read = vec![0_u8; bytes];
        let _ = std::io::Read::read_exact(&mut file, &mut read);
        read
    })
}

/// Lets a reader still waiting for a writer to open `fifo` go: a shell that never opened it must
/// not leave the test waiting on its reader.
fn release(fifo: &Path) {
    use std::os::unix::fs::OpenOptionsExt as _;
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(fifo);
}

#[test]
fn should_end_the_line_with_141_and_go_on_when_a_redirected_streams_reader_leaves() {
    // Review R6: a redirect target is not the shell's own output. When the reader of a named pipe
    // the line writes into leaves, the line ends as a program killed by `SIGPIPE` ends — 141 —
    // and the shell goes on with the next line. The source here never ends, so the reader leaving
    // has to be noticed without another value being written.
    let home = scratch();
    let source = home.write("jsonl/source.log", "first\n");
    let fifo = home.path().join("jsonl/reader.fifo");
    let reader = fifo_read_once(&fifo, "\"first\"\n".len());

    let run = run_bounded(
        &home,
        &format!(
            "tail file {} --lines 1 --follow | to jsonl > {}\necho \"after-$?\"",
            source.display(),
            fifo.display()
        ),
        BUDGET,
    );

    release(&fifo);
    assert_eq!(
        reader.join().expect("the reader ran"),
        b"\"first\"\n",
        "the reader got the line as it was written"
    );
    assert!(
        run.finished,
        "the reader leaving ended the line. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("after-141"),
        "the line's status is SIGPIPE's, and the shell went on to the next line. {}",
        run.report()
    );
}

#[test]
fn should_end_the_line_with_141_and_go_on_when_a_redirected_documents_reader_leaves() {
    // The same for a document written whole: more than a pipe holds, so the write is still going
    // when the reader leaves after its first byte.
    let home = scratch();
    let line = "x".repeat(200);
    let source = home.write(
        "jsonl/source.log",
        (0..2000).map(|_| format!("{line}\n")).collect::<String>(),
    );
    let fifo = home.path().join("jsonl/reader.fifo");
    let reader = fifo_read_once(&fifo, 1);

    let run = run_bounded(
        &home,
        &format!(
            "tail file {} --lines 2000 --follow false | to json > {}\necho \"after-$?\"",
            source.display(),
            fifo.display()
        ),
        BUDGET,
    );

    release(&fifo);
    let _ = reader.join();
    assert!(run.finished, "{}", run.report());
    assert!(
        run.stdout.contains("after-141"),
        "the line's status is SIGPIPE's, and the shell went on to the next line. {}",
        run.report()
    );
}

#[test]
fn should_report_each_failure_as_it_arrives_while_the_stream_goes_on() {
    // Review C4: a failure of one item is reported when it arrives, not held for an end an
    // unbounded stream never reaches — held, nothing was reported at all, and every failure was
    // one more value kept. The source here never ends; each line it gains fails `where`, and each
    // failure is on the terminal before the next line exists.
    let home = scratch();
    let source = home.write("jsonl/source.log", "first\n");
    let script = format!(
        "{} -c 'tail file {} --lines 1 --follow | where @ | to jsonl' 2>&1",
        ono_testkit::ono_binary().display(),
        source.display()
    );
    let reading = BashReading::start(&home, &script);

    let first = reading.next_line().unwrap_or_default();
    assert!(
        first.contains("predicate must be true, false or null"),
        "the first item's failure was reported while the stream was open, got {first:?}"
    );
    let _help = reading.next_line();
    append(&source, "second");
    let second = reading.next_line().unwrap_or_default();
    assert!(
        second.contains("predicate must be true, false or null"),
        "and so was the next one, when it arrived, got {second:?}"
    );
}

#[test]
fn should_keep_memory_flat_while_an_unbounded_stream_is_serialized() {
    // Bounded memory: how much of the source the shell had to read to answer `take 1` after the
    // serializer does not grow with how much the source holds.
    let small = serialized_through(200);
    let large = serialized_through(2000);

    assert!(small >= 1 && large >= 1, "got {small} and {large}");
    assert!(
        small < 200 && large < 200,
        "answering one line cost a bounded prefix of the source, whatever it held: 200 available \
         values cost {small}, 2000 cost {large}"
    );
}

fn serialized_through(lines: usize) -> usize {
    let home = scratch();
    let text: String = (0..lines).map(|line| format!("line-{line}\n")).collect();
    let source = home.write("jsonl/source.log", text);
    let trace = home.path().join("jsonl/worked.log");
    let run = run_bounded(
        &home,
        &format!(
            "tail file {} --lines {lines} --follow | each {{ @ | to text >> {}; @ }} | to jsonl \
             | take 1",
            source.display(),
            trace.display()
        ),
        BUDGET,
    );
    assert!(run.finished, "{}", run.report());
    assert_eq!(run.stdout, "\"line-0\"\n", "{}", run.report());
    std::fs::read_to_string(&trace)
        .unwrap_or_default()
        .lines()
        .count()
}

#[test]
fn should_stop_a_streaming_serializer_on_ctrl_c_and_keep_the_prompt() {
    // At a terminal: lines appear as values arrive, and Ctrl-C ends the line with the status
    // every shell reports for an interrupted command (ADR-0008).
    let directory = scratch();
    let source = directory.write("jsonl/source.log", "first\n");
    let mut shell = support::interactive_shell_in(&directory);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(
            format!(
                "tail file {} --lines 1 --follow | each {{ @ + \"-out\" }} | to jsonl\n",
                source.display()
            )
            .as_bytes(),
        )
        .expect("the terminal accepts input");
    let seen = read_until(&mut shell, "\"first-out\"", Duration::from_secs(20));
    assert!(
        seen.contains("\"first-out\""),
        "the first line is on the terminal while the source is open; saw:\n{seen}"
    );

    shell
        .write_all(&[0x03])
        .expect("the terminal accepts Ctrl-C");
    shell
        .write_all(b"echo \"after-$?\"\n")
        .expect("the terminal accepts input");
    let seen = read_until(&mut shell, "after-130", Duration::from_secs(20));
    assert!(
        seen.contains("after-130"),
        "Ctrl-C ended the streaming line and the shell stayed standing; saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

#[test]
fn should_give_the_prompt_back_when_a_fed_program_is_stopped_with_ctrl_z() {
    // Review C3: the program a streaming serializer feeds is the foreground job, so Ctrl-Z stops
    // it — and the shell, waiting for the next value of a source that never ends, did not notice
    // and never came back. A stopped fed program is a stopped job: the prompt returns with the
    // status of a stopped command, 128 + SIGTSTP, and `jobs` lists it.
    let directory = scratch();
    let source = directory.write("jsonl/source.log", "first\n");
    let mut shell = support::interactive_shell_in(&directory);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(
            format!(
                "tail file {} --lines 1 --follow | to jsonl | cat\n",
                source.display()
            )
            .as_bytes(),
        )
        .expect("the terminal accepts input");
    let seen = read_until(&mut shell, "\"first\"", Duration::from_secs(20));
    assert!(
        seen.contains("\"first\""),
        "the program got the first line while the source was open; saw:\n{seen}"
    );

    shell
        .write_all(&[0x1a])
        .expect("the terminal accepts Ctrl-Z");
    shell
        .write_all(b"echo \"after-$?\"; jobs\n")
        .expect("the terminal accepts input");
    let seen = read_until(&mut shell, "after-148", Duration::from_secs(20));
    assert!(
        seen.contains("after-148"),
        "Ctrl-Z stopped the fed program and gave the prompt back; saw:\n{seen}"
    );

    shell
        .write_all(b"kill %1; exit 0\n")
        .expect("the terminal accepts input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

/// `bash -c script`, with its standard output read line by line as it arrives.
struct BashReading {
    child: Child,
    lines: Receiver<String>,
}

impl BashReading {
    fn start(home: &Scratch, script: &str) -> Self {
        let mut child = Command::new("bash")
            .args(["-c", script])
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("xdg"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("bash is available");
        let stdout = child.stdout.take().expect("stdout was piped");
        let (sender, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        Self { child, lines }
    }

    fn next_line(&self) -> Option<String> {
        self.lines.recv_timeout(BUDGET).ok()
    }
}

impl Drop for BashReading {
    fn drop(&mut self) {
        ono_testkit::kill_tree(self.child.id());
        let _ = self.child.wait();
    }
}
