//! The journal as a historical source (spec v0.5 §21.4, §22.3, §23.2).
//!
//! §22.3 makes journald "a provider-owned historical event source", and it is the only built-in
//! source in this tree that answers about the past without the recorder. What that has to mean in
//! practice is checked here: a bounded question reaches the journal as a bound, the answer is
//! dated by journald's own `__REALTIME_TIMESTAMP`, every entry names the boot it belongs to and
//! carries the cursor two reads agree on, and a machine with no journal refuses rather than
//! answering an empty one.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml admits these inside `#[test]` functions, and the helpers here state a \
              test's preconditions the same way"
)]

use std::time::Duration;

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_provider_api::{Provider, Query, TimeWindow};
use ono_provider_systemd::JournalProvider;
use ono_testkit::{SkipReason, require};
use ono_value::{RecordValue, Value};

/// No test may hang; a journal read of a bounded window is well under this.
const BUDGET: Duration = Duration::from_secs(30);

fn field(record: &RecordValue, name: &str) -> Option<Value> {
    record.get(name).cloned()
}

#[test]
fn should_claim_history_and_nothing_stronger() {
    // §21.4: the journal answers about the past. §21.5: it drops entries under rate limiting, a
    // volatile journal is lost at reboot and a rotated one is gone, so its sequence carries no
    // absence claim — §7.4's own example is this source.
    let claims = JournalProvider::new().temporal();
    assert!(claims.historical_query);
    assert!(!claims.exhaustive_events);
    assert!(
        !claims.live_events,
        "`--follow` is a query option; the provider does not push (§21.3)"
    );
    assert!(
        !claims.checkpointable,
        "a journal is a stream of what happened, never a snapshot of object state (§21.7)"
    );
    assert_eq!(
        claims.retained_history, None,
        "the window is `journald.conf`'s and this provider does not read it; unknown stays \
         unknown rather than becoming forever (§35.3)"
    );
}

#[tokio::test]
async fn should_refuse_to_answer_about_the_past_where_there_is_no_journal_to_read() {
    // §10.5: having no journal is not the same as having no log records. A historical query that
    // answered an empty stream would let a caller conclude that nothing happened.
    let provider = JournalProvider::with_path(None);
    let stream = provider
        .history(&Query::target("journal"), &TimeWindow::unbounded())
        .expect_err("no journalctl means no answer, not an empty answer");
    assert_eq!(stream.code(), ErrorCode::ProviderUnavailable);
    assert!(stream.message().contains("journalctl"));
}

#[tokio::test]
async fn should_answer_only_within_the_window_it_was_asked_about() {
    // §21.4 and §23.2: the window is passed to the journal as a bound rather than applied to the
    // answer afterwards, and the answer is dated by `__REALTIME_TIMESTAMP` — journald's own
    // instant for the entry, which is the source time §3.3 asks for.
    let provider = JournalProvider::new();
    if require(
        provider.availability().is_available(),
        SkipReason::ExternalToolUnavailable,
        "no `journalctl` is on PATH, so the journal cannot be read here",
    )
    .unmet()
    {
        return;
    }

    let until = Timestamp::now();
    let from = until - jiff::SignedDuration::from_hours(24);
    let stream = provider
        .history(
            &Query::target("journal").option("lines", Value::Int(200)),
            &TimeWindow::between(from, until),
        )
        .expect("a journal that is present answers about its own past");
    let collected = tokio::time::timeout(BUDGET, stream.collect())
        .await
        .expect("a bounded journal read must not hang");

    let records: Vec<RecordValue> = collected
        .into_values()
        .into_iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some((*record).clone()),
            _ => None,
        })
        .collect();
    if require(
        !records.is_empty(),
        SkipReason::MissingPrivilege,
        "this user may read no journal entries at all, so there is nothing to bound",
    )
    .unmet()
    {
        return;
    }

    for record in &records {
        let Some(Value::Timestamp(at)) = field(record, "timestamp") else {
            panic!("every journal entry carries `__REALTIME_TIMESTAMP` as its timestamp");
        };
        // journalctl's `--since`/`--until` are whole seconds, so the bound it applies is the
        // second rather than the instant; the assertion is against the same resolution.
        assert!(
            at.as_second() >= from.as_second() && at.as_second() <= until.as_second(),
            "an entry at {at} is outside the window {from}..{until} the question named"
        );
    }
}

