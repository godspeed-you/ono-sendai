//! Outcome tests for the service-family commands the contract declares: `set service` (spec
//! §52 row `service/set`), and the log and journal queries
//! `get log`, `get journal`, `tail journal` (spec §8.1 targets `log` and `journal`, §7.1 `tail`
//! over `journal`, §33.2 and §41.4 `get log --service`).
//!
//! Contracts: `docs/spec/commands/service.yaml`; schemas `ono.action-result/1` (spec §11.5,
//! §16.5, ADR-0006) and `ono.journal-event/1` (spec v0.3 §1.37, ADR-0059 — the records the
//! adapted `journalctl` already emits, which `get journal` hands back without the user spelling
//! `journalctl`). Typing before execution: spec §11.3.
//!
//! Every test runs unprivileged and offline. The machine that runs the gate may have a readable
//! journal and a live systemd; the acceptance container has neither. So a query asserts the
//! *shape* of the records when they come back and accepts exactly `Ono-Sendai-E0401
//! provider.unavailable` (non-zero exit) when the backing system is absent — never an
//! "implements nothing" answer (E0101/E0102), which is what these tests forbid.
//! Everything here asserts outcomes at the command line, nothing about how they are produced
//! (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ono_testkit::Shell;

/// A unit every systemd system has. It is `static`, so even a privileged `--enabled false` could
/// not change anything on the machine running the tests.
const JOURNALD: &str = "systemd-journald.service";

/// Runs a one-liner with a budget, so a follower that never returns fails instead of hanging.
fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .timeout(Duration::from_secs(10))
        .run()
}

/// The stream `| to json` printed, as rows.
fn rows(run: &ono_testkit::Run) -> Vec<serde_yaml_ng::Value> {
    let text = run.stdout().trim();
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(text).unwrap_or_else(|error| {
        panic!(
            "spec §33.5: `to json` emits a JSON document, got {text:?}: {error}; stderr {:?}",
            run.stderr()
        )
    });
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!("spec §33.5: `to json` emits the stream as a JSON array, got {text:?}")
        })
        .clone()
}

fn string_field<'a>(row: &'a serde_yaml_ng::Value, field: &str) -> &'a str {
    row.get(field)
        .and_then(serde_yaml_ng::Value::as_str)
        .unwrap_or_else(|| panic!("field `{field}` must be a string in {row:?}"))
}

/// An RFC 3339 timestamp field, in a form two of them can be ordered by.
///
/// Comparing the rendered text is not the same comparison: the journal trims trailing zeros
/// from the fraction, so `…12.2754Z` and `…12.275498Z` are one microsecond apart and order the
/// other way round as strings. Everything up to the fraction is fixed-width and orders
/// correctly as text; the fraction is padded to nanoseconds so that it does too.
fn instant(row: &serde_yaml_ng::Value, field: &str) -> (String, String) {
    let text = string_field(row, field).trim_end_matches('Z').to_owned();
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole.to_owned(), fraction.to_owned()),
        None => (text, String::new()),
    };
    (whole, format!("{fraction:0<9}"))
}

fn int_field(row: &serde_yaml_ng::Value, field: &str) -> i64 {
    row.get(field)
        .and_then(serde_yaml_ng::Value::as_i64)
        .unwrap_or_else(|| panic!("field `{field}` must be an integer in {row:?}"))
}

/// Whether the run answered that the backing system is absent — the one failure a query may
/// report on a box without a journal or a service manager.
fn provider_unavailable(run: &ono_testkit::Run) -> bool {
    !run.status().is_success() && run.stderr().contains("Ono-Sendai-E0401")
}

