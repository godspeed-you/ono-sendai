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

mod support;
use support::last_line;

/// A listening agent, and how to reach and pin it.
struct Agent {
    process: Child,
    address: String,
    fingerprint: String,
    /// Everything the agent has written to stderr, which is where its audit trail goes
    /// (v0.4.1 §14.1). Collected by a reader thread so the suite never blocks on a line that
    /// has not been written yet.
    log: std::sync::Arc<std::sync::Mutex<String>>,
    /// Kept alive for the agent's lifetime: dropping it would close the channel the reader
    /// thread sends on, and the thread would stop collecting after the first two lines.
    _lines: std::sync::mpsc::Receiver<String>,
}

impl Agent {
    /// What the agent has said so far, after giving it a moment to say it.
    fn drain_stderr(&mut self) -> String {
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let seen = self.log.lock().expect("the log is not poisoned").clone();
            if seen.contains("event=connection.accepted") {
                return seen;
            }
        }
        self.log.lock().expect("the log is not poisoned").clone()
    }
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
    let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let collecting = std::sync::Arc::clone(&log);
    let (sender, lines) = std::sync::mpsc::channel::<String>();
    // The agent keeps writing to stderr for as long as it runs — its audit trail goes there
    // (§14.1) — so a reader thread owns the pipe and the suite reads what it collected.
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            collecting
                .lock()
                .expect("the log is not poisoned")
                .push_str(&line);
            if sender.send(line.trim().to_owned()).is_err() {
                return;
            }
        }
    });

    let (mut address, mut fingerprint) = (None, None);
    while address.is_none() || fingerprint.is_none() {
        let line = lines
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("the agent ended before it said where it listens");
        if let Some(bound) = line.split("listening on ").nth(1) {
            address = Some(bound.trim().to_owned());
        } else if let Some(printed) = line.split("host key ").nth(1) {
            fingerprint = Some(printed.trim().to_owned());
        }
    }
    Agent {
        process,
        address: address.unwrap_or_default(),
        fingerprint: fingerprint.unwrap_or_default(),
        log,
        _lines: lines,
    }
}

/// The fingerprint of the key *this* shell proves it holds — what the far side authorizes it by
/// (v0.4.1 §8.5, §9.7). Client and agent share a configuration directory in these tests, so the
/// client's own identity is the one the agent's store names.
fn client_fingerprint(home: &Scratch) -> String {
    let printed = Command::new(binary())
        .arg("--print-peer-key")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .output()
        .expect("the peer key is printable");
    String::from_utf8_lossy(&printed.stdout).trim().to_owned()
}

/// Authorizes this shell's own key to observe the agent it is about to link to (§9.4).
///
/// Every direct link in this suite needs one, because a v0.4.1 listening agent authorizes nobody
/// it was not told about — which is §59.1, and is the whole of phase H2.
fn authorize_self(home: &Scratch) -> String {
    let fingerprint = client_fingerprint(home);
    ono(home, &format!("add client-key {fingerprint} --label suite")).assert_success();
    fingerprint
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
    authorize_self(&home);

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

/// v0.4.1 §7.3: "the authenticated transport identity and the runtime `Identity { user, uid,
/// elevated }` MUST remain separate fields", and "the runtime identity is useful context but MUST
/// NOT grant authority". A link is where a person sees both, and seeing them apart is what stops
/// the second from being read as the first.
#[test]
fn should_show_the_proved_identity_and_the_reported_one_as_separate_fields() {
    let home = scratch();
    let agent = agent(&home, "first.pem");
    authorize_self(&home);
    ono(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {} | select status",
            agent.fingerprint
        ),
    )
    .assert_success();

    let run = ono(
        &home,
        &format!(
            "link host {} --transport tcp; get link | select transport_fingerprint \
             transport_trust runtime_user runtime_elevated | to json",
            agent.address
        ),
    );
    run.assert_success();

    let answered = last_line(&run);
    assert!(
        answered.contains(&format!(
            "\"transport_fingerprint\":\"{}\"",
            agent.fingerprint
        )),
        "the proved identity is the key this handshake verified, got {answered:?}"
    );
    assert!(
        answered.contains("\"transport_trust\":\"pinned\""),
        "§7.3's `transport_trust` says how that key was decided about, got {answered:?}"
    );
    assert!(
        answered.contains("\"runtime_user\":\"") && answered.contains("\"runtime_elevated\":"),
        "the reported identity is beside it, in its own fields and under its own names, got \
         {answered:?}"
    );
}

