//! Managing the clients this machine authorizes, through the ordinary verbs (v0.4.1 §9.4, §9.7).
//!
//! §9.7 fixes the four spellings — `get`, `add`, `set` and `remove client-key` — and §9.4 fixes
//! what the second of them grants: "adding a client without further options MUST grant
//! observe-only access". There is no option on `add` that grants an action, because an action is
//! granted by naming it, and naming it is `set --allow`.
//!
//! The user-facing concept stays "authorized client key" rather than a vague ACL blob (§9.7), so
//! every answer here is an object with named fields, and the whole surface is declared in
//! `docs/spec/commands/remote.yaml` — which is what gives it help and completion for free.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::{Scratch, scratch};

mod support;
use support::{binary, last_line, ono_at_home};

fn fingerprint(marker: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat_n(marker, 64).collect::<String>()
    )
}

#[test]
fn should_list_every_authorized_client_as_an_object_when_get_client_key_runs() {
    let home = scratch();
    let first = fingerprint('1');
    let second = fingerprint('2');
    ono_at_home(&home, &format!("add client-key {first} --label watcher")).assert_success();
    ono_at_home(&home, &format!("add client-key {second}")).assert_success();
    ono_at_home(
        &home,
        &format!("set client-key {second} --allow service.manage"),
    )
    .assert_success();

    let listed = ono_at_home(
        &home,
        "get client-key | select fingerprint label observe actions | to json",
    );
    listed.assert_success();
    assert_eq!(
        last_line(&listed),
        format!(
            "[{{\"fingerprint\":\"{first}\",\"label\":\"watcher\",\"observe\":true,\
             \"actions\":[]}},{{\"fingerprint\":\"{second}\",\"label\":null,\"observe\":true,\
             \"actions\":[\"service.manage\"]}}]"
        ),
        "§9.7: the listing shows fingerprint, label, observe permission and allowed action ids, \
         got {:?}",
        listed.stdout()
    );

    // §9.7's fifth column: where the store lives, so an operator can find the file.
    let path = ono_at_home(&home, "get client-key | select path | to json");
    assert!(
        last_line(&path).contains("authorized_clients"),
        "the listing says which file the grants are kept in, got {:?}",
        path.stdout()
    );
}

#[test]
fn should_add_a_client_key_and_show_it_in_the_next_listing() {
    let home = scratch();
    let client = fingerprint('3');

    let added = ono_at_home(
        &home,
        &format!("add client-key {client} --label deploy | select status changed | to json"),
    );
    added.assert_success();
    assert_eq!(
        last_line(&added),
        "[{\"status\":\"success\",\"changed\":true}]",
        "a grant is an action with an outcome, like every other mutation, got {:?}",
        added.stdout()
    );

    let listed = ono_at_home(&home, "get client-key | select fingerprint label | to json");
    assert_eq!(
        last_line(&listed),
        format!("[{{\"fingerprint\":\"{client}\",\"label\":\"deploy\"}}]")
    );

    // Adding twice is not a silent reset of a grant somebody made deliberately.
    let again = ono_at_home(
        &home,
        &format!("try {{ add client-key {client} }} catch e {{ $e | select code | to json }}"),
    );
    again.assert_success();
    assert!(
        last_line(&again).contains("Ono-Sendai-E0303"),
        "re-adding an authorized client refuses rather than narrowing it back to observe-only, \
         got {:?}",
        again.stdout()
    );
}

#[test]
fn should_grant_observe_only_when_a_client_key_is_added_without_grants() {
    let home = scratch();
    let client = fingerprint('4');
    ono_at_home(&home, &format!("add client-key {client}")).assert_success();

    let listed = ono_at_home(&home, "get client-key | select observe actions | to json");
    listed.assert_success();
    assert_eq!(
        last_line(&listed),
        "[{\"observe\":true,\"actions\":[]}]",
        "§9.4: adding a client grants observation and nothing else — no `Act`, no elevated \
         action, no destructive action, got {:?}",
        listed.stdout()
    );

    // And the file says the same thing, so an operator reading it sees the same grant the table
    // showed rather than a default that only exists inside the process.
    let text = std::fs::read_to_string(home.path().join("ono").join("authorized_clients"))
        .expect("the store was written");
    assert!(
        text.contains(&format!("{client} observe=true\n")),
        "the default grant is written out, not implied, got {text:?}"
    );
    let entry = text
        .lines()
        .find(|line| line.starts_with("sha256:"))
        .expect("the entry is written");
    assert!(
        !entry.contains("actions="),
        "an observer's line names no action at all, got {entry:?}"
    );
}

