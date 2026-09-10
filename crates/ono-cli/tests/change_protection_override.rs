//! A plan's own `--protection` against the configured mode (v0.6 §17.1, §17.3, §53, ADR-0834).
//!
//! §17.3 lets a plan override configuration, and §53 forbids configuration being weakened. The two
//! meet at one line: a plan may lower the *built-in* default, and may never lower a mode an
//! operator wrote, in a file or in the environment. These tests drive the real binary, because the
//! place the difference shows is whether a recovery asset is created.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod change_support;

use std::path::Path;

use change_support::{one, ono_at, ono_with, rows, text};

/// A home on the disk the build uses, removed when the test ends.
///
/// Protection is what these tests are about, and §11.2 refuses to protect a volatile filesystem.
/// The shared scratch home is under the system temporary directory, which is tmpfs on many hosts,
/// so these tests make their own under Cargo's per-target directory instead.
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
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "protection-override-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("a home on the build disk");
    Home(path)
}

/// Whether the plan record carries a PREPARE action, which is protection §17.1 would create.
fn prepares(plan: &serde_yaml_ng::Value) -> bool {
    plan["actions"]
        .as_sequence()
        .expect("a plan carries its actions")
        .iter()
        .any(|action| action["role"].as_str() == Some("prepare"))
}

#[test]
fn should_plan_and_apply_without_protection_when_the_plan_says_off_and_nothing_was_configured() {
    let home = home();
    let file = home.path().join("protected.txt");
    std::fs::write(&file, "before the plan\n").expect("written");

    let planned = ono_at(
        home.path(),
        &format!(
            "plan --protection off remove file {} | to json",
            file.display()
        ),
    );
    planned.assert_success();
    let plan = one(&planned);
    assert_eq!(
        text(&plan, "protection_mode"),
        "off",
        "§17.3: a plan MAY override configuration, and none was written"
    );
    assert!(
        !prepares(&plan),
        "§17.2: `off` creates no automatic recovery asset, so it plans none"
    );

    let id = text(&plan, "id")[..8].to_owned();
    ono_at(home.path(), &format!("apply {id}")).assert_success();
    assert!(!file.exists(), "the plan applied");
    let assets = ono_at(home.path(), "get recovery | to json");
    assets.assert_success();
    assert!(
        rows(&assets).is_empty(),
        "and applying it created no recovery asset, got {:?}",
        assets.stdout()
    );
}

#[test]
fn should_keep_a_configured_mode_when_a_plan_asks_for_less() {
    let home = home();
    let config = home.path().join("config/ono");
    std::fs::create_dir_all(&config).expect("a config directory");
    std::fs::write(
        config.join("config.ono"),
        "set config change.default_protection prefer\n",
    )
    .expect("written");
    let file = home.path().join("protected.txt");
    std::fs::write(&file, "before the plan\n").expect("written");

    let planned = ono_at(
        home.path(),
        &format!(
            "plan --protection off remove file {} | to json",
            file.display()
        ),
    );
    planned.assert_success();
    let plan = one(&planned);
    assert_eq!(
        text(&plan, "protection_mode"),
        "prefer",
        "§53: configuration the operator wrote MUST NOT be weakened, even when it says the default"
    );
    assert!(prepares(&plan), "and `prefer` plans its protection");
}

#[test]
fn should_keep_a_mode_configured_in_the_environment_when_a_plan_asks_for_less() {
    let home = home();
    let file = home.path().join("protected.txt");
    std::fs::write(&file, "before the plan\n").expect("written");

    let planned = ono_with(
        home.path(),
        "ONO_CHANGE_DEFAULT_PROTECTION",
        "prefer",
        &format!(
            "plan --protection off remove file {} | to json",
            file.display()
        ),
    );
    planned.assert_success();
    assert_eq!(
        text(&one(&planned), "protection_mode"),
        "prefer",
        "ADR-0010: the environment is a configuration layer like the file"
    );
}
