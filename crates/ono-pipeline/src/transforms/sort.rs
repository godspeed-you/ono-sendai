//! `sort`: order a stream that ends (spec §53, ADR-0014).

use ono_value::Value;

use crate::order::{Direction, compare_in};
use crate::transforms::filter::with_identity;
use crate::{Boundedness, InputRequirement, KeyFn, Transform, ValueStream, Window};

/// Orders a finite stream by a key.
///
/// The sort is stable in both directions: values with equal keys keep the order they arrived in,
/// so `sort cpu desc | sort name` composes the way a user expects. Unknown keys go last
/// ascending and first descending, so `null` is never mistaken for the smallest value
/// (ADR-0014).
///
/// A value whose key cannot be read is excluded and its failure reported: an unorderable row is
/// a partial failure, not a reason to fail the whole pipeline (spec §16.5).
pub struct Sort {
    key: Box<dyn KeyFn>,
    direction: Direction,
    window: Option<Window>,
}

impl Sort {
    /// Orders ascending by `key`.
    #[must_use]
    pub fn new(key: impl KeyFn) -> Self {
        Self {
            key: Box::new(key),
            direction: Direction::Ascending,
            window: None,
        }
    }

    /// Orders descending instead.
    #[must_use]
    pub const fn descending(mut self) -> Self {
        self.direction = Direction::Descending;
        self
    }

    /// Bounds an otherwise endless stream to its first `window` values (spec §11.1).
    #[must_use]
    pub const fn with_window(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }
}

impl Transform for Sort {
    fn name(&self) -> &'static str {
        "sort"
    }

    fn input_requirement(&self) -> InputRequirement {
        InputRequirement::Bounded(self.window)
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        input.stage(Boundedness::Bounded, move |mut input, sink| async move {
            let mut budget = input.budget_for("sort");
            let mut keyed: Vec<(Value, Value)> = Vec::new();
            while let Some(value) = input.next_value(&sink).await {
                if let Err(exceeded) = budget.charge(&value) {
                    let _ = sink.fail(exceeded.into_error()).await;
                    return;
                }
                match self.key.key(&value) {
                    Ok(key) => keyed.push((key, value)),
                    Err(error) => {
                        if sink.fail(with_identity(error, &value)).await.is_err() {
                            return;
                        }
                    }
                }
            }
            keyed.sort_by(|left, right| compare_in(self.direction, &left.0, &right.0));
            for (_, value) in keyed {
                if sink.send(value).await.is_err() {
                    return;
                }
            }
        })
    }
}

crate::transform::debug_as_name!(Sort);
