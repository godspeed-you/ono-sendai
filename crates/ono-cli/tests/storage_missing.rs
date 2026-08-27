//! Outcome tests for the storage family the contract declares but this build does not deliver:
//! `get device`, `mount filesystem`, `unmount filesystem`, `set`/`add`/`remove`/`start`/`stop
//! mount`, `watch mount`, `trace mount` and `enter mount`.
//!
//! Contract: `docs/spec/commands/storage.yaml`, schemas `ono.mount/1`, `ono.action-result/1`,
//! `ono.graph/1`, `ono.context/1`, and the deferred `ono.device/1`. Narrative: spec §9.1 (the
//! storage table), §8.1 (`device` is a system target), §14.1/§14.3 (object context, implicit
//! selector; ADR-0023), §16.5 (a failure is a `failed` row, never a collapsed boolean; ADR-0006
//! makes it exit 1), §18.2 (native live streams begin with a snapshot, ADR-0024/ADR-0034), §22.1
//! and §22.3 (`trace` is a graph of exact relationships), §23.5 (mount data from
//! `/proc/self/mountinfo`, options kept as structure), §28.6 (the Mount record).
//!
//! Every test runs unprivileged and offline. Where the kernel needs CAP_SYS_ADMIN, the observable
//! behaviour is still fixed: the command attempts the mutation and reports one
//! `ono.action-result/1` row with `status: failed` and `Ono-Sendai-E0302` (io.permission_denied),
//! exit status 1 — never E0101/E0102 "not implemented". Nothing here knows how a stage is wired
//! (AGENTS.md §11).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_testkit::{Shell, scratch};
use serde_yaml_ng::Value;

const PERMISSION_DENIED: &str = "Ono-Sendai-E0302";
const NOT_FOUND: &str = "Ono-Sendai-E0301";
const PROVIDER_UNAVAILABLE: &str = "Ono-Sendai-E0401";

/// Runs a one-liner and returns the finished run.
fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

/// Mutations that root would actually perform are only attempted unprivileged: a test that
/// really mounted or unmounted something on the developer's machine would be the defect.
fn unprivileged() -> bool {
    if ono_process::effective_uid() == 0 {
        eprintln!("skipped: this test asserts the unprivileged refusal and would mutate as root");
        return false;
    }
    true
}

/// Parses the JSON document `to json` wrote as the stream's values.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let text = run.stdout().trim().to_owned();
    let document: Value = serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!(
            "`to json` must emit a JSON document, got stdout {text:?}, stderr {:?}: {error}",
            run.stderr()
        )
    });
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!(
                "spec §33.5: `to json` emits the stream as an array, got stdout {text:?}, stderr {:?}",
                run.stderr()
            )
        })
        .clone()
}

/// The one `ono.action-result/1` row a single-target mutation emits.
fn single_result(run: &ono_testkit::Run) -> Value {
    let mut rows = rows(run);
    assert_eq!(
        rows.len(),
        1,
        "spec §11.5: one ActionResult per target, got {:?}",
        run.stdout()
    );
    rows.remove(0)
}

fn text(row: &Value, field: &str) -> String {
    row[field]
        .as_str()
        .unwrap_or_else(|| panic!("field `{field}` must be a string, got {row:?}"))
        .to_owned()
}

fn error_code(row: &Value) -> String {
    row["error"]["code"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("spec §43: a failed row carries a structured `error.code`, got {row:?}")
        })
        .to_owned()
}

/// A mutation that was attempted and refused: exit 1 (§16.5, ADR-0006), one `failed` row that
/// changed nothing and names its error — never E0101/E0102 on stderr.
fn assert_refused(run: &ono_testkit::Run, operation: &str, codes: &[&str]) -> Value {
    assert!(
        !run.stderr().contains("Ono-Sendai-E0101") && !run.stderr().contains("Ono-Sendai-E0102"),
        "`{operation}` is a delivered command, not a declaration; got stderr {:?}",
        run.stderr()
    );
    assert!(
        !run.stderr().contains("cannot be a pipeline stage"),
        "`{operation}` is a native mutation whose ActionResult flows on to `to json`, got stderr {:?}",
        run.stderr()
    );
    run.assert_status(1);
    let row = single_result(run);
    assert_eq!(
        text(&row, "operation"),
        operation,
        "spec §11.5: `operation` is the command id, got {row:?}"
    );
    assert_eq!(
        text(&row, "status"),
        "failed",
        "spec §16.5: the refusal is a `failed` row, not text on stderr, got {row:?}"
    );
    assert_eq!(
        row["changed"].as_bool(),
        Some(false),
        "a refused mutation changed nothing, got {row:?}"
    );
    let code = error_code(&row);
    assert!(
        codes.contains(&code.as_str()),
        "errors.yaml: the refusal carries one of {codes:?}, got {code} in {row:?}"
    );
    row
}

