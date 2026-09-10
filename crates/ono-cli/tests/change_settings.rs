//! §53's change settings, read from the operator's configuration (v0.6 §53, §17.2).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod change_support;

use change_support::{home, ono_at};

/// §53: a configuration nobody can read is reported rather than silently ignored, and one typo
/// does not throw away a protection requirement the operator wrote correctly beside it.
#[test]
fn should_report_an_unreadable_change_setting_and_keep_the_others() {
    let home = home();
    let config = home.path().join("config/ono");
    std::fs::create_dir_all(&config).expect("a config directory");
    std::fs::write(
        config.join("config.ono"),
        "set config change.default_protection require\nset config change.default_strategy sideways\n",
    )
    .expect("written");

    let problems = ono_at(home.path(), "get config --problems | to json");
    problems.assert_success();
    assert!(
        problems.stdout().contains("change.default_strategy"),
        "the unreadable key is reported where configuration problems are, got {:?}",
        problems.stdout()
    );

    let protection = ono_at(
        home.path(),
        "get config change.default_protection | to json",
    );
    protection.assert_success();
    assert!(
        protection.stdout().contains("require"),
        "the key written correctly still stands, got {:?}",
        protection.stdout()
    );
    let target = home.path().join("target");
    std::fs::write(&target, "x\n").expect("written");
    let plan = ono_at(
        home.path(),
        &format!("plan remove file {} | to json", target.display()),
    );
    assert!(
        plan.stdout().contains("\"protection_mode\":\"require\"")
            || plan.stderr().contains("require"),
        "and it governs the plan (§17.2), got {:?} / {:?}",
        plan.stdout(),
        plan.stderr()
    );
}

/// A process this test owns, killed at the end whatever happened.
///
/// It is started detached (`sh` exits and the `sleep` is reparented), so once `kill process`
/// ends it the system reaps it. A child of this test would stay a zombie in `/proc` until the test
/// waited for it, and a plan's `exists == false` check would rightly still see it.
struct Sleeper(u32);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = std::process::Command::new("kill")
            .args(["-9", &self.0.to_string()])
            .status();
    }
}

fn sleeper() -> Sleeper {
    let output = std::process::Command::new("sh")
        .args(["-c", "sleep 600 >/dev/null 2>&1 & echo $!"])
        .output()
        .expect("`sh` starts");
    let pid = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("`sh` printed the pid of the detached sleep");
    Sleeper(pid)
}

fn with_config(home: &std::path::Path, lines: &str) {
    let config = home.join("config/ono");
    std::fs::create_dir_all(&config).expect("a config directory");
    std::fs::write(config.join("config.ono"), lines).expect("written");
}

/// §19.4: HIGH plans need an explicit acknowledgement *or a non-interactive policy flag*, and
/// §53's `change.high_risk_requires_ack` is that flag. Irreversibility stays a gate of its own.
#[test]
fn should_waive_the_risk_acknowledgement_when_policy_says_high_risk_needs_none() {
    let home = home();
    with_config(
        home.path(),
        "set config change.high_risk_requires_ack false\n",
    );
    let process = sleeper();
    let pid = process.0;
    let planned = ono_at(home.path(), &format!("plan kill process {pid} | to json"));
    planned.assert_success();
    let id = change_support::text(&change_support::one(&planned), "id")[..8].to_owned();

    let applied = ono_at(
        home.path(),
        &format!("apply {id} --accept-irreversible --confirm"),
    );
    assert!(
        !applied.stderr().contains("accept-risk"),
        "the risk class is acknowledged by policy, got {:?}",
        applied.stderr()
    );
    applied.assert_success();
}

#[test]
fn should_still_require_the_risk_acknowledgement_by_default() {
    let home = home();
    let process = sleeper();
    let pid = process.0;
    let planned = ono_at(home.path(), &format!("plan kill process {pid} | to json"));
    planned.assert_success();
    let id = change_support::text(&change_support::one(&planned), "id")[..8].to_owned();

    let applied = ono_at(
        home.path(),
        &format!("apply {id} --accept-irreversible --confirm"),
    );
    assert!(
        !applied.status().is_success(),
        "§19.4: a HIGH plan is not applied without its acknowledgement"
    );
    assert!(
        applied.stderr().contains("accept-risk"),
        "and the refusal names the flag, got {:?}",
        applied.stderr()
    );
}
