//! The benchmark behind conservative predicate pushdown (issue #281, ADR-0980).
//!
//! A selective `where` over a journal whose matches lie far from its start: without the handover
//! Ono reads, decodes and rejects every entry before them; with it `journalctl` is asked for the
//! matching priorities and hands over those alone. The fixture is generated, so the measurement
//! is the same journal on every machine, and it is served by a `journalctl` shim first on `PATH`
//! that answers `--priority` the way the real tool does and writes down how many entries it
//! handed over.
//!
//! What is asserted is what does not depend on the machine: the two runs answer the same, and
//! the handover reduces what the source hands Ono from the whole journal to the matching entries.
//! The wall-clock figures are printed and never compared with anything — a timing is a property
//! of the machine it was taken on (v0.4.1 §32.4), so none of them is a threshold. ADR-0980
//! records one measurement and the command line that reproduces it:
//!
//! ```text
//! ONO_PUSHDOWN_BENCHMARK_ENTRIES=200000 cargo test -p ono-cli --features systemd \
//!     --test predicate_pushdown_benchmark -- --nocapture
//! ```
//!
//! The shim filters with one `awk` pass over the file either way, where journald answers
//! `--priority` from an index; the figures therefore show what Ono saves on its own side of the
//! boundary and understate what a real journal saves.

// The journal provider is the `systemd` tier; the core build leaves it out (ADR-0911).
#![cfg(feature = "systemd")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stdout,
    reason = "a test states its preconditions directly (AGENTS.md section 16), and a benchmark \
              prints what it measured"
)]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use ono_testkit::{Scratch, Shell, scratch};

/// How many entries the journal holds unless `ONO_PUSHDOWN_BENCHMARK_ENTRIES` says otherwise:
/// enough for the effect to show, small enough for every gate run.
const DEFAULT_ENTRIES: usize = 5_000;

/// How often each variant runs; the median of the rounds is the figure reported.
const ROUNDS: usize = 5;

/// The `journalctl` the queries reach: `--priority` keeps the entries whose `PRIORITY` is one of
/// the digits of the range, as journald's index answers it, and every start appends the number of
/// entries handed over to `emitted.log`.
const SHIM: &str = r#"#!/bin/sh
# A stand-in for journalctl over a generated journal
# (crates/ono-cli/tests/predicate_pushdown_benchmark.rs).
if [ "$1" = --version ]; then echo 'systemd 259 (259.5)'; exit 0; fi
low=0
high=7
for argument in "$@"; do
  case "$argument" in
    --output=json|--no-pager|--quiet) ;;
    --priority=*..*) range=${argument#--priority=}; low=${range%%..*}; high=${range##*..} ;;
    --priority=*) low=0; high=${argument#--priority=} ;;
    *) echo "journalctl shim: unexpected argument $argument" >&2; exit 64 ;;
  esac
done
awk -v low="$low" -v high="$high" -v log_file='@LOG@' '
  {
    priority = 8
    if (match($0, /"PRIORITY":"[0-7]"/)) priority = substr($0, RSTART + 12, 1) + 0
  }
  priority >= low && priority <= high { print; emitted++ }
  END { print emitted + 0 >> log_file }
' '@FIXTURE@'
"#;

/// A generated journal of `entries` entries, and where its matches are.
struct Journal {
    dir: Scratch,
    entries: usize,
    /// The cursors of the entries of priority 3 and more severe, oldest first.
    matches: Vec<String>,
    /// How far into the journal the first of them lies, in percent.
    first_match_at: usize,
}

