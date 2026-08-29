//! `join` and `diff`: the transforms that relate two sets of objects (spec §53).

use std::collections::HashMap;
use std::sync::Arc;

use ono_value::{RecordValue, Value};

use crate::order::KeyRepr;
use crate::schemas::{diff_schema, join_schema, provenance};
use crate::transforms::filter::with_identity;
use crate::{Boundedness, InputRequirement, KeyFn, StreamSink, Transform, ValueStream, Window};

/// Which unmatched rows a join keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JoinKind {
    /// Only rows that matched on both sides.
    #[default]
    Inner,
    /// Every left row, matched or not.
    Left,
    /// Every right row, matched or not.
    Right,
    /// Every row from either side.
    Outer,
}

impl JoinKind {
    const fn keeps_unmatched_left(self) -> bool {
        matches!(self, JoinKind::Left | JoinKind::Outer)
    }

    const fn keeps_unmatched_right(self) -> bool {
        matches!(self, JoinKind::Right | JoinKind::Outer)
    }
}

/// Joins a finite stream against a set of values already in hand (spec §53).
///
/// The right side arrives materialised because it is a sub-pipeline the evaluator has already
/// run — `get process | join (get socket) --on pid` evaluates the parenthesised pipeline first.
///
/// The result is a record with the key and both sides rather than a flat merge. Spec §53 says
/// outright that the shape of `join` "should not be frozen until common shell use cases justify
/// it", and merging two records would force a rule for two fields of the same name before there
/// is a use case to judge it by.
///
/// An unknown key matches nothing, the way SQL treats null, so a row whose key could not be read
/// never joins to an unrelated row that also could not be read. Duplicate keys pair every
/// combination, left order outermost, which is the ordinary relational answer and is
/// deterministic.
pub struct Join {
    right: Vec<Value>,
    left_key: Box<dyn KeyFn>,
    right_key: Option<Box<dyn KeyFn>>,
    kind: JoinKind,
    window: Option<Window>,
}

impl Join {
    /// Joins against `right`, keying both sides with `key`.
    #[must_use]
    pub fn new(right: impl IntoIterator<Item = Value>, key: impl KeyFn) -> Self {
        Self {
            right: right.into_iter().collect(),
            left_key: Box::new(key),
            right_key: None,
            kind: JoinKind::Inner,
            window: None,
        }
    }

    /// Keys the right side differently from the left.
    #[must_use]
    pub fn with_right_key(mut self, key: impl KeyFn) -> Self {
        self.right_key = Some(Box::new(key));
        self
    }

    /// Keeps unmatched rows according to `kind`.
    #[must_use]
    pub const fn with_kind(mut self, kind: JoinKind) -> Self {
        self.kind = kind;
        self
    }

    /// Bounds an otherwise endless stream to its first `window` values (spec §11.1).
    #[must_use]
    pub const fn with_window(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }
}