#[tokio::test]
async fn should_carry_the_boot_it_belongs_to_and_the_cursor_two_reads_agree_on() {
    // §23.2's identity mapping and deduplication key, as they arrive on a record. §25.5 makes the
    // boot the clock domain an instant belongs to, so an entry from a previous boot is never
    // silently ordered against one from this boot; §6.8 makes `__CURSOR` what lets an overlap
    // between two windows be recognised rather than ingested twice.
    let provider = JournalProvider::new();
    if require(
        provider.availability().is_available(),
        SkipReason::ExternalToolUnavailable,
        "no `journalctl` is on PATH, so the journal cannot be read here",
    )
    .unmet()
    {
        return;
    }

    let until = Timestamp::now();
    let from = until - jiff::SignedDuration::from_hours(24);
    let stream = provider
        .history(
            &Query::target("journal").option("lines", Value::Int(20)),
            &TimeWindow::between(from, until),
        )
        .expect("a journal that is present answers about its own past");
    let collected = tokio::time::timeout(BUDGET, stream.collect())
        .await
        .expect("a bounded journal read must not hang");

    let records: Vec<RecordValue> = collected
        .into_values()
        .into_iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some((*record).clone()),
            _ => None,
        })
        .collect();
    if require(
        !records.is_empty(),
        SkipReason::MissingPrivilege,
        "this user may read no journal entries at all, so there is nothing to identify",
    )
    .unmet()
    {
        return;
    }

    let mut cursors: Vec<String> = Vec::new();
    for record in &records {
        assert!(
            matches!(field(record, "boot_id"), Some(Value::String(_))),
            "§25.5: an entry states the boot its instant belongs to"
        );
        let Some(Value::String(cursor)) = field(record, "cursor") else {
            panic!("§6.8: an entry carries the cursor two reads of one journal agree on");
        };
        cursors.push(cursor.to_string());
    }
    let unique: std::collections::BTreeSet<&String> = cursors.iter().collect();
    assert_eq!(
        unique.len(),
        cursors.len(),
        "a deduplication key that repeats within one answer would collapse distinct entries"
    );

    // The same window asked again agrees on the cursors, which is what makes the key a
    // deduplication key rather than a sequence number.
    let again = provider
        .history(
            &Query::target("journal").option("lines", Value::Int(20)),
            &TimeWindow::between(from, until),
        )
        .expect("a second read of the same window");
    let repeated: Vec<String> = tokio::time::timeout(BUDGET, again.collect())
        .await
        .expect("a bounded journal read must not hang")
        .into_values()
        .into_iter()
        .filter_map(|value| match value {
            Value::Record(record) => match record.get("cursor") {
                Some(Value::String(cursor)) => Some(cursor.to_string()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let overlap = cursors.iter().filter(|c| repeated.contains(c)).count();
    assert!(
        overlap > 0,
        "two reads of one window must agree on the entries they both saw"
    );
}

#[tokio::test]
async fn should_leave_an_open_end_open_rather_than_reading_a_clock_to_close_it() {
    // §39.2's rule as it applies here: the provider does not substitute `now` for a bound the
    // caller left open. journalctl without `--until` reads to the end of the journal, which is
    // the honest reading of "up to the present" and costs no clock read.
    let provider = JournalProvider::new();
    if require(
        provider.availability().is_available(),
        SkipReason::ExternalToolUnavailable,
        "no `journalctl` is on PATH, so the journal cannot be read here",
    )
    .unmet()
    {
        return;
    }
    let from = Timestamp::now() - jiff::SignedDuration::from_mins(5);
    let stream = provider
        .history(
            &Query::target("journal").option("lines", Value::Int(5)),
            &TimeWindow::since(from),
        )
        .expect("a half-open window is a window");
    let collected = tokio::time::timeout(BUDGET, stream.collect())
        .await
        .expect("a half-open journal read must not hang");
    assert!(
        collected.errors().is_empty(),
        "a half-open window is answered rather than refused: {:?}",
        collected.errors()
    );
}
