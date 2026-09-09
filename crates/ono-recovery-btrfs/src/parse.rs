//! Readers for what `btrfs-progs` actually prints (spec v0.6 §14.1, Appendix B.9).
//!
//! Appendix B.9 is the reason this module exists rather than a set of path heuristics: *"The
//! Btrfs provider MUST resolve subvolume/root IDs and nested subvolume boundaries. A subdirectory
//! named like a subvolume is not sufficient evidence."* Every identity in this crate therefore
//! comes out of one of these readers, and every reader is fed the recorded output of a real
//! `btrfs-progs` run in the ordinary test suite.
//!
//! Two shapes of output need different handling and are easy to confuse:
//!
//! - `btrfs subvolume show` prints a header line and then `Key:<tab>value` pairs, so it is parsed
//!   **by key** rather than by line number. New keys appear between releases (Appendix G.4), and
//!   a positional reader would silently take the wrong field the first time one did.
//! - `btrfs subvolume list` prints one flat line per subvolume whose fields depend on the flags
//!   given. It is parsed as a token stream keyed by the word before the value, so `-s`'s extra
//!   `cgen` and `otime` columns do not shift anything.
//!
//! A failing `btrfs` invocation is a fact rather than an accident, and [`ShowOutcome`] keeps the
//! three cases apart: a path that is a plain directory, a path that does not exist, and a refusal
//! (an unprivileged search, a broken filesystem). §56.3 turns the third into a block, and
//! Appendix B.9's trap depends on the first being distinguishable from the second.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::ToolOutput;
use ono_value::{ByteSize, ErrorValue};

use crate::error::unreadable_output;

/// The stream a `btrfs` invocation said what it had to say on.
///
/// `btrfs-progs` writes its `ERROR:` lines to standard error and its data to standard output, and
/// a reader that looked at only one of them would read an empty string as an empty answer — which
/// §56.3 makes the one interpretation that must never happen silently.
#[must_use]
pub fn spoken_text(output: &ToolOutput) -> &str {
    if output.succeeded() || output.stderr().is_empty() {
        output.stdout()
    } else {
        output.stderr()
    }
}

/// What `btrfs subvolume show` said about a path (§14.1, Appendix B.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShowOutcome {
    /// The path is a subvolume, and this is its metadata.
    Subvolume(Box<SubvolumeShow>),
    /// The path exists and is an ordinary directory inside some other subvolume.
    ///
    /// This is Appendix B.9's trap answered by the filesystem: `looks-like-a-subvol` is a
    /// directory whose name resembles a subvolume, and `btrfs` says `ERROR: Not a Btrfs
    /// subvolume` for it.
    PlainDirectory,
    /// The path does not exist.
    Missing,
    /// The question could not be answered — no privilege, no filesystem, a tool error.
    Refused(Arc<str>),
}

/// The metadata `btrfs subvolume show` prints for one subvolume (§14.1, Appendix D.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubvolumeShow {
    tree_path: Arc<str>,
    name: Arc<str>,
    uuid: Arc<str>,
    parent_uuid: Option<Arc<str>>,
    received_uuid: Option<Arc<str>>,
    id: u64,
    parent_id: Option<u64>,
    top_level_id: Option<u64>,
    generation: Option<u64>,
    created_at: Option<Timestamp>,
    read_only: bool,
}

impl SubvolumeShow {
    /// The subvolume's path inside the filesystem tree, such as `@var/lib-app`.
    #[must_use]
    pub fn tree_path(&self) -> &str {
        &self.tree_path
    }

    /// The subvolume's own name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The subvolume UUID, which is what ties a snapshot to what it was taken from.
    #[must_use]
    pub fn uuid(&self) -> &str {
        &self.uuid
    }

    /// The UUID of the subvolume this one was snapshotted from, where there is one (§14.2).
    #[must_use]
    pub fn parent_uuid(&self) -> Option<&str> {
        self.parent_uuid.as_deref()
    }

