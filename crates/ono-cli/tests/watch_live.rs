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
