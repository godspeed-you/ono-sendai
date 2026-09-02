//! The retained result history of spec v0.4.1 §24: bounded in four dimensions, and truthful.
//!
//! §24.2 is the rule that separates this from every other budget in the shell: *"Result history
//! is a cache, not a correctness requirement. It therefore uses eviction rather than failing the
//! user's command."* §21.3 permits exactly two responses to a budget being reached — stop with a
//! structured error, or evict per a documented policy — and forbids mixing them implicitly. This
//! is the eviction half, and nothing here ever returns an error.
//!
//! §24.3 is what makes the eviction honest: *"If the user inspects a history entry that was
//! truncated for retention, Ono MUST say so. It MUST NOT present the retained subset as though it
//! were the complete original output."*

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_history::{ResultHistory, RetentionLimits};
use ono_value::{Value, estimated_size};

/// A value of roughly `bytes` estimated size.
fn payload_of_about(bytes: usize) -> Value {
    Value::string(&"x".repeat(bytes))
}

/// A result of `count` values, each about `bytes` in size.
fn result_of(count: usize, bytes: usize) -> Vec<Value> {
    (0..count).map(|_| payload_of_about(bytes)).collect()
}

/// Appendix A's limits, scaled down so a test does not have to allocate 64 MiB to reach them.
fn limits(results: usize, items: usize, per_result: u64, total: u64) -> RetentionLimits {
    RetentionLimits {
        results,
        items_per_result: items,
        bytes_per_result: per_result,
        bytes_total: total,
    }
}

#[test]
fn should_keep_appendix_a_defaults_when_nothing_configures_it() {
    let defaults = RetentionLimits::default();
    assert_eq!(defaults.results, 16);
    assert_eq!(defaults.items_per_result, 10_000);
    assert_eq!(defaults.bytes_per_result, 16 * 1024 * 1024);
    assert_eq!(defaults.bytes_total, 64 * 1024 * 1024);
}

#[test]
fn should_evict_the_oldest_result_when_the_total_byte_ceiling_is_reached() {
    // §24.2 rule 4: "oldest history entries are evicted until the total byte budget is satisfied".
    let mut history = ResultHistory::new(limits(16, 10_000, 1 << 20, 24_000));
    let each = result_of(1, 8_000);
    let cost = estimated_size(&Value::list(each.clone()));
    assert!(cost < 24_000, "one result fits the total ceiling");

    for _ in 0..8 {
        history.retain(&each);
    }

    assert!(
        history.retained_bytes() <= 24_000,
        "the total ceiling is a ceiling, and history held {} bytes",
        history.retained_bytes()
    );
    assert!(
        history.len() < 8,
        "eight results of 8 KiB cannot fit a 24 KiB history; {} were kept",
        history.len()
    );
    assert!(
        !history.is_empty(),
        "eviction empties the oldest, not the whole cache"
    );
}

#[test]
fn should_evict_the_oldest_result_when_the_slot_count_is_reached() {
    // §24.1's existing conceptual limit, preserved: sixteen slots by default, and here three.
    let mut history = ResultHistory::new(limits(3, 10_000, 1 << 20, 1 << 30));
    for index in 0..5 {
        history.retain(&[Value::Int(index)]);
    }
    assert_eq!(history.len(), 3);
    assert_eq!(
        history.previous(1),
        Some(&[Value::Int(4)][..]),
        "`@-1` is the newest result"
    );
    assert_eq!(
        history.previous(3),
        Some(&[Value::Int(2)][..]),
        "the oldest two were evicted, not the newest"
    );
    assert_eq!(history.previous(4), None);
}