impl Transform for Join {
    fn name(&self) -> &'static str {
        "join"
    }

    fn input_requirement(&self) -> InputRequirement {
        InputRequirement::Bounded(self.window)
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        input.stage(Boundedness::Bounded, move |mut input, sink| async move {
            let schema = match join_schema() {
                Ok(schema) => schema,
                Err(error) => {
                    let _ = sink.fail(error).await;
                    return;
                }
            };
            let right_key = self.right_key.as_ref().unwrap_or(&self.left_key);

            let mut buckets: HashMap<KeyRepr, Vec<usize>> = HashMap::new();
            let mut right_keys: Vec<Option<Value>> = Vec::with_capacity(self.right.len());
            for (index, value) in self.right.iter().enumerate() {
                match right_key.key(value) {
                    Ok(key) if key.is_null() => right_keys.push(None),
                    Ok(key) => {
                        buckets.entry(KeyRepr::of(&key)).or_default().push(index);
                        right_keys.push(Some(key));
                    }
                    Err(error) => {
                        if sink.fail(with_identity(error, value)).await.is_err() {
                            return;
                        }
                        right_keys.push(None);
                    }
                }
            }

            let mut matched = vec![false; self.right.len()];
            while let Some(value) = input.next_value(&sink).await {
                let key = match self.left_key.key(&value) {
                    Ok(key) => key,
                    Err(error) => {
                        if sink.fail(with_identity(error, &value)).await.is_err() {
                            return;
                        }
                        continue;
                    }
                };
                let partners = if key.is_null() {
                    Vec::new()
                } else {
                    buckets.get(&KeyRepr::of(&key)).cloned().unwrap_or_default()
                };

                if partners.is_empty() {
                    if self.kind.keeps_unmatched_left()
                        && emit(&sink, &schema, key, Some(value), None).await.is_err()
                    {
                        return;
                    }
                    continue;
                }
                for index in partners {
                    if let Some(slot) = matched.get_mut(index) {
                        *slot = true;
                    }
                    let partner = self.right.get(index).cloned();
                    if emit(&sink, &schema, key.clone(), Some(value.clone()), partner)
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }

            if !self.kind.keeps_unmatched_right() {
                return;
            }
            for (index, value) in self.right.iter().enumerate() {
                if matched.get(index).copied().unwrap_or(false) {
                    continue;
                }
                let key = right_keys
                    .get(index)
                    .cloned()
                    .flatten()
                    .unwrap_or(Value::Null);
                if emit(&sink, &schema, key, None, Some(value.clone()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        })
    }
}

async fn emit(
    sink: &StreamSink,
    schema: &Arc<ono_value::Schema>,
    key: Value,
    left: Option<Value>,
    right: Option<Value>,
) -> Result<(), crate::SinkClosed> {
    let record = RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set("key", key)
        .and_then(|builder| builder.set("left", crate::schemas::or_null(left)))
        .and_then(|builder| builder.set("right", crate::schemas::or_null(right)));
    match record {
        Ok(builder) => sink.send(builder.build().into_value()).await,
        Err(error) => sink.fail(error).await,
    }
}

/// Compares a finite stream against an earlier snapshot, by identity (spec §53).
///
/// The stream is the new state and `previous` is the old one, matching `get service | diff @-1`.
/// Only differences are reported: an object present in both and unchanged is not a change, and
/// listing it would bury the three rows that matter under four hundred that do not.
///
/// Changes come out in the new snapshot's order, followed by the removals in the old snapshot's
/// order, so two runs over the same data produce the same output.
pub struct Diff {
    previous: Vec<Value>,
    identity: Box<dyn KeyFn>,
    window: Option<Window>,
}

impl Diff {
    /// Compares against `previous`, identifying objects with `identity`.
    #[must_use]
    pub fn new(previous: impl IntoIterator<Item = Value>, identity: impl KeyFn) -> Self {
        Self {
            previous: previous.into_iter().collect(),
            identity: Box::new(identity),
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

impl Transform for Diff {
    fn name(&self) -> &'static str {
        "diff"
    }

    fn input_requirement(&self) -> InputRequirement {
        InputRequirement::Bounded(self.window)
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        input.stage(Boundedness::Bounded, move |mut input, sink| async move {
            let schema = match diff_schema() {
                Ok(schema) => schema,
                Err(error) => {
                    let _ = sink.fail(error).await;
                    return;
                }
            };

            let mut before: HashMap<KeyRepr, usize> = HashMap::new();
            for (index, value) in self.previous.iter().enumerate() {
                match self.identity.key(value) {
                    Ok(key) => {
                        before.insert(KeyRepr::of(&key), index);
                    }
                    Err(error) => {
                        if sink.fail(with_identity(error, value)).await.is_err() {
                            return;
                        }
                    }
                }
            }

            let mut seen = vec![false; self.previous.len()];
            while let Some(value) = input.next_value(&sink).await {
                let key = match self.identity.key(&value) {
                    Ok(key) => key,
                    Err(error) => {
                        if sink.fail(with_identity(error, &value)).await.is_err() {
                            return;
                        }
                        continue;
                    }
                };
                match before.get(&KeyRepr::of(&key)).copied() {
                    Some(index) => {
                        if let Some(slot) = seen.get_mut(index) {
                            *slot = true;
                        }
                        let old = self.previous.get(index);
                        // The comparison is about the objects, not about the readings: two
                        // snapshots of one unchanged object differ in the instant each was
                        // observed, which is provenance and not state (ADR-0229).
                        if old.is_some_and(|old| old.same_data(&value)) {
                            continue;
                        }
                        if change(&sink, &schema, "changed", key, Some(value), old.cloned())
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    None => {
                        if change(&sink, &schema, "added", key, Some(value), None)
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }

            for (index, value) in self.previous.iter().enumerate() {
                if seen.get(index).copied().unwrap_or(false) {
                    continue;
                }
                let key = self.identity.key(value).unwrap_or(Value::Null);
                if change(&sink, &schema, "removed", key, None, Some(value.clone()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        })
    }
}

async fn change(
    sink: &StreamSink,
    schema: &Arc<ono_value::Schema>,
    kind: &str,
    key: Value,
    left: Option<Value>,
    right: Option<Value>,
) -> Result<(), crate::SinkClosed> {
    let record = RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set("change", Value::string(kind))
        .and_then(|builder| builder.set("key", key))
        .and_then(|builder| builder.set("left", crate::schemas::or_null(left)))
        .and_then(|builder| builder.set("right", crate::schemas::or_null(right)));
    match record {
        Ok(builder) => sink.send(builder.build().into_value()).await,
        Err(error) => sink.fail(error).await,
    }
}

crate::transform::debug_as_name!(Join, Diff);