/// The dual expectation of every query here: records, or `provider.unavailable` — and in no
/// case the "declared but not implemented" answer. Returns the rows when there are any to check.
fn records_or_unavailable(run: &ono_testkit::Run, what: &str) -> Option<Vec<serde_yaml_ng::Value>> {
    assert!(
        !run.stderr().contains("Ono-Sendai-E0101") && !run.stderr().contains("Ono-Sendai-E0102"),
        "{what}: the command is part of the contract (docs/spec/commands/service.yaml) and must \
         answer with records, or with Ono-Sendai-E0401 provider.unavailable where the backing \
         system is absent — never with an unimplemented answer; got {:?}",
        run.output()
    );
    if provider_unavailable(run) {
        return None;
    }
    assert!(
        run.status().is_success(),
        "{what}: the only failure a query may report is Ono-Sendai-E0401 provider.unavailable \
         (exit non-zero); anything else must be a success with records; got {:?}",
        run.output()
    );
    Some(rows(run))
}

/// Asserts the required fields of `ono.journal-event/1` on one row.
fn assert_journal_event(row: &serde_yaml_ng::Value, what: &str) {
    let priority = int_field(row, "priority");
    assert!(
        (0..=7).contains(&priority),
        "{what}: ono.journal-event/1 `priority` is the syslog priority 0..=7, got {priority}"
    );
    let timestamp = string_field(row, "timestamp");
    assert!(
        timestamp.len() >= 19
            && timestamp.as_bytes()[4] == b'-'
            && timestamp.as_bytes()[10] == b'T',
        "{what}: ono.journal-event/1 `timestamp` is a timestamp, serialised as ISO 8601, got {timestamp:?}"
    );
    for field in ["message", "boot_id", "host", "cursor"] {
        assert!(
            !string_field(row, field).is_empty(),
            "{what}: ono.journal-event/1 requires a non-empty `{field}`, got {row:?}"
        );
    }
}

/// `seconds_ago` before now, as `YYYY-MM-DDTHH:MM:SS` in UTC, comparable with the prefix of the
/// timestamps `to json` prints.
fn utc_iso_seconds_ago(seconds_ago: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs();
    let secs = now - seconds_ago;
    let days = i64::try_from(secs / 86_400).expect("fits");
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days, so the test needs no calendar dependency.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

// --- get journal --------------------------------------------------------------------------

#[test]
fn should_emit_journal_events_with_the_schema_fields_when_the_journal_is_queried() {
    let run = ono("get journal | take 3 | to json");
    let Some(rows) = records_or_unavailable(&run, "`get journal | take 3`") else {
        return;
    };
    assert!(
        (1..=3).contains(&rows.len()),
        "`take 3` over the journal yields between one and three records, got {}",
        rows.len()
    );
    for row in &rows {
        assert_journal_event(row, "`get journal`");
    }
}

#[test]
fn should_restrict_journal_events_to_the_current_boot_when_boot_is_zero() {
    // `--boot <int>` restricts to one boot; 0 is the running one, as the journal counts boots.
    let run = ono("get journal --boot 0 | take 3 | select boot_id | to json");
    let Some(rows) = records_or_unavailable(&run, "`get journal --boot 0`") else {
        return;
    };
    let current = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .expect("/proc/sys/kernel/random/boot_id is readable")
        .trim()
        .replace('-', "");
    assert!(!rows.is_empty(), "the running boot has journal records");
    for row in &rows {
        assert_eq!(
            string_field(row, "boot_id"),
            current,
            "service.yaml `--boot 0` restricts `get journal` to the running boot, whose id is \
             /proc/sys/kernel/random/boot_id without dashes"
        );
    }
}

#[test]
fn should_only_emit_recent_events_when_since_is_a_relative_timestamp() {
    // The contract's own example (service.yaml `ono.journal.get`, spec §6.3 for `now() - 1h`).
    let bound = utc_iso_seconds_ago(3600);
    let run = ono("get journal --since (now() - 1h) | take 5 | select timestamp | to json");
    let Some(rows) = records_or_unavailable(&run, "`get journal --since (now() - 1h)`") else {
        return;
    };
    for row in &rows {
        let timestamp = string_field(row, "timestamp");
        assert!(
            timestamp
                .get(..19)
                .is_some_and(|prefix| prefix >= bound.as_str()),
            "service.yaml `--since` keeps only records at or after that time: {timestamp} is \
             older than {bound}"
        );
    }
}

#[test]
fn should_filter_journal_events_by_priority_when_where_composes_over_the_typed_stream() {
    let run = ono("get journal | where priority <= 3 | take 5 | select priority | to json");
    let Some(rows) = records_or_unavailable(&run, "`get journal | where priority <= 3`") else {
        return;
    };
    for row in &rows {
        let priority = int_field(row, "priority");
        assert!(
            priority <= 3,
            "spec v0.3 §1.37: `where priority <= 3` over journal events keeps only errors and \
             worse, got priority {priority}"
        );
    }
}

#[test]
fn should_reject_an_unknown_field_before_reading_the_journal_when_where_names_one() {
    // Spec §11.3: `get journal` advertises its schema, so `where prio == 1` is refused before
    // any record is read — on every box, with or without a journal.
    let run = ono("get journal | where prio == 1 | take 1");
    assert!(
        !run.status().is_success(),
        "an unknown field is a type error, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0202") && run.stderr().contains("prio"),
        "spec §11.3 / errors.yaml E0202 type.unknown_field: `where prio == 1` over the journal \
         names the unknown field before execution, got {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("perhaps"),
        "spec §11.3: the diagnostic suggests the field that was meant (`perhaps: …`), got {:?}",
        run.stderr()
    );
}

