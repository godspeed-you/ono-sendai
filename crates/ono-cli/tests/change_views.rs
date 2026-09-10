//! What the v0.6 views draw (spec v0.6 §19.4, §20.2, §21.3, §40.2, Appendix E.7).
//!
//! Two views are about decisions: `map --plan` answers "what does this change touch, and what
//! brings it back" (§21.3), and the plan view answers "what will I be asked for before it runs"
//! (§19.4, §40.2). Each test drives the real binary and reads the drawing, because the drawing is
//! the thing an operator reads; `| to json` is checked beside it so the structured value keeps
//! carrying the same facts.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use std::path::Path;
use std::time::Duration;

use change_support::{ono_at, plan, requires_a_service, rows, text};
use ono_testkit::{Run, Shell, SkipReason, skipped};
use serde_yaml_ng::Value;

/// A home on the disk the build uses, removed when the test ends.
///
/// The map marks what a recovery asset covers, and §11.2 refuses to protect a volatile filesystem.
/// The shared scratch home is under the system temporary directory, which is tmpfs on many hosts,
/// so these tests make their own under Cargo's per-target temporary directory instead.
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
        .join(format!("change-views-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&path).expect("a home on the build disk");
    Home(path)
}

/// Runs `script` in `home` at a stated locale and a terminal wide enough to draw a whole line.
fn drawn_at(home: &Path, locale: &str, script: &str) -> Run {
    let root = home.to_string_lossy().into_owned();
    Shell::new()
        .env("NO_COLOR", "1")
        .env("HOME", root.clone())
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .env("LC_ALL", locale)
        .env("LANG", locale)
        .env("TERM", "xterm")
        .env("COLUMNS", "240")
        .args(["-c", script])
        .timeout(Duration::from_secs(120))
        .run()
}

/// A file copy planned in `home`: the short plan reference and the path it overwrites.
fn copy_plan(home: &Path) -> (String, String) {
    let source = home.join("source.txt");
    let destination = home.join("destination.txt");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let reference = plan(
        home,
        &format!(
            "copy file {} {} --overwrite",
            source.display(),
            destination.display()
        ),
    );
    (reference, destination.display().to_string())
}

/// The same plan, applied, so the asset its protection rests on exists (§2.1).
fn applied_copy_plan(home: &Path) -> (String, String) {
    let (reference, destination) = copy_plan(home);
    ono_at(home, &format!("apply {reference} --confirm")).assert_success();
    (reference, destination)
}

/// The provider-native reference of the one recovery asset `home` holds.
fn the_asset_reference(home: &Path) -> String {
    let run = ono_at(home, "get recovery | to json");
    run.assert_success();
    let assets = rows(&run);
    assert_eq!(assets.len(), 1, "one apply made one asset, got {assets:?}");
    text(&assets[0], "reference")
}

/// The line of a drawing that names `object`.
fn line_naming<'a>(run: &'a Run, object: &str) -> &'a str {
    run.stdout()
        .lines()
        .find(|line| line.contains(object))
        .unwrap_or_else(|| {
            panic!(
                "v0.6 §21.2: the overlay names every object the plan touches, and `{object}` is \
                 missing. Got {:?}",
                run.output()
            )
        })
}

#[test]
fn should_mark_the_overwritten_file_with_the_asset_that_covers_it_when_the_map_carries_the_plan() {
    let home = home();
    let (reference, destination) = applied_copy_plan(home.path());
    let asset = the_asset_reference(home.path());
    let run = drawn_at(home.path(), "C", &format!("map --plan {reference}"));
    run.assert_success();
    let line = line_naming(&run, &destination);
    assert!(
        line.contains(&format!("<-> {asset}")),
        "v0.6 §21.3 and Appendix E.7: a covered object reads `<-> <asset>`, in ASCII where the \
         locale does not promise UTF-8. Got {line:?}"
    );
}

#[test]
fn should_draw_the_coverage_mark_in_unicode_when_the_terminal_promises_it() {
    let home = home();
    let (reference, destination) = applied_copy_plan(home.path());
    let run = drawn_at(home.path(), "C.UTF-8", &format!("map --plan {reference}"));
    run.assert_success();
    let line = line_naming(&run, &destination);
    assert!(
        line.contains('\u{2194}') && !line.contains("<->"),
        "Appendix E.7: a UTF-8 terminal gets the double arrow rather than its ASCII fallback. \
         Got {line:?}"
    );
}

#[test]
fn should_say_not_covered_before_the_asset_exists() {
    let home = home();
    let (reference, destination) = copy_plan(home.path());
    let run = drawn_at(home.path(), "C", &format!("map --plan {reference}"));
    run.assert_success();
    let line = line_naming(&run, &destination);
    assert!(
        line.contains("not covered") && !line.contains("<->"),
        "v0.6 §2.1 and Appendix E.8: an asset a plan only proposes covers nothing yet, and the \
         overlay says so rather than showing only the good news. Got {line:?}"
    );
}

#[test]
fn should_name_the_covering_asset_in_the_overlay_value() {
    let home = home();
    let (reference, destination) = applied_copy_plan(home.path());
    let asset = the_asset_reference(home.path());
    let run = ono_at(home.path(), &format!("map --plan {reference} | to json"));
    run.assert_success();
    let document: Value = serde_yaml_ng::from_str(run.stdout().trim())
        .unwrap_or_else(|error| panic!("`to json` emits JSON, got {:?} ({error})", run.stdout()));
    let map = document
        .as_sequence()
        .and_then(|rows| rows.first())
        .unwrap_or(&document);
    let objects = map["ono.change/plan-overlay"]["objects"]
        .as_sequence()
        .unwrap_or_else(|| panic!("the overlay lists the plan's objects, got {map:?}"));
    let object = objects
        .iter()
        .find(|object| object["object"].as_str() == Some(destination.as_str()))
        .unwrap_or_else(|| panic!("the overwritten file is in the overlay, got {objects:?}"));
    assert_eq!(object["covered"].as_bool(), Some(true), "{object:?}");
    assert_eq!(
        object["covered_by"]
            .as_sequence()
            .map(|assets| assets.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec![asset.as_str()]),
        "v0.6 §21.3: the value names the asset the drawing names. Got {object:?}"
    );
}

/// The lines under the plan view's `approval` heading (§20.2).
fn approval(run: &Run) -> Vec<String> {
    run.stdout()
        .lines()
        .skip_while(|line| line.trim() != "approval")
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn should_show_the_gate_apply_will_raise_with_the_rules_own_reason() {
    let home = home();
    if requires_a_service(home.path()).is_none() {
        skipped(
            SkipReason::FixtureNotApplicable,
            "this host serves no systemd unit, so there is no bulk restart to plan",
        );
        return;
    }
    let run = ono_at(home.path(), "get service | take 50 | plan restart service");
    run.assert_success();
    let asked = approval(&run).join("\n");
    assert!(
        asked.contains("--accept-risk") && asked.contains("risk.bulk.high-threshold"),
        "v0.6 §19.4 and §40.2: the view names the flag `apply` will demand and the rule that \
         demands it, rather than a generic class. Got {asked:?}"
    );
}

#[test]
fn should_ask_for_nothing_when_apply_raises_no_gate() {
    let home = home();
    let source = home.path().join("source.txt");
    let destination = home.path().join("destination.txt");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let run = ono_at(
        home.path(),
        &format!(
            "plan copy file {} {} --overwrite",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    assert_eq!(
        approval(&run),
        vec!["  none required".to_owned()],
        "v0.6 §40.1: an ordinary plan applies on `apply` alone. Got {:?}",
        run.output()
    );
}