    /// The received UUID, set on a subvolume that arrived by `btrfs send`.
    #[must_use]
    pub fn received_uuid(&self) -> Option<&str> {
        self.received_uuid.as_deref()
    }

    /// The stable subvolume ID §14.1 requires the provider to resolve.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// The ID of the subvolume this one lives inside, which is §14.3's nesting relation.
    #[must_use]
    pub const fn parent_id(&self) -> Option<u64> {
        self.parent_id
    }

    /// The top level ID.
    #[must_use]
    pub const fn top_level_id(&self) -> Option<u64> {
        self.top_level_id
    }

    /// The generation.
    #[must_use]
    pub const fn generation(&self) -> Option<u64> {
        self.generation
    }

    /// When the subvolume was created, as the filesystem recorded it.
    ///
    /// Appendix D.7 asks each member of a multi-subvolume set to carry its own creation instant.
    /// Taking it from the filesystem rather than from a clock keeps that instant a fact about the
    /// snapshot instead of a fact about when Ono got round to asking.
    #[must_use]
    pub const fn created_at(&self) -> Option<Timestamp> {
        self.created_at
    }

    /// Whether the `readonly` flag is set (§14.5).
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }
}

/// Reads one `btrfs subvolume show` invocation (§14.1).
///
/// # Errors
///
/// [`crate::error::unreadable_output`] when the command succeeded and the text it printed does not
/// carry a subvolume ID. A successful command whose output cannot be read is worse than a failed
/// one, because only the first can be mistaken for an answer.
pub fn read_subvolume_show(output: &ToolOutput) -> Result<ShowOutcome, ErrorValue> {
    let text = spoken_text(output);
    if !output.succeeded() {
        return Ok(classify_error(text));
    }
    Ok(ShowOutcome::Subvolume(Box::new(parse_subvolume_show(
        text,
    )?)))
}

/// Classifies a failed `btrfs` invocation by what it said (§56.3).
fn classify_error(text: &str) -> ShowOutcome {
    if text.contains("Not a Btrfs subvolume") {
        ShowOutcome::PlainDirectory
    } else if text.contains("cannot find real path") || text.contains("No such file or directory") {
        ShowOutcome::Missing
    } else {
        ShowOutcome::Refused(Arc::from(first_line(text)))
    }
}

/// The first non-empty line of `text`, which is where `btrfs` puts its diagnosis.
fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("the command failed and said nothing")
}

/// Parses the key-indented block `btrfs subvolume show` prints (§14.1).
///
/// # Errors
///
/// [`crate::error::unreadable_output`] when no `Subvolume ID` key is present, since without it
/// there is no stable identity and §14.1 asks for exactly that.
pub fn parse_subvolume_show(text: &str) -> Result<SubvolumeShow, ErrorValue> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next().unwrap_or_default().trim().to_owned();
    let mut show = SubvolumeShow {
        tree_path: Arc::from(header.trim_start_matches('/')),
        name: Arc::from(""),
        uuid: Arc::from(""),
        parent_uuid: None,
        received_uuid: None,
        id: 0,
        parent_id: None,
        top_level_id: None,
        generation: None,
        created_at: None,
        read_only: false,
    };
    let mut seen_id = false;
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "Name" => show.name = Arc::from(value),
            "UUID" => show.uuid = Arc::from(value),
            "Parent UUID" => show.parent_uuid = optional(value),
            "Received UUID" => show.received_uuid = optional(value),
            "Subvolume ID" => {
                show.id = value
                    .parse()
                    .map_err(|_| unreadable_output("btrfs subvolume show", line))?;
                seen_id = true;
            }
            "Parent ID" => show.parent_id = value.parse().ok(),
            "Top level ID" => show.top_level_id = value.parse().ok(),
            "Generation" => show.generation = value.parse().ok(),
            "Flags" => show.read_only = value.contains("readonly"),
            // The value itself contains colons, so it is recovered from the whole line.
            "Creation time" => show.created_at = parse_instant(after_first_colon(line)),
            _ => {}
        }
    }
    if !seen_id {
        return Err(unreadable_output(
            "btrfs subvolume show",
            "the output carries no `Subvolume ID`, so no stable subvolume identity was \
             established (§14.1)",
        ));
    }
    Ok(show)
}

