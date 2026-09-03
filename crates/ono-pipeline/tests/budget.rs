//! The shared resource budget of spec v0.4.1 §21 and the materialization contract of §22.
//!
//! §21.1 asks for *one* abstraction behind every operation that retains or materializes values,
//! and names the way such an abstraction is usually defeated: a production path that reaches an
//! unlimited budget through a default constructor. §21.3 fixes what happens at the ceiling — the
//! operation stops with a structured error, or a documented cache evicts, and the two are never
//! mixed by accident.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

mod common;

use std::marker::PhantomData;

use common::{demo, within};
use ono_core::{ErrorCode, ErrorKind};
use ono_pipeline::{
    Boundedness, Budget, Ceiling, Diff, Group, Join, MATERIALIZE_MAX_BYTES, MATERIALIZE_MAX_ITEMS,
    MaterializationLimits, Measure, PipelineConfig, Sort, ValueStream, materialize,
};
use ono_value::Value;

/// Answers whether `T` implements `Default`, without requiring that it does.
///
/// An inherent associated constant outranks a trait's, so `Probe<T>` reads `true` only when the
/// bound on the inherent block is satisfied. This is how a test can assert that a type offers no
/// `Default` — the property §21.1 asks for — without naming a constructor that must not compile.
struct Probe<T>(PhantomData<T>);

trait DefaultProbe {
    const IMPLEMENTS_DEFAULT: bool = false;
}

impl<T> DefaultProbe for Probe<T> {}

impl<T: Default> Probe<T> {
    const IMPLEMENTS_DEFAULT: bool = true;
}

/// A value of a stated estimated size, for spending a byte budget predictably.
fn payload_of_about(bytes: usize) -> Value {
    Value::string(&"x".repeat(bytes))
}

#[test]
fn should_require_both_an_item_and_a_byte_ceiling_when_a_budget_is_constructed() {
    let budget = Budget::of("sort", 10, 4096);
    assert_eq!(budget.max_items(), 10);
    assert_eq!(budget.max_bytes(), 4096);
    assert_eq!(budget.consumed_items(), 0);
    assert_eq!(budget.consumed_bytes(), 0);

    // §22.2: "A value of zero means 'no values permitted', not unlimited."
    let mut nothing = Budget::of("sort", 0, 0);
    let refused = nothing
        .charge(&Value::Null)
        .expect_err("a zero ceiling admits nothing");
    assert_eq!(refused.ceiling(), Ceiling::Items);
    assert_eq!(refused.configured(), 0);
}

#[test]
#[allow(
    clippy::assertions_on_constants,
    reason = "the constant is the property under test: whether `Budget` implements `Default` is \
              settled at compile time, and asserting on it is how the test reads that answer"
)]
fn should_offer_no_default_that_leaves_a_budget_unlimited() {
    assert!(
        Probe::<u64>::IMPLEMENTS_DEFAULT,
        "the probe cannot see a `Default` that is there, so its answer about `Budget` means \
         nothing"
    );
    assert!(
        !Probe::<Budget>::IMPLEMENTS_DEFAULT,
        "`Budget` implements `Default`. §21.1: a production interactive path must not be able to \
         obtain an unlimited budget through a default constructor, and a `Default` is exactly how \
         that happens"
    );

    // Every way to obtain a budget, and the ceilings each one states.
    let budgets = [
        ("Budget::of", Budget::of("stage", 1, 1)),
        ("Budget::materialization", Budget::materialization("sort")),
        ("Budget::command_captures", Budget::command_captures()),
    ];
    for (name, budget) in budgets {
        assert!(
            budget.max_items() < u64::MAX && budget.max_bytes() < u64::MAX,
            "`{name}` answered a ceiling of u64::MAX, which is unlimited spelled differently \
             (§21.1, §22.2)"
        );
    }

    assert_eq!(Budget::materialization("sort").max_items(), 100_000);
    assert_eq!(Budget::materialization("sort").max_bytes(), 134_217_728);
    assert_eq!(MATERIALIZE_MAX_ITEMS, 100_000);
    assert_eq!(MATERIALIZE_MAX_BYTES, 134_217_728);
}

