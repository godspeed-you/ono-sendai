//! Revalidation of a planned process mutation (v0.6 §7.1, §7.2, §7.3).
//!
//! §7.3 re-resolves every target immediately before PREPARE and stops on *material* drift. The
//! contract has two sides and both are held here: a process that did not move must not be read as
//! drift — otherwise no process plan could ever be applied — and a process that has gone must be.
//! Every process in this suite is one the test starts itself, so no test depends on what else the
//! host runs (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use std::path::Path;
use std::time::{Duration, Instant};

use change_support::{home, one, ono_at, text};

/// A `sleep 600` detached from the test, so the kernel's reaper collects it once it is killed and
/// "the process is gone" means gone rather than a zombie waiting for this test to `wait` on it.
struct Sleeper(u32);

impl Sleeper {
    fn start() -> Self {
        let output = std::process::Command::new("sh")
            .args(["-c", "sleep 600 >/dev/null 2>&1 & echo $!"])
            .output()
            .expect("sh starts a background sleep");
        let pid = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .expect("sh prints the pid of the sleep it started");
        Self(pid)
    }

    fn alive(&self) -> bool {
        // A zombie still has a /proc entry, and it is not a process anybody could plan against.
        std::fs::read_to_string(format!("/proc/{}/stat", self.0))
            .map(|stat| {
                stat.rsplit_once(')')
                    .and_then(|(_, rest)| rest.split_whitespace().next())
                    .is_some_and(|state| state != "Z")
            })
            .unwrap_or(false)
    }

    fn gone_within(&self, budget: Duration) -> bool {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if !self.alive() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        !self.alive()
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        // Usually already gone — the test killed it, or the plan did — so the answer is not read.
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &self.0.to_string()])
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Plans `kill process <pid>` and answers with the short reference of the plan.
fn plan_kill(home: &Path, pid: u32) -> String {
    let run = ono_at(home, &format!("plan kill process {pid} | to json"));
    run.assert_success();
    text(&one(&run), "id")[..4].to_owned()
}

/// `apply` with every acknowledgement a kill needs, so only drift could stop it (§19.4, §40).
fn apply(home: &Path, reference: &str) -> ono_testkit::Run {
    ono_at(
        home,
        &format!("apply {reference} --confirm --accept-irreversible --accept-risk"),
    )
}

#[test]
fn should_not_report_drift_when_the_planned_process_is_unchanged() {
    let home = home();
    let sleeper = Sleeper::start();
    let reference = plan_kill(home.path(), sleeper.0);
    assert!(sleeper.alive(), "planning a kill must not kill (§2.1)");

    let run = apply(home.path(), &reference);
    assert!(
        !run.output().contains("change.plan_drift_detected"),
        "v0.6 §7.3: nothing moved between the plan and the apply — the same pid, the same start \
         time — so revalidation must find no material drift. Got {:?}",
        run.output()
    );
}

#[test]
fn should_remove_the_process_when_a_planned_kill_is_applied() {
    let home = home();
    let sleeper = Sleeper::start();
    let reference = plan_kill(home.path(), sleeper.0);

    let run = apply(home.path(), &reference);
    run.assert_success();
    assert!(
        sleeper.gone_within(Duration::from_secs(10)),
        "v0.6 §4.7: applying `kill process {}` carries the kill out, so the process is gone \
         afterwards. apply said {:?}",
        sleeper.0,
        run.output()
    );
}

#[test]
fn should_stop_the_apply_when_the_planned_process_has_gone() {
    let home = home();
    let sleeper = Sleeper::start();
    let reference = plan_kill(home.path(), sleeper.0);
    let _ = std::process::Command::new("kill")
        .args(["-KILL", &sleeper.0.to_string()])
        .status();
    assert!(
        sleeper.gone_within(Duration::from_secs(10)),
        "the fixture could not end its own sleep"
    );

    let run = apply(home.path(), &reference);
    assert!(
        // Contract change (NEW-5): a target that no longer exists is refused as the changed target
        // it is — `change.target_changed` — rather than as generic drift.
        !run.status().is_success() && run.output().contains("change.target_changed"),
        "v0.6 §7.3: the object the plan was resolved against no longer exists, so the apply stops \
         and names the changed target. Got {:?}",
        run.output()
    );
}