/// `btrfs` spells "there is none" as a bare dash.
fn optional(value: &str) -> Option<Arc<str>> {
    if value.is_empty() || value == "-" {
        None
    } else {
        Some(Arc::from(value))
    }
}

/// The part of `line` after its first colon, for values that contain colons themselves.
fn after_first_colon(line: &str) -> &str {
    line.split_once(':').map_or("", |(_, rest)| rest.trim())
}

/// Reads the timestamps `btrfs` prints, which are local civil time plus an offset.
fn parse_instant(text: &str) -> Option<Timestamp> {
    Timestamp::strptime("%Y-%m-%d %H:%M:%S %z", text)
        .ok()
        .or_else(|| Timestamp::strptime("%Y-%m-%d %H:%M:%S", text).ok())
}

/// One line of `btrfs subvolume list` (§14.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubvolumeEntry {
    id: u64,
    generation: Option<u64>,
    parent_id: Option<u64>,
    top_level: Option<u64>,
    uuid: Option<Arc<str>>,
    parent_uuid: Option<Arc<str>>,
    received_uuid: Option<Arc<str>>,
    tree_path: Arc<str>,
}

impl SubvolumeEntry {
    /// The stable subvolume ID (§14.1).
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// The generation, where the flags asked for it.
    #[must_use]
    pub const fn generation(&self) -> Option<u64> {
        self.generation
    }

    /// The ID of the subvolume this one is nested inside (§14.3).
    #[must_use]
    pub const fn parent_id(&self) -> Option<u64> {
        self.parent_id
    }

    /// The top level ID.
    #[must_use]
    pub const fn top_level(&self) -> Option<u64> {
        self.top_level
    }

    /// The subvolume UUID.
    #[must_use]
    pub fn uuid(&self) -> Option<&str> {
        self.uuid.as_deref()
    }

    /// The UUID this subvolume was snapshotted from (§14.2).
    #[must_use]
    pub fn parent_uuid(&self) -> Option<&str> {
        self.parent_uuid.as_deref()
    }

    /// The received UUID.
    #[must_use]
    pub fn received_uuid(&self) -> Option<&str> {
        self.received_uuid.as_deref()
    }

    /// The path inside the filesystem tree, with `<FS_TREE>/` removed.
    #[must_use]
    pub fn tree_path(&self) -> &str {
        &self.tree_path
    }

}

/// Parses `btrfs subvolume list` output, whatever combination of flags produced it (§14.3).
///
/// # Errors
///
/// [`crate::error::unreadable_output`] when a line begins with `ID` and its ID is not a number.
/// Skipping such a line would drop a subvolume boundary, and §14.3 makes a missed boundary the
/// error that loses data.
pub fn parse_subvolume_list(text: &str) -> Result<Vec<SubvolumeEntry>, ErrorValue> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with("ID ") {
            continue;
        }
        entries.push(parse_list_line(line)?);
    }
    Ok(entries)
}

