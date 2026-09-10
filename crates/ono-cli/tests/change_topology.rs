//! Impact walks the v0.4 topology (v0.6 §3.5, §9.3, §9.6).
//!
//! §9.3 makes the v0.4 relationships the primary topology source, and §9.6 makes the place where
//! the graph ends visible. A plan over an object the spatial layer knows therefore has one of two
//! honest answers: something beyond the direct target, or a boundary saying where Ono stopped
//! seeing. What it may not answer is "complete, and nothing else" for an object whose exits were
//! never read.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{home, one, ono_at, requires_a_service};
use ono_testkit::{SkipReason, skipped};

/// What a host that serves no systemd unit cannot present (v0.4.1 §38.1).
const NO_SERVICE: &str =
    "this host serves no systemd unit, so there is no service topology to walk";

/// A count out of `impact_summary`, where the plan states one.
fn count(summary: &serde_yaml_ng::Value, key: &str) -> u64 {
    summary[key].as_u64().unwrap_or(0)
}

#[test]
fn should_walk_past_the_direct_target_or_name_a_boundary_when_a_restart_is_planned() {
    let home = home();
    let Some(service) = requires_a_service(home.path()) else {
        skipped(SkipReason::FixtureNotApplicable, NO_SERVICE);
        return;
    };
    let run = ono_at(
        home.path(),
        &format!("plan restart service {service} | to json"),
    );
    run.assert_success();
    let plan = one(&run);
    let summary = &plan["impact_summary"];
    let beyond = count(summary, "dependents") + count(summary, "transitive");
    let boundaries = count(summary, "boundary_count");
    assert!(
        beyond > 0 || boundaries > 0,
        "v0.6 §9.3 and §9.6: `{service}` is a place in the v0.4 topology, so its impact either \
         reaches past the direct target or says where Ono stopped seeing — never `complete` with \
         nothing but the target. impact_summary: {summary:?}"
    );
}

/// The mount points strictly beneath `directory`, read from `/proc/self/mountinfo` (§32.3).
fn mounts_beneath(directory: &str) -> Vec<String> {
    let Ok(table) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return Vec::new();
    };
    let prefix = format!("{}/", directory.trim_end_matches('/'));
    table
        .lines()
        .filter_map(|line| line.split(' ').nth(4))
        .map(|point| point.replace("\\040", " "))
        .filter(|point| point.starts_with(&prefix))
        .collect()
}

#[test]
fn should_name_the_mounts_beneath_a_directory_and_its_size_when_a_recursive_removal_is_planned() {
    let home = home();
    let beneath = mounts_beneath("/dev");
    if beneath.is_empty() {
        skipped(
            SkipReason::FixtureNotApplicable,
            "nothing is mounted beneath /dev on this host, so there is no boundary to name",
        );
        return;
    }
    // Planning changes nothing (§2.1), which is why a directory the test could never remove is
    // the right fixture: it is the one every Linux host mounts filesystems beneath.
    let run = ono_at(home.path(), "plan remove dir /dev --recursive | to json");
    run.assert_success();
    let plan = one(&run);
    let labels: Vec<String> = plan["impact_summary"]["boundary_labels"]
        .as_sequence()
        .map(|labels| {
            labels
                .iter()
                .filter_map(|label| label.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        beneath.iter().any(|point| labels.contains(point)),
        "v0.6 §32.2 and §32.3: the filesystems mounted beneath the directory ({beneath:?}) are \
         recovery scopes of their own, and the plan names them before it is sealed. Boundary \
         labels: {labels:?}"
    );
    let findings = format!("{:?}", plan["risk_findings"]);
    assert!(
        findings.contains("§32.2"),
        "v0.6 §32.2: a recursive deletion's size is calculated before sealing and stated in the \
         plan's risk. Risk findings: {findings}"
    );
}