// --- tail journal -------------------------------------------------------------------------

#[test]
fn should_emit_the_most_recent_event_and_return_when_the_journal_is_tailed_through_take() {
    // Spec §7.1: `tail journal` follows the journal; `take 1` bounds the follow and the pipeline
    // returns as soon as one record arrived (ADR-0059 point 5).
    let run = ono("tail journal | take 1 | to json");
    let Some(rows) = records_or_unavailable(&run, "`tail journal | take 1`") else {
        return;
    };
    assert_eq!(
        rows.len(),
        1,
        "`take 1` over the followed journal yields exactly one record, got {:?}",
        run.stdout()
    );
    assert_journal_event(&rows[0], "`tail journal`");
}

#[test]
fn should_emit_existing_records_in_order_before_following_when_lines_is_given() {
    let run = ono("tail journal --lines 2 | take 2 | select timestamp cursor | to json");
    let Some(rows) = records_or_unavailable(&run, "`tail journal --lines 2`") else {
        return;
    };
    assert_eq!(
        rows.len(),
        2,
        "service.yaml `--lines 2` emits two existing records before following, got {:?}",
        run.stdout()
    );
    assert!(
        instant(&rows[0], "timestamp") <= instant(&rows[1], "timestamp"),
        "the journal is append-ordered: the first emitted record is not newer than the second, \
         got {:?}",
        run.stdout()
    );
    assert_ne!(
        string_field(&rows[0], "cursor"),
        string_field(&rows[1], "cursor"),
        "ono.journal-event/1 identity is the cursor; two records are two cursors"
    );
}

// --- get log -------------------------------------------------------------------------------

#[test]
fn should_emit_structured_log_records_when_the_log_is_queried() {
    let run = ono("get log | take 1 | to json");
    let Some(rows) = records_or_unavailable(&run, "`get log | take 1`") else {
        return;
    };
    assert_eq!(
        rows.len(),
        1,
        "`take 1` yields one record, got {:?}",
        run.stdout()
    );
    let row = &rows[0];
    assert!(
        !string_field(row, "message").is_empty(),
        "service.yaml `ono.log.get`: log records are structured values with a `message`, got {row:?}"
    );
    assert!(
        string_field(row, "timestamp").len() >= 19,
        "a log record carries the `timestamp` it was recorded at, got {row:?}"
    );
}

#[test]
fn should_restrict_log_records_to_one_unit_when_service_is_given() {
    let run = ono(&format!("get log --service {JOURNALD} | take 2 | to json"));
    let Some(rows) = records_or_unavailable(&run, "`get log --service`") else {
        return;
    };
    for row in &rows {
        assert!(
            !string_field(row, "message").is_empty(),
            "structured record, got {row:?}"
        );
        if let Some(unit) = row.get("unit").and_then(serde_yaml_ng::Value::as_str) {
            assert_eq!(
                unit, JOURNALD,
                "spec §33.2 `get log --service <ref>` restricts to that unit's records"
            );
        }
    }
}

