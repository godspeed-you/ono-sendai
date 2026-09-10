//! A plan is about the host it freezes on (spec v0.6 §29.1, §7.1, ADR-0848).
//!
//! §29.1 makes truth per host, and §7.1 has a target on another host record that host. Inside
//! `enter link` a plan resolves its objects through the link's providers and records the link as
//! their host; it runs only where its host is, and a file — which this build resolves against this
//! machine's filesystem and mount table — is refused rather than frozen on the wrong machine.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{Sleeper, home, one, ono_at, text};

/// The last JSON array `stdout` printed; `link host` reports the link it made on the same output.
fn last_json(stdout: &str) -> Vec<serde_yaml_ng::Value> {
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.starts_with('['))
        .unwrap_or_else(|| panic!("a JSON array was printed: {stdout:?}"));
    serde_yaml_ng::from_str(line).expect("the JSON parses")
}

/// `plan kill process <pid> | to json` inside `enter link l1`: the plan's record.
fn remote_kill_plan(home: &std::path::Path, pid: u32) -> serde_yaml_ng::Value {
    let run = ono_at(
        home,
        &format!(
            "link host l1 --transport local; enter link l1; plan kill process {pid} | to json"
        ),
    );
    run.assert_success();
    last_json(run.stdout())
        .into_iter()
        .next()
        .expect("one plan")
}

/// A `sleep` nobody in this process waits for: its shell exits at once, so once it is killed the
/// system reaps it and `/proc/<pid>` goes, as a daemon's process would.
fn detached_sleep() -> u32 {
    let output = std::process::Command::new("sh")
        .args(["-c", "sleep 300 >/dev/null 2>&1 & echo $!"])
        .output()
        .expect("sh starts");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("sh prints the pid it started")
}

/// Whether `pid` is still a process, waiting up to two seconds for it to be reaped.
fn still_running(pid: u32) -> bool {
    for _ in 0..40 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    true
}

/// Kills `pid` where a test that expected it gone found it still running.
fn clean_up(pid: u32) {
    if still_running(pid) {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
    }
}