/// Reads one `ID ...` line as a token stream keyed by the word before each value.
fn parse_list_line(line: &str) -> Result<SubvolumeEntry, ErrorValue> {
    let mut entry = SubvolumeEntry {
        id: 0,
        generation: None,
        parent_id: None,
        top_level: None,
        uuid: None,
        parent_uuid: None,
        received_uuid: None,
        tree_path: Arc::from(""),
    };
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut index = 0;
    let mut seen_id = false;
    while index < tokens.len() {
        let key = tokens[index];
        match key {
            "ID" => {
                entry.id = next(&tokens, index)
                    .and_then(|value| value.parse().ok())
                    .ok_or_else(|| unreadable_output("btrfs subvolume list", line))?;
                seen_id = true;
                index += 2;
            }
            "gen" | "cgen" => {
                let generation = next(&tokens, index).and_then(|value| value.parse().ok());
                entry.generation = entry.generation.or(generation);
                index += 2;
            }
            "parent" => {
                entry.parent_id = next(&tokens, index).and_then(|value| value.parse().ok());
                index += 2;
            }
            "top" if tokens.get(index + 1) == Some(&"level") => {
                entry.top_level = tokens.get(index + 2).and_then(|value| value.parse().ok());
                index += 3;
            }
            // `-s` prints `otime <date> <time>` with no UTC offset, so it names no instant a
            // reader could place. §35.3 makes unknown data null rather than a guess about which
            // timezone the machine was in, and the exact creation instant is available with an
            // offset from `btrfs subvolume show`. The two tokens are stepped over, not read.
            "otime" => index += 3,
            "uuid" => {
                entry.uuid = next(&tokens, index).and_then(optional);
                index += 2;
            }
            "parent_uuid" => {
                entry.parent_uuid = next(&tokens, index).and_then(optional);
                index += 2;
            }
            "received_uuid" => {
                entry.received_uuid = next(&tokens, index).and_then(optional);
                index += 2;
            }
            "path" => {
                let path = tokens[index + 1..].join(" ");
                entry.tree_path = Arc::from(normalise_tree_path(&path));
                index = tokens.len();
            }
            _ => index += 1,
        }
    }
    if !seen_id {
        return Err(unreadable_output("btrfs subvolume list", line));
    }
    Ok(entry)
}

/// The token after the one at `index`, where there is one.
fn next<'a>(tokens: &[&'a str], index: usize) -> Option<&'a str> {
    tokens.get(index + 1).copied()
}

/// Strips the `<FS_TREE>/` prefix and any leading slash so every path is tree-relative.
///
/// `btrfs subvolume list -a` prefixes a subvolume that is not below the mount's own subvolume
/// with `<FS_TREE>/`, and prints the others bare. Both spellings name the same object, and a
/// boundary comparison that treated them as different paths would miss exactly the nested
/// subvolume §14.3 is about.
#[must_use]
pub fn normalise_tree_path(path: &str) -> &str {
    path.trim()
        .strip_prefix("<FS_TREE>")
        .unwrap_or(path.trim())
        .trim_start_matches('/')
}

/// What `btrfs filesystem show` says about the filesystem itself (Appendix D.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemInfo {
    label: Option<Arc<str>>,
    uuid: Arc<str>,
    devices: Vec<Arc<str>>,
    bytes_used: Option<ByteSize>,
}

impl FilesystemInfo {
    /// The filesystem label, where it has one.
    ///
    /// The gated real-filesystem suite refuses to run against a filesystem whose label is not the
    /// harness's own, which is Appendix G.3's rule expressed as something the code can check.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// The filesystem UUID — Appendix D.6's `filesystem_uuid`.
    #[must_use]
    pub fn uuid(&self) -> &str {
        &self.uuid
    }

    /// The devices the filesystem is made of.
    ///
    /// §14.7 is the reason they are carried: a snapshot lives on these devices and on nothing
    /// else, so naming them is how the provider states the shared failure domain concretely.
    #[must_use]
    pub fn devices(&self) -> &[Arc<str>] {
        &self.devices
    }

    /// How much the filesystem reports as used.
    #[must_use]
    pub const fn bytes_used(&self) -> Option<ByteSize> {
        self.bytes_used
    }
}

