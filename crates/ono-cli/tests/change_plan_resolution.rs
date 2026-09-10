//! What a plan freezes, and what a rebase freezes again (v0.6 §2.6, §4.3, §5.3, §7.2, §7.5, §22.4).
//!
//! §4.3 turns a selector into a concrete identity, and every other promise the plan makes is about
//! that identity: a pipeline's objects are frozen by their own paths (§5.3), a mutation of an
//! object that is not there has nothing to freeze (§4.3), and a rebase is resolved against the
//! world as it is now rather than copied from the revision that drifted (§7.5). The files live on
//! the build disk, because protecting a file needs a persistence domain and the shared scratch
//! home is on tmpfs on many hosts (§11.2, Appendix B.7).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod change_support;

use std::path::{Path, PathBuf};

use change_support::{one, ono_at, rows, text};

/// A home on the disk the build uses, removed when the test ends.
struct Home(PathBuf);

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
        .join(format!("plan-resolution-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&path).expect("a home on the build disk");
    Home(path.canonicalize().expect("the home resolves"))
}

/// The identities a plan record froze, sorted.
fn frozen(plan: &serde_yaml_ng::Value) -> Vec<String> {
    let mut identities: Vec<String> = plan["targets"]
        .as_sequence()
        .expect("a plan carries its targets")
        .iter()
        .map(|target| text(target, "identity"))
        .collect();
    identities.sort();
    identities
}

#[test]
fn should_freeze_each_piped_file_by_its_own_path_when_a_pipeline_supplies_the_objects() {
    let home = home();
    let dir = home.path().join("pipe");
    std::fs::create_dir_all(&dir).expect("a directory");
    for name in ["a", "b", "c"] {
        std::fs::write(dir.join(format!("{name}.conf")), name).expect("written");
    }
    let run = ono_at(
        home.path(),
        &format!(
            "get file {}/*.conf | plan remove file | to json",
            dir.display()
        ),
    );
    run.assert_success();
    let expected: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|name| dir.join(format!("{name}.conf")).display().to_string())
        .collect();
    assert_eq!(
        frozen(&one(&run)),
        expected,
        "§5.3 and §7.1: the plan froze the objects the pipeline carried, by their real paths, and \
         not a name joined onto the working directory"
    );
}

#[test]
fn should_refuse_to_plan_removing_a_file_that_does_not_exist() {
    let home = home();
    let missing = home.path().join("no-such-file-1a2b3c");
    let run = ono_at(
        home.path(),
        &format!("plan remove file {}", missing.display()),
    );
    assert!(
        !run.status().is_success(),
        "§4.3: a removal whose object does not exist has nothing to freeze, so it is refused"
    );
    assert!(
        run.stderr().contains("change.target_unresolved"),
        "§4.3's own code names the refusal: {}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("no-such-file-1a2b3c"),
        "and it names the object that did not resolve: {}",
        run.stderr()
    );
    let listed = ono_at(home.path(), "get plan | to json");
    listed.assert_success();
    assert!(
        rows(&listed).is_empty(),
        "§4.3: nothing was sealed, so there is no half-plan to apply later"
    );
}

#[test]
fn should_apply_the_rebased_revision_of_a_plan_whose_file_moved_after_the_seal() {
    let home = home();
    let etc = home.path().join("etc");
    std::fs::create_dir_all(&etc).expect("a directory");
    let target = etc.join("target");
    let source = etc.join("source");
    std::fs::write(&target, "one\n").expect("written");
    std::fs::write(&source, "two\n").expect("written");
    let planned = ono_at(
        home.path(),
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            target.display()
        ),
    );
    planned.assert_success();
    let id = text(&one(&planned), "id");
    std::fs::write(&target, "three\n").expect("the world moves after the seal");
    let refused = ono_at(home.path(), &format!("apply {id}"));
    assert!(
        refused.stderr().contains("change.plan_drift_detected"),
        "§7.3: the first revision refuses: {}",
        refused.stderr()
    );

    let rebased = ono_at(home.path(), &format!("rebase plan {id} | to json"));
    rebased.assert_success();
    let applied = ono_at(home.path(), &format!("apply {id}"));
    assert!(
        applied.status().is_success(),
        "§7.5: the rebased revision was resolved against the world as it is now, so it applies: \
         {}",
        applied.stderr()
    );
    assert_eq!(
        std::fs::read_to_string(&target).expect("the target is there"),
        "two\n",
        "and the file holds what the rebased revision said it would write"
    );
    let state = ono_at(home.path(), &format!("get plan {id} | to json"));
    assert_eq!(text(&one(&state), "state"), "verified");
}

