//! Linking to a host that proves who it is, and refusing one that does not (spec §21.5, §49).
//!
//! ADR-0274 recorded why none of this could be tested at the shell: both production transports
//! ran behind ssh, whose `peer_key` is truthfully `None`, so the trust store was never consulted
//! and `remote.host_key_changed` was unreachable. ADR-0353 built the transport that closes that,
//! and these are the outcomes a person sees over it: an unknown host is refused, a pinned host
//! links, a *changed* key is `Ono-Sendai-E0603`, and the pins are ordinary objects that can be
//! read, replaced and forgotten.
//!
//! The far side is a real second `ono` process listening on the loopback interface with a key
//! the test generated, so nothing here needs a network or a fixture host.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::io::{BufRead as _, BufReader};
use std::process::{Child, Command, Stdio};

use ono_testkit::{Scratch, Shell, scratch};

/// A listening agent, and how to reach and pin it.
struct Agent {
    process: Child,
    address: String,
    fingerprint: String,
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
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

/// Starts `ono --agent --listen 127.0.0.1:0` with `key` as its identity, and reads back the port
/// the system chose and the fingerprint a peer has to pin.
fn agent(home: &Scratch, key: &str) -> Agent {
    let mut process = Command::new(binary())
        .args([
            "--agent",
            "--listen",
            "127.0.0.1:0",
            "--host-key",
            home.path().join(key).to_string_lossy().as_ref(),
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the agent starts");

    let stderr = process.stderr.take().expect("stderr was piped");
    let mut reader = BufReader::new(stderr);
    let (mut address, mut fingerprint) = (None, None);
    let mut line = String::new();
    while address.is_none() || fingerprint.is_none() {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            panic!("the agent ended before it said where it listens");
        }
        if let Some(rest) = line.trim().strip_suffix('\n').or(Some(line.trim())) {
            if let Some(bound) = rest.split("listening on ").nth(1) {
                address = Some(bound.trim().to_owned());
            } else if let Some(printed) = rest.split("host key ").nth(1) {
                fingerprint = Some(printed.trim().to_owned());
            }
        }
    }
    Agent {
        process,
        address: address.unwrap_or_default(),
        fingerprint: fingerprint.unwrap_or_default(),
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

fn last_line(run: &ono_testkit::Run) -> String {
    run.stdout()
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn should_refuse_a_host_whose_key_was_never_pinned() {
    let home = scratch();
    let agent = agent(&home, "first.pem");

    let run = ono(
        &home,
        &format!(
            "try {{ link host {} --transport tcp }} catch e {{ $e | select name | to json }}",
            agent.address
        ),
    );
    run.assert_success();

    assert!(
        last_line(&run).contains("safety.policy_denied"),
        "ADR-0015 T5: an unknown key is refused, not recorded and not prompted past, got {:?}",
        run.stdout()
    );
    let pins = ono(&home, "get host-key | to json");
    assert_eq!(
        last_line(&pins),
        "[]",
        "a refused link records nothing: the store is what a person decided, not what answered"
    );
}

#[test]
fn should_link_to_a_host_whose_key_is_pinned() {
    let home = scratch();
    let agent = agent(&home, "first.pem");

    let pinned = ono(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {} | select status changed | to json",
            agent.fingerprint
        ),
    );
    pinned.assert_success();
    assert!(
        last_line(&pinned).contains("\"status\":\"success\""),
        "got {:?}",
        pinned.stdout()
    );

    let run = ono(
        &home,
        &format!(
            "link host {} --transport tcp; get link | select transport mode state | to json",
            agent.address
        ),
    );
    run.assert_success();
    assert!(
        last_line(&run).contains("\"transport\":\"tcp\"")
            && last_line(&run).contains("\"state\":\"connected\""),
        "a pinned host links over the authenticated transport, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_refuse_a_changed_host_key_with_the_stable_safety_code() {
    let home = scratch();
    let first = agent(&home, "first.pem");

    let pinned = ono(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {} | select status | to json",
            first.fingerprint
        ),
    );
    pinned.assert_success();
    drop(first);

    // The same host, a different machine answering for it: exactly the case spec §49 and
    // ADR-0015 T6 exist for. The pin is kept under the host, not under `host:port` — a port is
    // where a host answers, not who it is — so a second identity on a second port is the same
    // contradiction as a rebuilt server on the same one, and needs no port to be re-bound.
    let impostor = agent(&home, "second.pem");
    let run = ono(
        &home,
        &format!(
            "try {{ link host {} --transport tcp }} catch e {{ $e | select code name retryable | to json }}",
            impostor.address
        ),
    );
    run.assert_success();

    let answered = last_line(&run);
    assert!(
        answered.contains("Ono-Sendai-E0603"),
        "ADR-0015 T6: a changed key is E0603, on a link a user can actually make, got {answered:?}"
    );
    assert!(
        answered.contains("remote.host_key_changed"),
        "the dotted name is user-visible too, got {answered:?}"
    );
    assert!(
        !answered.contains("\"retryable\":true"),
        "retrying would not make the key match, got {answered:?}"
    );
}

#[test]
fn should_show_replace_and_forget_a_pinned_key() {
    let home = scratch();
    let a = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    let b = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

    let added = ono(
        &home,
        &format!("add host-key prod-db --fingerprint {a} | select status | to json"),
    );
    added.assert_success();

    let listed = ono(
        &home,
        "get host-key | select host algorithm fingerprint | to json",
    );
    listed.assert_success();
    assert_eq!(
        last_line(&listed),
        format!("[{{\"host\":\"prod-db\",\"algorithm\":\"tls-x509\",\"fingerprint\":\"{a}\"}}]"),
        "a pin is an object, with the full fingerprint and how the key was proved"
    );

    // `add` will not quietly replace a different key; re-trusting is `set`, a separate act.
    let refused = ono(
        &home,
        &format!(
            "try {{ add host-key prod-db --fingerprint {b} }} catch e {{ $e | select code | to json }}"
        ),
    );
    refused.assert_success();
    assert!(
        last_line(&refused).contains("Ono-Sendai-E0603"),
        "ADR-0015 T6: replacing a pin is never something that merely happens, got {:?}",
        refused.stdout()
    );

    let replaced = ono(
        &home,
        &format!("set host-key prod-db --fingerprint {b} | select changed | to json"),
    );
    replaced.assert_success();
    assert_eq!(last_line(&replaced), "[{\"changed\":true}]");

    let after = ono(&home, "get host-key | select fingerprint | to json");
    assert_eq!(last_line(&after), format!("[{{\"fingerprint\":\"{b}\"}}]"));

    let forgotten = ono(
        &home,
        "remove host-key prod-db | select status | to json; get host-key | to json",
    );
    forgotten.assert_success();
    assert_eq!(
        last_line(&forgotten),
        "[]",
        "a forgotten host must be trusted again deliberately, got {:?}",
        forgotten.stdout()
    );
}

#[test]
fn should_keep_a_pin_in_a_file_a_person_can_read() {
    let home = scratch();
    let fingerprint = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
    ono(
        &home,
        &format!("add host-key prod-db --fingerprint {fingerprint}"),
    )
    .assert_success();

    let text = std::fs::read_to_string(home.path().join("ono").join("trusted_hosts"))
        .expect("the pins are written where the shell says they are");

    assert!(
        text.contains(&format!("prod-db tls-x509 {fingerprint}")),
        "one line per peer, `<host> <algorithm> <fingerprint>`, got {text:?}"
    );
    let shown = ono(&home, "get host-key | select path | to json");
    assert!(
        last_line(&shown).contains("trusted_hosts"),
        "the table says which file the pins are kept in, got {:?}",
        shown.stdout()
    );
}
