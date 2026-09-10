//! `get recovery` and `remove recovery --dry-run`, the two views of an asset's life
//! (spec v0.6 §37.3, §37.5, §55.5 case 25).
//!
//! §55.5 case 25 asks that `get recovery` show retention and scope: an operator deciding whether
//! an asset can go needs to know how long it is kept and what it would bring back. §37.3 asks that
//! a cleanup preview name the plans a removal would leave unrecoverable and remove nothing. Each
//! test applies a real protected change so a real asset exists, then asks the real binary.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use std::path::Path;

use change_support::{build_disk_home, one, ono_at, ono_with, rows, text};
use serde_yaml_ng::Value;

/// Applies a protected copy over `name` in `home`, answering with the plan's id.
fn apply_copy(home: &Path, name: &str) -> String {
    let source = home.join(format!("{name}.source"));
    let destination = home.join(format!("{name}.conf"));
    std::fs::write(&source, "changed\n").expect("the source is written");
    std::fs::write(&destination, "original\n").expect("the destination is written");
    let planned = ono_at(
        home,
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            destination.display()
        ),
    );
    planned.assert_success();
    let plan = text(&one(&planned), "id");
    ono_at(home, &format!("apply {}", &plan[..8])).assert_success();
    plan
}

/// Every asset `home` holds, as `get recovery | to json` lists them.
fn assets(home: &Path) -> Vec<Value> {
    let listed = ono_at(home, "get recovery | to json");
    listed.assert_success();
    rows(&listed)
}

/// One applied protected copy: `(plan id, the one asset it made)`.
fn applied_asset(home: &Path) -> (String, Value) {
    let plan = apply_copy(home, "app");
    let assets = assets(home);
    assert_eq!(
        assets.len(),
        1,
        "the fixture's one protected apply made one asset, got {assets:?}"
    );
    (plan, assets[0].clone())
}

#[test]
fn should_carry_retention_and_scope_in_every_listed_asset() {
    let home = build_disk_home("recovery-assets");
    let (_, asset) = applied_asset(home.path());

    let scope = asset
        .get("scope")
        .and_then(Value::as_mapping)
        .unwrap_or_else(|| {
            panic!("§11.1 and §55.5 case 25: an asset states its scope. Got {asset:?}")
        });
    assert!(
        scope
            .get("domain")
            .and_then(Value::as_str)
            .is_some_and(|domain| !domain.is_empty()),
        "§11.2: the scope names the domain the asset covers. Got {scope:?}"
    );
    assert!(
        asset.get("retention").is_some_and(|value| !value.is_null()),
        "§37.1 and §55.5 case 25: an asset states how long it is kept. Got {asset:?}"
    );
    assert!(
        asset.get("expires_at").and_then(Value::as_str).is_some(),
        "§37.5: a retained asset states when it expires. Got {asset:?}"
    );
}

#[test]
fn should_draw_expiry_in_the_list_and_scope_and_retention_for_one_asset() {
    let home = build_disk_home("recovery-assets");
    apply_copy(home.path(), "first");
    apply_copy(home.path(), "second");
    let listed = assets(home.path());
    assert_eq!(listed.len(), 2, "two protected applies made two assets");

    let list = ono_with(home.path(), "COLUMNS", "200", "get recovery");
    let first = text(&listed[0], "id");
    let single = ono_with(
        home.path(),
        "COLUMNS",
        "200",
        &format!("get recovery {}", &first[..8]),
    );

    list.assert_success();
    assert!(
        list.stdout().contains("EXPIRES"),
        "§37.5: the list draws each asset with its expiry. Got {:?}",
        list.stdout()
    );
    for asset in &listed {
        let id = text(asset, "id");
        assert!(
            list.stdout().contains(&id[..4]),
            "§37.5: the list draws every asset, and {id} is missing. Got {:?}",
            list.stdout()
        );
    }
    single.assert_success();
    for label in ["scope", "retained"] {
        assert!(
            single.stdout().contains(label),
            "§55.5 case 25: one asset is drawn with its {label}. Got {:?}",
            single.stdout()
        );
    }
}

#[test]
fn should_name_the_plan_a_removal_would_strand_and_remove_nothing_when_previewed() {
    let home = build_disk_home("recovery-assets");
    let (plan, asset) = applied_asset(home.path());
    let id = text(&asset, "id");

    let preview = ono_at(
        home.path(),
        &format!("remove recovery {} --dry-run", &id[..8]),
    );

    preview.assert_success();
    assert!(
        preview.output().contains(&plan[..4]),
        "§37.3: the preview names the plan the removal would leave unrecoverable. Got {:?}",
        preview.output()
    );
    let still = assets(home.path());
    assert!(
        still
            .iter()
            .any(|row| text(row, "id") == id && text(row, "state") == "ready"),
        "§37.3: a preview removes nothing, so the asset is still listed and still ready. Got {still:?}"
    );
    assert!(
        Path::new(&text(&asset, "reference")).exists(),
        "§37.3 and §2.15: the stored copy behind the asset is still on disk after a preview"
    );
}

#[test]
fn should_name_the_plan_a_blocked_removal_would_leave_unrecoverable() {
    let home = build_disk_home("recovery-assets");
    let source = home.path().join("checked.source");
    let checked = home.path().join("checked.conf");
    std::fs::write(&source, "changed\n").expect("the source is written");
    std::fs::write(&checked, "original\n").expect("the target is written");
    // §2.14 and §37.2: a plan whose required verification fails keeps its asset.
    let planned = ono_at(
        home.path(),
        &format!(
            "plan {{ copy file {} {} --overwrite; verify file {} size == 999 }} | to json",
            source.display(),
            checked.display(),
            checked.display()
        ),
    );
    planned.assert_success();
    let plan = text(&one(&planned), "id");
    let _ = ono_at(home.path(), &format!("apply {}", &plan[..8]));
    let asset = text(
        assets(home.path())
            .first()
            .expect("the failed plan's protection made an asset"),
        "id",
    );

    let refused = ono_at(
        home.path(),
        &format!("remove recovery {} --confirm", &asset[..8]),
    );

    assert!(
        !refused.status().is_success() && refused.stderr().contains("recovery.cleanup_blocked"),
        "§2.15: the asset a failed plan requires is refused without `--force`. Got {:?}",
        refused.output()
    );
    assert!(
        refused.stderr().contains(&plan[..4]),
        "§37.3: the refusal names the plan that would become unrecoverable. Got {:?}",
        refused.stderr()
    );
}