/// Parses `btrfs filesystem show` (Appendix D.6).
///
/// # Errors
///
/// [`crate::error::unreadable_output`] when no `uuid:` appears. Without the filesystem UUID a
/// subvolume ID is only unique within one filesystem, and §56.2's first fact is the exact
/// filesystem *and* subvolume ID.
pub fn parse_filesystem_show(text: &str) -> Result<FilesystemInfo, ErrorValue> {
    let mut info = FilesystemInfo {
        label: None,
        uuid: Arc::from(""),
        devices: Vec::new(),
        bytes_used: None,
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Label:") {
            let (label, uuid) = rest.split_once("uuid:").unwrap_or((rest, ""));
            let label = label.trim().trim_matches('\'');
            if label != "none" && !label.is_empty() {
                info.label = Some(Arc::from(label));
            }
            info.uuid = Arc::from(uuid.trim());
        } else if let Some(rest) = line.strip_prefix("Total devices") {
            info.bytes_used = rest
                .split_once("FS bytes used")
                .and_then(|(_, used)| ByteSize::parse(used.trim()).ok());
        } else if line.starts_with("devid")
            && let Some((_, path)) = line.split_once(" path ")
        {
            info.devices.push(Arc::from(path.trim()));
        }
    }
    if info.uuid.is_empty() {
        return Err(unreadable_output(
            "btrfs filesystem show",
            "the output carries no filesystem UUID, so §56.2's exact filesystem identity was not \
             established",
        ));
    }
    Ok(info)
}

/// What `btrfs filesystem usage` reports about space (§38).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FilesystemUsage {
    device_size: Option<ByteSize>,
    device_allocated: Option<ByteSize>,
    device_unallocated: Option<ByteSize>,
    used: Option<ByteSize>,
    free_estimated: Option<ByteSize>,
}

impl FilesystemUsage {
    /// The total size of the devices.
    #[must_use]
    pub const fn device_size(&self) -> Option<ByteSize> {
        self.device_size
    }

    /// How much of the devices is allocated to block groups.
    #[must_use]
    pub const fn device_allocated(&self) -> Option<ByteSize> {
        self.device_allocated
    }

    /// How much of the devices is not allocated to any block group.
    #[must_use]
    pub const fn device_unallocated(&self) -> Option<ByteSize> {
        self.device_unallocated
    }

    /// How much data the filesystem holds.
    #[must_use]
    pub const fn used(&self) -> Option<ByteSize> {
        self.used
    }

    /// Btrfs's own estimate of what is still free.
    ///
    /// It is an estimate in the tool as well as here — §37.5 requires the label to travel with
    /// the figure, and §38.2 forbids turning any of this into the word "free" for a snapshot.
    #[must_use]
    pub const fn free_estimated(&self) -> Option<ByteSize> {
        self.free_estimated
    }
}

/// Parses `btrfs filesystem usage` (§38).
///
/// # Errors
///
/// [`crate::error::unreadable_output`] when the overall section carries no device size, which
/// means the text is not the output of this command at all.
pub fn parse_filesystem_usage(text: &str) -> Result<FilesystemUsage, ErrorValue> {
    let mut usage = FilesystemUsage::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let size = value
            .split_whitespace()
            .next()
            .and_then(|first| ByteSize::parse(first).ok());
        match key.trim() {
            "Device size" => usage.device_size = size,
            "Device allocated" => usage.device_allocated = size,
            "Device unallocated" => usage.device_unallocated = size,
            "Used" => usage.used = size,
            "Free (estimated)" => usage.free_estimated = size,
            _ => {}
        }
    }
    if usage.device_size.is_none() {
        return Err(unreadable_output(
            "btrfs filesystem usage",
            "the output carries no device size, so nothing about capacity was established",
        ));
    }
    Ok(usage)
}

/// Which subvolume the filesystem mounts when nothing says otherwise (§56.2, Appendix D.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultSubvolume {
    id: u64,
    tree_path: Option<Arc<str>>,
}

impl DefaultSubvolume {
    /// The default subvolume's ID.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Its path, where the tool printed one.
    ///
    /// `ID 5 (FS_TREE)` names the top level of the filesystem rather than a named subvolume, and
    /// that answer carries no path.
    #[must_use]
    pub fn tree_path(&self) -> Option<&str> {
        self.tree_path.as_deref()
    }