/// The same rule from the other side: a transport that authenticated nobody says so.
/// §4.3 spells out what an ssh-carried agent must report — "peer key visible to ono: no" — and
/// `local` is the same shape, a child of this very process. The runtime identity is still there
/// and still grants nothing, which is the whole distinction in one row.
#[test]
fn should_report_no_proved_key_over_a_transport_that_proves_nothing() {
    let home = scratch();

    let run = ono(
        &home,
        "link host here --transport local; get link | select transport_fingerprint \
         transport_trust runtime_user | to json",
    );
    run.assert_success();

    let answered = last_line(&run);
    assert!(
        answered.contains("\"transport_fingerprint\":null"),
        "§2.6: unknown remains unknown — a transport that cannot report a peer key reports none, \
         rather than borrowing OpenSSH's verification or the child's parentage, got {answered:?}"
    );
    assert!(
        answered.contains("\"transport_trust\":\"unauthenticated\""),
        "§7.4 asks that an unauthenticated transport be named `unauthenticated` where it is \
         described, got {answered:?}"
    );
    assert!(
        answered.contains("\"runtime_user\":\""),
        "the far side still reports who it runs as; it just does not prove it, got {answered:?}"
    );
}

// --- phase H2: authentication is not authorization (v0.4.1 §9, §10, §14.3, §59.1–§59.3) -------
//
// ADR-0437 wrote down what H1 deliberately left open: "a listening agent today authenticates
// every client and authorizes all of them". These are the cases at the product — a second `ono`
// really listening on the loopback interface, and a first one that has to be let in.

/// v0.4.1 §59.1: "given an agent with one authorized client key A, when client B connects with a
/// valid but unknown certificate, TLS/key proof may complete but Ono authorization MUST refuse
/// the session before provider negotiation."
#[test]
fn should_refuse_an_authenticated_client_the_agent_never_authorized() {
    let home = scratch();
    let agent = agent(&home, "first.pem");
    ono(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {} | select status",
            agent.fingerprint
        ),
    )
    .assert_success();
    // The client's key is real and the handshake completes. Nobody authorized it.

    let run = ono(
        &home,
        &format!(
            "try {{ link host {} --transport tcp }} catch e {{ $e | select code name retryable | to json }}",
            agent.address
        ),
    );
    run.assert_success();

    let answered = last_line(&run);
    assert!(
        answered.contains("Ono-Sendai-E1202") && answered.contains("remote.unauthorized"),
        "§9.1: holding a private key proves who connected, never that the operator wants them \
         here, got {answered:?}"
    );
    assert!(
        !answered.contains("\"retryable\":true"),
        "§59.9: the refusal is deterministic, so retrying is not the remedy, got {answered:?}"
    );

    // §59.1: "no process list, schema list or capability inventory beyond minimal rejection
    // protocol data may be disclosed."
    let whole = ono(
        &home,
        &format!(
            "try {{ link host {} --transport tcp }} catch e {{ $e | to json }}",
            agent.address
        ),
    );
    for withheld in ["linux.procfs", "process.list", "ono.process/1", "systemd"] {
        assert!(
            !whole.stdout().contains(withheld),
            "the refused client learned `{withheld}` from the refusal: {:?}",
            whole.stdout()
        );
    }
    assert_eq!(
        last_line(&ono(&home, "get link | to json")),
        "[]",
        "a refused client holds no link"
    );
}