fn mountinfo() -> String {
    std::fs::read_to_string("/proc/self/mountinfo").expect("/proc/self/mountinfo is readable")
}

fn is_mounted(path: &std::path::Path) -> bool {
    let target = path.display().to_string();
    mountinfo()
        .lines()
        .filter_map(|line| line.split_whitespace().nth(4))
        .any(|mount_point| mount_point == target)
}

fn integer(row: &Value, field: &str) -> i64 {
    row[field]
        .as_i64()
        .unwrap_or_else(|| panic!("field `{field}` must be an integer, got {row:?}"))
}

// --- get device -------------------------------------------------------------------------------
//
// `ono.device/1` is deferred (deferred.yaml), so the fields asserted are the ones spec §9.1 and
// §28.6 imply for a device a mount can reference: the node under `/dev`, whether it is a block
// or a character device, and the `major`/`minor` numbers the kernel identifies it by — the
// numbers `ono.mount/1.device` already renders as `major:minor`. Block devices carry a `size`
// (block-device.v1.yaml); character devices have none.

#[test]
fn should_list_devices_with_their_node_kind_and_numbers() {
    let run = ono("get device | to json");
    run.assert_success();
    let devices = rows(&run);
    assert!(
        !devices.is_empty(),
        "spec §9.1: `get device` enumerates the devices under /dev; /dev/null always exists"
    );
    for device in &devices {
        assert!(
            text(device, "path").starts_with("/dev/"),
            "spec §9.1: a device record names its node under /dev, got {device:?}"
        );
        let kind = text(device, "kind");
        assert!(
            kind == "block" || kind == "char",
            "spec §9.1: a device is a block or a character device, got {device:?}"
        );
        assert!(
            integer(device, "major") >= 0 && integer(device, "minor") >= 0,
            "a device carries the kernel's major/minor numbers, got {device:?}"
        );
    }
}

