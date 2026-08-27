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

#[test]
fn should_adapt_the_ip_family_into_canonical_network_records() {
    // Spec v0.3 §1.33: `ip address | where family == inet6`, `ip link | where state == up`,
    // `ip route | where protocol == static`, `ip neigh | where state == reachable`. Loopback
    // exists everywhere; the rest is asserted by shape, not by this machine's network.
    let addresses = ono("ip address | where interface == \"lo\" | select family address | to json");
    addresses.assert_success();
    assert!(
        addresses.stdout().contains("127.0.0.1/8"),
        "one record per address with a typed ipnetwork, got {:?}",
        addresses.stdout()
    );

    let links = ono("ip link | where name == \"lo\" | select name state mtu | to json");
    links.assert_success();
    assert!(
        links.stdout().contains("\"mtu\"") && links.stdout().contains("\"state\""),
        "canonical Interface fields (spec §28.5), got {:?}",
        links.stdout()
    );

    let routes = ono("ip route | where family == \"inet\" | count | to text");
    routes.assert_success();
    assert!(
        routes.stdout().trim().parse::<u64>().is_ok(),
        "routes decode, family from the invocation, got {:?}",
        routes.stdout()
    );

    let neighbours = ono("ip neigh | select address family state | count | to text");
    neighbours.assert_success();
    assert!(
        neighbours.stdout().trim().parse::<u64>().is_ok(),
        "got {:?}",
        neighbours.stdout()
    );

    let unsupported = ono("ip -s link | where state == \"up\"");
    assert_ne!(unsupported.status().code(), 0);
    assert!(
        unsupported.stderr().contains("Ono-Sendai-E0903") && unsupported.stderr().contains("`-s`"),
        "statistics change the shape and are not adapted (spec v0.3 §1.14), got {:?}",
        unsupported.stderr()
    );
}

/// A fake `journalctl` that answers the version probe and then runs `body`.
fn journal_shim(body: &str) -> Scratch {
    let dir = scratch();
    dir.write(
        "journalctl",
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'systemd 259 (259.5)'; exit 0; fi\n{body}\n"
        ),
    );
    let mode = std::os::unix::fs::PermissionsExt::from_mode(0o755);
    std::fs::set_permissions(dir.path().join("journalctl"), mode).unwrap();
    dir
}

const ENTRY_ONE: &str = r#"{"MESSAGE":"first","PRIORITY":"6","__REALTIME_TIMESTAMP":"1787820400000000","_BOOT_ID":"b","_HOSTNAME":"h","__CURSOR":"c1"}"#;
const ENTRY_TWO: &str = r#"{"MESSAGE":"second","PRIORITY":"3","__REALTIME_TIMESTAMP":"1787820401000000","_BOOT_ID":"b","_HOSTNAME":"h","__CURSOR":"c2"}"#;

#[test]
fn should_stream_decoded_records_while_the_child_still_runs() {
    // Spec v0.3 §1.37, ADAPT-005: journal entries flow as they arrive. The shim writes one
    // entry, waits five seconds, writes another; `take 1` must answer long before that.
    let dir = journal_shim(&format!("echo '{ENTRY_ONE}'; sleep 5; echo '{ENTRY_TWO}'"));
    let started = Instant::now();
    let run = Shell::new()
        .args([
            "-c",
            "journalctl -n 2 | take 1 | select message priority | to json",
        ])
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .timeout(Duration::from_secs(20))
        .run();
    run.assert_success();
    assert!(
        run.stdout().contains("\"first\""),
        "the first record arrived, got {:?}",
        run.stdout()
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "values flowed before the child finished (took {:?})",
        started.elapsed()
    );
}