/// Plans `copy file <source> <destination> --overwrite` and answers with the plan's reference.
fn plan_copy(home: &Path, source: &Path, destination: &Path) -> String {
    let run = ono_at(
        home,
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    text(&one(&run), "id")[..4].to_owned()
}

#[test]
fn should_refuse_to_write_through_a_symlink_swapped_in_after_the_seal() {
    let home = home();
    let source = home.path().join("source.conf");
    let destination = home.path().join("app.conf");
    let victim = home.path().join("victim.conf");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    // The victim holds the destination's very bytes, so the digest the plan froze still matches
    // through the link: only the object's own identity can tell the two apart.
    std::fs::write(&victim, b"old\n").expect("the victim is written");
    let reference = plan_copy(home.path(), &source, &destination);

    std::fs::remove_file(&destination).expect("the destination is removed");
    std::os::unix::fs::symlink(&victim, &destination).expect("the symlink is swapped in");

    let run = ono_at(home.path(), &format!("apply {reference} --confirm"));
    assert!(
        !run.status().is_success(),
        "v0.6 §43.5: the path the plan froze is now a symlink to another object, so revalidation \
         must refuse rather than mutate a replaced symlink target. Got {:?}",
        run.output()
    );
    assert!(
        run.output().contains("change.plan_drift_detected")
            || run.output().contains("change.target_changed"),
        "v0.6 §7.3: the refusal is the drift refusal, naming what moved. Got {:?}",
        run.output()
    );
    assert_eq!(
        std::fs::read(&victim).expect("the victim is readable"),
        b"old\n",
        "v0.6 §43.5: nothing is written through the swapped-in link"
    );
}

#[test]
fn should_freeze_the_installed_version_when_a_package_mutation_is_planned() {
    let home = home();
    let listed = ono_at(home.path(), "get package | take 1 | to json");
    let package = listed
        .status()
        .is_success()
        .then(|| change_support::rows(&listed).into_iter().next())
        .flatten();
    let Some(package) = package else {
        ono_testkit::skipped(
            ono_testkit::SkipReason::FixtureNotApplicable,
            "this host has no package manager that lists an installed package",
        );
        return;
    };
    let name = text(&package, "name");
    let version = text(&package, "version");

    let run = ono_at(
        home.path(),
        &format!("plan remove package {name} | to json"),
    );
    run.assert_success();
    let plan = one(&run);
    let frozen: Vec<_> = plan["actions"]
        .as_sequence()
        .expect("the plan lists its actions")
        .iter()
        .flat_map(|action| {
            action["preconditions"]
                .as_sequence()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|precondition| precondition["kind"].as_str() == Some("version"))
        .collect();
    assert!(
        frozen
            .iter()
            .any(|precondition| precondition["expected"].as_str() == Some(version.as_str())),
        "v0.6 §7.2: \"package installed version still equals Z\" — a package plan freezes the \
         installed version `{version}` of `{name}` as a precondition. Version preconditions: \
         {frozen:?}"
    );
}

#[test]
fn should_answer_unknown_when_a_verified_field_is_missing_from_the_object() {
    let home = home();
    let sleeper = Sleeper::start();
    let source = home.path().join("source.conf");
    let destination = home.path().join("app.conf");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let run = ono_at(
        home.path(),
        &format!(
            "plan {{ copy file {} {} --overwrite; verify process {} no_such_field != x }} | to json",
            source.display(),
            destination.display(),
            sleeper.0
        ),
    );
    run.assert_success();
    let reference = text(&one(&run), "id")[..4].to_owned();
    let _ = ono_at(home.path(), &format!("apply {reference} --confirm"));

    let verified = ono_at(home.path(), &format!("verify {reference} | to json"));
    let rows = change_support::rows(&verified);
    let check = rows
        .iter()
        .find(|row| {
            row["expression"].as_str() == Some("no_such_field != x")
                || format!("{row:?}").contains("no_such_field")
        })
        .unwrap_or_else(|| panic!("the missing-field check is reported; got {rows:?}"));
    assert_eq!(
        check["status"].as_str(),
        Some("unknown"),
        "v0.6 §2.4 and §23.3: the process carries no `no_such_field`, so whether it differs from \
         `x` is unknown — neither a pass nor a failure. Got {check:?}"
    );
}
