//! `take`, `skip`, `first` and `last`: the transforms that cut a stream by position (spec §53).

use crate::stream::forward_at_most;
use crate::{Boundedness, InputRequirement, Transform, ValueStream, Window};

/// Emits the first `count` values and then stops (spec §53: streaming and lazy).
///
/// Its output is bounded whatever its input was, which is what makes
/// `get log --follow | take 100 | sort time` legal. Stopping also drops the upstream channel, so
/// an endless producer learns to stop the way `yes | head -1` does.
pub struct Take {
    count: usize,
}

impl Take {
    /// Takes the first `count` values.
    #[must_use]
    pub const fn new(count: usize) -> Self {
        Self { count }
    }
}

impl Transform for Take {
    fn name(&self) -> &'static str {
        "take"
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        let count = self.count;
        input.stage(Boundedness::Bounded, move |input, sink| async move {
            forward_at_most(input, &sink, count).await;
        })
    }
}

/// Drops the first `count` values and emits the rest (spec §53).
pub struct Skip {
    count: usize,
}

impl Skip {
    /// Skips the first `count` values.
    #[must_use]
    pub const fn new(count: usize) -> Self {
        Self { count }
    }
}

impl Transform for Skip {
    fn name(&self) -> &'static str {
        "skip"
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        // Skipping a prefix cannot make an endless stream end.
        let boundedness = input.boundedness();
        let count = self.count;
        input.stage(boundedness, move |mut input, sink| async move {
            let mut seen = 0;
            while let Some(value) = input.next_value(&sink).await {
                seen += 1;
                if seen <= count {
                    continue;
                }
                if sink.send(value).await.is_err() {
                    return;
                }
            }
        })
    }
}

/// Emits the first value and stops. `take 1` phrased as a question about one object.
pub struct First;

impl First {
    /// The first value of the stream.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for First {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for First {
    fn name(&self) -> &'static str {
        "first"
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        input.stage(Boundedness::Bounded, move |input, sink| async move {
            forward_at_most(input, &sink, 1).await;
        })
    }
}

/// Emits the last value of a stream that ends.
///
/// It holds one value at a time rather than the whole stream, but it still needs the input to
/// end: a stream that never ends has no last value, and waiting for one would hang instead of
/// answering (spec §11.1).
pub struct Last {
    window: Option<Window>,
}

impl Last {
    /// The last value of the stream.
    #[must_use]
    pub const fn new() -> Self {
        Self { window: None }
    }

    /// Bounds an otherwise endless stream to its first `window` values (spec §11.1).
    #[must_use]
    pub const fn with_window(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }
}

impl Default for Last {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for Last {
    fn name(&self) -> &'static str {
        "last"
    }

    fn input_requirement(&self) -> InputRequirement {
        InputRequirement::Bounded(self.window)
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        input.stage(Boundedness::Bounded, move |mut input, sink| async move {
            let mut last = None;
            while let Some(value) = input.next_value(&sink).await {
                last = Some(value);
            }
            if let Some(value) = last {
                let _ = sink.send(value).await;
            }
        })
    }
}

crate::transform::debug_as_name!(Take, Skip, First, Last);
