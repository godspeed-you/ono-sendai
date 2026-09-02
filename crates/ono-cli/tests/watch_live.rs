//! `watch` at the boundary a user sees: live at a terminal, explicit anywhere else (spec §18.3).

use std::time::{Duration, Instant};

use ono_process::{Command, Executor, WindowSize};
use ono_testkit::Shell;

#[test]
fn should_refuse_a_live_stream_into_a_pipe_without_a_representation() {
    // Spec §18.3: piped mode emits ordinary values through a serializer. An endless
    // unserialised stream into a pipe is a table that never learns its widths, so it is refused
    // with the fix — never silently buffered forever.
    let run = Shell::new().args(["-c", "watch process"]).run();
    assert!(!run.status().is_success());
    assert!(
        run.stderr().contains("to json"),
        "the refusal says how to choose a representation, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_emit_events_through_a_serializer_when_bounded() {
    // `watch … | take N | to json` is the §18.3 piped form: ordinary event values, and `take`
    // bounds the stream so the document can end.
    let run = Shell::new()
        .args([
            "-c",
            "watch process --every 100ms | take 2 | select kind | to json",
        ])
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"kind":"snapshot"},{"kind":"snapshot"}]"#,
        "the stream begins with the current state (ADR-0024)"
    );
}

#[test]
fn should_render_in_place_at_a_terminal_and_stop_on_ctrl_c() {
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    let mut shell = executor
        .run_pty(&command, WindowSize::new(30, 100))
        .expect("a pseudo-terminal");

    let mut seen = String::new();
    let mut buffer = [0u8; 8192];
    let read_until =
        |shell: &mut ono_process::PtySession, seen: &mut String, needle: &str, budget: Duration| {
            let deadline = Instant::now() + budget;
            while Instant::now() < deadline {
                match shell.read_timeout(&mut buffer.clone(), Duration::from_millis(150)) {
                    Ok(Some(0)) | Err(_) => break,
                    Ok(Some(_count)) => {}
                    Ok(None) => {}
                }
                // NOTE: buffer copies are fine here; the assertion below re-reads.
                if seen.contains(needle) {
                    return true;
                }
            }
            seen.contains(needle)
        };
    let _ = read_until;

    // Simple inline loop (the closure above cannot borrow the buffer mutably twice).
    let mut wait_for = |shell: &mut ono_process::PtySession, needle: &str, budget: Duration| {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if let Ok(Some(count)) = shell.read_timeout(&mut buffer, Duration::from_millis(150)) {
                seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
            }
            if seen.contains(needle) {
                return true;
            }
        }
        false
    };

    assert!(
        wait_for(&mut shell, ">", Duration::from_secs(10)),
        "a prompt"
    );
    shell
        .write_all(b"watch process --every 200ms\n")
        .expect("the terminal accepts the watch");
    assert!(
        wait_for(&mut shell, "\x1b[0J", Duration::from_secs(8)),
        "the table repaints in place (spec §18.3); saw:\n{seen}"
    );

    shell.write_all(&[0x03]).expect("Ctrl-C");
    shell.write_all(b"echo done-$?\n").expect("the follow-up");
    assert!(
        wait_for(&mut shell, "done-130", Duration::from_secs(8)),
        "Ctrl-C ends the watch with 128+SIGINT and the prompt survives; saw:\n{seen}"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

#[test]
fn should_reattach_a_backgrounded_watch_and_end_it_with_ctrl_c() {
    // ADR-0024: `fg` brings a live view's rendering back to the foreground, and Ctrl-C then
    // ends it exactly as it ends a foreground watch.
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    let mut shell = executor
        .run_pty(&command, WindowSize::new(30, 100))
        .expect("a pseudo-terminal");

    let mut seen = String::new();
    let mut buffer = [0u8; 8192];
    let mut wait_for = |shell: &mut ono_process::PtySession, needle: &str, budget: Duration| {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if let Ok(Some(count)) = shell.read_timeout(&mut buffer, Duration::from_millis(150)) {
                seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
            }
            if seen.contains(needle) {
                return true;
            }
        }
        false
    };

    assert!(
        wait_for(&mut shell, ">", Duration::from_secs(10)),
        "a prompt"
    );
    shell
        .write_all(b"watch process --every 200ms &\n")
        .expect("the terminal accepts the background watch");
    assert!(
        wait_for(&mut shell, "[%1]", Duration::from_secs(8)),
        "the job is announced under its number (spec §18.4); saw:\n{seen}"
    );

    shell.write_all(b"fg 1\n").expect("the terminal accepts fg");
    assert!(
        wait_for(&mut shell, "\x1b[0J", Duration::from_secs(8)),
        "fg reattaches the in-place rendering (ADR-0024); saw {} bytes",
        seen.len()
    );

    shell.write_all(&[0x03]).expect("Ctrl-C");
    shell.write_all(b"jobs; echo after-$?\n").expect("input");
    assert!(
        wait_for(&mut shell, "after-0", Duration::from_secs(8)),
        "the prompt survives; saw:\n{seen}"
    );
    let after_fg = seen
        .rsplit_once("fg 1")
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    assert!(
        !after_fg.contains("running watch"),
        "the collected job is gone from the table; saw:\n{seen}"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

// --- `map --live` stabilization (v0.4.1 §35.1–§35.5, §61.2, §61.5) -----------------------------

#[test]
fn should_answer_a_bounded_first_projection_before_any_update_arrives() {
    // v0.4.1 §35.2: "`map --live` MUST construct an initial projection with a bounded work
    // budget… A first frame does not need every edge if the chosen semantic zoom level
    // intentionally aggregates detail, but it MUST be truthful about omitted/pending detail."
    //
    // The opening value is the state the caller is standing in, produced before any event is
    // read, and it says what it left out rather than presenting a bound as a complete picture.
    let run = ono_testkit::ono_within(
        "map --live --json | take 1 | to json",
        Duration::from_secs(60),
    );
    run.assert_success();

    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(run.stdout().trim())
        .unwrap_or_else(|error| panic!("`to json` emits a document ({error}): {}", run.stdout()));
    let frame = document
        .as_sequence()
        .and_then(|values| values.first())
        .and_then(|value| value.as_str())
        .map(|json| {
            serde_yaml_ng::from_str::<serde_yaml_ng::Value>(json)
                .expect("`--json` renders each frame as a JSON document")
        })
        .unwrap_or_else(|| panic!("one frame was taken, got {}", run.stdout()));

    let completeness = frame["completeness"]
        .as_str()
        .unwrap_or_else(|| panic!("§35.2: the first frame states its completeness, got {frame:?}"));
    assert!(
        ["complete", "bounded", "partial", "unknown"].contains(&completeness),
        "§35.2's truthfulness about omitted detail is the `completeness` of §3.6, got \
         {completeness:?}"
    );
    let hidden = frame["hidden"]["count"]
        .as_u64()
        .unwrap_or_else(|| panic!("§3.6: a bounded projection counts what it hid, got {frame:?}"));
    assert!(
        completeness == "complete" || hidden > 0 || !frame["clusters"].is_null(),
        "a projection that is not complete must say what it stands in for — a hidden count or a \
         cluster — rather than simply being smaller (§2.17, §35.2), got {frame:?}"
    );
}

#[test]
fn should_release_the_query_task_promptly_when_a_live_map_is_cancelled() {
    // v0.4.1 §35.5: "Ctrl-C MUST end a live map promptly without waiting for a complete expensive
    // recomputation." §61.5 asks the same of the heaviest live benchmark.
    //
    // Proven by what stops rather than by a stopwatch (ADR-0459): the interrupt is sent, the
    // process is required to have gone by a budget wide enough that only a shell waiting for a
    // recomputation could miss it, and — the part that matters — nothing is left behind.
    let mut child = std::process::Command::new(ono_testkit::ono_binary())
        .args(["-c", "map --live --json | take 3 | to json"])
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("the ono binary must be built before an integration test runs it");

    // Give the opening projection time to land, so the interrupt arrives while the live loop is
    // watching rather than while the process is still starting.
    std::thread::sleep(Duration::from_secs(2));
    let pid = child.id();
    let signalled = Instant::now();
    let killed = std::process::Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .expect("`kill` is available");
    assert!(killed.success(), "the interrupt was delivered to {pid}");

    let deadline = signalled + Duration::from_secs(10);
    let mut ended = None;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => {
                ended = Some(status);
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => panic!("cannot wait for {pid}: {error}"),
        }
    }
    if ended.is_none() {
        // Nothing is left running whatever the assertion says: issue #22 found two strays
        // holding a pipeline open for seven hours.
        let _ = child.kill();
        let _ = child.wait();
    }
    let status = ended.unwrap_or_else(|| {
        panic!(
            "v0.4.1 §35.5: Ctrl-C must end a live map promptly, and this one was still running \
             {:?} after the interrupt",
            signalled.elapsed()
        )
    });
    assert!(
        !status.success(),
        "an interrupted run does not report success, got {status:?}"
    );
    let _ = child.wait();
}
