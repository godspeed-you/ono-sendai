//! The ceilings a listening agent enforces come from the same configuration the shell reads
//! (issue #190, v0.4.1 §12.4, §55.1, §55.2; ADR-0010, ADR-0504).
//!
//! `limits.remote_*` are ordinary settings: `get config` lists them, `inspect limits` reports
//! them, and `config.ono` and the `ONO_*` variables set them. Until v0.6.6 `ono --agent --listen`
//! read only the environment layer, so a ceiling an operator wrote into `config.ono` was a figure
//! `inspect limits` printed and the listener ignored. These cases start a real agent on the
//! loopback interface and hold it to the file.
//!
//! The other half is what a ceiling that cannot be read does. §55.2: "a security-sensitive agent
//! limit MUST NOT silently become unlimited because a value failed to parse". An agent that fell
//! back to Appendix A's figure when the operator wrote a smaller one would be exactly that, so a
//! configuration it cannot read refuses the listener instead of widening it.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0913).
#![cfg(feature = "remote")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::io::{BufRead as _, BufReader, Read as _};
use std::process::{Command, Stdio};
use std::time::Duration;

use ono_testkit::{Scratch, scratch};

mod support;
use support::binary;

/// What a started agent said, and how it ended if it did.
struct Agent {
    process: ono_testkit::OwnedChild,
    lines: Vec<String>,
    address: Option<String>,
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

impl Agent {
    fn field(&self, field: &str) -> Option<&str> {
        self.lines
            .iter()
            .find_map(|line| line.strip_prefix(&format!("ono: {field} ")))
            .map(str::trim)
    }