#[test]
fn should_describe_dev_null_as_character_device_one_three() {
    // The one device every Linux system has, with numbers fixed by Documentation/admin-guide/
    // devices.txt — so the assertion is deterministic on a host and in an empty container alike.
    // The contract declares no selector for `get device`, so the record is picked with `where`.
    let run = ono(r#"get device | where path == "/dev/null" | to json"#);
    run.assert_success();
    let devices = rows(&run);
    assert_eq!(
        devices.len(),
        1,
        "exactly one device record answers to /dev/null, got {devices:?}"
    );
    let null = &devices[0];
    assert_eq!(
        text(null, "kind"),
        "char",
        "/dev/null is a character device, got {null:?}"
    );
    assert_eq!(
        (integer(null, "major"), integer(null, "minor")),
        (1, 3),
        "/dev/null is major 1, minor 3, got {null:?}"
    );
}

#[test]
fn should_restrict_to_character_devices_when_kind_char_is_given() {
    let run = ono("get device --kind char | to json");
    run.assert_success();
    let devices = rows(&run);
    assert!(
        !devices.is_empty(),
        "storage.yaml `--kind`: the character devices include /dev/null, got none"
    );
    assert!(
        devices.iter().all(|device| text(device, "kind") == "char"),
        "storage.yaml `--kind char` restricts to character devices, got {devices:?}"
    );
}

#[test]
fn should_carry_a_size_for_every_block_device() {
    let run = ono("get device --kind block | to json");
    run.assert_success();
    // A container may expose no block device at all; the contract is then vacuously kept.
    for device in rows(&run) {
        assert_eq!(
            text(&device, "kind"),
            "block",
            "storage.yaml `--kind block` restricts to block devices, got {device:?}"
        );
        assert!(
            device["size"].as_i64().is_some_and(|size| size >= 0),
            "block-device.v1: a block device has a size in bytes, got {device:?}"
        );
    }
}

// --- mount filesystem -------------------------------------------------------------------------

#[test]
fn should_report_a_failed_row_and_leave_the_target_unmounted_when_mounting_unprivileged() {
    if !unprivileged() {
        return;
    }
    let directory = scratch();
    let target = directory.path().join("mnt");
    std::fs::create_dir(&target).expect("the scratch mount point");

    let run = ono(&format!(
        "mount filesystem tmpfs {} --type tmpfs | to json",
        target.display()
    ));
    let row = assert_refused(&run, "ono.filesystem.mount", &[PERMISSION_DENIED]);
    assert_eq!(
        error_code(&row),
        PERMISSION_DENIED,
        "mount(2) needs CAP_SYS_ADMIN; the refusal is io.permission_denied, got {row:?}"
    );
    assert!(
        !is_mounted(&target),
        "a refused mount leaves nothing mounted at {}",
        target.display()
    );
    assert!(
        std::fs::read_dir(&target)
            .expect("the scratch mount point")
            .next()
            .is_none(),
        "a refused mount leaves the mount point untouched"
    );
}

#[test]
fn should_accept_one_mount_option_per_occurrence_when_mounting() {
    // Spec §23.5: options are structure, one `--option` each — never a joined string. The
    // attempt is still refused unprivileged, which is how an accepted option list is observable.
    if !unprivileged() {
        return;
    }
    let directory = scratch();
    let target = directory.path().join("mnt");
    std::fs::create_dir(&target).expect("the scratch mount point");

    let run = ono(&format!(
        "mount filesystem tmpfs {} --type tmpfs --option size=1m --option mode=0700 --read-only | to json",
        target.display()
    ));
    assert!(
        !run.stderr().contains("Ono-Sendai-E02"),
        "storage.yaml: `--option` is repeatable and `--read-only` is a flag, got {:?}",
        run.stderr()
    );
    assert_refused(&run, "ono.filesystem.mount", &[PERMISSION_DENIED]);
    assert!(!is_mounted(&target), "nothing was mounted unprivileged");
}

// --- unmount filesystem -----------------------------------------------------------------------

#[test]
fn should_report_a_failed_row_when_unmounting_the_root_filesystem_unprivileged() {
    if !unprivileged() {
        return;
    }
    let run = ono("unmount filesystem / | to json");
    let row = assert_refused(&run, "ono.filesystem.unmount", &[PERMISSION_DENIED]);
    // storage.yaml declares no confirmation for `unmount filesystem`, so the only thing standing
    // between an unprivileged user and the root mount is the kernel's EPERM. A resolved
    // selector's row may carry the label the provider knows the mount by after the identity
    // (ADR-0088 §4); the identity is what names the mount.
    assert!(
        text(&row, "target").starts_with("ono.mount/1[/]"),
        "spec §11.5 / ADR-0068 §2: the row names the mount it acted on by its identity, got {row:?}"
    );
    assert!(
        is_mounted(std::path::Path::new("/")),
        "the root mount is still there"
    );
}

#[test]
fn should_report_not_found_when_unmounting_a_path_that_is_no_mount_point() {
    // A directory that is not a mount point is decidable from /proc/self/mountinfo (§23.5)
    // before any privileged call: there is no mount at that path, so the error is io.not_found
    // (E0301) — the resource `unmount` acts on is the mount, and it does not exist. E0304
    // (io.not_directory) would misdescribe a directory, and E0302 would blame privilege for a
    // request that could not succeed as root either.
    let directory = scratch();
    let run = ono(&format!(
        "unmount filesystem {} | to json",
        directory.path().display()
    ));
    assert_refused(&run, "ono.filesystem.unmount", &[NOT_FOUND]);
}

#[test]
fn should_unmount_the_mounts_piped_in_from_get_mount() {
    // storage.yaml: input `null | stream<ono.mount/1>`. The piped mount is the target — and the
    // attempt on `/` is refused unprivileged, which is how the piped target is observable.
    if !unprivileged() {
        return;
    }
    let run = ono("get mount / | unmount filesystem | to json");
    let row = assert_refused(&run, "ono.filesystem.unmount", &[PERMISSION_DENIED]);
    assert_eq!(
        text(&row, "target"),
        "ono.mount/1[/]",
        "the piped `ono.mount/1` record is the target (ADR-0068 §2 form), got {row:?}"
    );
}

// --- set / add / remove / start / stop mount (phase: planned, still delivered as behaviour) ----

#[test]
fn should_report_a_failed_row_when_remounting_unprivileged() {
    if !unprivileged() {
        return;
    }
    let before = ono("get mount / | select read_only | to json");
    before.assert_success();

    let run = ono("set mount / --read-only | to json");
    assert_refused(&run, "ono.mount.set", &[PERMISSION_DENIED]);

    let after = ono("get mount / | select read_only | to json");
    after.assert_success();
    assert_eq!(
        after.stdout(),
        before.stdout(),
        "a refused remount leaves the root mount's options as they were"
    );
}

#[test]
fn should_report_a_failed_row_when_adding_a_persistent_mount_unprivileged() {
    if !unprivileged() {
        return;
    }
    let fstab_before = std::fs::read_to_string("/etc/fstab").ok();
    let directory = scratch();
    let run = ono(&format!(
        "add mount tmpfs {} --type tmpfs --option size=1m | to json",
        directory.path().display()
    ));
    // The persistent definition lives in a root-owned file (/etc/fstab or a systemd mount unit);
    // writing it unprivileged is io.permission_denied.
    assert_refused(&run, "ono.mount.add", &[PERMISSION_DENIED]);
    assert_eq!(
        std::fs::read_to_string("/etc/fstab").ok(),
        fstab_before,
        "a refused definition changes nothing on disk"
    );
    assert!(
        !is_mounted(directory.path()),
        "adding a definition mounts nothing"
    );
}

#[test]
fn should_report_a_failed_row_when_removing_a_persistent_mount_unprivileged() {
    if !unprivileged() {
        return;
    }
    let run = ono("remove mount / | to json");
    // Either the definition exists and the file holding it is root-owned (E0302), or this
    // system carries no definition for `/` at all (E0301); both are structured refusals.
    assert_refused(&run, "ono.mount.remove", &[PERMISSION_DENIED, NOT_FOUND]);
    assert!(
        is_mounted(std::path::Path::new("/")),
        "the root mount is still there"
    );
}

#[test]
fn should_report_a_failed_row_when_starting_a_mount_unprivileged() {
    if !unprivileged() {
        return;
    }
    let run = ono("start mount / | to json");
    // Activation goes through the kernel or the service manager: privilege denied on a host,
    // provider.unavailable where no systemd answers (a container).
    assert_refused(
        &run,
        "ono.mount.start",
        &[PERMISSION_DENIED, PROVIDER_UNAVAILABLE],
    );
}

#[test]
fn should_report_a_failed_row_when_stopping_a_mount_unprivileged() {
    if !unprivileged() {
        return;
    }
    let run = ono("stop mount / | to json");
    assert_refused(
        &run,
        "ono.mount.stop",
        &[PERMISSION_DENIED, PROVIDER_UNAVAILABLE],
    );
    assert!(
        is_mounted(std::path::Path::new("/")),
        "the root mount is still there"
    );
}

// --- watch mount ------------------------------------------------------------------------------

#[test]
fn should_begin_watching_mounts_with_a_snapshot() {
    let run = ono("watch mount | take 1 | select kind | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"kind":"snapshot"}]"#,
        "spec §18.2 / ADR-0024: a live stream begins with the current state as snapshot events"
    );
}

#[test]
fn should_include_the_root_mount_in_the_first_snapshot() {
    let run =
        ono(r#"watch mount | where mount.target == "/" | take 1 | select kind mount | to json"#);
    run.assert_success();
    let events = rows(&run);
    assert_eq!(
        events.len(),
        1,
        "the root mount is in the snapshot, got {events:?}"
    );
    let event = &events[0];
    assert_eq!(
        text(event, "kind"),
        "snapshot",
        "ADR-0024: the root mount arrives as a snapshot before any change, got {event:?}"
    );
    assert_eq!(
        text(&event["mount"], "target"),
        "/",
        "the event carries the `ono.mount/1` record it is about, got {event:?}"
    );
    assert!(
        event["mount"]["options"].is_sequence(),
        "spec §23.5: the options are a list, not a joined string, got {event:?}"
    );
}

// --- trace mount ------------------------------------------------------------------------------

fn graph(run: &ono_testkit::Run) -> Value {
    let mut graphs = rows(run);
    assert_eq!(
        graphs.len(),
        1,
        "storage.yaml: `trace mount` yields one `ono.graph/1`, got {:?}",
        run.stdout()
    );
    graphs.remove(0)
}

fn node_kind(node: &Value) -> String {
    text(node, "kind")
}

fn is_root_mount_node(node: &Value) -> bool {
    node_kind(node) == "ono.mount/1"
        && (node["id"]["identity"]["target"].as_str() == Some("/")
            || node["value"]["target"].as_str() == Some("/"))
}

#[test]
fn should_trace_a_mount_to_what_it_sits_on_and_who_uses_it() {
    let run = ono("trace mount / | to json");
    run.assert_success();
    let graph = graph(&run);
    let nodes = graph["nodes"]
        .as_sequence()
        .unwrap_or_else(|| panic!("graph.v1: `nodes` is a list, got {graph:?}"))
        .clone();
    let edges = graph["edges"]
        .as_sequence()
        .unwrap_or_else(|| panic!("graph.v1: `edges` is a list, got {graph:?}"))
        .clone();

    assert!(
        nodes.iter().any(is_root_mount_node),
        "spec §22.1: the traced mount `/` is a node of kind ono.mount/1, got {nodes:?}"
    );
    assert!(
        nodes.iter().any(|node| {
            let kind = node_kind(node);
            kind == "ono.device/1" || kind == "ono.filesystem/1"
        }),
        "storage.yaml: the graph shows the mount's device or filesystem (its source), got {nodes:?}"
    );
    assert!(
        nodes.iter().any(|node| node_kind(node) == "ono.process/1"),
        "storage.yaml: the graph shows the processes using the mount — at least the shell itself \
         has `/` as its root, got {nodes:?}"
    );
    assert!(
        edges.iter().any(|edge| {
            edge["from"]["schema"].as_str() == Some("ono.mount/1")
                || edge["to"]["schema"].as_str() == Some("ono.mount/1")
        }),
        "spec §22.1: every relationship is an edge touching the mount, got {edges:?}"
    );
    assert!(
        edges
            .iter()
            .all(|edge| matches!(edge["confidence"].as_str(), Some("exact" | "inferred"))),
        "spec §22.2: every edge says whether it is exact or inferred, got {edges:?}"
    );
}

#[test]
fn should_report_not_found_when_tracing_a_path_that_is_no_mount_point() {
    let run = ono("trace mount /definitely/not/a/mount | to json");
    assert!(
        !run.status().is_success(),
        "tracing a mount that does not exist fails, got {:?}",
        run.output()
    );
    assert!(
        !run.stderr().contains("Ono-Sendai-E0101"),
        "`trace mount` is a delivered command, not a declaration; got {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0102") || run.stderr().contains(NOT_FOUND),
        "spec §43: nothing answers to that mount point — a structured not-found (E0102, as \
         `trace process` spells it, or E0301), got {:?}",
        run.stderr()
    );
    assert_eq!(
        run.stdout().trim(),
        "",
        "no graph is emitted for a mount that does not exist"
    );
}

// --- enter mount ------------------------------------------------------------------------------

#[test]
fn should_push_an_object_frame_when_entering_a_mount() {
    let run = ono("enter mount /; get context | to json");
    run.assert_success();
    assert!(
        !run.stderr().contains("Ono-Sendai-E"),
        "`enter mount` is delivered, got {:?}",
        run.stderr()
    );
    let frames = rows(&run);
    assert_eq!(
        frames.len(),
        2,
        "spec §14.1: `enter` pushes one frame on the ground frame, got {frames:?}"
    );
    let top = &frames[1];
    // A mount is a navigable object (spec §14.3), not a working directory: storage.yaml notes
    // that `enter dir <mount point>` is the filesystem-frame spelling of nearly the same thing.
    assert_eq!(
        text(top, "kind"),
        "object",
        "context.v1: a mount frame is an object frame, got {top:?}"
    );
    assert_eq!(
        text(top, "target"),
        "mount",
        "context.v1: the frame narrows to the `mount` target, got {top:?}"
    );
    assert_eq!(
        text(top, "identity"),
        "/",
        "context.v1: the frame carries the mount point as its identity, got {top:?}"
    );
}

#[test]
fn should_keep_the_working_directory_when_entering_a_mount() {
    let directory = scratch();
    let run = Shell::new()
        .cwd(directory.path())
        .args(["-c", "enter mount /; pwd"])
        .timeout(Duration::from_secs(30))
        .run();
    run.assert_success();
    assert!(
        !run.stderr().contains("Ono-Sendai-E"),
        "`enter mount /` succeeds before `pwd` is asked, got {:?}",
        run.stderr()
    );
    assert_eq!(
        run.stdout().trim(),
        directory.path().display().to_string(),
        "storage.yaml: `enter mount` narrows queries; only `enter dir` moves the session"
    );
}

#[test]
fn should_narrow_get_mount_to_the_entered_mount() {
    // Spec §14.3 / ADR-0023: the frame contributes an implicit selector, so `get mount` inside
    // `enter mount /` is `get mount /` — one record, the mount that was entered.
    let run = ono("enter mount /; get mount | select target | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"target":"/"}]"#,
        "spec §14.3: the mount frame is the implicit selector of `get mount`"
    );
}

#[test]
fn should_pop_the_mount_frame_when_leaving() {
    let run =
        ono("enter mount /; get context | count | to json; leave; get context | count | to json");
    run.assert_success();
    let depths: Vec<&str> = run.stdout().lines().map(str::trim).collect();
    assert_eq!(
        depths,
        vec!["[2]", "[1]"],
        "spec §14.1: `enter mount` pushes a frame and `leave` pops it, got {:?}",
        run.output()
    );
}
