//! What `--listen` tells the operator, and what it does when nobody is authorized (v0.4.1 spec
//! §11.1, §11.2, §11.3, §12.1; issue #55).
//!
//! §11.1 makes network exposure explicit: `ono --agent` without `--listen` opens no socket at
//! all. §11.2 then fixes what an agent that *is* exposed must say for itself before it accepts
//! anybody:
//!
//! ```text
//! bound address
//! server peer fingerprint
//! authorization store path
//! authorized client count
//! maximum concurrent connections
//! ```
//!
//! An operator reads that block to know what they just put on a network, and it is the only
//! moment at which they can read it — a fingerprint they have to pin is worth something on the
//! host's own console and nothing over the link that would have to be trusted to carry it.
//!
//! §11.2's last sentence is the other half, and it settles a question that reads both ways:
//! "If the authorization store contains zero clients, the agent **MAY** listen but MUST refuse all
//! connections after cryptographic handshake." So an empty store is not a startup failure; it is a
//! listener that says no to everyone, loudly, and says so on the way up (ADR-0504).
//!
//! The agent here is a real second `ono` process on the loopback interface, so nothing needs a
//! network or a fixture host.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::io::{BufRead as _, BufReader};
use std::process::{Child, Command, Stdio};

use ono_cli::invocation::Invocation;
use ono_testkit::{Scratch, Shell, scratch};

/// A listening agent and everything it said on the way up.
struct Agent {
    process: Child,
    summary: Vec<String>,
    address: String,
}

impl Drop for Agent {
    /// Killed and reaped by the test that started it: a leaked agent outlives the suite.
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

impl Agent {
    /// The value the startup summary printed for `field`, if it printed one.
    fn field(&self, field: &str) -> Option<&str> {
        self.summary
            .iter()
            .find_map(|line| line.strip_prefix(&format!("ono: {field} ")))
            .map(str::trim)
    }
}

fn binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("the test binary knows where it is");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("ono")
}

/// Starts `ono --agent --listen 127.0.0.1:0` and collects everything it says before it waits.
fn agent(home: &Scratch) -> Agent {
    let mut process = Command::new(binary())
        .args(["--agent", "--listen", "127.0.0.1:0"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the agent starts");

    let stderr = process.stderr.take().expect("stderr was piped");
    let (sender, lines) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            if sender.send(line.trim_end().to_owned()).is_err() {
                return;
            }
        }
    });

    // The summary ends with the last ceiling the agent prints; everything up to it is the block
    // an operator reads.
    let mut summary = Vec::new();
    let mut address = String::new();
    loop {
        let line = lines
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("the agent ended before it printed its startup summary");
        if let Some(bound) = line.strip_prefix("ono: listening on ") {
            address = bound.trim().to_owned();
        }
        let last = line.starts_with("ono: handshake timeout ");
        summary.push(line);
        if last {
            break;
        }
    }
    Agent {
        process,
        summary,
        address,
    }
}

fn ono(home: &Scratch, script: &str) -> ono_testkit::Run {
    Shell::new()
        .env("HOME", home.path().to_string_lossy().into_owned())
        .env(
            "XDG_CONFIG_HOME",
            home.path().to_string_lossy().into_owned(),
        )
        .args(["-c", script])
        .run()
}

/// This shell's own fingerprint — what the agent's store would have to name.
fn client_fingerprint(home: &Scratch) -> String {
    let printed = Command::new(binary())
        .arg("--print-peer-key")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .output()
        .expect("the peer key is printable");
    String::from_utf8_lossy(&printed.stdout).trim().to_owned()
}