#[test]
fn should_scope_the_timeline_to_the_current_plan_when_it_is_named_as_quoted_at() {
    let home = home();
    let etc = home.path().join("etc");
    std::fs::create_dir_all(&etc).expect("a directory");
    std::fs::write(etc.join("source"), "a\n").expect("written");
    std::fs::write(etc.join("target"), "b\n").expect("written");
    let run = ono_at(
        home.path(),
        &format!(
            "plan copy file {}/source {}/target --overwrite | apply\ntimeline --plan \"@\"",
            etc.display(),
            etc.display()
        ),
    );
    assert!(
        run.stdout().contains("ono.plan.created") && run.stdout().contains("ono.plan.verified"),
        "§22.4 and ADR-0803: `\"@\"` is the plan this shell just produced, so the timeline scoped \
         to it shows its lifecycle: {}",
        run.stdout()
    );
}

#[test]
fn should_reject_an_auto_recovery_declaration_at_seal_and_store_nothing() {
    let home = home();
    let etc = home.path().join("etc");
    std::fs::create_dir_all(&etc).expect("a directory");
    std::fs::write(etc.join("a"), "a\n").expect("written");
    std::fs::write(etc.join("b"), "b\n").expect("written");
    let run = ono_at(
        home.path(),
        &format!(
            "plan --auto-recover copy file {}/a {}/b --overwrite",
            etc.display(),
            etc.display()
        ),
    );
    assert!(
        !run.status().is_success(),
        "§26.3: a declaration whose conditions do not hold is rejected at seal"
    );
    assert!(
        run.stderr().contains("change.auto_recovery_rejected"),
        "with its own code: {}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("fully constructed"),
        "and it names the conditions that are unmet: {}",
        run.stderr()
    );
    let listed = ono_at(home.path(), "get plan | to json");
    listed.assert_success();
    assert!(
        rows(&listed).is_empty(),
        "§26.3: a rejected declaration seals and stores nothing"
    );
}

#[test]
fn should_record_no_persistence_domain_for_a_file_on_a_volatile_filesystem() {
    let is_tmpfs = std::fs::read_to_string("/proc/mounts").is_ok_and(|mounts| {
        mounts.lines().any(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            fields.get(1) == Some(&"/dev/shm") && fields.get(2) == Some(&"tmpfs")
        })
    });
    let target = Path::new("/dev/shm").join(format!("ono-volatile-{}", std::process::id()));
    if !is_tmpfs || std::fs::write(&target, "x\n").is_err() {
        ono_testkit::skipped(
            ono_testkit::SkipReason::FixtureNotApplicable,
            "this host has no writable tmpfs at /dev/shm",
        );
        return;
    }
    let home = home();
    let source = home.path().join("source");
    std::fs::write(&source, "a\n").expect("written");
    let run = ono_at(
        home.path(),
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            target.display()
        ),
    );
    let _ = std::fs::remove_file(&target);
    run.assert_success();
    let plan = one(&run);
    let frozen = &plan["targets"][0];
    assert_eq!(text(frozen, "identity"), target.display().to_string());
    assert!(
        frozen["persistence_domain"].is_null(),
        "Appendix B.7: a tmpfs is never a persistence domain, so the target records none rather \
         than the filesystem's label: {frozen:?}"
    );
}
