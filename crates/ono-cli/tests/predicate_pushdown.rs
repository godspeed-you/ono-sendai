//! Outcome tests for conservative predicate pushdown (issue #281, ADR-0980).
//!
//! A `where` that directly follows a provider-backed producer hands the provider the comparisons
//! the provider declared it can evaluate natively; the journal declares `priority` with `==`, `<`
//! and `<=` over an integer literal (`docs/contracts/providers/systemd.yaml`). The pipeline decides
//! the whole predicate again over whatever the provider returns, so what a pipeline yields is the
//! same with the handover and without it.
//!
//! Everything here is observed at the two external boundaries: the command line of `ono`, and the
//! command line of `journalctl`. A `journalctl` shim first on `PATH` (as in `services_logs.rs`,
//! issue #280) serves a fixed journal the way the real tool does — `--priority` keeps exactly the
//! entries whose `PRIORITY` is one of the digits in the range, which is how journald's index
//! answers it — and writes down every argument list it was started with. Nothing of Ono's own
//! layers is replaced.
//!
//! The handover is switched off with the setting `providers.pushdown`
//! (`ONO_PROVIDERS_PUSHDOWN=false`), which is what the equivalence tests compare against.

// The journal provider is the `systemd` tier; the core build leaves it out (ADR-0911).
#![cfg(feature = "systemd")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_testkit::{Scratch, Shell, scratch};

/// The unit most fixture entries belong to.
const CRON: &str = "cron.service";
/// The unit beside it.
const SSHD: &str = "ssh.service";

/// One journal entry of the fixture.
struct Entry {
    cursor: &'static str,
    /// The text of the `PRIORITY` field as journald stored it.
    priority: &'static str,
    /// The unit, where the entry has one: the kernel and processes outside units have none.
    unit: Option<&'static str>,
    /// The process, where the entry has one.
    pid: Option<u32>,
}

/// The journal, oldest first. Every priority from 0 to 7 is there, most of them twice, so each
/// threshold a test uses has entries on both sides of it and one exactly on it; three entries
/// carry no unit and two no process, so a predicate over those fields meets an unknown.
const ENTRIES: [Entry; 14] = [
    entry("c01", "6", Some(CRON), Some(42)),
    entry("c02", "7", Some(SSHD), Some(77)),
    entry("c03", "4", None, None),
    entry("c04", "3", Some(CRON), Some(42)),
    entry("c05", "5", Some(SSHD), Some(77)),
    entry("c06", "2", None, Some(1)),
    entry("c07", "0", Some(CRON), Some(42)),
    entry("c08", "6", Some(SSHD), None),
    entry("c09", "1", Some(SSHD), Some(77)),
    entry("c10", "3", None, Some(1)),
    entry("c11", "7", Some(CRON), Some(42)),
    entry("c12", "4", Some(SSHD), Some(77)),
    entry("c13", "2", Some(CRON), Some(42)),
    entry("c14", "5", Some(CRON), Some(42)),
];

const fn entry(
    cursor: &'static str,
    priority: &'static str,
    unit: Option<&'static str>,
    pid: Option<u32>,
) -> Entry {
    Entry {
        cursor,
        priority,
        unit,
        pid,
    }
}