#[test]
fn should_report_a_failing_streamed_child_after_its_records() {
    // Records that arrived are shown; the child's status still stands (spec v0.3 §1.20).
    let dir = journal_shim(&format!("echo '{ENTRY_ONE}'; exit 3"));
    let run = Shell::new()
        .args(["-c", "journalctl | select message | to json"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .run();
    assert_ne!(run.status().code(), 0);
    assert!(
        run.stderr().contains("Ono-Sendai-E0501"),
        "got {:?}",
        run.stderr()
    );
}

#[test]
fn should_decode_a_streamed_stage_into_typed_journal_events() {
    let dir = journal_shim(&format!("printf '%s\\n%s\\n' '{ENTRY_ONE}' '{ENTRY_TWO}'"));
    let run = Shell::new()
        .args([
            "-c",
            "journalctl -p 3 | where priority <= 3 | select message timestamp | to json",
        ])
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .run();
    run.assert_success();
    assert!(
        run.stdout().contains("\"second\"") && !run.stdout().contains("\"first\""),
        "spec v0.3 §1.37: `where priority <= 3` over typed events, got {:?}",
        run.stdout()
    );
    assert!(
        run.stdout().contains("2026-08-27T08:46:41"),
        "microseconds since the epoch became a timestamp, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_follow_the_journal_live_at_the_terminal_until_interrupted() {
    // Spec v0.3 §1.37: `journalctl -f` is a live stream — unbounded, rendered in place at a
    // terminal (spec §18.3), ended by Ctrl-C, which reaches the shell and stops the child.
    let dir = journal_shim(&format!(
        "trap 'exit 0' TERM; echo '{ENTRY_ONE}'; sleep 0.3; echo '{ENTRY_TWO}'; while true; do sleep 0.2; done"
    ));
    let mut executor = ono_process::Executor::detached();
    let command = ono_process::Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string())
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
    let mut session = executor
        .run_pty(&command, ono_process::WindowSize::new(30, 120))
        .expect("a pseudo-terminal must be available");
    let _ = read_until(&mut session, "> ", Duration::from_secs(10));
    session.write_all(b"journalctl -f\n").expect("typed");
    let seen = read_until(&mut session, "second", Duration::from_secs(10));
    assert!(
        seen.contains("first") && seen.contains("second"),
        "both entries rendered as they arrived, got {seen:?}"
    );
    session.write_all(b"\x03").expect("Ctrl-C");
    session.write_all(b"echo alive-$?\n").expect("typed");
    let after = read_until(&mut session, "alive-", Duration::from_secs(10));
    assert!(
        after.contains("alive-"),
        "the prompt is back after Ctrl-C and the follower is gone, got {after:?}"
    );
}

#[test]
fn should_make_ps_compose_while_keeping_its_selection_and_its_bytes() {
    // Spec v0.3 §1.71 opens with `ps aux | where cpu > 20`; pid 1 exists everywhere.
    let one = ono("ps aux | where pid == 1 | select pid name user state | to json");
    one.assert_success();
    assert!(
        one.stdout().contains("\"pid\": 1") || one.stdout().contains("\"pid\":1"),
        "got {:?}",
        one.stdout()
    );
    assert!(
        one.stdout().contains("\"state\""),
        "canonical Process fields, got {:?}",
        one.stdout()
    );

    let sorted = ono("ps aux | sort memory desc | take 3 | count | to text");
    sorted.assert_success();
    assert_eq!(
        sorted.stdout().trim(),
        "3",
        "`sort memory desc` over typed byte sizes"
    );

    // A bare `ps` keeps ps's own selection (this terminal's processes), which under a test
    // harness is nothing or next to nothing — but never every process.
    let own = ono("ps | count | to text");
    own.assert_success();
    let every = ono("ps -e | count | to text");
    every.assert_success();
    let own: u64 = own.stdout().trim().parse().unwrap_or(0);
    let every: u64 = every.stdout().trim().parse().unwrap_or(0);
    assert!(
        every > own,
        "spec v0.3 §1.14: `ps` is not widened into `ps -e` (own={own}, every={every})"
    );

    let raw = ono("raw ps -o pid= -p 1");
    let bytes = ono("ps -o pid= -p 1 | cat");
    assert_eq!(
        raw.stdout(),
        bytes.stdout(),
        "bytes downstream are ps's own"
    );

    let refused = ono("ps -o pid= | where pid == 1");
    assert_ne!(refused.status().code(), 0);
    assert!(
        refused.stderr().contains("Ono-Sendai-E0903"),
        "`-o` changes what a row is, got {:?}",
        refused.stderr()
    );
}

#[test]
fn should_adapt_df_and_refuse_human_units() {
    // GNU df is on every Linux machine this suite runs on.
    let run = ono("df / | select source type size available target | to json");
    run.assert_success();
    assert!(
        run.stdout().contains("\"target\": \"/\"") || run.stdout().contains("\"target\":\"/\""),
        "the root filesystem as a typed record, got {:?}",
        run.stdout()
    );
    let human = ono("df -h / | where available > 1MiB");
    assert_ne!(human.status().code(), 0);
    assert!(
        human.stderr().contains("Ono-Sendai-E0903"),
        "`-h` runs raw (spec v0.3 §1.39), got {:?}",
        human.stderr()
    );
}

#[test]
fn should_adapt_gnu_stat_and_find_or_say_why_not() {
    // The contracts are written for GNU coreutils and findutils; a machine with uutils' stat
    // or bfs' find gets an honest version refusal (spec v0.3 §1.46) — and the container, which
    // has the GNU tools, proves the structured path (case 079).
    for (line, needle) in [
        (
            "stat /etc/hostname | select path kind size | to json",
            "\"kind\":\"file\"",
        ),
        (
            "find /etc/hostname -maxdepth 0 | select path kind | to json",
            "\"kind\":\"file\"",
        ),
    ] {
        let run = ono(line);
        if run.status().code() == 0 {
            assert!(run.stdout().contains(needle), "got {:?}", run.stdout());
        } else {
            assert!(
                run.stderr().contains("Ono-Sendai-E0904"),
                "only an incompatible version may refuse, got {:?}",
                run.stderr()
            );
        }
    }
}