#[test]
fn should_refuse_rather_than_truncate_when_a_budget_is_exceeded() {
    let mut budget = Budget::of("collect", 2, 1 << 30);
    budget.charge(&Value::Int(1)).expect("the first value fits");
    budget
        .charge(&Value::Int(2))
        .expect("the second value fits");

    let refused = budget
        .charge(&Value::Int(3))
        .expect_err("§21.3: the third value is refused, not dropped");
    assert_eq!(refused.ceiling(), Ceiling::Items);
    assert_eq!(refused.configured(), 2);
    assert_eq!(refused.observed(), 3);
    assert_eq!(refused.stage(), "collect");

    // The refusal does not admit the value it refused: a budget that keeps collecting while
    // warning is the behaviour §21.3 forbids.
    assert_eq!(budget.consumed_items(), 2);

    let error = refused.into_error();
    assert_eq!(error.code(), ErrorCode::ResourceItemLimit);
    assert_eq!(error.code().kind(), ErrorKind::Resource);
    assert!(
        error.help().is_some(),
        "a refusal tells the user what to do about it (§54.1)"
    );
}

#[test]
fn should_charge_a_byte_ceiling_from_the_estimated_size_of_what_it_admits() {
    let mut budget = Budget::of("collect", 1_000_000, 64 * 1024);
    let refused = loop {
        match budget.charge(&payload_of_about(8 * 1024)) {
            Ok(()) => continue,
            Err(refusal) => break refusal,
        }
    };

    assert_eq!(refused.ceiling(), Ceiling::Bytes);
    assert_eq!(refused.configured(), 64 * 1024);
    assert!(
        refused.observed() > 64 * 1024,
        "the refusal reports the consumption that crossed the ceiling, {} (§21.4)",
        refused.observed()
    );
    assert!(
        budget.consumed_items() < 1_000,
        "the byte ceiling stopped it long before the item ceiling, at {} values",
        budget.consumed_items()
    );
    assert_eq!(refused.into_error().code(), ErrorCode::ResourceByteLimit);
}

#[test]
fn should_charge_a_nested_budget_against_the_budget_it_was_taken_from() {
    // §23.4: nested captures must not each independently consume the full global allowance.
    let mut parent = Budget::of("command", 100, 10_000);
    {
        let mut child = parent.child("inner capture");
        assert!(
            child.max_bytes() <= 10_000,
            "a child budget cannot be larger than what remains of its parent"
        );
        child.charge(&payload_of_about(4_000)).expect("it fits");
        parent.absorb(child);
    }

    let sibling = parent.child("second capture");
    let spent = sibling.max_bytes();
    assert!(
        spent < 10_000,
        "the second capture started with the whole allowance again, {spent}; §23.4 calls that \
         each capture independently consuming the full global allowance"
    );
}

// --- §22: the budget-aware materialization helper (issue #67) ---

/// The first failure a stream reports, drained to the end so nothing is left running.
async fn refusal_of(mut stream: ValueStream) -> ono_value::ErrorValue {
    let mut first = None;
    while let Some(event) = stream.recv().await {
        if let ono_pipeline::StreamEvent::Failure(error) = event {
            first.get_or_insert(error);
        }
    }
    first.expect("the stage was expected to refuse and did not")
}

/// A pipeline whose materializers may collect `items` values and `bytes` bytes.
fn limited_to(items: u64, bytes: u64) -> PipelineConfig {
    PipelineConfig::new().with_materialization_limits(MaterializationLimits::new(items, bytes))
}

#[tokio::test]
async fn should_refuse_the_hundred_thousand_and_first_value_a_global_operation_collects() {
    within(async {
        // §60.4: a finite source of 100 001 small values sent to a default global materializer.
        let sorted = ValueStream::from_values((0..100_001).map(Value::Int))
            .transform(Sort::new(|value: &Value| Ok(value.clone())))
            .expect("a finite source may be sorted");
        let error = refusal_of(sorted).await;

        assert_eq!(error.code(), ErrorCode::ResourceItemLimit);
        assert_eq!(error.code().kind(), ErrorKind::Resource);
        let rendered = error.render_full();
        assert!(
            rendered.contains("100000") || rendered.contains("100,000"),
            "§21.4: the refusal carries the configured limit: {rendered}"
        );
        assert!(
            !rendered.contains("Int(") && error.metadata().get("values").is_none(),
            "§21.4: a resource error must not dump the retained values: {rendered}"
        );

        // The same ceiling, reached through the helper the evaluator calls (§30.2).
        let stream = ValueStream::from_values((0..100_001).map(Value::Int));
        let through_helper = materialize(stream, Budget::materialization("collect"))
            .await
            .expect_err("the helper enforces the same ceiling");
        assert_eq!(through_helper.code(), ErrorCode::ResourceItemLimit);
    })
    .await;
}