/// One `journalctl --output=json` line.
fn json_line(position: usize, entry: &Entry) -> String {
    let unit = entry
        .unit
        .map(|unit| format!(r#""_SYSTEMD_UNIT":"{unit}","#))
        .unwrap_or_default();
    let pid = entry
        .pid
        .map(|pid| format!(r#""_PID":"{pid}","#))
        .unwrap_or_default();
    format!(
        r#"{{"__CURSOR":"{cursor}","__REALTIME_TIMESTAMP":"{micros}","_BOOT_ID":"0c0ffee0000000000000000000000001","_HOSTNAME":"fixture-host","PRIORITY":"{priority}",{unit}{pid}"SYSLOG_IDENTIFIER":"fixture","MESSAGE":"entry {cursor}"}}"#,
        cursor = entry.cursor,
        micros = (1_700_000_000 + position as u64 * 60) * 1_000_000,
        priority = entry.priority,
    )
}

/// The `journalctl` the queries reach: a shim over the fixture, first on `PATH`.
///
/// `--priority` is answered the way journald answers it. The index is asked for the entries whose
/// `PRIORITY` field is exactly one of the digits of the range, so an entry whose field is absent
/// or is anything but one such digit is not among them; a later `--priority` replaces an earlier
/// one. Every start except the `--version` probe appends its arguments to `argv.log`.
const SHIM: &str = r#"#!/bin/sh
# A stand-in for journalctl over a fixed journal (crates/ono-cli/tests/predicate_pushdown.rs).
if [ "$1" = --version ]; then echo 'systemd 259 (259.5)'; exit 0; fi
echo "$*" >> '@LOG@'
unit=
filtered=
low=0
high=7
lines=
follow=
number() {
  case "$1" in
    emerg) echo 0 ;; alert) echo 1 ;; crit) echo 2 ;; err) echo 3 ;;
    warning) echo 4 ;; notice) echo 5 ;; info) echo 6 ;; debug) echo 7 ;;
    [0-7]) echo "$1" ;;
    *) echo "journalctl shim: unknown priority $1" >&2; exit 64 ;;
  esac
}
for argument in "$@"; do
  case "$argument" in
    --output=json|--no-pager|--quiet) ;;
    --follow) follow=1 ;;
    --unit=*) unit=${argument#--unit=} ;;
    --priority=*..*)
      range=${argument#--priority=}
      low=$(number "${range%%..*}") || exit 64
      high=$(number "${range##*..}") || exit 64
      filtered=1 ;;
    --priority=*)
      low=0
      high=$(number "${argument#--priority=}") || exit 64
      filtered=1 ;;
    --lines=*) lines=${argument#--lines=} ;;
    *) echo "journalctl shim: unexpected argument $argument" >&2; exit 64 ;;
  esac
