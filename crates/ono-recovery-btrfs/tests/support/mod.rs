#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    dead_code,
    reason = "a test states its preconditions directly (AGENTS.md section 16), and each test binary uses part of this module"
)]
//! The recorded output of real `btrfs-progs`, replayed into the provider.
//!
//! Every file in `tests/fixtures/` is the verbatim stdout and stderr of one command run against a
//! real Btrfs filesystem, with the command on the first line and `[exit <status>]` on the last.
//! [`fixture`] takes those two lines off and puts the rest where the tool put it: on standard
//! output for a command that succeeded, on standard error for one that failed, which is where
//! `btrfs` writes its `ERROR:` lines and where this crate's readers look for them.

use std::path::PathBuf;
use std::sync::Arc;

use ono_change_core::{
    ChangePlan, Intent, PlanId, RecoveryAsset, RecoveryAssetType, RecoveryExclusion, RecoveryScope,
    ScriptedRunner, ToolOutput,
};
use ono_recovery_btrfs::{BtrfsMounts, BtrfsProvider, RecordedFiles, SCOPE_KIND, SubvolumeRef};

/// The program the provider runs, and the key the scripted runner answers on.
pub const BTRFS: &str = "btrfs";

/// The filesystem the fixtures were recorded against.
pub const FILESYSTEM: &str = "1ba9ceee-e246-4007-aee5-a98f80e3e3c8";

/// The root subvolume of the recorded layout.
pub const ROOT_ID: u64 = 256;

/// The `@home` subvolume of the recorded layout.
pub const HOME_ID: u64 = 257;

/// The `@var` subvolume of the recorded layout.
pub const VAR_ID: u64 = 258;

/// The `@var/lib-app` subvolume nested inside `@var` — §14.3's whole point.
pub const NESTED_ID: u64 = 260;

/// The recorded snapshot of the root subvolume.
pub const ROOT_SNAPSHOT: &str = "/mnt/top/@snapshots/ono-a82f-root";

/// The recorded snapshot of `@var`.
pub const VAR_SNAPSHOT: &str = "/mnt/top/@snapshots/ono-a82f-var";

/// The configuration file the recorded `cat` output belongs to.
pub const NGINX_CONF: &str = "/mnt/root/etc/nginx/nginx.conf";

/// Reads one recorded command's output back as the tool produced it.
pub fn fixture(name: &str) -> ToolOutput {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.txt"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the fixture {} is readable: {error}", path.display()));
    let mut lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.first().is_some_and(|line| line.starts_with("$ ")),
        "every fixture opens with the command it recorded"
    );
    let status = lines
        .pop()
        .and_then(|line| line.strip_prefix("[exit "))
        .and_then(|line| line.strip_suffix(']'))
        .and_then(|code| code.parse::<i32>().ok())
        .expect("every fixture closes with its exit status");
    lines.remove(0);
    let body = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    if status == 0 {
        ToolOutput::ok(body)
    } else {
        ToolOutput::failed(status, body)
    }
}

/// The command a fixture recorded, for a test that wants to name it.
pub fn fixture_command(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.txt"));
    let text = std::fs::read_to_string(path).expect("the fixture is readable");
    text.lines()
        .next()
        .and_then(|line| line.strip_prefix("$ "))
        .unwrap_or_default()
        .to_owned()
}

/// The body of a recorded `cat`, which is the file's content.
pub fn content(name: &str) -> String {
    let output = fixture(name);
    output.stdout().to_owned()
}

/// A failed command that printed `text`.
pub fn failure(text: &str) -> ToolOutput {
    ToolOutput::failed(1, format!("{text}\n"))
}

/// A command that succeeded and printed nothing at all.
pub fn silence() -> ToolOutput {
    ToolOutput::ok("")
}

/// A runner that answers `outputs` in order, for the `btrfs` program only.
pub fn runner(outputs: Vec<ToolOutput>) -> Arc<ScriptedRunner> {
    Arc::new(
        ScriptedRunner::new(outputs.into_iter().map(|output| (BTRFS, output)).collect())
            .with_available(&[BTRFS]),
    )
}

/// The mount table the fixtures were recorded against (Appendix B.1).
pub fn mounts() -> BtrfsMounts {
    BtrfsMounts::from_mountinfo(fixture("mountinfo").stdout())
}

/// A provider over the recorded mounts, answering from `outputs`.
pub fn provider(outputs: Vec<ToolOutput>) -> BtrfsProvider {
    BtrfsProvider::new(runner(outputs)).with_mounts(mounts())
}

/// A provider over the recorded mounts and the two recorded file contents.
pub fn provider_with_files(outputs: Vec<ToolOutput>) -> BtrfsProvider {
    BtrfsProvider::new(runner(outputs))
        .with_mounts(mounts())
        .with_files(Arc::new(recorded_files()))
}

