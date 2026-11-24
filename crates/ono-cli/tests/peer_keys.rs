//! This shell's own identities, and the fact that two of them can disagree (issue #201;
//! v0.4.1 §7.2, §8.1, §8.2, §8.5; ADR-0435).
//!
//! §8.2 keeps the legacy `host_key.pem` beside `link_identity.pem` and forbids the two from
//! diverging *silently* for one process role. ADR-0435 met that by giving each role one file at a
//! time — but `ono --agent --host-key <path>` presents whatever file it is given, so a machine can
//! run a listening agent on one identity and link out with another, and before v0.6.6 nothing said
//! so: the operator had to open both files and compare them by hand.
//!
//! Two surfaces close it. `get peer-key` lists the identities in the configuration directory by
//! fingerprint, each saying whether it is the link identity; a listening agent started with
//! `--host-key` names the link identity beside the key it presents when the two differ. Neither
//! ever prints key material: the fingerprint is the public contract of §7.2.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0913).
#![cfg(feature = "remote")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::io::{BufRead as _, BufReader};
use std::process::{Command, Stdio};

use ono_testkit::{Scratch, scratch};

mod support;
use support::{binary, last_line, ono_at_home};

/// `ono <arguments>` with `home` as its whole configuration.
fn ono(home: &Scratch, arguments: &[&str]) -> ono_testkit::Run {
    ono_testkit::Shell::new()
        .env("HOME", home.path().to_string_lossy().into_owned())
        .env(
            "XDG_CONFIG_HOME",
            home.path().to_string_lossy().into_owned(),
        )
        .args(arguments.iter().copied())
        .run()
}

/// The fingerprint `ono --print-peer-key` prints, generating the link identity if need be.
fn link_fingerprint(home: &Scratch) -> String {
    let printed = ono(home, &["--print-peer-key"]);
    printed.assert_success();
    printed.stdout().trim().to_owned()
}

/// Generates an identity at `path` and returns its fingerprint, the way an operator who points
/// `--agent --host-key` at a new file gets one.
fn host_key_fingerprint(home: &Scratch, path: &std::path::Path) -> String {
    let printed = ono(
        home,
        &[
            "--agent",
            "--host-key",
            &path.to_string_lossy(),
            "--print-host-key",
        ],
    );
    printed.assert_success();
    printed.stdout().trim().to_owned()
}