#[test]
fn should_mark_a_result_it_kept_only_in_part_and_say_how_much_it_kept() {
    // §24.2 rules 2 and 3, and §54.1's own example sentence:
    // "result history kept 10,000 of 84,212 values because the 16 MiB history budget was reached".
    let mut history = ResultHistory::new(limits(16, 4, 1 << 30, 1 << 30));
    let outcome = history.retain(&result_of(10, 16));

    assert!(
        outcome.truncated_for_history(),
        "§24.2 rule 3: a partially kept result is marked `truncated_for_history=true`"
    );
    assert_eq!(outcome.kept(), 4, "the per-result item cap is four");
    assert_eq!(outcome.total(), 10, "and the result really held ten");
    assert_eq!(outcome.dropped(), 6);

    let note = outcome.notice().expect("a truncated entry says so (§24.3)");
    assert!(
        note.contains("4") && note.contains("10"),
        "§54.1: the notice says how much of how much was kept: {note:?}"
    );

    assert_eq!(
        history.previous(1).map(<[Value]>::len),
        Some(4),
        "what `@-1` answers is the retained subset"
    );
    assert!(
        history.was_truncated(1),
        "§24.3: inspecting the entry tells the user it is partial, so the marker outlives the run"
    );

    // A result that fitted carries no marker: the notice must mean something when it appears.
    let whole = history.retain(&result_of(2, 16));
    assert!(!whole.truncated_for_history());
    assert_eq!(whole.notice(), None);
    assert!(!history.was_truncated(1));
}

#[test]
fn should_stop_retaining_at_the_per_result_byte_cap_before_the_item_cap() {
    // §24.1 adds byte ceilings beside the count ones, and §65.6 is why: ten thousand values each
    // of arbitrary size is not a memory bound.
    let mut history = ResultHistory::new(limits(16, 10_000, 20_000, 1 << 30));
    let outcome = history.retain(&result_of(100, 8_000));

    assert!(outcome.truncated_for_history());
    assert!(
        outcome.kept() < 10,
        "a 20 KB per-result ceiling holds a couple of 8 KB values, not a hundred; it kept {}",
        outcome.kept()
    );
    assert!(
        outcome.kept() >= 1,
        "the ceiling is larger than one value, so one value is kept"
    );
}

#[test]
fn should_not_retain_a_single_value_larger_than_the_per_result_byte_limit() {
    // §24.2 rule 5: "a single value larger than the per-result history byte limit is not retained,
    // but it still flows through the pipeline normally."
    let mut history = ResultHistory::new(limits(16, 10_000, 1_000, 1 << 30));
    let enormous = vec![payload_of_about(100_000)];
    let outcome = history.retain(&enormous);

    assert_eq!(outcome.kept(), 0, "nothing of it is retained");
    assert!(
        outcome.truncated_for_history(),
        "and the entry says it is not the whole result (§24.3)"
    );
    assert_eq!(
        enormous.len(),
        1,
        "§24.2 rule 1: the values themselves are untouched — history never edits what it was shown"
    );
}

#[test]
fn should_leave_the_emitted_output_complete_when_the_retained_copy_is_truncated() {
    // §60.6: "A pipeline producing more than history limits MUST still emit its complete result
    // to the user/downstream. Only retained history is truncated/evicted." The store is handed a
    // borrow and never a `Vec` it could shorten, so the emitted values cannot be its to change.
    let mut history = ResultHistory::new(limits(16, 2, 64, 128));
    let emitted = result_of(50, 4_000);
    let before: Vec<u64> = emitted.iter().map(estimated_size).collect();

    let outcome = history.retain(&emitted);
    assert!(outcome.truncated_for_history());

    assert_eq!(
        emitted.len(),
        50,
        "§60.6: the emitted result still holds fifty values"
    );
    assert_eq!(
        emitted.iter().map(estimated_size).collect::<Vec<_>>(),
        before,
        "§60.6: and not one of them was altered to fit the cache"
    );
}

#[test]
fn should_never_fail_the_command_however_far_past_its_ceilings_a_result_is() {
    // §24.2: history is a cache, so it evicts. §21.3 forbids mixing that with the other lawful
    // response, and the type is where the two are kept apart: `retain` cannot return an error.
    let mut history = ResultHistory::new(limits(1, 1, 1, 1));
    for _ in 0..20 {
        let outcome = history.retain(&result_of(1_000, 1_000));
        assert_eq!(outcome.kept(), 0);
        assert!(outcome.truncated_for_history());
    }
    assert!(
        history.retained_bytes() <= 1,
        "a history of one byte retains nothing and still answers"
    );
}