#[test]
fn should_change_exactly_the_grants_named_when_set_client_key_runs() {
    let home = scratch();
    let client = fingerprint('5');
    ono_at_home(&home, &format!("add client-key {client} --label bot")).assert_success();

    // §9.7: `--allow` "replaces/sets the exact action allowlist while preserving observe state".
    let allowed = ono_at_home(
        &home,
        &format!(
            "set client-key {client} --allow \"process.signal,service.manage\" | select changed | to json"
        ),
    );
    allowed.assert_success();
    assert_eq!(last_line(&allowed), "[{\"changed\":true}]");
    let after = ono_at_home(
        &home,
        "get client-key | select observe actions label | to json",
    );
    assert_eq!(
        last_line(&after),
        "[{\"observe\":true,\"actions\":[\"process.signal\",\"service.manage\"],\"label\":\"bot\"}]",
        "the allowlist is replaced and the observe state and label are left alone, got {:?}",
        after.stdout()
    );

    // §9.7: `--observe true|false` "changes query/subscription permission", and nothing else.
    let closed = ono_at_home(
        &home,
        &format!("set client-key {client} --observe false | select changed | to json"),
    );
    closed.assert_success();
    let narrowed = ono_at_home(&home, "get client-key | select observe actions | to json");
    assert_eq!(
        last_line(&narrowed),
        "[{\"observe\":false,\"actions\":[\"process.signal\",\"service.manage\"]}]",
        "changing observation leaves the action allowlist where it was, got {:?}",
        narrowed.stdout()
    );

    // Replacing the allowlist with one id removes the other: it is a set, not an addition.
    ono_at_home(
        &home,
        &format!("set client-key {client} --allow service.manage"),
    )
    .assert_success();
    let replaced = ono_at_home(&home, "get client-key | select actions | to json");
    assert_eq!(
        last_line(&replaced),
        "[{\"actions\":[\"service.manage\"]}]",
        "`--allow` replaces the allowlist, so a grant is never widened by a command that reads \
         like a narrowing, got {:?}",
        replaced.stdout()
    );
}

#[test]
fn should_remove_a_client_key_so_the_store_no_longer_lists_it() {
    let home = scratch();
    let client = fingerprint('6');
    ono_at_home(&home, &format!("add client-key {client}")).assert_success();

    let removed = ono_at_home(
        &home,
        &format!("remove client-key {client} | select status | to json"),
    );
    removed.assert_success();
    assert_eq!(last_line(&removed), "[{\"status\":\"success\"}]");

    let listed = ono_at_home(&home, "get client-key | to json");
    assert_eq!(
        last_line(&listed),
        "[]",
        "a revoked client is gone from the store, and its next connection has nothing to match"
    );

    let missing = ono_at_home(
        &home,
        &format!("try {{ remove client-key {client} }} catch e {{ $e | select kind | to json }}"),
    );
    assert!(
        last_line(&missing).contains("resolution"),
        "revoking a client that is not authorized says so rather than succeeding quietly, got \
         {:?}",
        missing.stdout()
    );
}

#[test]
fn should_act_on_the_client_keys_that_arrive_through_the_pipe() {
    // The piped form every mutation has (ADR-0118): `get client-key | remove client-key` acts on
    // each record, so revoking everything is one line rather than a loop an operator writes.
    let home = scratch();
    for marker in ['7', '8'] {
        ono_at_home(&home, &format!("add client-key {}", fingerprint(marker))).assert_success();
    }

    let removed = ono_at_home(
        &home,
        "get client-key | remove client-key | select status | to json",
    );
    removed.assert_success();
    assert_eq!(
        last_line(&removed),
        "[{\"status\":\"success\"},{\"status\":\"success\"}]"
    );
    assert_eq!(
        last_line(&ono_at_home(&home, "get client-key | to json")),
        "[]"
    );
}