#[test]
fn should_run_the_failed_service_example_when_a_level_threshold_composes() {
    // Spec §41.4 and the contract example: `get log --service <ref> | where level >= error |
    // take 20`. A documented example must parse and run (spec §50).
    let run = ono(&format!(
        "get log --service {JOURNALD} | where level >= error | take 20 | to json"
    ));
    let Some(_) = records_or_unavailable(
        &run,
        "spec §41.4 `get log --service … | where level >= error`",
    ) else {
        return;
    };
}

#[test]
fn should_order_the_level_by_severity_rather_than_by_spelling() {
    // Spec §41.4's own example is `where level >= error`. As text, `warning` is greater than
    // `error` and `crit` is less than it, so the threshold kept exactly the records it was meant
    // to drop (ADR-0222).
    let run = ono("get log | take 400 | where level >= warning | select level | to json");
    let Some(rows) = records_or_unavailable(&run, "`where level >= warning`") else {
        return;
    };
    for row in &rows {
        let level = string_field(row, "level");
        assert!(
            matches!(level, "warning" | "error" | "crit" | "alert" | "emerg"),
            "`level >= warning` keeps only what is at least a warning, got {level:?}"
        );
    }

    let below = ono("get log | take 400 | where level < warning | select level | to json");
    let Some(rows) = records_or_unavailable(&below, "`where level < warning`") else {
        return;
    };
    for row in &rows {
        let level = string_field(row, "level");
        assert!(
            matches!(level, "debug" | "info" | "notice"),
            "`level < warning` keeps only what is milder, got {level:?}"
        );
    }
}

#[test]
fn should_restrict_log_records_by_minimum_severity_when_level_is_given() {
    let run = ono("get log --level error | take 3 | to json");
    let Some(rows) = records_or_unavailable(&run, "`get log --level error`") else {
        return;
    };
    for row in &rows {
        assert!(
            !string_field(row, "message").is_empty(),
            "structured record, got {row:?}"
        );
        // The record's own severity, however the schema spells it, is not below the threshold:
        // as a syslog priority that is `error` (3) or a smaller number.
        if let Some(priority) = row.get("priority").and_then(serde_yaml_ng::Value::as_i64) {
            assert!(
                priority <= 3,
                "service.yaml `--level` is a minimum severity; `error` admits priority <= 3, got {row:?}"
            );
        }
    }
}

// --- set service ---------------------------------------------------------------------------

/// The one failed row a refused or impossible mutation reports (spec §16.5, ADR-0006), or `None`
/// when the service manager is absent (E0401).
fn one_failed_row(run: &ono_testkit::Run, what: &str) -> Option<serde_yaml_ng::Value> {
    assert!(
        !run.stderr().contains("Ono-Sendai-E0101") && !run.stderr().contains("Ono-Sendai-E0102"),
        "{what}: `set service` is declared (service.yaml `ono.service.set`) and must act, or \
         report Ono-Sendai-E0401 where no service manager runs — never an unimplemented answer; \
         got {:?}",
        run.output()
    );
    if provider_unavailable(run) {
        return None;
    }
    assert_eq!(
        run.status().code(),
        1,
        "{what}: ADR-0006 / ADR-0008 — a native mutation whose ActionResult is `failed` exits 1, \
         got {:?}",
        run.output()
    );
    assert!(
        !run.stdout().trim().is_empty(),
        "{what}: service.yaml declares `set service` a streaming native command whose \
         ono.action-result/1 rows reach `to json`; got no rows, stderr {:?}",
        run.stderr()
    );
    let rows = rows(run);
    assert_eq!(
        rows.len(),
        1,
        "{what}: spec §16.5 — one ono.action-result/1 row per target, got {:?}",
        run.stdout()
    );
    let row = rows.into_iter().next().expect("one row");
    assert_eq!(
        string_field(&row, "status"),
        "failed",
        "{what}: the outcome is `failed`, got {row:?}"
    );
    assert_eq!(
        row.get("changed").and_then(serde_yaml_ng::Value::as_bool),
        Some(false),
        "{what}: nothing changed, so `changed` is false, got {row:?}"
    );
    assert!(
        string_field(&row, "operation").ends_with("set"),
        "{what}: `operation` names the command (`ono.service.set`), got {row:?}"
    );
    assert!(
        row.get("error").is_some_and(|error| !error.is_null()),
        "{what}: a failed row carries its structured error (spec §11.5), got {row:?}"
    );
    Some(row)
}