#[test]
fn should_print_the_bind_address_the_limits_and_the_authorized_client_count_when_listening_starts()
{
    let home = scratch();
    let fingerprint = client_fingerprint(&home);
    ono(&home, &format!("add client-key {fingerprint} --label one")).assert_success();
    let agent = agent(&home);

    // §11.2, field by field.
    assert!(
        agent.address.starts_with("127.0.0.1:"),
        "the bound address is printed, and it is the address actually bound rather than the one \
         asked for — a caller that said port 0 learns which port the system chose, got {:?}",
        agent.summary
    );
    assert_eq!(
        agent.field("host key").map(str::to_owned),
        Some(
            String::from_utf8_lossy(
                &Command::new(binary())
                    .args(["--agent", "--print-host-key"])
                    .env("HOME", home.path())
                    .env("XDG_CONFIG_HOME", home.path())
                    .output()
                    .expect("the host key is printable")
                    .stdout
            )
            .trim()
            .to_owned()
        ),
        "the server peer fingerprint is printed, and it is the one a person pins the host by \
         (§8.5), got {:?}",
        agent.summary
    );
    let store = agent
        .field("authorization store")
        .expect("the authorization store path is printed");
    assert!(
        store.ends_with("authorized_clients") && store.starts_with(&*home.path().to_string_lossy()),
        "the store path is printed so an operator knows which file decides who is served (§9.2), \
         got {store:?}"
    );
    assert_eq!(
        agent.field("authorized clients"),
        Some("1"),
        "the authorized client count is printed, got {:?}",
        agent.summary
    );
    assert_eq!(
        agent.field("maximum connections"),
        Some(&*ono_protocol::MAX_CONNECTIONS.to_string()),
        "the maximum concurrent connections is printed, and it is the figure Appendix A fixes \
         and `docs/spec/hardening/limits.yaml` declares, got {:?}",
        agent.summary
    );
    assert!(
        !agent.summary.join("\n").contains("BEGIN"),
        "a summary an operator reads on a console carries public identity material and no key \
         material (§14.2, §53.3), got {:?}",
        agent.summary
    );
}

#[test]
fn should_refuse_every_connection_when_the_authorization_store_is_empty_or_absent() {
    // §11.2: "If the authorization store contains zero clients, the agent MAY listen but MUST
    // refuse all connections after cryptographic handshake. It MUST NOT infer authorization from
    // network locality." The client below dials from 127.0.0.1 and is refused, which is §11.3
    // asserted at the product: being on the loopback interface is not being authorized.
    let home = scratch();
    let agent = agent(&home);

    assert_eq!(
        agent.field("authorized clients"),
        Some("0"),
        "the count is printed even — especially — when it is zero, got {:?}",
        agent.summary
    );
    assert!(
        agent
            .summary
            .iter()
            .any(|line| line.contains("no client is authorized")),
        "an agent listening for nobody says so on the way up rather than leaving an operator to \
         discover it from a refused client, got {:?}",
        agent.summary
    );

    // The host is pinned deliberately, so the only thing left for the agent to refuse is who the
    // client is (§21.5, §9.1).
    let refused = ono(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {key}; try {{ link host {address} --transport \
             tcp }} catch e {{ $e | select code | to json }}",
            address = agent.address,
            key = agent.field("host key").expect("the fingerprint is printed"),
        ),
    );

    assert!(
        refused.stdout().contains("Ono-Sendai-E1202"),
        "every client is refused after the cryptographic handshake when nobody is authorized, \
         got {:?} / {:?}",
        refused.stdout(),
        refused.stderr()
    );
}

#[test]
fn should_bind_the_documented_default_address_when_none_is_given() {
    // §11.1 makes the socket explicit — `--listen` and nothing else opens one — and §11.2 leaves
    // the address open. The default is the loopback interface on the port every `link` command
    // already assumes, because a first `--listen` on a machine should expose the agent to the
    // machine and not to the building (§11.3 is why that is a default and never a trust decision).
    let parsed = Invocation::from_args(["--agent", "--listen"].map(String::from));
    let Invocation::Agent(_, options) = parsed else {
        panic!("`--agent --listen` starts a listening agent");
    };
    assert_eq!(
        options.listen.as_deref(),
        Some(&*format!("127.0.0.1:{}", ono_remote::DEFAULT_PORT)),
        "the documented default is the loopback address and the port `link host` assumes"
    );

    // And `--agent` alone still opens no socket at all (§11.1).
    let parsed = Invocation::from_args(["--agent"].map(String::from));
    let Invocation::Agent(_, options) = parsed else {
        panic!("`--agent` serves stdin and stdout");
    };
    assert_eq!(options.listen, None);
}
