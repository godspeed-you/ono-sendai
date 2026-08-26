//! `group`: bucket a stream that ends, by a key (spec §53).

use std::collections::HashMap;
use std::sync::Arc;

use ono_value::{RecordValue, Value};

use crate::order::KeyRepr;
use crate::schemas::{grouping_schema, provenance};
use crate::transforms::filter::with_identity;
use crate::{Boundedness, InputRequirement, KeyFn, Transform, ValueStream, Window};

/// Groups a finite stream by a key, yielding one `Group` record per key (spec §53).
///
/// Groups appear in the order their key was first seen, so the output is deterministic without
/// imposing an order on keys that may not be orderable. An unknown key gets a group of its own
/// rather than being dropped: "which of these has no owner" is a question worth being able to
/// ask, and answering it by omission would be the silent-loss spec §10.5 forbids.
pub struct Group {
    key: Box<dyn KeyFn>,
    window: Option<Window>,
}

impl Group {
    /// Groups by `key`.
    #[must_use]
    pub fn new(key: impl KeyFn) -> Self {
        Self {
            key: Box::new(key),
            window: None,
        }
    }

    /// Bounds an otherwise endless stream to its first `window` values (spec §11.1).
    #[must_use]
    pub const fn with_window(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }
}

impl Transform for Group {
    fn name(&self) -> &'static str {
        "group"
    }

    fn input_requirement(&self) -> InputRequirement {
        InputRequirement::Bounded(self.window)
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        input.stage(Boundedness::Bounded, move |mut input, sink| async move {
            let schema = match grouping_schema() {
                Ok(schema) => schema,
                Err(error) => {
                    let _ = sink.fail(error).await;
                    return;
                }
            };

            let mut order: Vec<KeyRepr> = Vec::new();
            let mut buckets: HashMap<KeyRepr, (Value, Vec<Value>)> = HashMap::new();
            while let Some(value) = input.next_value(&sink).await {
                match self.key.key(&value) {
                    Ok(key) => {
                        let repr = KeyRepr::of(&key);
                        match buckets.get_mut(&repr) {
                            Some((_, members)) => members.push(value),
                            None => {
                                order.push(repr.clone());
                                buckets.insert(repr, (key, vec![value]));
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

            for repr in order {
                let Some((key, members)) = buckets.remove(&repr) else {
                    continue;
                };
                let record = RecordValue::builder(Arc::clone(&schema), provenance(&schema))
                    .set("key", key)
                    .and_then(|builder| builder.set("count", Value::Int(members.len() as i128)))
                    .and_then(|builder| builder.set("items", Value::list(members)));
                let record = match record {
                    Ok(builder) => builder.build().into_value(),
                    Err(error) => {
                        let _ = sink.fail(error).await;
                        return;
                    }
                };
                if sink.send(record).await.is_err() {
                    return;
                }
            }
        })
    }
}

crate::transform::debug_as_name!(Group);
