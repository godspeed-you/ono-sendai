//! Applying a recovery plan (v0.6 §2.12, §5.8, §24.1, §24.5, §40.1, Appendix C.4).
//!
//! `recover` builds a plan and changes nothing; `apply` on that plan is the restore. These tests
//! drive the real binary through that second half, because it is where a recovery either keeps
//! the promise §24.5 makes — nothing newer is discarded without the explicit gate — or silently
//! breaks it.

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
/// A recovery needs an asset, and §11.2 refuses to protect a volatile filesystem. The shared
/// scratch home is under the system temporary directory, which is tmpfs on many hosts, so these
/// tests make their own under Cargo's per-target temporary directory instead.
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
        .join(format!("recovery-apply-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&path).expect("a home on the build disk");
    Home(path)
}

/// A file changed by an applied, protected plan: `(config, plan reference)`.
fn applied_copy(home: &Path) -> (std::path::PathBuf, String) {
    let etc = home.join("etc");
    std::fs::create_dir_all(&etc).expect("a directory");
    let config = etc.join("config");
    std::fs::write(&config, "original\n").expect("written");
    std::fs::write(etc.join("source"), "changed\n").expect("written");
    let planned = ono_at(
        home,
        &format!(
            "plan copy file {} {} --overwrite | to json",
            etc.join("source").display(),
            config.display()
        ),
    );
    planned.assert_success();
    let id = text(&one(&planned), "id")[..8].to_owned();
    ono_at(home, &format!("apply {id}")).assert_success();
    assert_eq!(
        std::fs::read_to_string(&config).expect("readable"),
        "changed\n",
        "the fixture's plan applied"
    );
    (config, id)
}

/// `recover <plan>`, answering with the recovery plan's reference.
fn recovery_of(home: &Path, plan: &str) -> String {
    let run = ono_at(home, &format!("recover {plan} | to json"));
    run.assert_success();
    text(&one(&run), "id")[..8].to_owned()
}

#[test]
fn should_refuse_to_discard_a_later_edit_without_the_explicit_acceptance() {
    let home = home();
    let (config, plan) = applied_copy(home.path());
    std::fs::write(&config, "newer, written after the change\n").expect("edited");
    let recovery = recovery_of(home.path(), &plan);
    let run = ono_at(home.path(), &format!("apply {recovery} --confirm"));
    assert!(
        !run.status().is_success(),
        "§24.5: no recovery execution occurs without the explicit gate"
    );
    assert!(
        run.stderr().contains("recovery.newer_state_conflict"),
        "Appendix C.4: the refusal names the conflict, got {:?}",
        run.stderr()
    );
    assert_eq!(
        std::fs::read_to_string(&config).expect("readable"),
        "newer, written after the change\n",
        "§2.12: the newer edit is still there"
    );
}

#[test]
fn should_restore_over_a_later_edit_once_its_loss_is_accepted() {
    let home = home();
    let (config, plan) = applied_copy(home.path());
    std::fs::write(&config, "newer, written after the change\n").expect("edited");
    let recovery = recovery_of(home.path(), &plan);
    ono_at(
        home.path(),
        &format!("apply {recovery} --accept-newer-state-loss --confirm"),
    )
    .assert_success();
    assert_eq!(
        std::fs::read_to_string(&config).expect("readable"),
        "original\n",
        "§15.4: the restore put back what the asset held"
    );
}

#[test]
fn should_restore_the_plans_own_change_without_any_acknowledgement() {
    let home = home();
    let (config, plan) = applied_copy(home.path());
    let recovery = recovery_of(home.path(), &plan);
    ono_at(home.path(), &format!("apply {recovery}")).assert_success();
    assert_eq!(
        std::fs::read_to_string(&config).expect("readable"),
        "original\n",
        "§40.1 applied to recovery: undoing the plan's own write loses nothing and needs no flag"
    );
}

#[test]
fn should_refuse_a_recovery_plan_whose_world_moved_after_it_was_built() {
    let home = home();
    let (config, plan) = applied_copy(home.path());
    let recovery = recovery_of(home.path(), &plan);
    std::fs::write(&config, "edited after the recovery plan was shown\n").expect("edited");
    let run = ono_at(home.path(), &format!("apply {recovery} --confirm"));
    assert!(
        !run.status().is_success(),
        "§7.3 applied to recovery: the plan shown lost nothing, the world now would"
    );
    assert_eq!(
        std::fs::read_to_string(&config).expect("readable"),
        "edited after the recovery plan was shown\n",
        "§2.12: the edit nobody was shown as a loss is still there"
    );
}

/// §25.3 and v0.5 §22.1: a recovery that restored and verified is recorded on the ledger as the
/// success it is. Its terminal state is `recovery-verified`, and a mapping that knew only
/// `verified` recorded every successful recovery as a failed plan.
#[test]
fn should_record_a_verified_recovery_as_verified_on_the_ledger() {
    let home = home();
    let (_config, plan) = applied_copy(home.path());
    let recovery = recovery_of(home.path(), &plan);
    change_support::ono_with(
        home.path(),
        "ONO_TEMPORAL_RECORDING_ENABLED",
        "true",
        &format!("apply {recovery}"),
    )
    .assert_success();
    let run = change_support::ono_with(
        home.path(),
        "ONO_TEMPORAL_RECORDING_ENABLED",
        "true",
        "find event | to json",
    );
    run.assert_success();
    let verified = change_support::rows(&run).into_iter().any(|event| {
        event["subtype"].as_str() == Some("ono.plan.verified")
            && serde_yaml_ng::to_string(&event)
                .unwrap_or_default()
                .contains(&recovery)
    });
    assert!(
        verified,
        "the recovery plan's verification is recorded as `ono.plan.verified`, got {}",
        run.stdout()
    );
}

/// v0.6 §41.3 and §24.5: `resume plan` on a recovery plan passes the same newer-state gate as
/// `apply`. A resumed restore never runs past what the operator accepted.
#[test]
fn should_hold_a_resumed_recovery_to_the_same_newer_state_gate() {
    let home = home();
    let (config, plan) = applied_copy(home.path());
    std::fs::write(&config, "newer, written after the change\n").expect("edited");
    let recovery = recovery_of(home.path(), &plan);

    let refused = ono_at(home.path(), &format!("resume plan {recovery} --confirm"));
    assert!(
        !refused.status().is_success(),
        "§24.5: resuming a recovery that would discard newer state needs the acceptance"
    );
    assert!(
        refused.stderr().contains("recovery.newer_state_conflict"),
        "the refusal names the conflict, got {:?}",
        refused.stderr()
    );
    assert_eq!(
        std::fs::read_to_string(&config).expect("readable"),
        "newer, written after the change\n",
        "and the newer edit is intact"
    );

    ono_at(
        home.path(),
        &format!("resume plan {recovery} --accept-newer-state-loss --confirm"),
    )
    .assert_success();
    assert_eq!(
        std::fs::read_to_string(&config).expect("readable"),
        "original\n",
        "with the acceptance, the resumed recovery restores"
    );
}

/// ADR-0821: one creation, one event. Planning a recovery is a later fact about the source plan,
/// and it must not record the source plan's creation a second time.
#[test]
fn should_not_record_the_source_plans_creation_again_when_recovery_is_planned() {
    let home = home();
    let recording = |script: &str| {
        change_support::ono_with(
            home.path(),
            "ONO_TEMPORAL_RECORDING_ENABLED",
            "true",
            script,
        )
    };
    let etc = home.path().join("etc");
    std::fs::create_dir_all(&etc).expect("a directory");
    std::fs::write(etc.join("config"), "original\n").expect("written");
    std::fs::write(etc.join("source"), "changed\n").expect("written");
    let planned = recording(&format!(
        "plan copy file {} {} --overwrite | to json",
        etc.join("source").display(),
        etc.join("config").display()
    ));
    planned.assert_success();
    let plan = text(&one(&planned), "id")[..8].to_owned();
    recording(&format!("apply {plan}")).assert_success();
    recording(&format!("recover {plan}")).assert_success();
    let events = recording("find event | to json");
    events.assert_success();
    let created = change_support::rows(&events)
        .into_iter()
        .filter(|event| {
            event["subtype"].as_str() == Some("ono.plan.created")
                && serde_yaml_ng::to_string(&event)
                    .unwrap_or_default()
                    .contains(&plan)
        })
        .count();
    assert_eq!(
        created, 1,
        "the source plan was created once, and recorded once"
    );
}

/// ADR-0830 and §25.1: a copy is verified by the bytes the destination now holds — the source's
/// digest taken when the plan was sealed — not by the destination merely existing.
#[test]
fn should_verify_a_copy_by_the_bytes_of_its_source() {
    let home = home();
    let etc = home.path().join("etc");
    std::fs::create_dir_all(&etc).expect("a directory");
    let source = etc.join("source");
    let destination = etc.join("destination");
    std::fs::write(&source, "worker_processes 4;\n").expect("written");
    std::fs::write(&destination, "worker_processes 1;\n").expect("written");
    let planned = ono_at(
        home.path(),
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            destination.display()
        ),
    );
    planned.assert_success();
    let digest = {
        use sha2::Digest as _;
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"worker_processes 4;\n");
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert!(
        planned.stdout().contains(&format!("sha256 == {digest}")),
        "the sealed check is the source's digest, got {}",
        planned.stdout()
    );
    let plan = text(&one(&planned), "id")[..8].to_owned();
    ono_at(home.path(), &format!("apply {plan}")).assert_success();

    std::fs::write(&destination, "tampered\n").expect("rewritten");
    let verified = ono_at(home.path(), &format!("verify {plan} | to json"));
    assert!(
        verified.stdout().contains("\"failed\""),
        "a destination that no longer holds the copied bytes fails its check, got {}",
        verified.stdout()
    );
}