#[test]
fn should_carry_help_and_completion_for_every_client_key_command() {
    // §50 of the base spec: a capability is not delivered until help is complete and completion
    // metadata exists. The contracts in `docs/spec/commands/remote.yaml` are what supply both,
    // so this is the check that the commands really are registry commands.
    let home = scratch();
    for spelling in [
        "get client-key",
        "add client-key",
        "set client-key",
        "remove client-key",
    ] {
        let helped = ono_at_home(&home, &format!("help {spelling}"));
        helped.assert_success();
        assert!(
            helped.stdout().contains("client"),
            "`help {spelling}` says nothing about client keys: {:?}",
            helped.stdout()
        );
    }

    let options = ono_at_home(&home, "help set client-key");
    for option in ["--allow", "--observe", "--label"] {
        assert!(
            options.stdout().contains(option),
            "§9.7 names `{option}` on `set client-key`, and help does not mention it: {:?}",
            options.stdout()
        );
    }

    let listed = ono_at_home(
        &home,
        "get command | where target == \"client-key\" | count | to json",
    );
    listed.assert_success();
    assert_eq!(
        last_line(&listed),
        "[4]",
        "the four commands of §9.7 are in the registry, so completion and `get command` find \
         them, got {:?}",
        listed.stdout()
    );
}

// --- what removing a client key does, and what it says it does (§12.5) ------------------------

/// A listening agent for the one command whose effect reaches past the store (v0.4.1 §12.5).
struct Agent {
    process: std::process::Child,
    address: String,
    fingerprint: String,
}

impl Drop for Agent {
    /// Killed and reaped by the test that started it: a leaked agent outlives the suite.
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Starts `ono --agent --listen 127.0.0.1:0` and reads back the port and the fingerprint to pin.
fn agent(home: &Scratch) -> Agent {
    use std::io::{BufRead as _, BufReader};

    let mut process = std::process::Command::new(binary())
        .args(["--agent", "--listen", "127.0.0.1:0"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
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
    let (mut address, mut fingerprint) = (None, None);
    while address.is_none() || fingerprint.is_none() {
        let line = lines
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("the agent ended before it said where it listens");
        if let Some(bound) = line.strip_prefix("ono: listening on ") {
            address = Some(bound.trim().to_owned());
        } else if let Some(printed) = line.strip_prefix("ono: host key ") {
            fingerprint = Some(printed.trim().to_owned());
        }
    }
    Agent {
        process,
        address: address.unwrap_or_default(),
        fingerprint: fingerprint.unwrap_or_default(),
    }
}

#[test]
fn should_refuse_the_next_connection_after_a_client_key_is_removed() {
    // §12.5: "Removing an authorized client MUST prevent all new connections immediately." And
    // the sentence after it is about this command's *words*: whichever behaviour is chosen for a
    // session already running, `remove client-key` has to say which one it is, because that is
    // where an operator forms the expectation.
    let home = scratch();
    let agent = agent(&home);
    ono_at_home(
        &home,
        &format!(
            "add host-key 127.0.0.1 --fingerprint {} | select status",
            agent.fingerprint
        ),
    )
    .assert_success();
    let client = String::from_utf8_lossy(
        &std::process::Command::new(binary())
            .arg("--print-peer-key")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path())
            .output()
            .expect("the peer key is printable")
            .stdout,
    )
    .trim()
    .to_owned();
    ono_at_home(&home, &format!("add client-key {client} --label suite")).assert_success();

    let linked = ono_at_home(
        &home,
        &format!(
            "link host {} --transport tcp; get link | select state | to json",
            agent.address
        ),
    );
    assert_eq!(
        last_line(&linked),
        "[{\"state\":\"connected\"}]",
        "the authorized client links before anything is revoked, got {:?} / {:?}",
        linked.stdout(),
        linked.stderr()
    );

    let removed = ono_at_home(
        &home,
        &format!("remove client-key {client} | select message | to json"),
    );
    removed.assert_success();
    let said = removed.stdout().to_lowercase();
    assert!(
        said.contains("next connection"),
        "§12.5: the command says that new connections are refused, got {:?}",
        removed.stdout()
    );
    assert!(
        said.contains("closed") || said.contains("close"),
        "§12.5: whichever behaviour is chosen for a session already running, the command says \
         which one it is rather than leaving an operator to find out, got {:?}",
        removed.stdout()
    );

    let refused = ono_at_home(
        &home,
        &format!(
            "try {{ link host {} --transport tcp }} catch e {{ $e | select code name | to json }}",
            agent.address
        ),
    );
    refused.assert_success();
    assert!(
        last_line(&refused).contains("Ono-Sendai-E1202"),
        "§12.5: removing a client key prevents all new connections immediately, got {:?}",
        refused.stdout()
    );
}