/// The recorded contents of the same file inside the snapshot and in the live subvolume.
///
/// `snapshot-file.txt` holds `worker_processes 4;` and `live-file.txt` holds
/// `worker_processes 8;` — the two sides of Appendix C.4's conflict, as a real `cat` printed them.
pub fn recorded_files() -> RecordedFiles {
    RecordedFiles::new()
        .holding(
            format!("{ROOT_SNAPSHOT}/etc/nginx/nginx.conf"),
            content("snapshot-file"),
        )
        .holding(NGINX_CONF, content("live-file"))
}

/// The plan the recorded snapshot names.
pub fn plan_id() -> PlanId {
    PlanId::of("session-1", "1970-01-01T00:00:00Z", "update nginx")
}

/// A sealed plan, for a recovery that names what it is recovering from.
pub fn source_plan() -> ChangePlan {
    ChangePlan::draft(
        Intent::new("update nginx", "update nginx"),
        "session-1",
        jiff::Timestamp::UNIX_EPOCH,
    )
}

/// The scope of a subvolume of the recorded filesystem (§11.2).
pub fn scope(id: u64, tree_path: &str, covers: &[&str]) -> RecoveryScope {
    let mut scope = RecoveryScope::new(
        SCOPE_KIND,
        SubvolumeRef::new(FILESYSTEM, id, tree_path).reference(),
        "localhost",
    );
    for object in covers {
        scope = scope.covering(*object);
    }
    scope
}

/// A ready-looking asset over the recorded root snapshot (§11.1).
pub fn root_asset() -> RecoveryAsset {
    RecoveryAsset::proposed(
        ono_recovery_btrfs::PROVIDER_ID,
        RecoveryAssetType::BtrfsSnapshot,
        ROOT_SNAPSHOT,
        scope(ROOT_ID, "@", &[NGINX_CONF]),
        jiff::Timestamp::UNIX_EPOCH,
    )
    .excluding(RecoveryExclusion::new(
        "the nested subvolume @home (257)",
        "§14.3",
    ))
    .excluding(RecoveryExclusion::new(
        "the nested subvolume @var (258)",
        "§14.3",
    ))
    .excluding(RecoveryExclusion::new(
        "the nested subvolume @var/lib-app (260)",
        "§14.3",
    ))
}

/// An asset over the recorded `@var` snapshot, covering `covers`.
pub fn var_asset(covers: &[&str]) -> RecoveryAsset {
    RecoveryAsset::proposed(
        ono_recovery_btrfs::PROVIDER_ID,
        RecoveryAssetType::BtrfsSnapshot,
        VAR_SNAPSHOT,
        scope(VAR_ID, "@var", covers),
        jiff::Timestamp::UNIX_EPOCH,
    )
    .excluding(RecoveryExclusion::new(
        "the nested subvolume @var/lib-app (260)",
        "§14.3",
    ))
}

/// The six answers `plan_recovery` asks for over the root subvolume, in the order it asks them.
///
/// A test that blocks one of §56.2's facts replaces exactly one of these, which is what makes the
/// §56.3 suite a set of single-variable experiments rather than six different scripts.
pub fn root_recovery_script() -> Vec<ToolOutput> {
    vec![
        fixture("subvol-show-snapshot"),
        fixture("snapshot-ro-flag"),
        fixture("fs-show-mount"),
        fixture("subvol-show-root"),
        fixture("subvol-list-root"),
        fixture("get-default"),
    ]
}

/// The same six over `@var`.
pub fn var_recovery_script() -> Vec<ToolOutput> {
    vec![
        var_snapshot_show(),
        fixture("snapshot-ro-flag"),
        fixture("fs-show-mount"),
        fixture("subvol-show-var"),
        fixture("subvol-list-root"),
        fixture("get-default"),
    ]
}

/// The recorded snapshot metadata, retold for the `@var` snapshot.
///
/// The recorded `subvol-show-snapshot.txt` is the snapshot of `@`; the fixtures record no
/// `subvolume show` of the `@var` snapshot, so its parent uuid is taken from the recorded
/// `subvol-list-after.txt`, which does list it with parent uuid
/// `d8f32c45-9c7c-5342-ae8c-b34f8a3010e7` — `@var`'s own uuid in `subvol-list-root.txt`.
pub fn var_snapshot_show() -> ToolOutput {
    ToolOutput::ok(
        fixture("subvol-show-snapshot")
            .stdout()
            .replace(
                "b08a487d-1079-ac45-9f7e-12d9f15bea63",
                "d8f32c45-9c7c-5342-ae8c-b34f8a3010e7",
            )
            .replace("ono-a82f-root", "ono-a82f-var"),
    )
}

/// Replaces the `index`-th answer of a script, for the §56.3 single-variable tests.
pub fn replacing(mut script: Vec<ToolOutput>, index: usize, output: ToolOutput) -> Vec<ToolOutput> {
    script[index] = output;
    script
}
