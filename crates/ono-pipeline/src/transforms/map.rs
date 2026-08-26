//! `each`: map one value to zero, one or many (spec §53).

use crate::transforms::filter::with_identity;
use crate::{Mapper, Transform, ValueStream};

/// Maps each value through an already-resolved body.
///
/// Spec §53 warns that `each` "must be specified carefully to avoid accidental nested streams".
/// The outputs of one body are a flat list and are emitted flat, so `each` never produces a
/// stream of streams; a body that wants nesting builds a list value explicitly.
///
/// A body that fails does so for one value: the value is dropped, the failure is reported with
/// that value's identity, and the rest of the stream keeps running (spec §16.5).
pub struct Each {
    body: Box<dyn Mapper>,
}

impl Each {
    /// Maps every value through `body`.
    #[must_use]
    pub fn new(body: impl Mapper) -> Self {
        Self {
            body: Box::new(body),
        }
    }
}

impl Transform for Each {
    fn name(&self) -> &'static str {
        "each"
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        // One value in, zero or more out: a stream that ends still ends.
        let boundedness = input.boundedness();
        input.stage(boundedness, move |mut input, sink| async move {
            while let Some(value) = input.next_value(&sink).await {
                match self.body.map(&value) {
                    Ok(outputs) => {
                        for output in outputs {
                            if sink.send(output).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        if sink.fail(with_identity(error, &value)).await.is_err() {
                            return;
                        }
                    }
                }
            }
        })
    }
}

crate::transform::debug_as_name!(Each);
