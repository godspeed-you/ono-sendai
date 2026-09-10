//! Appendix H's profiles, selected by `change.profile` (v0.6 Appendix H, §17, §19.4, §40.3, §53).
//!
//! A profile is a preset over settings an operator could have written by hand, and H.5 is the rule
//! that keeps it safe to have: it may tighten what is in force and may never loosen it. These
//! tests drive the real binary, because the place a loosening would show is the plan the shell
//! seals and the gate `apply` raises.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod change_support;

use std::path::Path;

use change_support::{one, ono_at, text};

/// A home on the disk the build uses, removed when the test ends.
///
/// `require` refuses an apply whose files cannot be protected, and §11.2 refuses to protect a
/// volatile filesystem. The shared scratch home is under the system temporary directory, which is
/// tmpfs on many hosts, so these tests make their own under Cargo's per-target directory instead.
struct Home(std::path::PathBuf);

impl Home {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn home() -> Home {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("change-profiles-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&path).expect("a home on the build disk");
    Home(path)
}

fn with_config(home: &Path, lines: &str) {
    let config = home.join("config/ono");
    std::fs::create_dir_all(&config).expect("a config directory");
    std::fs::write(config.join("config.ono"), lines).expect("written");
}

/// A file in `home` with something in it, for a plan to change.
fn target(home: &Path) -> std::path::PathBuf {
    let path = home.join("target.txt");
    std::fs::write(&path, "original\n").expect("written");
    path
}

/// The protection mode the plan of `script` was sealed under.
fn sealed_mode(home: &Path, script: &str) -> String {
    let run = ono_at(home, &format!("{script} | to json"));
    run.assert_success();
    text(&one(&run), "protection_mode")
}

#[test]
fn should_plan_under_require_when_the_cautious_profile_is_chosen() {
    let home = home();
    with_config(home.path(), "set config change.profile cautious\n");
    let file = target(home.path());

    assert_eq!(
        sealed_mode(home.path(), &format!("plan remove file {}", file.display())),
        "require",
        "Appendix H.2: `cautious` protects at `require`, so a plan that asks for nothing gets it"
    );
}

#[test]
fn should_keep_an_explicit_require_when_the_profile_asks_for_less() {
    let home = home();
    with_config(
        home.path(),
        "set config change.default_protection require\nset config change.profile fleet\n",
    );
    let file = target(home.path());

    let chosen = ono_at(home.path(), "get config change.profile | to json");
    chosen.assert_success();
    assert_eq!(
        text(&one(&chosen), "value"),
        "fleet",
        "the profile was selected, so what follows is a profile meeting a stricter setting"
    );
    assert_eq!(
        sealed_mode(home.path(), &format!("plan remove file {}", file.display())),
        "require",
        "Appendix H.5 and ADR-0810: `fleet` asks for `prefer`, and a profile never loosens a \
         stricter setting the operator wrote"
    );
}

#[test]
fn should_not_let_a_plan_ask_for_less_than_the_profile_requires() {
    let home = home();
    with_config(home.path(), "set config change.profile cautious\n");
    let file = target(home.path());

    assert_eq!(
        sealed_mode(
            home.path(),
            &format!("plan --protection off remove file {}", file.display())
        ),
        "require",
        "ADR-0810: the strictest requirement in force applies, whichever source states it"
    );
}

#[test]
fn should_show_the_chosen_profile_with_the_layer_that_chose_it() {
    let home = home();
    with_config(home.path(), "set config change.profile cautious\n");

    let chosen = ono_at(home.path(), "get config change.profile | to json");
    chosen.assert_success();
    let record = one(&chosen);
    assert_eq!(text(&record, "value"), "cautious");
    assert_eq!(
        text(&record, "layer"),
        "user",
        "Appendix H: a profile is an inspectable setting, attributed like every other one"
    );
}

#[test]
fn should_default_to_no_profile() {
    let home = home();

    let chosen = ono_at(home.path(), "get config change.profile | to json");
    chosen.assert_success();
    assert_eq!(
        text(&one(&chosen), "value"),
        "none",
        "no profile applies until an operator chooses one"
    );
}

#[test]
fn should_report_a_profile_nobody_defined_where_configuration_problems_are() {
    let home = home();
    with_config(home.path(), "set config change.profile paranoid\n");

    let problems = ono_at(home.path(), "get config --problems | to json");
    problems.assert_success();
    assert!(
        problems.stdout().contains("change.profile") && problems.stdout().contains("paranoid"),
        "§53: a configuration nobody can read is reported rather than ignored, got {:?}",
        problems.stdout()
    );
}

/// Appendix H.2's risk gate is `moderate+`: a plan §40.1 would let through on `apply` alone needs
/// its acknowledgement under `cautious`, and the flag answers it.
#[test]
fn should_gate_a_plan_below_high_risk_under_the_cautious_profile() {
    let home = home();
    with_config(home.path(), "set config change.profile cautious\n");
    let file = target(home.path());
    let planned = ono_at(
        home.path(),
        &format!("plan remove file {} | to json", file.display()),
    );
    planned.assert_success();
    let id = text(&one(&planned), "id")[..8].to_owned();

    let refused = ono_at(home.path(), &format!("apply {id} --confirm"));
    assert!(
        !refused.status().is_success(),
        "Appendix H.2: under `cautious` a plan below HIGH still needs its acknowledgement"
    );
    assert!(
        refused.stderr().contains("--accept-risk"),
        "§40.3: the refusal names the flag that answers it, got {:?}",
        refused.stderr()
    );
    assert!(file.exists(), "a refused gate has changed nothing (§40.2)");

    let applied = ono_at(home.path(), &format!("apply {id} --accept-risk --confirm"));
    applied.assert_success();
    assert!(!file.exists(), "and the acknowledged plan applies");
}

#[test]
fn should_leave_a_plan_below_high_risk_ungated_without_a_profile() {
    let home = home();
    let file = target(home.path());
    let planned = ono_at(
        home.path(),
        &format!("plan remove file {} | to json", file.display()),
    );
    planned.assert_success();
    let id = text(&one(&planned), "id")[..8].to_owned();

    let applied = ono_at(home.path(), &format!("apply {id}"));
    applied.assert_success();
    assert!(
        !file.exists(),
        "§40.1: without a profile the same plan applies on `apply` alone"
    );
}

/// Appendix H.4: `scripted` never prompts, so every question a command would ask is answered by
/// its flag, and the refusal says it is the profile that made the flag necessary — which is what a
/// person at a terminal who expected to be asked needs to read. `protect` asks §40.3's
/// confirmation of every plan, so it is the gate every plan has.
#[test]
fn should_name_the_scripted_profile_when_a_gate_needs_its_flag() {
    let home = home();
    with_config(home.path(), "set config change.profile scripted\n");
    let file = target(home.path());
    let planned = ono_at(
        home.path(),
        &format!("plan remove file {} | to json", file.display()),
    );
    planned.assert_success();
    let id = text(&one(&planned), "id")[..8].to_owned();

    let refused = ono_at(home.path(), &format!("protect {id}"));
    assert!(
        !refused.status().is_success(),
        "§40.3: a commitment is not made without its confirmation"
    );
    assert!(
        refused.stderr().contains("--confirm"),
        "the refusal names the flag, got {:?}",
        refused.stderr()
    );
    assert!(
        refused.stderr().contains("scripted"),
        "Appendix H.4: the refusal says the `scripted` profile never prompts, got {:?}",
        refused.stderr()
    );
}

/// Appendix H: a profile expands to inspectable settings. `get config --profile` shows each
/// setting the profile touches, what it asks for and what is in force.
#[test]
fn should_expand_the_chosen_profile_into_inspectable_settings() {
    let home = home();
    with_config(home.path(), "set config change.profile cautious\n");
    let run = ono_at(home.path(), "get config --profile | to json");
    run.assert_success();
    let rows = change_support::rows(&run);
    let protection = rows
        .iter()
        .find(|row| {
            row["key"]
                .as_str()
                .is_some_and(|key| key.contains("protection"))
        })
        .unwrap_or_else(|| panic!("the protection mode is in the expansion, got {rows:?}"));
    assert_eq!(protection["profile"].as_str(), Some("require"));
    assert_eq!(protection["effective"].as_str(), Some("require"));
}

/// Appendix H.3: under `fleet`, a plan that states no strategy and meets no written default runs
/// as a canary, with the batch share taken of the plan's own targets.
#[test]
fn should_default_a_fleet_plan_to_a_canary() {
    let home = home();
    with_config(home.path(), "set config change.profile fleet\n");
    let dir = home.path().join("fleet");
    std::fs::create_dir_all(&dir).expect("a directory");
    for name in ["a.conf", "b.conf", "c.conf"] {
        std::fs::write(dir.join(name), "x\n").expect("written");
    }
    let run = ono_at(
        home.path(),
        &format!(
            "get file {}/*.conf | plan remove file | to json",
            dir.display()
        ),
    );
    run.assert_success();
    assert!(
        run.stdout().contains("canary"),
        "the fleet profile's strategy is the plan's default, got {}",
        run.stdout()
    );
}