#[test]
fn should_report_one_failed_row_when_disabling_a_unit_is_refused_unprivileged() {
    let run = ono(&format!("set service {JOURNALD} --enabled false | to json"));
    let Some(row) = one_failed_row(&run, "`set service --enabled false` unprivileged") else {
        return;
    };
    let error = format!("{:?}", row.get("error"));
    assert!(
        error.contains("permission_denied") || error.contains("E0302"),
        "errors.yaml E0302 io.permission_denied: the service manager refuses an unprivileged \
         change to what starts at boot, and that refusal is the row's error, got {row:?}"
    );
    assert!(
        format!("{:?}", row.get("target")).contains(JOURNALD),
        "the row's `target` references the unit that was addressed, got {row:?}"
    );
}

#[test]
fn should_report_one_failed_row_when_the_unit_to_modify_does_not_exist() {
    // The selector resolves the unit before the mutation (spec §6.1), so a unit that is not
    // there is `io.not_found` whatever the caller's privilege.
    let run = ono("set service no-such-unit-xyz.service --enabled true | to json");
    let Some(row) = one_failed_row(&run, "`set service` on a missing unit") else {
        return;
    };
    let error = format!("{:?}", row.get("error"));
    assert!(
        error.contains("not_found") || error.contains("E0301"),
        "errors.yaml E0301 io.not_found: `no-such-unit-xyz.service` does not exist and the row \
         says so, got {row:?}"
    );
}

#[test]
fn should_accept_piped_service_records_when_set_service_has_no_selector() {
    // service.yaml: `input: null | stream<ono.service/1>` — the units to modify may be piped in,
    // as `get service | where state == failed | restart service` does for restart.
    let run = ono(&format!(
        "get service {JOURNALD} | set service --enabled false | to json"
    ));
    let Some(row) = one_failed_row(&run, "`get service … | set service --enabled false`") else {
        return;
    };
    assert!(
        format!("{:?}", row.get("target")).contains(JOURNALD),
        "the piped unit is the row's `target`, got {row:?}"
    );
}

#[test]
fn should_refuse_set_service_without_a_property_when_nothing_is_asked_to_change() {
    let run = ono(&format!("set service {JOURNALD}"));
    assert!(
        !run.status().is_success(),
        "`set service <name>` with no property to set is a usage error, got {:?}",
        run.output()
    );
    assert!(
        !run.stderr().contains("Ono-Sendai-E0101") && !run.stderr().contains("Ono-Sendai-E0102"),
        "the command exists; only its arguments are wrong. Got {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0201") || run.stderr().contains("Ono-Sendai-E0202"),
        "errors.yaml: a missing argument is a type error (E0201, as `start service` without a \
         selector reports it, or E0202 for the option surface), got {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("--enabled"),
        "the diagnostic names the property option service.yaml declares, got {:?}",
        run.stderr()
    );
}

// --- trace service (spec §22.3, §41.6; `ono.service.trace`) -----------------------------------