    /// Whether the default is the top level of the filesystem rather than a named subvolume.
    #[must_use]
    pub const fn is_filesystem_tree(&self) -> bool {
        self.id == FS_TREE_ID
    }
}

/// The subvolume ID Btrfs gives the top of the filesystem tree.
pub const FS_TREE_ID: u64 = 5;

/// Parses `btrfs subvolume get-default` (§56.2's default-subvolume fact).
///
/// # Errors
///
/// [`crate::error::unreadable_output`] when no ID can be read. §56.2 asks the implementation to
/// prove the default-subvolume and boot impact, and an unread default proves nothing.
pub fn parse_get_default(text: &str) -> Result<DefaultSubvolume, ErrorValue> {
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.first() != Some(&"ID") {
            continue;
        }
        let Some(id) = tokens.get(1).and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        let tree_path = tokens
            .iter()
            .position(|token| *token == "path")
            .and_then(|at| tokens.get(at + 1))
            .map(|path| Arc::from(normalise_tree_path(path)));
        return Ok(DefaultSubvolume { id, tree_path });
    }
    Err(unreadable_output(
        "btrfs subvolume get-default",
        "the output names no default subvolume ID",
    ))
}

/// Reads `btrfs property get <path> ro` (§14.5).
///
/// # Errors
///
/// [`crate::error::unreadable_output`] when the output carries no `ro=` line. §14.5 wants the
/// read-only state of a retained snapshot established rather than assumed, and an absent answer
/// is not `true`.
pub fn parse_read_only_property(text: &str) -> Result<bool, ErrorValue> {
    for line in text.lines() {
        if let Some(value) = line.trim().strip_prefix("ro=") {
            return Ok(value.trim() == "true");
        }
    }
    Err(unreadable_output(
        "btrfs property get",
        "the output carries no `ro=` line, so the read-only state of the snapshot is unknown \
         (§14.5)",
    ))
}

/// The version of `btrfs-progs` in use (Appendix G.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtrfsVersion {
    raw: Arc<str>,
    major: u32,
    minor: u32,
}

impl BtrfsVersion {
    /// The version exactly as the tool printed it.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The major version.
    #[must_use]
    pub const fn major(&self) -> u32 {
        self.major
    }

    /// The minor version.
    #[must_use]
    pub const fn minor(&self) -> u32 {
        self.minor
    }

    /// The `major.minor` series, which is the grain Appendix G.4's validation is recorded at.
    #[must_use]
    pub fn series(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

/// Parses `btrfs --version` (Appendix G.4).
///
/// # Errors
///
/// [`crate::error::unreadable_output`] when no `vN.N` appears. A provider that cannot tell which
/// version it is talking to degrades rather than guessing, which is Appendix G.4's own rule.
pub fn parse_version(text: &str) -> Result<BtrfsVersion, ErrorValue> {
    for token in text.split_whitespace() {
        let Some(digits) = token.strip_prefix('v') else {
            continue;
        };
        let mut parts = digits.split('.');
        let major = parts.next().and_then(|part| part.parse().ok());
        let minor = parts.next().and_then(|part| part.parse().ok());
        if let (Some(major), Some(minor)) = (major, minor) {
            return Ok(BtrfsVersion {
                raw: Arc::from(token),
                major,
                minor,
            });
        }
    }
    Err(unreadable_output(
        "btrfs --version",
        "no `vMAJOR.MINOR` version appeared in the output",
    ))
}

/// The subvolume ID `btrfs subvolume delete` reported removing (§37).
///
/// The tool prints `Delete subvolume 261 (no-commit): '<path>'`, and reading the ID back is how
/// cleanup can say which object it removed rather than which path it aimed at.
#[must_use]
pub fn parse_deleted_subvolume_id(text: &str) -> Option<u64> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("Delete subvolume "))
        .find_map(|rest| {
            rest.split_whitespace()
                .next()
                .and_then(|id| id.parse().ok())
        })
}
