//! External command adapters end to end (spec v0.3 §1.4, §1.18, §1.20, §1.57, §1.71): the real
//! util-linux tools run through their bundled adapters, and the failure paths run through
//! shadowing scripts that answer the version probe and then misbehave.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

use std::time::{Duration, Instant};

use ono_testkit::{Scratch, Shell, scratch};

fn ono(source: &str) -> ono_testkit::Run {
    Shell::new().args(["-c", source]).run()
}

/// A directory holding a fake `lsblk` that answers the version probe like util-linux and
/// otherwise runs `body`.
fn shadow(body: &str) -> Scratch {
    let dir = scratch();
    dir.write(
        "lsblk",
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'lsblk from util-linux 2.41.3'; exit 0; fi\n{body}\n"
        ),
    );
    let mode = std::os::unix::fs::PermissionsExt::from_mode(0o755);
    std::fs::set_permissions(dir.path().join("lsblk"), mode).unwrap();
    dir
}

fn shadowed(dir: &Scratch, source: &str) -> ono_testkit::Run {
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Shell::new().args(["-c", source]).env("PATH", path).run()
}

#[test]
fn should_compose_a_familiar_command_with_ono_semantics() {
    // Spec v0.3 §1.71: "familiar commands suddenly become composable with Ono semantics
    // without losing their Unix identity".
    let run = ono("findmnt | where target == \"/\" | select target filesystem | to json");
    run.assert_success();
    assert!(
        run.stdout().contains("\"target\": \"/\"") || run.stdout().contains("\"target\":\"/\""),
        "the root mount is one typed record, got {:?}",
        run.stdout()
    );
    assert!(
        run.stdout().contains("\"filesystem\""),
        "canonical field names, not findmnt's, got {:?}",
        run.stdout()
    );

    let counted = ono("lsns | where processes > 0 | count | to text");
    counted.assert_success();
    assert!(
        counted.stdout().trim().parse::<u64>().is_ok_and(|n| n > 0),
        "namespaces exist on every Linux machine, got {:?}",
        counted.stdout()
    );
}

#[test]
fn should_expose_adapter_provenance_through_inspect() {
    let run = ono("findmnt | take 1 | inspect | to json");
    run.assert_success();
    let text = run.stdout();
    for needle in [
        "adapter:org.ono.compat.util-linux.findmnt",
        "actual_invocation",
        "findmnt --json --list --bytes --output TARGET,SOURCE,FSTYPE,OPTIONS",
        "executable_version",
        "user_invocation",
    ] {
        assert!(
            text.contains(needle),
            "spec v0.3 §1.8: `{needle}` is answered, got {text}"
        );
    }
}

#[test]
fn should_keep_the_raw_form_byte_identical_to_the_tool() {
    let raw = ono("raw findmnt --json --list --output TARGET /");
    raw.assert_success();
    let plain = ono("findmnt --json --list --output TARGET / | cat");
    plain.assert_success();
    assert_eq!(
        raw.stdout(),
        plain.stdout(),
        "bytes downstream keep the tool's own output (spec v0.3 §1.4)"
    );
}

#[test]
fn should_fail_a_structured_pipeline_on_an_undeclared_flag() {
    let run = ono("lsblk -p | where type == \"disk\"");
    assert_ne!(run.status().code(), 0);
    assert!(
        run.stderr().contains("Ono-Sendai-E0903"),
        "spec v0.3 §1.16: adapter.unsupported_invocation, got {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("raw lsblk -p"),
        "the recovery names the raw form (spec v0.3 §1.16), got {:?}",
        run.stderr()
    );
}

#[test]
fn should_never_turn_a_failing_child_into_success() {
    // Valid JSON on stdout, exit status 2: the child failed, and that is the answer.
    let dir = shadow(
        "echo '{\"blockdevices\": [{\"name\": \"sda\", \"path\": \"/dev/sda\", \"type\": \"disk\", \"size\": 1, \"mountpoints\": [], \"ro\": false, \"rm\": false, \"maj:min\": \"8:0\"}]}'; exit 2",
    );
    let run = shadowed(&dir, "lsblk | where type == \"disk\" | count");
    assert_ne!(
        run.status().code(),
        0,
        "spec v0.3 §1.20: exit status keeps Unix semantics"
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0501"),
        "external.exit_nonzero, got {:?}",
        run.stderr()
    );
    assert!(
        !run.stdout().contains('1'),
        "nothing decoded from a failed child is shown, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_fail_structurally_when_the_tool_writes_something_else() {
    let dir = shadow("echo 'NAME MAJ:MIN RM SIZE RO TYPE'; echo 'sda 8:0 0 128G 0 disk'");
    let run = shadowed(&dir, "lsblk | where type == \"disk\"");
    assert_ne!(run.status().code(), 0);
    assert!(
        run.stderr().contains("Ono-Sendai-E0907"),
        "adapter.decode_failed under a structured demand (spec v0.3 §1.18), got {:?}",
        run.stderr()
    );
}

fn read_until(session: &mut ono_process::PtySession, needle: &str, budget: Duration) -> String {
    let deadline = Instant::now() + budget;
    let mut seen = String::new();
    let mut buffer = [0u8; 4096];
    while Instant::now() < deadline {
        match session.read_timeout(&mut buffer, Duration::from_millis(200)) {
            Ok(Some(0)) | Err(_) => break,
            Ok(Some(count)) => {
                seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
                if seen.contains(needle) {
                    return seen;
                }
            }
            Ok(None) => {}
        }
    }
    seen
}

fn at_terminal(path: Option<&Scratch>, source: &str) -> String {
    let mut executor = ono_process::Executor::detached();
    let mut command = ono_process::Command::new(ono_testkit::ono_binary())
        .args(["-c", source])
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    if let Some(dir) = path {
        command = command.env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
    } else {
        command = command.env("PATH", std::env::var("PATH").unwrap_or_default());
    }
    let mut session = executor
        .run_pty(&command, ono_process::WindowSize::new(40, 120))
        .expect("a pseudo-terminal must be available");
    read_until(&mut session, "\u{0}never", Duration::from_secs(15))
}

#[test]
fn should_render_an_adapted_command_as_a_table_at_the_terminal() {
    // Spec v0.3 §1.4: at a terminal a high-confidence adapter may let the renderer display.
    let text = at_terminal(None, "findmnt");
    assert!(
        text.contains("TARGET") && text.contains("FILESYSTEM"),
        "the canonical default view, not findmnt's own table, got {text:?}"
    );
    let raw = at_terminal(None, "raw findmnt");
    assert!(
        raw.contains("FSTYPE") && !raw.contains("FILESYSTEM"),
        "`raw` keeps findmnt's own header (spec v0.3 §1.17), got {raw:?}"
    );
}

#[test]
fn should_fall_back_to_the_captured_bytes_at_the_terminal_when_decoding_fails() {
    // Spec v0.3 §1.57: at the terminal a decode failure shows the diagnostic and the tool's
    // own output — without running the tool a second time.
    let dir = shadow("echo 'sda 8:0 0 128G 0 disk once'; echo ran >> \"$0.count\"");
    let text = at_terminal(Some(&dir), "lsblk");
    assert!(
        text.contains("sda 8:0 0 128G 0 disk once"),
        "the bytes the tool wrote reach the terminal, got {text:?}"
    );
    assert!(
        text.contains("falling back to raw output"),
        "the diagnostic of §1.57, got {text:?}"
    );
    let runs = std::fs::read_to_string(dir.path().join("lsblk.count")).unwrap_or_default();
    assert_eq!(runs.lines().count(), 1, "the tool ran exactly once");
}