#[tokio::test]
async fn should_refuse_on_the_byte_ceiling_when_a_few_large_values_exceed_it() {
    within(async {
        // §60.5: "A small number of individually large values whose estimated total exceeds
        // 128 MiB MUST hit the byte limit even though the item count remains far below 100000."
        let one_mib = || payload_of_about(1 << 20);
        let sorted = ValueStream::from_values_with(
            limited_to(MATERIALIZE_MAX_ITEMS, 16 << 20),
            (0..200).map(|_| one_mib()),
        )
        .transform(Sort::new(|value: &Value| Ok(value.clone())))
        .expect("a finite source may be sorted");
        let error = refusal_of(sorted).await;

        assert_eq!(error.code(), ErrorCode::ResourceByteLimit);
        assert_eq!(error.code().kind(), ErrorKind::Resource);
        assert_eq!(
            error
                .metadata()
                .get("consumed")
                .and_then(|value| match value {
                    Value::Int(bytes) => Some(*bytes > 16 * 1024 * 1024),
                    _ => None,
                }),
            Some(true),
            "§21.4: the refusal reports the consumption that crossed the ceiling"
        );

        let stream = ValueStream::from_values((0..200).map(|_| one_mib()));
        let budget = Budget::of("collect", MATERIALIZE_MAX_ITEMS, 16 << 20);
        let through_helper = materialize(stream, budget)
            .await
            .expect_err("sixteen MiB of budget does not hold two hundred MiB of values");
        assert_eq!(through_helper.code(), ErrorCode::ResourceByteLimit);
    })
    .await;
}

#[tokio::test]
async fn should_bound_every_transform_that_buffers_its_whole_input() {
    within(async {
        // Appendix E's global classes: reorder, grouping, relational and exact aggregates all
        // hold their input, so §2.4's byte bound has to reach every one of them.
        let names: [&str; 5] = ["sort", "group", "join", "diff", "measure"];
        for name in names {
            let source = || {
                ValueStream::from_values_with(
                    limited_to(4, 1 << 30),
                    (0..64).map(|index| demo(index, "process", Some(1.0))),
                )
            };
            let staged = match name {
                "sort" => source().transform(Sort::new(|value: &Value| Ok(value.clone()))),
                "group" => source().transform(Group::new(|value: &Value| Ok(value.clone()))),
                // `join` and `diff` stream one side and hold the other, so the side they hold
                // is the collection §2.4 bounds.
                "join" => source().transform(Join::new(
                    (0..64).map(|index| demo(index, "process", None)),
                    |value: &Value| Ok(value.clone()),
                )),
                "diff" => source().transform(Diff::new(
                    (0..64).map(|index| demo(index, "process", None)),
                    |value: &Value| Ok(value.clone()),
                )),
                _ => source().transform(
                    Measure::new(|_: &Value| Ok(Value::Int(1))).with_percentiles([50.0]),
                ),
            };
            let error = refusal_of(staged.expect("a finite source may be transformed")).await;
            assert_eq!(
                error.code().kind(),
                ErrorKind::Resource,
                "`{name}` buffered 64 values under a 4-value budget without refusing; §65.6 \
                 calls a count that ignores its ceiling a limit that is not one"
            );
        }
    })
    .await;
}

#[tokio::test]
async fn should_admit_everything_a_finite_stream_holds_when_it_fits_the_budget() {
    within(async {
        let stream = ValueStream::from_values((0..1_000).map(Value::Int));
        let collected = materialize(stream, Budget::materialization("sort"))
            .await
            .expect("a thousand small values fit the default budget");
        assert_eq!(collected.values().len(), 1_000);
    })
    .await;
}

#[tokio::test]
async fn should_refuse_an_unbounded_stream_before_it_consumes_a_value() {
    within(async {
        let stream = ValueStream::spawn(
            PipelineConfig::new().with_capacity(4),
            Boundedness::Unbounded,
            |sink| async move {
                let mut next: i128 = 0;
                while sink.send(Value::Int(next)).await.is_ok() {
                    next += 1;
                }
            },
        );
        let error = materialize(stream, Budget::materialization("sort"))
            .await
            .expect_err("§22.3: a materializer refuses a declared-unbounded upstream");
        assert_eq!(error.code(), ErrorCode::StreamUnboundedOperation);
    })
    .await;
}