/// Every line of PEM armour or key material that must never reach an output.
fn assert_no_key_material(home: &Scratch, said: &str) {
    assert!(
        !said.contains("PRIVATE KEY") && !said.contains("BEGIN"),
        "§7.2: the fingerprint is the public contract, and nothing else of the file is printed, \
         got:\n{said}"
    );
    for entry in std::fs::read_dir(home.path().join("ono")).expect("the directory exists") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|extension| extension == "pem") {
            let pem = std::fs::read_to_string(&path).expect("the identity is readable");
            for line in pem.lines().filter(|line| line.len() > 20) {
                assert!(
                    !said.contains(line),
                    "a line of {} reached the output:\n{said}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn should_list_the_link_identity_by_its_fingerprint() {
    let home = scratch();
    let fingerprint = link_fingerprint(&home);

    let listed = ono_at_home(
        &home,
        "get peer-key | select role fingerprint same_as_link_identity | to json",
    );
    listed.assert_success();

    assert_eq!(
        last_line(&listed),
        format!(
            "[{{\"role\":\"link_identity\",\"fingerprint\":\"{fingerprint}\",\
             \"same_as_link_identity\":true}}]"
        ),
        "the identity a direct link presents is listed by the fingerprint `--print-peer-key` \
         prints, got {:?}",
        listed.stdout()
    );
    assert_no_key_material(&home, &format!("{}{}", listed.stdout(), listed.stderr()));
}

#[test]
fn should_show_a_legacy_host_key_that_differs_from_the_link_identity() {
    let home = scratch();
    let link = link_fingerprint(&home);
    let legacy = host_key_fingerprint(&home, &home.path().join("ono").join("host_key.pem"));
    assert_ne!(link, legacy, "the fixture is a diverging pair");

    let listed = ono_at_home(
        &home,
        "get peer-key | select role fingerprint same_as_link_identity | to json",
    );
    listed.assert_success();

    assert_eq!(
        last_line(&listed),
        format!(
            "[{{\"role\":\"link_identity\",\"fingerprint\":\"{link}\",\
             \"same_as_link_identity\":true}},\
             {{\"role\":\"legacy_host_key\",\"fingerprint\":\"{legacy}\",\
             \"same_as_link_identity\":false}}]"
        ),
        "the diverging pair is visible without opening either file, got {:?}",
        listed.stdout()
    );
    assert_no_key_material(&home, &format!("{}{}", listed.stdout(), listed.stderr()));
}

#[test]
fn should_create_no_identity_when_asked_to_list_them() {
    let home = scratch();

    let listed = ono_at_home(&home, "get peer-key | count | to json");
    listed.assert_success();

    assert_eq!(
        last_line(&listed),
        "[0]",
        "a shell that never linked has no identity to list, got {:?}",
        listed.stdout()
    );
    assert!(
        !home.path().join("ono").join("link_identity.pem").exists(),
        "`get` observes: listing identities never generates one"
    );
}

/// Starts `ono --agent --host-key <path> --listen 127.0.0.1:0` and returns its startup summary.
fn listening_summary(home: &Scratch, host_key: &std::path::Path) -> Vec<String> {
    let mut process: ono_testkit::OwnedChild = Command::new(binary())
        .args(["--agent", "--host-key"])
        .arg(host_key)
        .args(["--listen", "127.0.0.1:0"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the agent starts")
        .into();
    let mut reader = BufReader::new(process.stderr.take().expect("stderr was piped"));
    let mut summary = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let trimmed = line.trim_end().to_owned();
        let last = trimmed.starts_with("ono: handshake timeout ");
        summary.push(trimmed);
        if last {
            break;
        }
    }
    let _ = process.kill();
    let _ = process.wait();
    summary
}

#[test]
fn should_name_the_link_identity_when_a_listening_agent_presents_a_different_key() {
    let home = scratch();
    let link = link_fingerprint(&home);
    let elsewhere = home.path().join("agent.pem");
    let presented = host_key_fingerprint(&home, &elsewhere);

    let summary = listening_summary(&home, &elsewhere);
    let said = summary.join("\n");

    assert!(
        said.contains(&format!("ono: host key {presented}")),
        "the key the agent presents is printed, as before, got:\n{said}"
    );
    let line = summary
        .iter()
        .find(|line| line.starts_with("ono: link identity "))
        .unwrap_or_else(|| {
            panic!("an agent presenting a key other than the link identity says so, got:\n{said}")
        });
    assert!(
        line.contains(&link) && line.contains("differs"),
        "the line names the link identity's fingerprint and that the two differ, got {line:?}"
    );
    assert_no_key_material(&home, &said);
}

#[test]
fn should_say_nothing_more_when_the_host_key_file_is_the_link_identity() {
    let home = scratch();
    link_fingerprint(&home);
    let canonical = home.path().join("ono").join("link_identity.pem");

    let summary = listening_summary(&home, &canonical);

    assert!(
        !summary
            .iter()
            .any(|line| line.starts_with("ono: link identity ")),
        "no divergence, no extra line, got:\n{}",
        summary.join("\n")
    );
}

#[test]
fn should_report_an_exposed_identity_beside_the_others_rather_than_read_it() {
    use std::os::unix::fs::PermissionsExt as _;

    let home = scratch();
    link_fingerprint(&home);
    host_key_fingerprint(&home, &home.path().join("ono").join("host_key.pem"));
    let link = home.path().join("ono").join("link_identity.pem");
    std::fs::set_permissions(&link, std::fs::Permissions::from_mode(0o644))
        .expect("the link identity is made world-readable");

    let listed = ono_at_home(
        &home,
        "get peer-key | select role same_as_link_identity | to json",
    );

    assert!(
        listed.stderr().contains("Ono-Sendai-E0604"),
        "§8.3: an identity anyone can read is refused by name, never read and never silently \
         absent, got {:?}",
        listed.stderr()
    );
    assert_eq!(
        last_line(&listed),
        "[{\"role\":\"legacy_host_key\",\"same_as_link_identity\":null}]",
        "the other identity is still listed, and with nothing readable to compare it with the \
         comparison is unknown, got {:?}",
        listed.stdout()
    );
    assert_no_key_material(&home, &format!("{}{}", listed.stdout(), listed.stderr()));
}
