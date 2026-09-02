//! External command adapters end to end (spec v0.3 §1.4, §1.18, §1.20, §1.57, §1.71): the real
//! util-linux tools run through their bundled adapters, and the failure paths run through
//! shadowing scripts that answer the version probe and then misbehave.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

use std::time::{Duration, Instant};

use ono_testkit::ono;
use ono_testkit::{Scratch, Shell, scratch};

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

    // lsns itself exits 1 when a process it was reading vanishes mid-scan — which happens
    // under this suite's load — and a non-zero child fails the stage (spec v0.3 §1.20). The
    // race is the machine's, not the adapter's, so the enumeration is retried.
    let counted = (0..5)
        .map(|_| ono("lsns | where processes > 0 | count | to text"))
        .find(|run| run.status().code() == 0)
        .expect("lsns succeeds at least once in five runs");
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
    ono_testkit::executable_script(
        dir.path(),
        "journalctl",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'systemd 259 (259.5)'; exit 0; fi\n{body}\n"
        ),
    );
    dir
}

/// Runs `script` with the shim directory first on `PATH`, waiting out a thread that is still
/// holding the shim open.
///
/// A thread that forks between this thread's `open` and `close` of the shim inherits the write
/// descriptor, and until that child execs, the shell's `execve` on it answers ETXTBSY. The shell
/// reports that as exit 126 with "Text file busy" in the diagnostic — about a file that is
/// executable — and the assertion the test wanted to make never gets a chance. Issue #7 is one
/// sighting of that; issue #27 is the same race one crate down (ADR-0520). Every other failure is
/// answered on the first attempt.
fn shimmed(dir: &Scratch, script: &str) -> ono_testkit::Run {
    ono_testkit::while_text_file_busy(
        |run: &ono_testkit::Run| run.stderr().contains("Text file busy"),
        || {
            Shell::new()
                .args(["-c", script])
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        dir.path().display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .run()
        },
    )
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
    let run = shimmed(&dir, "journalctl | select message | to json");
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

#[test]
fn should_adapt_git_status_and_log_in_a_repository() {
    // Spec v0.3 §1.42: `git status | where state != …`, `git log | where author_email == …`.
    let repo = scratch();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(repo.path())
            .env("GIT_AUTHOR_NAME", "Case")
            .env("GIT_AUTHOR_EMAIL", "case@example.org")
            .env("GIT_COMMITTER_NAME", "Case")
            .env("GIT_COMMITTER_EMAIL", "case@example.org")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    repo.write("tracked.txt", "one\n");
    git(&["add", "tracked.txt"]);
    git(&["commit", "-q", "-m", "first commit"]);
    repo.write("tracked.txt", "two\n");
    repo.write("new file.txt", "x");

    let in_repo = |source: &str| Shell::new().args(["-c", source]).cwd(repo.path()).run();
    let status = in_repo("git status | select path state index worktree | to json");
    status.assert_success();
    assert!(
        status.stdout().contains("\"state\":\"modified\"")
            && status.stdout().contains("\"state\":\"untracked\""),
        "porcelain v2 became typed entries, got {:?}",
        status.stdout()
    );
    assert!(
        status.stdout().contains("new file.txt"),
        "a name with a space survives -z, got {:?}",
        status.stdout()
    );

    let log = in_repo(
        "git log | where author_email == \"case@example.org\" | select subject parents | to json",
    );
    log.assert_success();
    assert!(
        log.stdout().contains("\"subject\":\"first commit\"")
            && log.stdout().contains("\"parents\":[]"),
        "the explicit format became Commit records, got {:?}",
        log.stdout()
    );

    let raw = in_repo("git status --short | grep -c txt");
    raw.assert_success();
    assert_eq!(
        raw.stdout().trim(),
        "2",
        "`--short` is git's own format and stays bytes"
    );
}

#[test]
fn should_adapt_lsof_for_the_callers_own_process() {
    let run =
        ono("lsof -c ono | where fd == \"cwd\" | take 1 | select process fd kind path | to json");
    if run.status().code() != 0 {
        // lsof is optional on a developer machine; the container has it (case 080).
        assert!(
            run.stderr().contains("Ono-Sendai-E0101") || run.stderr().contains("Ono-Sendai-E0904"),
            "got {:?}",
            run.stderr()
        );
        return;
    }
    assert!(
        run.stdout().contains("\"fd\":\"cwd\"") && run.stdout().contains("\"kind\":\"DIR\""),
        "lsof's field protocol became OpenFile records, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_adapt_ss_with_combined_flags_or_say_why_not() {
    // Spec v0.3 §1.32: `ss -tunap | where state == established | select local remote process`.
    let run = ono("ss -tlnp | select protocol local state | to json");
    if run.status().code() != 0 {
        assert!(
            run.stderr().contains("Ono-Sendai-E0904") || run.stderr().contains("Ono-Sendai-E0101"),
            "got {:?}",
            run.stderr()
        );
        return;
    }
    assert!(
        run.stdout().starts_with('['),
        "typed sockets, got {:?}",
        run.stdout()
    );
    // The header line is ss's own text and does not change between two invocations; a count of
    // LISTEN rows does whenever another test opens a listener at the same time.
    let raw = ono("ss -tln | head -1");
    raw.assert_success();
    let bytes = ono("raw ss -tln | head -1");
    assert!(
        raw.stdout().contains("State") && raw.stdout() == bytes.stdout(),
        "bytes downstream keep ss's own output, got {:?} vs {:?}",
        raw.stdout(),
        bytes.stdout()
    );
}

// ---------------------------------------------------------------------------------------------
// Forced adaptation (spec v0.3 §1.18, ADR-0054): `adapt <program> …` must produce structure or
// fail — never downgrade to text — and curl is the tool that only ever adapts when asked.

#[test]
fn should_keep_curl_bytes_unless_adaptation_is_asked_for() {
    let classic = ono("curl -s file:///etc/hostname | cat");
    classic.assert_success();
    let expected = std::fs::read_to_string("/etc/hostname").unwrap_or_default();
    assert_eq!(
        classic.stdout(),
        expected,
        "spec v0.3 §1.41: stdout is the body"
    );

    let adapted =
        ono("adapt curl file:///etc/hostname | select scheme status redirects size | to json");
    if adapted.status().code() != 0 && adapted.stderr().contains("Ono-Sendai-E0101") {
        return; // no curl on this machine; the container has it (case 082)
    }
    adapted.assert_success();
    assert!(
        adapted.stdout().contains("\"scheme\":\"file\"")
            && adapted.stdout().contains("\"status\":null"),
        "an exchange record with a null status for a local scheme, got {:?}",
        adapted.stdout()
    );
    // Bytes serialise as hex (spec §33.5), so the body is compared in that form.
    let body = ono("adapt curl file:///etc/hostname | select body | to json");
    body.assert_success();
    let hex: String = expected.bytes().map(|b| format!("{b:02x}")).collect();
    assert!(
        body.stdout().contains(&hex),
        "the body is a bytes field of the record, exact, got {:?}",
        body.stdout()
    );
}

#[test]
fn should_fail_forced_adaptation_when_no_adapter_can_answer() {
    let none = ono("adapt grep x /etc/hostname");
    assert_ne!(none.status().code(), 0);
    assert!(
        none.stderr().contains("Ono-Sendai-E0911"),
        "spec v0.3 §1.18: a forced structured invocation fails rather than downgrades, got {:?}",
        none.stderr()
    );
    let refused = ono("adapt lsblk -p > /dev/null");
    assert_ne!(refused.status().code(), 0);
    assert!(
        refused.stderr().contains("Ono-Sendai-E0903"),
        "the specific refusal, got {:?}",
        refused.stderr()
    );
    let bare = ono("adapt");
    assert_eq!(bare.status().code(), 127, "got {:?}", bare.stderr());
}

#[test]
fn should_explain_and_document_forced_adaptation() {
    let run = Shell::new()
        .args(["-c", "explain adapt ps aux | grep x"])
        .run();
    run.assert_success();
    assert!(
        run.stdout()
            .contains("demand       structured (`adapt` requires structure)"),
        "got {:?}",
        run.stdout()
    );
    let help = Shell::new().args(["-c", "help adapt"]).run();
    help.assert_success();
    assert!(
        help.stdout().contains("adapt") && help.stdout().contains("raw"),
        "got {:?}",
        help.stdout()
    );
}

fn completion_shell(path_prefix: Option<&Scratch>) -> ono_process::PtySession {
    let mut executor = ono_process::Executor::detached();
    let mut command = ono_process::Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    if let Some(dir) = path_prefix {
        command = command.env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
    }
    executor
        .run_pty(&command, ono_process::WindowSize::new(30, 120))
        .expect("a pseudo-terminal must be available")
}

#[test]
fn should_complete_fields_of_the_adapted_schema_after_the_pipe_and_declared_flags_before_it() {
    // Spec v0.3 §1.59: `ss -tunap | where <TAB>` knows Socket fields; before the pipe an adapter
    // offers the invocations it declares and invents nothing else.
    let mut shell = completion_shell(None);
    let _ = read_until(&mut shell, "> ", Duration::from_secs(10));
    shell.write_all(b"ps aux | where cp\t").expect("typed");
    let seen = read_until(&mut shell, "cpu", Duration::from_secs(10));
    assert!(
        seen.contains("where cpu"),
        "the Process field completed, got {seen:?}"
    );
    shell.write_all(b"\x15").expect("clear the line");
    shell.write_all(b"lsblk --node\t").expect("typed");
    let seen = read_until(&mut shell, "--nodeps", Duration::from_secs(10));
    assert!(
        seen.contains("--nodeps"),
        "a declared flag completes, got {seen:?}"
    );
    shell.write_all(b"\x15").expect("clear the line");
    shell.write_all(b"lsblk --pa\t").expect("typed");
    std::thread::sleep(Duration::from_millis(600));
    let seen = read_until(&mut shell, "\u{0}never", Duration::from_millis(600));
    assert!(
        !seen.contains("--paths"),
        "an undeclared flag is not invented, got {seen:?}"
    );
    shell.write_all(b"\x15exit\n").expect("leave");
}

#[test]
fn should_record_the_adapter_in_history() {
    // Spec v0.3 §1.58: history remembers that a command was adapted.
    let home = scratch();
    let mut executor = ono_process::Executor::detached();
    let command = ono_process::Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", home.path().display().to_string())
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    let mut shell = executor
        .run_pty(&command, ono_process::WindowSize::new(30, 120))
        .expect("a pseudo-terminal must be available");
    let _ = read_until(&mut shell, "> ", Duration::from_secs(10));
    shell
        .write_all(b"findmnt | where target == \"/\" | count\n")
        .expect("typed");
    let _ = read_until(&mut shell, "VALUE", Duration::from_secs(10));
    shell.write_all(b"exit\n").expect("leave");
    std::thread::sleep(Duration::from_millis(500));
    let mut found = String::new();
    let mut stack = vec![home.path().to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(text) = std::fs::read_to_string(&path)
                && text.contains("findmnt")
            {
                found.push_str(&text);
            }
        }
    }
    assert!(
        found.contains("org.ono.compat.util-linux.findmnt"),
        "the history entry names the adapter, got {found:?}"
    );
}

#[test]
fn should_explain_the_adaptation_behind_the_adapt_keyword() {
    // Spec v0.3 §1.18, §1.23: `adapt` forces structure, and `explain` names the adapter that
    // gives it — the keyword is not a program, the word after it is.
    let explained = ono("explain adapt ps aux | count");
    explained.assert_success();
    assert!(
        explained
            .stdout()
            .contains("adapted by org.ono.compat.procps.ps")
            && !explained.stdout().contains("`adapt` resolves to nothing"),
        "the adapter behind `adapt ps aux` is named, got {:?}",
        explained.stdout()
    );
}

#[test]
fn should_write_one_field_s_bytes_verbatim_when_to_bytes_names_it() {
    // Spec §12.2's escape hatch, applied to one field: `adapt curl url | to bytes --field body`
    // writes the body and nothing else — no envelope, no added newline (ADR-0223).
    let dir = ono_testkit::scratch();
    let out = dir.path().join("page.html");
    let run = ono_testkit::Shell::new()
        .args([
            "-c",
            &format!(
                "from json | to bytes --field body > {}",
                out.to_string_lossy()
            ),
        ])
        .stdin("[{\"body\":\"<html>\"}]")
        .run();
    run.assert_success();
    assert_eq!(
        std::fs::read(&out).expect("the written file"),
        b"<html>",
        "the field's bytes reach the file verbatim: {:?}",
        run.output()
    );
}
