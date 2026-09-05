//! Decoding the kernel's process interface (spec §23.1).
//!
//! Everything here reads `/proc` directly. Nothing shells out and nothing parses the output of
//! `ps`: spec §50 forbids it, and the files below are a kernel ABI rather than a human report.

use std::fs;
use std::path::Path;

use nix::unistd::{SysconfVar, sysconf};

/// The fields of `/proc/<pid>/stat` this crate uses, in the numbering `proc(5)` gives them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcStat {
    /// Field 2: the executable name, without the parentheses the kernel wraps it in.
    pub comm: String,
    /// Field 3: the scheduling state letter.
    pub state: char,
    /// Field 4: the parent process id.
    pub ppid: i64,
    /// Field 14: user-mode time in clock ticks.
    pub utime: u64,
    /// Field 15: kernel-mode time in clock ticks.
    pub stime: u64,
    /// Field 20: the number of threads.
    pub threads: i64,
    /// Field 22: the start time, in clock ticks since boot.
    pub starttime: u64,
    /// Field 23: virtual memory size in bytes.
    pub vsize: u64,
    /// Field 24: the resident set size, in pages.
    pub rss_pages: i64,
}

impl ProcStat {
    /// The CPU time the process has used, in clock ticks.
    #[must_use]
    pub fn cpu_ticks(&self) -> u64 {
        self.utime.saturating_add(self.stime)
    }
}

/// Decodes one `/proc/<pid>/stat` line.
///
/// The executable name is delimited by parentheses and may itself contain both spaces and
/// parentheses — `(a b) c` is a legal `comm` — so the split is on the *last* `)` rather than on
/// whitespace. Getting this wrong is the classic procfs bug, and a process can be named
/// deliberately to trigger it.
#[must_use]
pub fn parse_stat(text: &str) -> Option<ProcStat> {
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

/// The scheduling state, mapped onto the enumeration of `docs/contracts/schemas/process.v1.yaml`.
///
/// A letter this provider does not model becomes `unknown` rather than a guess, because spec
/// §35.3 forbids fabricating what was not observed.
#[must_use]
pub fn state_name(state: char) -> &'static str {
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
#[must_use]
pub fn parse_status_ids(text: &str) -> (Option<u32>, Option<u32>) {
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
#[must_use]
pub fn service_unit(text: &str) -> Option<String> {
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

/// How long the machine has been up, in seconds, from `/proc/uptime`.
///
/// It is what turns a process's `starttime` — a tick count measured from the same boot — into a
/// lifetime, which is the longest window a single observation can measure a CPU share over
/// (ADR-0232).
pub(crate) fn uptime_seconds(proc_root: &Path) -> Option<f64> {
    let text = fs::read_to_string(proc_root.join("uptime")).ok()?;
    text.split_whitespace().next()?.parse().ok()
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
#[must_use]
pub fn parse_cmdline(bytes: &[u8]) -> Vec<String> {
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

#[cfg(test)]
mod fuzz {
    //! The procfs decoders read text the kernel writes, which is *usually* well formed — and is
    //! not, on an old kernel, in a container with a synthetic `/proc`, or when a process is
    //! named to make it so. Spec §35.6 requires fuzzing of the procfs decoders and ADR-0015 T7
    //! makes an unbounded allocation a release-blocking threat.
    //!
    //! These are unit tests rather than integration tests because the decoders are `pub(crate)`,
    //! and widening an API to test it is a worse trade than testing it from inside (AGENTS.md
    //! §11).
    //!
    //! The contract asserted is narrow: **no panic, and a return.** What a decoder decides a
    //! particular malformed line means is the implementation's business.

    use super::{parse_stat, parse_status_ids};
    use ono_testkit::Rng;

    /// Pieces of the real formats, so a generated line reaches past the first rejection.
    const PIECES: &[&str] = &[
        "1",
        "0",
        "-1",
        " ",
        "(",
        ")",
        "((",
        "))",
        "R",
        "S",
        "Z",
        "\n",
        "\t",
        "\0",
        "18446744073709551615",
        "99999999999999999999999999",
        "Name:",
        "Uid:",
        "Gid:",
        ":",
        "\u{feff}",
        "é",
        "-",
        "+",
        ".",
        "e",
        "comm with spaces",
        "(weird) name",
    ];

    #[test]
    fn should_never_panic_on_anything_that_arrives_as_a_stat_line() {
        let mut rng = Rng::seeded(0x50_52_4f_43);
        for _ in 0..4000 {
            let _ = parse_stat(&rng.assemble(PIECES, 30));
        }
    }

    #[test]
    fn should_never_panic_on_anything_that_arrives_as_a_status_file() {
        let mut rng = Rng::seeded(0x53_54_41_54);
        for _ in 0..4000 {
            let _ = parse_status_ids(&rng.assemble(PIECES, 30));
        }
    }

    #[test]
    fn should_return_rather_than_recurse_on_a_pathologically_long_line() {
        // A line a few kilobytes long is cheap to write and must stay cheap to read.
        for length in [1_000usize, 100_000] {
            let _ = parse_stat(&"1 ".repeat(length));
            let _ = parse_stat(&"(".repeat(length));
            let _ = parse_status_ids(&"Uid:\t".repeat(length));
        }
    }

    #[test]
    fn should_read_a_command_name_that_contains_the_delimiters_it_is_wrapped_in() {
        // A process can be named `((weird) name)`. Splitting on the first `)` is the classic
        // procfs bug and it is reachable by anyone who can name a process.
        // Fields 3 through 24, which is everything the decoder reads.
        let line = "42 (((weird) name)) S 1 0 0 0 0 0 0 0 0 0 100 200 0 0 0 0 3 0 777 4096 10";
        let parsed = parse_stat(line).expect("a stat line with a hostile comm must still parse");
        assert_eq!(parsed.comm, "((weird) name)");
        assert_eq!(parsed.ppid, 1);
        assert_eq!(parsed.state, 'S');
        assert_eq!(
            parsed.starttime, 777,
            "the fields after a hostile comm still line up"
        );
        assert_eq!(parsed.threads, 3);
    }
}
