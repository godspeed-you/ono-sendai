//! Replaying the recorded output of the real ZFS tools (§54.4, Appendix G.1).
//!
//! Every file under `tests/fixtures/` is the verbatim output of one command run against a real
//! OpenZFS 2.4.1 pool: the first line is the command, the last is `[exit <status>]`, and what lies
//! between is what the tool printed. This module turns one of those files into the
//! [`ToolOutput`] a `ScriptedRunner` replays, so the parsers, the boundary arithmetic and the
//! refusals in the deterministic suite are all exercised against bytes nobody wrote by hand.

#![allow(
    dead_code,
    reason = "one support module serves every test binary, and each uses part of it"
)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    PlanId, RecoveryAsset, RecoveryAssetType, RecoveryScope, ScriptedRunner, ToolOutput, ToolRunner,
};
use ono_recovery_zfs::{CP, GUID_FINGERPRINT, MountTable, ZFS, ZPOOL, ZfsProvider};

/// The instant every deterministic test stamps its work with.
///
/// Fixed rather than read from the clock, because a provider that names its snapshots after the
/// current second produces a different plan on every run and proves nothing twice.
pub fn instant() -> Timestamp {
    Timestamp::from_second(1_788_985_942).expect("a fixed instant inside the representable range")
}

/// The plan the deterministic suite attributes its protection to.
pub fn plan() -> PlanId {
    PlanId::of(
        "session-1",
        "1788985942",
        "set file /tank/data/customer/db.sqlite",
    )
}

/// The dataset holding the child-dataset target of §13.4.
pub const CHILD_DATASET: &str = "tank/data/customer";

/// The parent of that dataset, whose snapshot §13.4 says does not protect the child.
pub const PARENT_DATASET: &str = "tank/data";

/// The boot-environment dataset of §13.7, as the recorded layout has it.
pub const ROOT_DATASET: &str = "rpool/ROOT/debian";

/// The snapshot the recorded fixtures hold for the boot-environment dataset.
pub const ROOT_SNAPSHOT: &str = "rpool/ROOT/debian@ono-a82f-20260909T194500Z";

/// The GUID `zfs list -t snapshot` reports for it.
pub const ROOT_SNAPSHOT_GUID: &str = "10572816341271854659";

/// The snapshot one `zfs snapshot -r` made over `tank/data` and its child.
pub const RECURSIVE_SNAPSHOT: &str = "tank/data@ono-b91c-20260909T194501Z";

/// The child's half of that same recursive creation.
pub const RECURSIVE_CHILD_SNAPSHOT: &str = "tank/data/customer@ono-b91c-20260909T194501Z";

/// The newer snapshot that stands in the way of rolling the boot environment back.
pub const NEWER_SNAPSHOT: &str = "rpool/ROOT/debian@later-1";

/// The bookmark of that newer snapshot.
pub const NEWER_BOOKMARK: &str = "rpool/ROOT/debian#book-1";

/// The clone that depends on the recursive snapshot.
pub const CLONE: &str = "tank/cloned";

/// Reads one recorded command's output back into the value a runner replays.
///
/// The `$ <command>` line and the `[exit <status>]` line are the record's own framing; everything
/// else is what the tool printed. A non-zero status makes the text stderr, which is where ZFS
/// puts a refusal.
pub fn out(fixture: &str) -> ToolOutput {
    let path = format!(
        "{}/tests/fixtures/{fixture}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the recorded fixture {path} is readable: {error}"));
    let mut lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.first().is_some_and(|line| line.starts_with("$ ")),
        "{path} begins with the command it recorded"
    );
    lines.remove(0);
    let status = lines
        .pop()
        .and_then(|line| line.strip_prefix("[exit "))
        .and_then(|line| line.strip_suffix(']'))
        .and_then(|code| code.parse::<i32>().ok())
        .unwrap_or_else(|| panic!("{path} ends with the status the command exited"));
    let mut body = lines.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    if status == 0 {
        ToolOutput::ok(body)
    } else {
        ToolOutput::failed(status, body)
    }
}

/// The mount table the recorded `mountinfo.txt` describes (Appendix B.1, B.8).
pub fn mounts() -> MountTable {
    let recorded = out("mountinfo");
    MountTable::from_text(recorded.stdout())
}

/// One query in the fixed order [`ZfsProvider::survey`] and the recovery examination ask them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// `zfs list -t filesystem -o name,mountpoint,...`
    Filesystems,
    /// `zfs list -t snapshot -o name,creation,used,referenced,guid,defer_destroy`
    Snapshots,
    /// `zfs list -t snapshot -o name,creation -s creation`
    SnapshotOrder,
    /// `zfs list -t bookmark -o name,creation,guid`
    Bookmarks,
    /// `zfs list -t filesystem -o name,origin`
    Origins,
    /// `zpool list -H -p -o name,size,alloc,free,capacity,fragmentation,health`
    PoolList,
    /// `zpool status`
    PoolStatus,
    /// `zfs get clones <snapshot>`
    Clones,
    /// `zfs get written <dataset>`
    Written,
    /// `zfs get usedbysnapshots,usedbydataset <dataset>`
    Space,
    /// `zfs get mounted,mountpoint,canmount,readonly,origin,snapdir <dataset>`
    Placement,
}