impl Journal {
    /// A journal of informational and debug entries with three severe ones in its last tenth:
    /// one exactly on the threshold the benchmark asks for, two beyond it, and one just short of
    /// it beside them, so that both sides of the threshold lie where the matches are.
    fn generated(entries: usize) -> Self {
        assert!(
            entries >= 100,
            "the benchmark needs a journal, got {entries}"
        );
        let severe = [
            (entries - entries / 10, 3_u8),
            (entries - entries / 20, 0),
            (entries - 2, 2),
        ];
        let short_of_it = entries - entries / 20 + 1;
        let mut text = String::with_capacity(entries * 260);
        let mut matches = Vec::new();
        for position in 0..entries {
            let priority = severe
                .iter()
                .find(|(at, _)| *at == position)
                .map(|(_, priority)| *priority)
                .unwrap_or(if position == short_of_it {
                    4
                } else if position % 2 == 0 {
                    6
                } else {
                    7
                });
            let cursor = format!("g{position:07}");
            let _ = writeln!(
                text,
                r#"{{"__CURSOR":"{cursor}","__REALTIME_TIMESTAMP":"{micros}","_BOOT_ID":"0c0ffee0000000000000000000000001","_HOSTNAME":"fixture-host","PRIORITY":"{priority}","_SYSTEMD_UNIT":"unit-{unit}.service","_PID":"{pid}","SYSLOG_IDENTIFIER":"generated","MESSAGE":"generated entry {position} of the benchmark journal"}}"#,
                micros = (1_700_000_000 + position as u64) * 1_000_000,
                unit = position % 17,
                pid = 100 + position % 400,
            );
            if priority <= 3 {
                matches.push(cursor);
            }
        }
        let dir = scratch();
        let fixture = dir.write("journal.jsonl", text);
        let shim = SHIM
            .replace("@FIXTURE@", &fixture.display().to_string())
            .replace(
                "@LOG@",
                &dir.path().join("emitted.log").display().to_string(),
            );
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("the shim directory is created");
        ono_testkit::executable_script(&bin, "journalctl", &shim);
        Self {
            dir,
            entries,
            matches,
            first_match_at: severe[0].0 * 100 / entries,
        }
    }

    /// Runs `pipeline` once and answers its stdout, how long it took, and how many entries
    /// `journalctl` handed over for it.
    fn measure(&self, pipeline: &str, pushdown: bool) -> (String, Duration, usize) {
        let log = self.dir.path().join("emitted.log");
        let _ = std::fs::remove_file(&log);
        let shell = Shell::new()
            .args(["-c", pipeline])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.dir.path().join("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env(
                "ONO_PROVIDERS_PUSHDOWN",
                if pushdown { "true" } else { "false" },
            )
            .timeout(Duration::from_secs(600));
        let started = Instant::now();
        let run = shell.run();
        let elapsed = started.elapsed();
        assert!(
            run.status().is_success(),
            "`{pipeline}` succeeds, got {:?} with {}",
            run.status(),
            run.stderr()
        );
        let handed_over: usize = std::fs::read_to_string(&log)
            .expect("the shim wrote down what it handed over")
            .lines()
            .map(|line| line.trim().parse::<usize>().expect("a count"))
            .sum();
        (run.stdout().to_owned(), elapsed, handed_over)
    }
}

/// The median of `rounds` durations.
fn median(mut rounds: Vec<Duration>) -> Duration {
    rounds.sort();
    rounds[rounds.len() / 2]
}

#[test]
fn should_read_only_the_matching_entries_when_the_matches_lie_far_into_a_large_journal() {
    let entries = std::env::var("ONO_PUSHDOWN_BENCHMARK_ENTRIES")
        .ok()
        .map(|text| text.parse().expect("a number of entries"))
        .unwrap_or(DEFAULT_ENTRIES);
    let journal = Journal::generated(entries);
    let pipeline = "get journal | where priority <= 3 | select cursor | to json";

    let mut without = Vec::new();
    let mut with = Vec::new();
    let mut answers = Vec::new();
    let mut read = (0, 0);
    for _ in 0..ROUNDS {
        let (off, took, handed_over) = journal.measure(pipeline, false);
        without.push(took);
        read.0 = handed_over;
        let (on, took, handed_over) = journal.measure(pipeline, true);
        with.push(took);
        read.1 = handed_over;
        answers.push((on, off));
    }

    let expected = format!(
        "[{}]",
        journal
            .matches
            .iter()
            .map(|cursor| format!(r#"{{"cursor":"{cursor}"}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    for (on, off) in &answers {
        assert_eq!(
            on.trim(),
            expected,
            "the handover yields the entries the predicate keeps, in the journal's order"
        );
        assert_eq!(on, off, "ADR-0980: the answer is the same either way");
    }
    assert_eq!(
        read,
        (journal.entries, journal.matches.len()),
        "ADR-0980: without the handover journalctl hands Ono the whole journal, with it the \
         matching entries alone — (without, with)"
    );

    let (without, with) = (median(without), median(with));
    println!(
        "predicate pushdown benchmark: {entries} entries, {matches} matching, the first at \
         {first} % of the journal; median of {ROUNDS} rounds of `{pipeline}`\n  \
         providers.pushdown = false: {off:>9.1} ms, {entries} entries read\n  \
         providers.pushdown = true:  {on:>9.1} ms, {matches} entries read\n  \
         ratio: {ratio:.1}x",
        matches = journal.matches.len(),
        first = journal.first_match_at,
        off = without.as_secs_f64() * 1000.0,
        on = with.as_secs_f64() * 1000.0,
        ratio = without.as_secs_f64() / with.as_secs_f64().max(f64::EPSILON),
    );
}