#[test]
fn should_trace_a_service_to_the_processes_it_owns() {
    // service.yaml `ono.service.trace`: "Show a service's processes, sockets, dependencies and
    // recent journal context" as one `ono.graph/1`. `systemd-journald.service` runs on every
    // systemd system and owns at least the journal daemon, so the graph's root is the unit and
    // the processes it claims are nodes beneath it.
    let run = ono(&format!("trace service {JOURNALD} | to json"));
    let Some(graphs) = records_or_unavailable(&run, "trace service") else {
        return;
    };
    assert_eq!(
        graphs.len(),
        1,
        "`trace` yields one Graph (spec §9.1), got {graphs:?}"
    );
    let graph = &graphs[0];
    assert_eq!(
        graph["root"]["schema"].as_str(),
        Some("ono.service/1"),
        "graph.v1.yaml `root`: the traced service is the root, got {:?}",
        graph["root"]
    );
    assert!(
        graph["root"]["identity"]
            .get("name")
            .and_then(serde_yaml_ng::Value::as_str)
            == Some(JOURNALD),
        "the root's identity names the unit that was traced, got {:?}",
        graph["root"]
    );
    let nodes = graph["nodes"]
        .as_sequence()
        .cloned()
        .unwrap_or_else(|| panic!("graph.v1.yaml `nodes` is a list, got {:?}", graph["nodes"]));
    assert!(
        nodes
            .iter()
            .any(|node| node["kind"].as_str() == Some("ono.process/1")),
        "spec §41.6: a running unit's processes are among its nodes, got {} nodes",
        nodes.len()
    );
    assert!(
        graph["edges"]
            .as_sequence()
            .is_some_and(|edges| !edges.is_empty()),
        "spec §22.1: the relationships are edges, not implied by node order, got {:?}",
        graph["edges"]
    );
}

#[test]
fn should_relate_a_service_to_the_units_it_requires() {
    // v0.4 §13 lists dependencies among a service place's groups, and
    // `docs/spec/spatial/relations.yaml` declares `service.depends_on`. Until ADR-0239 nothing
    // claimed it: `ListUnits` carries no dependency information and the per-unit properties that
    // do were read and thrown away. `systemd-journald.service` requires its own sockets on every
    // systemd system, so the trace must relate it to at least one other unit.
    let run = ono(&format!("trace service {JOURNALD} | to json"));
    let Some(graphs) = records_or_unavailable(&run, "trace service") else {
        return;
    };
    let edges = graphs[0]["edges"]
        .as_sequence()
        .cloned()
        .unwrap_or_default();
    let dependencies: Vec<&str> = edges
        .iter()
        .filter(|edge| edge["relation"].as_str() == Some("depends-on"))
        .filter_map(|edge| edge["to"]["label"].as_str())
        .collect();
    assert!(
        !dependencies.is_empty(),
        "service.yaml `ono.service.trace`: a unit's dependencies are part of what it relates \
         to, and systemd states them (ADR-0239); got the relations {:?}",
        edges
            .iter()
            .filter_map(|edge| edge["relation"].as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        dependencies.iter().all(|unit| *unit != JOURNALD),
        "a unit is not its own dependency, got {dependencies:?}"
    );
}

#[test]
fn should_list_the_dependencies_of_a_unit_as_a_field_of_its_record() {
    // The dependency edge is composed from a fact the record carries, not observed a second
    // time (spec §2.16): `get service` answers it, so `where` and `select` compose over it.
    let run = ono(&format!(
        "get service {JOURNALD} | select name dependencies | to json"
    ));
    let Some(rows) = records_or_unavailable(&run, "`get service` with its dependencies") else {
        return;
    };
    let units = rows
        .first()
        .and_then(|row| row.get("dependencies"))
        .and_then(serde_yaml_ng::Value::as_sequence)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "service.v1.yaml `dependencies` is a list, and systemd has the notion, so it is \
                 never null here; got {:?}",
                run.stdout()
            )
        });
    assert!(
        !units.is_empty(),
        "`{JOURNALD}` requires its sockets on every systemd system, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_refuse_to_trace_a_service_that_does_not_exist() {
    let run = ono("trace service nothing-answers-to-this.service | to json");
    assert!(
        !run.stderr().contains("Ono-Sendai-E0101"),
        "`trace service` is implemented; a unit that is not there is a resolution failure, not \
         an unimplemented command. Got {:?}",
        run.output()
    );
    assert!(
        !run.status().is_success(),
        "tracing a unit nothing answers to is a failure, not an empty graph (spec §16.5), got \
         {:?}",
        run.output()
    );
}