/// §59.2: an authorized observer reads, and is refused an action; §9.4's default grant is
/// exactly that and no more.
#[test]
fn should_let_an_authorized_observer_read_and_refuse_it_every_action() {
    let home = scratch();
    let agent = agent(&home, "first.pem");
    ono(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {} | select status",
            agent.fingerprint
        ),
    )
    .assert_success();
    authorize_self(&home);

    let read = ono(
        &home,
        &format!(
            "link host {} --transport tcp; enter link {}; get process | count | to json",
            agent.address, agent.address
        ),
    );
    read.assert_success();
    assert_ne!(
        last_line(&read),
        "[0]",
        "§59.2: an authorized observer executes representative read operations, got {:?}",
        read.stdout()
    );

    // §9.4: "no `Act` request, no elevated action, no destructive action." The offer the client
    // negotiated carries no action capability at all, so the mutation has nothing to bind to and
    // the dispatch path refuses it in any case (§10.1, §10.2).
    let refused = ono(
        &home,
        &format!(
            "link host {} --transport tcp; enter link {}; \
             try {{ stop process 1 --signal TERM }} catch e {{ $e | select kind | to json }}",
            agent.address, agent.address
        ),
    );
    refused.assert_success();
    assert!(
        !last_line(&refused).contains("\"status\":\"success\""),
        "§59.2: an `Act` request from an observer must not succeed, got {:?}",
        refused.stdout()
    );
}

/// §9.7 and §12.5: revoking a client is a deliberate act, and the next connection feels it.
#[test]
fn should_refuse_the_next_connection_from_a_revoked_client_key() {
    let home = scratch();
    let agent = agent(&home, "first.pem");
    ono(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {} | select status",
            agent.fingerprint
        ),
    )
    .assert_success();
    let client = authorize_self(&home);

    let linked = ono(
        &home,
        &format!(
            "link host {} --transport tcp; get link | select state | to json",
            agent.address
        ),
    );
    assert_eq!(last_line(&linked), "[{\"state\":\"connected\"}]");

    ono(
        &home,
        &format!("remove client-key {client} | select changed | to json"),
    )
    .assert_success();

    let refused = ono(
        &home,
        &format!(
            "try {{ link host {} --transport tcp }} catch e {{ $e | select code | to json }}",
            agent.address
        ),
    );
    refused.assert_success();
    assert!(
        last_line(&refused).contains("Ono-Sendai-E1202"),
        "§10.3: authorization changes reach the next connection, got {:?}",
        refused.stdout()
    );
}

/// §14.3 and §19.1: "the words `authenticated`, `authorized`, `pinned` and `self-reported
/// identity` MUST not be conflated." Four concepts, four fields, four separate values.
#[test]
fn should_distinguish_authenticated_authorized_pinned_and_self_reported_on_a_link() {
    let home = scratch();
    let agent = agent(&home, "first.pem");
    ono(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {} | select status",
            agent.fingerprint
        ),
    )
    .assert_success();
    authorize_self(&home);

    let run = ono(
        &home,
        &format!(
            "link host {} --transport tcp; get link | select authenticated authorized \
             transport_trust transport_fingerprint runtime_user runtime_elevated | to json",
            agent.address
        ),
    );
    run.assert_success();

    let answered = last_line(&run);
    assert!(
        answered.contains("\"authenticated\":true"),
        "§19.1 `authenticated`: cryptographic peer proof was verified, got {answered:?}"
    );
    assert!(
        answered.contains("\"authorized\":true"),
        "§19.1 `authorized`: the authenticated principal is permitted by policy — a different \
         fact, in a different field, got {answered:?}"
    );
    assert!(
        answered.contains("\"transport_trust\":\"pinned\""),
        "§19.1 `pinned`: the fingerprint matches a recorded trust decision, got {answered:?}"
    );
    assert!(
        answered.contains(&format!(
            "\"transport_fingerprint\":\"{}\"",
            agent.fingerprint
        )),
        "the key the proof is about is shown in full (§53.3), got {answered:?}"
    );
    assert!(
        answered.contains("\"runtime_user\":\"") && answered.contains("\"runtime_elevated\":"),
        "§19.1 self-reported identity: what the peer said, beside what it proved and never \
         merged into it, got {answered:?}"
    );
}