done
if [ -n "$follow" ] && [ -z "$lines" ]; then lines=10; fi
awk -v unit="$unit" -v filtered="$filtered" -v low="$low" -v high="$high" '
  unit != "" && index($0, "\"_SYSTEMD_UNIT\":\"" unit "\"") == 0 { next }
  {
    priority = ""
    if (match($0, /"PRIORITY":"[^"]*"/)) priority = substr($0, RSTART + 12, RLENGTH - 13)
  }
  filtered != "" && (priority !~ /^[0-7]$/ || priority + 0 < low + 0 || priority + 0 > high + 0) { next }
  { print }
' '@FIXTURE@' | if [ -n "$lines" ]; then tail -n "$lines"; else cat; fi
if [ -n "$follow" ]; then
  i=0
  while [ "$i" -lt 100 ]; do sleep 0.2; i=$((i + 1)); done
fi
"#;

/// What one `ono -c` run answered, in the three things a caller can observe.
#[derive(Debug, PartialEq, Eq)]
struct Answer {
    stdout: String,
    stderr: String,
    success: bool,
}

/// A fixed journal and the `journalctl` that serves it and records how it was asked.
struct Journal {
    dir: Scratch,
}

impl Journal {
    /// The journal of [`ENTRIES`].
    fn new() -> Self {
        let lines: Vec<String> = ENTRIES
            .iter()
            .enumerate()
            .map(|(position, entry)| json_line(position, entry))
            .collect();
        Self::of(&lines)
    }

    /// A journal of exactly these `journalctl --output=json` lines.
    fn of(lines: &[String]) -> Self {
        let dir = scratch();
        let fixture = dir.write("journal.jsonl", lines.join("\n") + "\n");
        let shim = SHIM
            .replace("@FIXTURE@", &fixture.display().to_string())
            .replace("@LOG@", &dir.path().join("argv.log").display().to_string());
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("the shim directory is created");
        ono_testkit::executable_script(&bin, "journalctl", &shim);
        Self { dir }
    }

    fn shell(&self, script: &str) -> Shell {
        Shell::new()
            .args(["-c", script])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.dir.path().join("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env_remove("ONO_PROVIDERS_PUSHDOWN")
            .timeout(Duration::from_secs(20))
    }

    /// Runs `script` with the handover as the build ships it: on.
    fn run(&self, script: &str) -> Answer {
        answer(&self.shell(script).run())
    }

    /// Runs `script` with `providers.pushdown` switched off.
    fn run_without_pushdown(&self, script: &str) -> Answer {
        answer(
            &self
                .shell(script)
                .env("ONO_PROVIDERS_PUSHDOWN", "false")
                .run(),
        )
    }

    /// The argument lists `journalctl` was started with since the last call, oldest first.
    fn invocations(&self) -> Vec<String> {
        let path = self.dir.path().join("argv.log");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::remove_file(&path);
        text.lines().map(str::to_owned).collect()
    }

    /// Runs `script` and answers the `--priority` arguments its one `journalctl` was given.
    fn priority_arguments(&self, script: &str) -> Vec<String> {
        let _ = self.invocations();
        let _ = self.run(script);
        let invocations = self.invocations();
        assert_eq!(
            invocations.len(),
            1,
            "`{script}` asks the journal once, got {invocations:?}"
        );
        invocations[0]
            .split_whitespace()
            .filter(|argument| argument.starts_with("--priority"))
            .map(str::to_owned)
            .collect()
    }
}

fn answer(run: &ono_testkit::Run) -> Answer {
    Answer {
        stdout: run.stdout().to_owned(),
        stderr: run.stderr().to_owned(),
        success: run.status().is_success(),
    }
}

/// The `cursor` of every row `| to json` printed, in stream order.
fn cursors(answer: &Answer) -> Vec<String> {
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(answer.stdout.trim())
        .unwrap_or_else(|error| panic!("`to json` emits a JSON document, got {answer:?}: {error}"));
    document
        .as_sequence()
        .unwrap_or_else(|| panic!("`to json` emits the stream as an array, got {answer:?}"))
        .iter()
        .map(|row| {
            row.get("cursor")
                .and_then(serde_yaml_ng::Value::as_str)
                .unwrap_or_else(|| panic!("every journal record has a cursor, got {row:?}"))
                .to_owned()
        })
        .collect()
}

/// The cursors of the fixture entries `keep` accepts, oldest first.
fn expected(keep: impl Fn(u8, &Entry) -> bool) -> Vec<String> {
    ENTRIES
        .iter()
        .filter(|entry| keep(entry.priority.parse().expect("a digit"), entry))
        .map(|entry| entry.cursor.to_owned())
        .collect()
}

// --- supported predicates: the native argument is observed at journalctl's command line --------

#[test]
fn should_hand_journalctl_the_priority_ceiling_when_where_compares_priority_with_a_literal() {
    let journal = Journal::new();

    let arguments = journal.priority_arguments("get journal | where priority <= 3 | to json");

    assert_eq!(
        arguments,
        ["--priority=3"],
        "ADR-0980: `where priority <= 3` directly after `get journal` reaches journalctl as the \
         one argument that means the same"
    );
    assert_eq!(
        cursors(&journal.run("get journal | where priority <= 3 | to json")),
        expected(|priority, _| priority <= 3),
        "the records are the ones the predicate keeps, in the journal's order"
    );
}

#[test]
fn should_translate_each_declared_operator_when_it_stands_first_in_the_predicate() {
    let journal = Journal::new();
    let cases: [(&str, &str); 8] = [
        ("priority <= 0", "--priority=0"),
        ("priority < 3", "--priority=2"),
        ("priority == 3", "--priority=3..3"),
        ("priority == 0", "--priority=0"),
        ("priority == 7", "--priority=7..7"),
        ("3 >= priority", "--priority=3"),
        ("4 > priority", "--priority=3"),
        ("(priority <= 5)", "--priority=5"),
    ];
    for (predicate, argument) in cases {
        assert_eq!(
            journal.priority_arguments(&format!("get journal | where {predicate} | to json")),
            [argument],
            "ADR-0980: `where {predicate}` is declared for the journal's `priority` and reaches \
             journalctl as `{argument}`"
        );
    }
    // The `log` target is the same journal seen as log lines, and declares the same field.
    assert_eq!(
        journal.priority_arguments("get log | where priority <= 2 | to json"),
        ["--priority=2"],
        "ADR-0980: `log` declares `priority` as `journal` does"
    );
}

#[test]
fn should_ask_for_nothing_narrower_when_the_comparison_keeps_every_priority() {
    let journal = Journal::new();
    // 7 is the last priority there is, and an entry journald stored with a larger number is a
    // record Ono keeps: a ceiling of 7 or more must not become a range that withholds it.
    for predicate in ["priority <= 7", "priority <= 12", "priority < 8"] {
        assert_eq!(
            journal.priority_arguments(&format!("get journal | where {predicate} | to json")),
            Vec::<String>::new(),
            "ADR-0980: `where {predicate}` excludes nothing journalctl could exclude"
        );
    }
}

// --- unsupported predicates fall back to local filtering ---------------------------------------

#[test]
fn should_leave_journalctl_unfiltered_when_the_predicate_is_outside_the_declaration() {
    let journal = Journal::new();
    let predicates = [
        // Operators the journal does not declare for `priority`.
        "priority >= 5",
        "priority > 5",
        "priority != 3",
        "priority in [1, 2]",
        // Fields the journal does not declare.
        "unit == \"cron.service\"",
        "pid == 42",
        "message == \"entry c04\"",
        // Not a field against a literal.
        "priority <= 1 + 2",
        "priority + 0 <= 3",
        "priority <= pid",
        "priority <= -1",
        "not (priority > 3)",
        // A literal of another type than the declared one.
        "priority <= 3.5",
        // `or` across fields, and `or` at all: a disjunction excludes nothing by itself.
        "priority <= 3 or unit == \"cron.service\"",
        "priority <= 1 or priority == 3",
        // A conjunction whose first conjunct is not declared: the comparison after it stays.
        "unit == \"cron.service\" and priority <= 3",
    ];
    for predicate in predicates {
        assert_eq!(
            journal.priority_arguments(&format!("get journal | where {predicate} | to json")),
            Vec::<String>::new(),
            "ADR-0980: `where {predicate}` is outside the journal's declaration and is decided \
             locally, as before"
        );
    }
    // `level` orders by severity, not by number or spelling (ADR-0222), and is nullable: the log
    // target does not declare it.
    for predicate in ["level >= error", "level == error"] {
        assert_eq!(
            journal.priority_arguments(&format!("get log | where {predicate} | to json")),
            Vec::<String>::new(),
            "ADR-0980: `where {predicate}` over `log` is decided locally"
        );
    }
}

#[test]
fn should_leave_journalctl_unfiltered_when_the_where_does_not_follow_the_producer_directly() {
    let journal = Journal::new();
    for pipeline in [
        // `take 5` then `where` keeps what the first five entries hold; filtering first would
        // answer a different question.
        "get journal | take 5 | where priority <= 3 | to json",
        "get journal | sort priority | where priority <= 3 | to json",
        "get journal | where unit == \"cron.service\" | where priority <= 3 | to json",
    ] {
        assert_eq!(
            journal.priority_arguments(pipeline),
            Vec::<String>::new(),
            "ADR-0980: only the `where` directly after the producer is handed over: `{pipeline}`"
        );
    }
}

#[test]
fn should_leave_journalctl_unfiltered_when_the_query_asks_for_the_last_entries() {
    let journal = Journal::new();
    // `--lines` keeps the last N entries *after* journalctl's own filters, so a pushed priority
    // would change which N those are: the last four entries are c11..c14, and c13 is the first of
    // them with a priority of 3 or less.
    let _ = journal.invocations();
    let answered = journal.run("tail journal --lines 4 | where priority <= 3 | take 1 | to json");
    let invocations = journal.invocations();
    assert_eq!(invocations.len(), 1, "one journalctl, got {invocations:?}");
    assert!(
        invocations[0].contains("--lines=4") && !invocations[0].contains("--priority"),
        "ADR-0980: with `--lines` the comparison stays local, got {invocations:?}"
    );
    assert_eq!(cursors(&answered), ["c13"]);

    // A follower without `--lines` starts from the last ten entries, c05..c14: the same rule.
    let answered = journal.run("tail journal | where priority <= 3 | take 1 | to json");
    let invocations = journal.invocations();
    assert_eq!(invocations.len(), 1, "one journalctl, got {invocations:?}");
    assert!(
        invocations[0].contains("--follow") && !invocations[0].contains("--priority"),
        "ADR-0980: a follower's comparison stays local, got {invocations:?}"
    );
    assert_eq!(cursors(&answered), ["c06"]);
}

#[test]
fn should_leave_journalctl_unfiltered_when_the_setting_switches_the_handover_off() {
    let journal = Journal::new();
    let _ = journal.invocations();

    let answered = journal.run_without_pushdown("get journal | where priority <= 3 | to json");

    let invocations = journal.invocations();
    assert_eq!(invocations.len(), 1, "one journalctl, got {invocations:?}");
    assert!(
        !invocations[0].contains("--priority"),
        "ADR-0980: `providers.pushdown = false` hands the provider nothing, got {invocations:?}"
    );
    assert_eq!(cursors(&answered), expected(|priority, _| priority <= 3));
}

// --- mixed conjunctions: the leading declared comparisons go, the rest stays -------------------

#[test]
fn should_hand_over_the_leading_comparisons_when_a_conjunction_is_only_partly_declared() {
    let journal = Journal::new();

    assert_eq!(
        journal.priority_arguments(
            "get journal | where priority <= 3 and unit == \"cron.service\" | to json"
        ),
        ["--priority=3"],
        "ADR-0980: the declared conjunct is handed over, the undeclared one is decided locally"
    );
    assert_eq!(
        cursors(
            &journal
                .run("get journal | where priority <= 3 and unit == \"cron.service\" | to json")
        ),
        expected(|priority, entry| priority <= 3 && entry.unit == Some(CRON)),
        "and the whole predicate still decides what is kept"
    );

    assert_eq!(
        journal.priority_arguments(
            "get journal | where priority <= 5 and priority == 4 and pid == 77 | to json"
        ),
        ["--priority=4..4"],
        "ADR-0980: two leading declared conjuncts meet in one argument"
    );
    assert_eq!(
        journal.priority_arguments(
            "get journal | where priority <= 5 and pid == 77 and priority <= 2 | to json"
        ),
        ["--priority=5"],
        "ADR-0980: a declared conjunct after an undeclared one stays local, because the one \
         before it is evaluated first and may fail on a record the later one rejects"
    );
    assert_eq!(
        journal.priority_arguments("get journal | where priority <= 2 and priority == 4 | to json"),
        Vec::<String>::new(),
        "ADR-0980: comparisons no priority satisfies together are left to the pipeline, which \
         keeps nothing"
    );
}

#[test]
fn should_give_journalctl_one_priority_argument_when_level_and_where_both_narrow() {
    let journal = Journal::new();

    // journalctl lets a later `--priority` replace an earlier one, so the two are combined.
    assert_eq!(
        journal.priority_arguments("get log --level warning | where priority <= 2 | to json"),
        ["--priority=2"],
        "ADR-0980: `--level warning` (0..4) and `priority <= 2` meet in 0..2"
    );
    assert_eq!(
        journal.priority_arguments("get log --level err | where priority == 2 | to json"),
        ["--priority=2..2"]
    );
    assert_eq!(
        journal.priority_arguments("get log --level err | where priority <= 5 | to json"),
        ["--priority=3"],
        "ADR-0980: the option is the narrower of the two and stays the bound"
    );
    assert_eq!(
        journal.priority_arguments("get log --level err | where priority == 5 | to json"),
        ["--priority=err"],
        "ADR-0980: where option and comparison exclude each other the option alone is passed, \
         as it is without the handover"
    );
    assert_eq!(
        cursors(&journal.run("get log --level warning | where priority <= 2 | to json")),
        expected(|priority, _| priority <= 2)
    );
}