    fn said(&self) -> String {
        self.lines.join("\n")
    }
}

/// Starts `ono [before…] --agent --listen 127.0.0.1:0` in `home` with `environment`, and reads
/// its standard error until the startup summary is complete or the process has ended.
fn agent(home: &Scratch, before: &[&str], environment: &[(&str, &str)]) -> Agent {
    let mut command = Command::new(binary());
    command
        .args(before)
        .args(["--agent", "--listen", "127.0.0.1:0"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env_remove("ONO_CONFIG")
        .env_remove("ONO_CONFIG_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for name in [
        "ONO_LIMITS_REMOTE_CONNECTIONS",
        "ONO_LIMITS_REMOTE_PENDING_HANDSHAKES",
        "ONO_LIMITS_REMOTE_CONNECTIONS_PER_CLIENT",
        "ONO_LIMITS_REMOTE_HANDSHAKE_TIMEOUT_MS",
    ] {
        command.env_remove(name);
    }
    for (name, value) in environment {
        command.env(name, value);
    }
    let mut process: ono_testkit::OwnedChild = command.spawn().expect("the agent starts").into();

    let stderr = process.stderr.take().expect("stderr was piped");
    let mut reader = BufReader::new(stderr);
    let mut lines = Vec::new();
    let mut address = None;
    let mut line = String::new();
    loop {
        line.clear();
        // End of stream is an agent that ended before it finished its summary.
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let trimmed = line.trim_end().to_owned();
        if let Some(bound) = trimmed.strip_prefix("ono: listening on ") {
            address = Some(bound.trim().to_owned());
        }
        let last = trimmed.starts_with("ono: handshake timeout ");
        lines.push(trimmed);
        if last {
            break;
        }
    }
    Agent {
        process,
        lines,
        address,
    }
}

fn configure(home: &Scratch, source: &str) {
    home.write("ono/config.ono", source);
}

#[test]
fn should_enforce_a_remote_ceiling_written_in_the_configuration_file() {
    let home = scratch();
    // One handshake may be pending; the next connection is closed at once (§12.2). The budget is
    // long so the first connection is still pending when the second arrives.
    configure(
        &home,
        "set config limits.remote_pending_handshakes = 1\n\
         set config limits.remote_handshake_timeout_ms = 600000\n",
    );
    let agent = agent(&home, &[], &[]);
    assert_eq!(
        agent.field("maximum pending handshakes"),
        Some("1"),
        "the summary prints the ceiling `config.ono` sets, got:\n{}",
        agent.said()
    );
    let address = agent.address.clone().expect("the agent is listening");

    let pending = std::net::TcpStream::connect(&address).expect("the first peer connects");
    let mut over = std::net::TcpStream::connect(&address).expect("the second peer connects");
    over.set_read_timeout(Some(ono_testkit::under_load(Duration::from_secs(10))))
        .expect("a read timeout can be set");
    let mut byte = [0u8; 1];
    let read = over.read(&mut byte);
    assert!(
        matches!(read, Ok(0)),
        "the listener enforces the file's ceiling: a peer over one pending handshake is closed \
         at once rather than held, got {read:?}"
    );
    drop(pending);
}

#[test]
fn should_let_the_environment_override_the_configuration_file_as_it_does_for_the_shell() {
    let home = scratch();
    configure(&home, "set config limits.remote_connections = 3\n");

    let from_file = agent(&home, &[], &[]);
    assert_eq!(
        from_file.field("maximum connections"),
        Some("3"),
        "the file's figure, got:\n{}",
        from_file.said()
    );
    drop(from_file);

    let overridden = agent(&home, &[], &[("ONO_LIMITS_REMOTE_CONNECTIONS", "5")]);
    assert_eq!(
        overridden.field("maximum connections"),
        Some("5"),
        "the environment is the later layer for an agent exactly as for the shell (ADR-0010), \
         got:\n{}",
        overridden.said()
    );
}

#[test]
fn should_read_no_configuration_file_when_told_not_to() {
    let home = scratch();
    configure(&home, "set config limits.remote_connections = 3\n");

    let agent = agent(&home, &["--no-config"], &[]);

    assert_eq!(
        agent.field("maximum connections"),
        Some("32"),
        "`--no-config` reads no configuration layer, so Appendix A's figure applies, got:\n{}",
        agent.said()
    );
}

#[test]
fn should_keep_the_environments_ceilings_when_told_to_read_no_configuration_file() {
    // v0.6.5's listener read the `ONO_LIMITS_REMOTE_*` variables whatever `--no-config` said, and
    // the advice the agent gives on a configuration it cannot read is `--no-config`: dropping an
    // operator's tighter ceiling there would widen the listener with no word (review COR-R1,
    // v0.4.1 §55.2). `--no-config` leaves the files out; the environment still applies.
    let home = scratch();
    configure(&home, "set config limits.remote_connections = 3\n");

    let agent = agent(
        &home,
        &["--no-config"],
        &[("ONO_LIMITS_REMOTE_CONNECTIONS", "1")],
    );

    assert_eq!(
        agent.field("maximum connections"),
        Some("1"),
        "the environment's ceiling holds under `--no-config`, the file's does not, got:\n{}",
        agent.said()
    );
}

#[test]
fn should_refuse_to_listen_when_the_configuration_file_sets_a_ceiling_out_of_range() {
    let home = scratch();
    configure(&home, "set config limits.remote_connections = 0\n");

    let mut agent = agent(&home, &[], &[]);

    assert_eq!(
        agent.address,
        None,
        "an agent whose configured ceiling cannot be applied opens no socket, got:\n{}",
        agent.said()
    );
    let status = agent.process.wait().expect("the agent ends");
    assert!(!status.success(), "and it ends with a failure status");
    assert!(
        agent.said().contains("limits.remote_connections"),
        "the refusal names the setting the operator has to fix, got:\n{}",
        agent.said()
    );
}

#[test]
fn should_refuse_to_listen_when_the_environment_sets_a_ceiling_it_cannot_read() {
    let home = scratch();

    let mut agent = agent(
        &home,
        &[],
        &[("ONO_LIMITS_REMOTE_PENDING_HANDSHAKES", "a few")],
    );

    assert_eq!(
        agent.address,
        None,
        "§55.2: an unreadable ceiling never becomes Appendix A's wider one, got:\n{}",
        agent.said()
    );
    assert!(!agent.process.wait().expect("the agent ends").success());
    assert!(
        agent
            .said()
            .contains("ONO_LIMITS_REMOTE_PENDING_HANDSHAKES"),
        "the refusal names the variable that set it, got:\n{}",
        agent.said()
    );
}

#[test]
fn should_refuse_to_listen_when_the_configuration_file_cannot_be_read() {
    let home = scratch();
    // A file that does not parse may well have been where the ceilings were.
    configure(&home, "set config limits.remote_connections = (\n");

    let mut agent = agent(&home, &[], &[]);

    assert_eq!(
        agent.address,
        None,
        "a configuration the agent could not read is not a configuration that set no ceiling, \
         got:\n{}",
        agent.said()
    );
    assert!(!agent.process.wait().expect("the agent ends").success());
    assert!(
        agent.said().contains("config.ono"),
        "the refusal names the file, got:\n{}",
        agent.said()
    );
}

#[test]
fn should_describe_the_remote_ceilings_as_enforced_by_the_listening_agent() {
    let home = scratch();
    for key in [
        "limits.remote_connections",
        "limits.remote_pending_handshakes",
        "limits.remote_connections_per_client",
        "limits.remote_handshake_timeout_ms",
    ] {
        let described = support::ono_at_home(
            &home,
            &format!("get config {key} | select description | to json"),
        );
        described.assert_success();
        let said = described.stdout();
        assert!(
            !said.contains("phase H3") && said.contains("listening agent enforces"),
            "`{key}` is enforced by the listening agent (limits.yaml: enforced_by ono-remote), \
             and its description says so rather than promising a later phase, got {said:?}"
        );
    }
}