/// The other half of the same distinction: a link that is authenticated and *not* authorized is
/// reported as exactly that, and never as a transport failure.
#[test]
fn should_report_an_authenticated_but_unauthorized_link_as_exactly_that() {
    let home = scratch();
    let agent = agent(&home, "first.pem");
    ono(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {} | select status",
            agent.fingerprint
        ),
    )
    .assert_success();

    let refused = ono(
        &home,
        &format!(
            "try {{ link host {} --transport tcp }} catch e {{ $e | select code name kind | to json }}",
            agent.address
        ),
    );
    refused.assert_success();
    let answered = last_line(&refused);
    assert!(
        answered.contains("remote.unauthorized"),
        "the refusal names authorization, so a reader is not left guessing whether the key was \
         wrong, got {answered:?}"
    );
    assert!(
        answered.contains("\"kind\":\"safety\""),
        "§53.2: the kind is what an internal caller branches on, and this is a policy decision \
         rather than a transport failure, got {answered:?}"
    );
    assert!(
        !answered.contains("remote.host_key_changed")
            && !answered.contains("remote.unreachable")
            && !answered.contains("remote.peer_unauthenticated"),
        "an unauthorized client is not an unauthenticated one and not an unreachable host, got \
         {answered:?}"
    );

    // The host key really was pinned and the handshake really did complete: this is a client the
    // agent authenticated and then declined to authorize, which is the state §14.3 is about.
    assert_eq!(
        last_line(&ono(&home, "get host-key | select fingerprint | to json")),
        format!("[{{\"fingerprint\":\"{}\"}}]", agent.fingerprint)
    );
}

/// §14.1 and §14.2 at the product: the listening agent writes structured events an operator can
/// read, and never a key or a payload.
#[test]
fn should_write_a_structured_audit_line_for_every_decision_the_agent_makes() {
    let home = scratch();
    let mut agent = agent(&home, "first.pem");
    ono(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {} | select status",
            agent.fingerprint
        ),
    )
    .assert_success();

    // One refused client, then one authorized client.
    ono(
        &home,
        &format!(
            "try {{ link host {} --transport tcp }} catch e {{ }}",
            agent.address
        ),
    );
    let client = authorize_self(&home);
    let linked = ono(
        &home,
        &format!(
            "link host {} --transport tcp; get process | count",
            agent.address
        ),
    );
    assert!(
        linked.status().is_success(),
        "the authorized client links: {:?} {:?}",
        linked.stdout(),
        linked.stderr()
    );

    let log = agent.drain_stderr();
    assert!(
        log.contains("event=connection.unknown_client_refused")
            && log.contains("error_code=remote.unauthorized"),
        "§14.1: an unknown/unapproved client refusal is recorded with its stable code, got {log}"
    );
    assert!(
        log.contains("event=connection.accepted"),
        "§14.1: a successful authenticated connection is recorded, got {log}; the link answered \
         {:?}",
        linked.stdout()
    );
    assert!(
        log.contains(&client),
        "§14.2: the peer fingerprint travels with the event, and §53.3 lets it be shown in full"
    );
    assert!(
        log.contains("source_address=127.0.0.1:"),
        "§14.2 names `source_address` among the fields, got {log}"
    );
    for secret in ["BEGIN PRIVATE KEY", "PRIVATE KEY", "-----BEGIN"] {
        assert!(
            !log.contains(secret),
            "§14.2: an audit event carries no key material, and `{secret}` appeared in {log}"
        );
    }
}
