//! Decoding the kernel's process interface (spec §23.1).
//!
//! Everything here reads `/proc` directly. Nothing shells out and nothing parses the output of
//! `ps`: spec §50 forbids it, and the files below are a kernel ABI rather than a human report.

use std::fs;
use std::path::Path;

use nix::unistd::{SysconfVar, sysconf};

/// The fields of `/proc/<pid>/stat` this crate uses, in the numbering `proc(5)` gives them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcStat {
    /// Field 2: the executable name, without the parentheses the kernel wraps it in.
    pub(crate) comm: String,
    /// Field 3: the scheduling state letter.
    pub(crate) state: char,
    /// Field 4: the parent process id.
    pub(crate) ppid: i64,
    /// Field 14: user-mode time in clock ticks.
    pub(crate) utime: u64,
    /// Field 15: kernel-mode time in clock ticks.
    pub(crate) stime: u64,
    /// Field 20: the number of threads.
    pub(crate) threads: i64,
    /// Field 22: the start time, in clock ticks since boot.
    pub(crate) starttime: u64,
    /// Field 23: virtual memory size in bytes.
    pub(crate) vsize: u64,
    /// Field 24: the resident set size, in pages.
    pub(crate) rss_pages: i64,
}

impl ProcStat {
    /// The CPU time the process has used, in clock ticks.
    pub(crate) fn cpu_ticks(&self) -> u64 {
        self.utime.saturating_add(self.stime)
    }
}

/// Decodes one `/proc/<pid>/stat` line.
///
/// The executable name is delimited by parentheses and may itself contain both spaces and
/// parentheses — `(a b) c` is a legal `comm` — so the split is on the *last* `)` rather than on
/// whitespace. Getting this wrong is the classic procfs bug, and a process can be named
/// deliberately to trigger it.
pub(crate) fn parse_stat(text: &str) -> Option<ProcStat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let comm = text.get(open + 1..close)?.to_owned();
    let rest: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    // `rest[0]` is field 3, so field N sits at index N - 3.
    let field = |number: usize| rest.get(number - 3).copied();
    Some(ProcStat {
        comm,
        state: field(3)?.chars().next()?,
        ppid: field(4)?.parse().ok()?,
        utime: field(14)?.parse().ok()?,
        stime: field(15)?.parse().ok()?,
        threads: field(20)?.parse().ok()?,
        starttime: field(22)?.parse().ok()?,
        vsize: field(23)?.parse().ok()?,
        rss_pages: field(24)?.parse().ok()?,
    })
}

/// The scheduling state, mapped onto the enumeration of `docs/spec/schemas/process.v1.yaml`.
///
/// A letter this provider does not model becomes `unknown` rather than a guess, because spec
/// §35.3 forbids fabricating what was not observed.
pub(crate) fn state_name(state: char) -> &'static str {
    match state {
        'R' => "running",
        'S' => "sleeping",
        'D' => "disk-sleep",
        'T' => "stopped",
        't' => "tracing-stop",
        'Z' => "zombie",
        'X' | 'x' => "dead",
        'I' => "idle",
        _ => "unknown",
    }
}

/// The effective user and group ids from `/proc/<pid>/status`.
///
/// The `Uid:` and `Gid:` lines carry four ids — real, effective, saved and filesystem — and the
/// effective one is what a user means by "whose process is this".
pub(crate) fn parse_status_ids(text: &str) -> (Option<u32>, Option<u32>) {
    let effective = |prefix: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(prefix))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    (effective("Uid:"), effective("Gid:"))
}

/// The systemd unit named by `/proc/<pid>/cgroup`, when a service claims the process.
///
/// The line format is `hierarchy:controllers:path`; a service's path ends in `<unit>.service`.
/// Reading the unit name here costs one file and saves the service provider a reverse lookup;
/// resolving it to a whole `Service` object is that provider's job, not this one's.
pub(crate) fn service_unit(text: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| line.rsplit(':').next())
        .flat_map(|path| path.rsplit('/'))
        .find(|component| component.ends_with(".service"))
        .map(ToOwned::to_owned)
}

/// The moment the system booted, in seconds since the epoch, from `/proc/stat`'s `btime` line.
///
/// This is what turns field 22 of `/proc/<pid>/stat` — ticks since boot — into the wall-clock
/// start time that spec §23.1 makes half of a process's identity.
pub(crate) fn boot_time_seconds(proc_root: &Path) -> Option<i64> {
    let text = fs::read_to_string(proc_root.join("stat")).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()
}

/// The frequency of the statistics clock, in ticks per second.
pub(crate) fn clock_ticks() -> u64 {
    sysconf(SysconfVar::CLK_TCK)
        .ok()
        .flatten()
        .and_then(|ticks| u64::try_from(ticks).ok())
        .filter(|ticks| *ticks > 0)
        // Every Linux userspace ABI has used 100 since the tick became a userspace constant; the
        // fallback matters only if `sysconf` itself fails.
        .unwrap_or(100)
}

/// The size of a page, in bytes — the unit `/proc/<pid>/stat` reports the resident set in.
pub(crate) fn page_size() -> u128 {
    sysconf(SysconfVar::PAGE_SIZE)
        .ok()
        .flatten()
        .and_then(|size| u128::try_from(size).ok())
        .filter(|size| *size > 0)
        .unwrap_or(4096)
}

/// Splits `/proc/<pid>/cmdline`, which separates arguments with NUL and ends with one.
pub(crate) fn parse_cmdline(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(|argument| String::from_utf8_lossy(argument).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_keep_the_fields_aligned_when_the_name_contains_spaces_and_parentheses() {
        let line = "4419 ((weird) name) S 1 4419 4419 0 -1 4194304 100 0 0 0 11 22 0 0 20 0 \
                    7 0 987654 123456789 512 18446744073709551615";
        let stat = parse_stat(line).expect("the line is a well-formed stat line");
        assert_eq!(stat.comm, "(weird) name");
        assert_eq!(stat.state, 'S');
        assert_eq!(stat.ppid, 1);
        assert_eq!(stat.utime, 11);
        assert_eq!(stat.stime, 22);
        assert_eq!(stat.threads, 7);
        assert_eq!(stat.starttime, 987_654);
        assert_eq!(stat.vsize, 123_456_789);
        assert_eq!(stat.rss_pages, 512);
    }

    #[test]
    fn should_read_the_effective_ids_when_status_lists_four_of_them() {
        let status = "Name:\tbash\nUid:\t1000\t1001\t1000\t1000\nGid:\t100\t101\t100\t100\n";
        assert_eq!(parse_status_ids(status), (Some(1001), Some(101)));
    }

    #[test]
    fn should_find_the_unit_when_the_cgroup_path_names_a_service() {
        let cgroup = "0::/system.slice/nginx.service\n";
        assert_eq!(service_unit(cgroup).as_deref(), Some("nginx.service"));
        assert_eq!(service_unit("0::/user.slice/user-1000.slice\n"), None);
    }
}