/// `ono -c script`, run with `home` as its working directory, so the agent a local link starts
/// works there too and can be told apart from every other test's.
fn session_in(home: &std::path::Path, script: &str) -> (bool, String, String) {
    use std::io::Read;
    let root = home.to_string_lossy().into_owned();
    let mut session = std::process::Command::new(env!("CARGO_BIN_EXE_ono"))
        .current_dir(home)
        .env("NO_COLOR", "1")
        .env("HOME", &root)
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .args(["-c", script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("ono starts");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let status = loop {
        if let Some(status) = session.try_wait().expect("the session can be polled") {
            break status;
        }
        if std::time::Instant::now() > deadline {
            let _ = session.kill();
            panic!("the session did not finish: {script}");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let mut out = String::new();
    let mut errors = String::new();
    session
        .stdout
        .take()
        .expect("stdout is piped")
        .read_to_string(&mut out)
        .expect("stdout reads");
    session
        .stderr
        .take()
        .expect("stderr is piped")
        .read_to_string(&mut errors)
        .expect("stderr reads");
    (status.success(), out, errors)
}

#[test]
fn should_freeze_a_target_inside_a_link_as_the_linked_hosts() {
    let home = home();
    let sleeper = Sleeper::start();

    let plan = remote_kill_plan(home.path(), sleeper.pid());

    let target = &plan["targets"][0];
    assert_eq!(
        target["host"].as_str(),
        Some("l1"),
        "§7.1: a target on another host records that host. Got {target:?}"
    );
    assert!(
        target["identity"]
            .as_str()
            .is_some_and(|identity| identity.contains(&sleeper.pid().to_string())),
        "§7.1: the target is the process the link's provider answered for. Got {target:?}"
    );
}

#[test]
fn should_name_the_host_in_the_protection_of_a_plan_inside_a_link() {
    let home = home();
    let sleeper = Sleeper::start();

    let plan = remote_kill_plan(home.path(), sleeper.pid());

    let named = plan["coverage_exclusions"]
        .as_sequence()
        .is_some_and(|exclusions| {
            exclusions
                .iter()
                .any(|exclusion| exclusion["subject"].as_str() == Some("l1"))
        });
    assert!(
        named,
        "§29.2: protection is per host, and the matrix says which host it is about. Got {:?}",
        plan["coverage_exclusions"]
    );
}

#[test]
fn should_apply_a_plan_inside_the_link_its_host_is_reached_over() {
    let home = home();
    let pid = detached_sleep();

    // SIGKILL is irreversible and the plan is HIGH risk, so a script states both (§19.4, §40.3);
    // the host check comes before either gate.
    let run = ono_at(
        home.path(),
        &format!(
            "link host l1 --transport local; enter link l1; \
             plan kill process {pid} | apply --accept-risk --accept-irreversible --confirm"
        ),
    );

    let running = still_running(pid);
    clean_up(pid);
    run.assert_success();
    assert!(
        !running,
        "§29.1: the action ran on the host the plan was frozen on"
    );
}

#[test]
fn should_leave_an_action_unknown_when_the_link_drops_under_it() {
    let home = change_support::build_disk_home("change-remote-disconnect");
    let root = home.path().display().to_string();

    // The plan kills the process on the far side that answers for the link — its agent, told
    // apart from every other test's by the directory this session works in — so the request goes
    // out and its answer never comes back: §29.3's link failure in the middle of an action.
    let (succeeded, _, errors) = session_in(
        home.path(),
        &format!(
            "link host l1 --transport local; enter link l1; \
             get process | where \"--agent\" in command | where cwd == \"{root}\" \
             | plan kill process | apply --accept-risk --accept-irreversible --confirm"
        ),
    );

    assert!(
        !succeeded
            && errors.contains("change.remote_state_unknown")
            && !errors.contains("change.apply_failed"),
        "§29.3: an action whose link dropped under it is unknown, never failed. Got {errors:?}"
    );
    let plans = ono_at(home.path(), "get plan | select state | to json");
    plans.assert_success();
    assert!(
        plans.stdout().contains("applying"),
        "Appendix F.2: the plan stays applying until the host can be asked again. Got {:?}",
        plans.stdout()
    );
}

#[test]
fn should_plan_recovery_per_host_and_say_the_linked_host_cannot_proceed() {
    let home = home();
    let pid = detached_sleep();
    let applied = ono_at(
        home.path(),
        &format!(
            "link host l1 --transport local; enter link l1; \
             plan kill process {pid} | apply --accept-risk --accept-irreversible --confirm; \
             get plan | select id | to json"
        ),
    );
    clean_up(pid);
    applied.assert_success();
    let plan = text(&last_json(applied.stdout())[0], "id");

    let run = ono_at(
        home.path(),
        &format!(
            "link host l1 --transport local; enter link l1; recover {}",
            &plan[..8]
        ),
    );

    assert!(
        !run.status().is_success() && run.stderr().contains("recovery.plan_incomplete"),
        "§29.4: recovery is planned per host, and a host it cannot proceed on is a refusal. \
         Got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("l1"),
        "§29.4: the refusal says which host. Got {:?}",
        run.stderr()
    );
}

#[test]
fn should_refuse_to_apply_a_remote_plan_outside_the_link_its_host_is_reached_over() {
    let home = home();
    let mut sleeper = Sleeper::start();
    let plan = text(&remote_kill_plan(home.path(), sleeper.pid()), "id");

    let run = ono_at(home.path(), &format!("apply {}", &plan[..8]));

    assert!(
        !run.status().is_success() && run.stderr().contains("change.precondition_failed"),
        "§29.1: a plan about host l1 runs only where provider calls go to l1. Got {:?}",
        run.output()
    );
    assert!(run.stderr().contains("l1"), "the refusal names the host");
    assert!(sleeper.is_alive(), "§2.3: nothing ran");
}

#[test]
fn should_refuse_to_apply_a_local_plan_inside_a_link() {
    let home = home();
    let mut sleeper = Sleeper::start();
    let planned = ono_at(
        home.path(),
        &format!("plan kill process {} | to json", sleeper.pid()),
    );
    planned.assert_success();
    let plan = text(&one(&planned), "id");

    let run = ono_at(
        home.path(),
        &format!(
            "link host l1 --transport local; enter link l1; apply {}",
            &plan[..8]
        ),
    );

    assert!(
        !run.status().is_success() && run.stderr().contains("change.precondition_failed"),
        "§29.1: a plan about this machine does not run where provider calls go to another host. \
         Got {:?}",
        run.output()
    );
    assert!(sleeper.is_alive(), "§2.3: nothing ran");
}

#[test]
fn should_refuse_to_plan_a_file_inside_a_link_rather_than_freeze_this_machines_file() {
    let home = home();
    let source = home.path().join("a");
    let target = home.path().join("b");
    std::fs::write(&source, "x\n").expect("the source is written");
    std::fs::write(&target, "y\n").expect("the target is written");

    let run = ono_at(
        home.path(),
        &format!(
            "link host l1 --transport local; enter link l1; \
             plan copy file {} {} --overwrite",
            source.display(),
            target.display()
        ),
    );

    assert!(
        !run.status().is_success() && run.stderr().contains("change.action_not_plannable"),
        "§29.1: a plan inside a link is refused rather than frozen on this machine. Got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("link"),
        "§29.1: the refusal says the link is why. Got {:?}",
        run.stderr()
    );
    let plans = ono_at(home.path(), "get plan | count | to json");
    plans.assert_success();
    assert_eq!(
        plans.stdout().split_whitespace().collect::<String>(),
        "[0]",
        "§2.1: nothing was sealed"
    );
}

/// A listening agent on a loopback port the system chose, pinned and authorized both ways, as case
/// 180 sets one up. Killed when dropped.
struct Agent {
    child: std::process::Child,
    address: String,
}

impl Agent {
    fn start(home: &std::path::Path) -> Self {
        let root = home.to_string_lossy().into_owned();
        let ono = |args: &[&str]| {
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ono"));
            command
                .env("NO_COLOR", "1")
                .env("HOME", &root)
                .env("XDG_CONFIG_HOME", format!("{root}/config"))
                .env("XDG_DATA_HOME", format!("{root}/data"))
                .env("XDG_STATE_HOME", format!("{root}/state"))
                .args(args);
            command
        };
        let key = home.join("agent.pem").to_string_lossy().into_owned();
        let printed = |args: &[&str]| {
            let output = ono(args).output().expect("ono runs");
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        let host_key = printed(&["--agent", "--host-key", &key, "--print-host-key"]);
        let client_key = printed(&["--print-peer-key"]);
        ono_at(
            home,
            &format!("add host-key 127.0.0.1 --fingerprint {host_key}"),
        )
        .assert_success();
        ono_at(home, &format!("add client-key {client_key} --label test")).assert_success();
        let log = home.join("agent.log");
        let mut child = ono(&["--agent", "--listen", "127.0.0.1:0", "--host-key", &key])
            .stderr(std::fs::File::create(&log).expect("the log is created"))
            .spawn()
            .expect("the agent starts");
        for _ in 0..200 {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            if let Some(address) = text
                .lines()
                .find_map(|line| line.strip_prefix("ono: listening on "))
            {
                return Self {
                    child,
                    address: address.trim().to_owned(),
                };
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
        panic!(
            "the agent did not listen: {:?}",
            std::fs::read_to_string(&log)
        );
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The rules `plan`'s risk assessment fired, with their classes.
fn findings(plan: &serde_yaml_ng::Value) -> Vec<(String, String)> {
    plan["risk_findings"]
        .as_sequence()
        .expect("a plan carries its risk findings")
        .iter()
        .map(|finding| (text(finding, "rule"), text(finding, "class")))
        .collect()
}

#[test]
fn should_make_a_plan_that_stops_the_interface_the_active_link_runs_over_critical() {
    let home = home();
    let agent = Agent::start(home.path());

    let run = ono_at(
        home.path(),
        &format!(
            "link host {} --transport tcp; plan stop interface lo | to json",
            agent.address
        ),
    );

    run.assert_success();
    let plan = last_json(run.stdout())
        .into_iter()
        .next()
        .expect("one plan");
    assert!(
        findings(&plan).contains(&("risk.remote.link-loss".to_owned(), "critical".to_owned())),
        "§34.2: a plan that may remove the path the active link uses is a CRITICAL landmark. \
         Got {:?}",
        findings(&plan)
    );
    assert_eq!(
        plan["risk"].as_str(),
        Some("critical"),
        "§19.2: the plan is as risky as its worst finding"
    );
}

#[test]
fn should_not_call_stopping_an_interface_link_loss_when_no_network_link_is_held() {
    let home = home();

    let run = ono_at(
        home.path(),
        "link host l1 --transport local; plan stop interface lo | to json",
    );

    run.assert_success();
    let plan = last_json(run.stdout())
        .into_iter()
        .next()
        .expect("one plan");
    assert!(
        !findings(&plan)
            .iter()
            .any(|(rule, _)| rule == "risk.remote.link-loss"),
        "§34.2 is about a network path, and a local link has none. Got {:?}",
        findings(&plan)
    );
}