/// A script of recorded answers, addressable by the question each one answers.
///
/// Naming the slots rather than counting positions is what lets a §56.3 test say "this one query
/// failed" without the rest of the suite depending on how many calls a survey happens to make.
#[derive(Debug, Clone)]
pub struct Script {
    entries: Vec<(Slot, &'static str, ToolOutput)>,
    extra: Vec<(&'static str, ToolOutput)>,
}

impl Script {
    /// The answers §13.1's discovery asks for.
    pub fn survey() -> Self {
        Self {
            entries: vec![
                (Slot::Filesystems, ZFS, out("list-filesystems")),
                (Slot::Snapshots, ZFS, out("list-snapshots")),
                (Slot::SnapshotOrder, ZFS, out("list-snapshots-sorted")),
                (Slot::Bookmarks, ZFS, out("list-bookmarks")),
                (Slot::Origins, ZFS, out("list-clones")),
                (Slot::PoolList, ZPOOL, out("zpool-list")),
                (Slot::PoolStatus, ZPOOL, out("zpool-status")),
            ],
            extra: Vec::new(),
        }
    }

    /// The answers a recovery examination asks for: the survey, then the snapshot's own facts.
    pub fn examination() -> Self {
        let mut script = Self::survey();
        script.entries.extend([
            (Slot::Clones, ZFS, out("get-clones")),
            (Slot::Written, ZFS, out("get-written")),
            (Slot::Space, ZFS, out("get-used-by-snapshots")),
            (Slot::Placement, ZFS, out("get-mounted")),
        ]);
        script
    }

    /// Answers `slot` with `output` instead of what was recorded.
    #[must_use]
    pub fn answering(mut self, slot: Slot, output: ToolOutput) -> Self {
        for entry in &mut self.entries {
            if entry.0 == slot {
                entry.2 = output.clone();
            }
        }
        self
    }

    /// Appends one more answer, for a command that comes after the survey.
    #[must_use]
    pub fn then(mut self, program: &'static str, output: ToolOutput) -> Self {
        self.extra.push((program, output));
        self
    }

    /// The runner that will replay it.
    #[must_use]
    pub fn runner(self) -> Arc<ScriptedRunner> {
        let mut responses: Vec<(&str, ToolOutput)> = self
            .entries
            .into_iter()
            .map(|(_, program, output)| (program, output))
            .collect();
        responses.extend(self.extra);
        Arc::new(ScriptedRunner::new(responses).with_available(&[ZFS, ZPOOL, CP]))
    }
}

/// A runner that answers exactly `responses`, with the ZFS tools reported present.
pub fn runner(responses: Vec<(&'static str, ToolOutput)>) -> Arc<ScriptedRunner> {
    Arc::new(ScriptedRunner::new(responses).with_available(&[ZFS, ZPOOL, CP]))
}

/// The provider the deterministic suite drives: recorded mounts, a fixed instant, a fixed plan.
pub fn provider(runner: &Arc<ScriptedRunner>) -> ZfsProvider {
    let tools: Arc<dyn ToolRunner> = Arc::clone(runner) as Arc<dyn ToolRunner>;
    ZfsProvider::new(tools)
        .reading_mounts(mounts())
        .at_instant(instant())
        .for_plan(plan())
        .on_host("localhost")
}

/// Every argument vector a runner was asked to run, flattened for a whole-suite assertion.
pub fn argument_vectors(runner: &Arc<ScriptedRunner>) -> Vec<Vec<String>> {
    runner.calls().into_iter().map(|(_, argv)| argv).collect()
}

/// The `error.name` of a refusal, which is what a script branches on (§45).
pub fn code(error: &ono_value::ErrorValue) -> String {
    error.code().name().to_owned()
}

/// The `facts` metadata a §56.3 refusal carries, as plain strings.
pub fn blocked_facts(error: &ono_value::ErrorValue) -> Vec<String> {
    match error.metadata().get("facts") {
        Some(ono_value::Value::List(items)) => items
            .iter()
            .map(|item| match item {
                ono_value::Value::String(text) => text.to_string(),
                other => other.to_string(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub fn asset() -> RecoveryAsset {
    RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        ROOT_SNAPSHOT,
        RecoveryScope::new("zfs-dataset", ROOT_DATASET, "localhost")
            .covering(ROOT_DATASET)
            .covering("/altroot/debian/etc/nginx/nginx.conf"),
        instant(),
    )
    .capturing(format!("{GUID_FINGERPRINT}{ROOT_SNAPSHOT_GUID}"))
}
