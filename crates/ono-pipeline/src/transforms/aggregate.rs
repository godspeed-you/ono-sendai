//! `count`, `measure` and `reduce`: the transforms that answer with one value (spec §53).

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_value::{ErrorValue, MapValue, RecordValue, Value};

use crate::order::compare_keys;
use crate::schemas::{measure_schema, provenance};
use crate::transforms::filter::with_identity;
use crate::{
    Boundedness, Folder, InputRequirement, KeyFn, StreamSink, Transform, ValueStream, Window,
};

/// Counts the values of a stream that ends (spec §53).
///
/// Nulls are skipped and counted separately, so `count` answers "how many are there" rather than
/// "how many rows arrived, some of which were nothing" — and the number it skipped is on the
/// pipeline's diagnostics rather than lost (ADR-0014).
pub struct Count {
    window: Option<Window>,
}

impl Count {
    /// Counts the values.
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

impl Default for Count {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for Count {
    fn name(&self) -> &'static str {
        "count"
    }

    fn input_requirement(&self) -> InputRequirement {
        InputRequirement::Bounded(self.window)
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        input.stage(Boundedness::Bounded, move |mut input, sink| async move {
            let mut counted: i128 = 0;
            while let Some(value) = input.next_value(&sink).await {
                if value.is_null() {
                    sink.diagnostics().record_skipped_null();
                } else {
                    counted += 1;
                }
            }
            // Spec §35.3: what could not be read is not zero. A stream that produced nothing but
            // a failure has no count, and answering `0` would turn a refusal into data
            // (ADR-0221). A stream that lost *some* of its objects still counts the rest, which
            // is spec §16.5's partial failure.
            if counted == 0 && input.saw_failure() {
                return;
            }
            let _ = sink.send(Value::Int(counted)).await;
        })
    }
}

/// Computes statistics over a stream (spec §53): in constant state, or over the held distribution.
///
/// The results stay typed: the sum of byte sizes is a byte size, the mean of durations is a
/// duration. The one exception is `stddev`, which is a plain number in the key's own scale —
/// bytes, seconds, percent points — because a standard deviation is derived from a squared
/// quantity and Ono models no squared dimension to name the result in.
///
/// Nulls are skipped and reported in the `skipped` field, so an average is never quietly
/// computed over a different population than the user thinks (ADR-0014). A value that is not a
/// number at all is a partial failure: it is reported and excluded, and the rest is still
/// measured (spec §16.5).
///
/// Percentiles use nearest-rank, so every reported percentile is a value that actually occurred
/// rather than an interpolation between two of them — which also keeps them typed.
pub struct Measure {
    key: Box<dyn KeyFn>,
    percentiles: Vec<f64>,
    window: Option<Window>,
    /// Whether the distribution is held — every sample, for the median and the percentiles — or
    /// only the state the other statistics fold into (ADR-0953).
    distribution: bool,
}

impl Measure {
    /// Measures the values `key` extracts, holding every sample so the median and any requested
    /// percentile can be reported. Requires input that ends and materializes within budget.
    #[must_use]
    pub fn new(key: impl KeyFn) -> Self {
        Self {
            key: Box::new(key),
            percentiles: Vec::new(),
            window: None,
            distribution: true,
        }
    }

    /// Measures the values `key` extracts in constant state: count, sum, mean, min, max and the
    /// standard deviation fold into a fixed amount of state, so nothing is held and no
    /// materialization budget is charged; the median and the percentiles stay null (ADR-0953).
    ///
    /// A stream that ends is answered once, at its end. A stream declared unbounded is a legal
    /// input — §22.3's refusal is for stages that need the end — and is answered after every
    /// value with the statistics so far, so a consumer that bounds the answers is answered while
    /// the source is still open.
    #[must_use]
    pub fn constant_state(key: impl KeyFn) -> Self {
        Self {
            key: Box::new(key),
            percentiles: Vec::new(),
            window: None,
            distribution: false,
        }
    }

    /// Also reports these percentiles, each given as a number between 0 and 100.
    #[must_use]
    pub fn with_percentiles(mut self, percentiles: impl IntoIterator<Item = f64>) -> Self {
        self.percentiles = percentiles.into_iter().collect();
        self
    }

    /// Bounds an otherwise endless stream to its first `window` values (spec §11.1).
    #[must_use]
    pub const fn with_window(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }
}

impl Transform for Measure {
    fn name(&self) -> &'static str {
        "measure"
    }

    fn materializes(&self) -> bool {
        true
    }

    fn input_requirement(&self) -> InputRequirement {
        if self.distribution {
            InputRequirement::Bounded(self.window)
        } else {
            InputRequirement::Streaming
        }
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        if !self.distribution {
            return running(self.key, input);
        }
        input.stage(Boundedness::Bounded, move |mut input, sink| async move {
            let schema = match measure_schema() {
                Ok(schema) => schema,
                Err(error) => {
                    let _ = sink.fail(error).await;
                    return;
                }
            };

            // `measure` retains a sample per value only because the percentiles of §53 need the
            // whole distribution; count, min, max and mean would be constant-state (Appendix E).
            let mut budget = input.budget_for("measure");
            let mut samples: Vec<Value> = Vec::new();
            let mut skipped: i128 = 0;
            while let Some(value) = input.next_value(&sink).await {
                if let Err(exceeded) = budget.charge(&value) {
                    let _ = sink.fail(exceeded.into_error()).await;
                    return;
                }
                match self.key.key(&value) {
                    Ok(Value::Null) => {
                        skipped += 1;
                        sink.diagnostics().record_skipped_null();
                    }
                    Ok(sample) if scale_of(&sample).is_none() => {
                        let error = ErrorValue::new(
                            ErrorCode::TypeMismatch,
                            format!("`measure` needs numbers, but found {}", sample.type_name()),
                        );
                        if sink.fail(with_identity(error, &value)).await.is_err() {
                            return;
                        }
                    }
                    Ok(sample) => samples.push(sample),
                    Err(error) => {
                        if sink.fail(with_identity(error, &value)).await.is_err() {
                            return;
                        }
                    }
                }
            }

            // As `count`: a summary of a stream that could not be read is not a summary of
            // nothing (spec §35.3, ADR-0221).
            if samples.is_empty() && skipped == 0 && input.saw_failure() {
                return;
            }
            let record = match summarise(&schema, samples, skipped, &self.percentiles, &sink).await
            {
                Ok(record) => record,
                Err(error) => {
                    let _ = sink.fail(error).await;
                    return;
                }
            };
            let _ = sink.send(record).await;
        })
    }
}

/// The constant-state statistics of one stream: what [`Measure::constant_state`] folds each value
/// into. Its size does not depend on how many values there were.
#[derive(Default)]
struct Running {
    count: i128,
    skipped: i128,
    sum: Option<Value>,
    min: Option<Value>,
    max: Option<Value>,
    /// Welford's running mean and sum of squared deviations, in the samples' own scale, for the
    /// population standard deviation without a second pass over samples nobody kept.
    scaled_mean: f64,
    squared_deviations: f64,
}

impl Running {
    /// Folds one sample in. A sum that cannot be formed — two incompatible units — is the
    /// stream's failure, as it is for the collecting `measure`.
    fn fold(&mut self, sample: Value, scale: f64) -> Result<(), ErrorValue> {
        self.sum = Some(match self.sum.take() {
            None => sample.clone(),
            Some(total) => total.add(&sample)?,
        });
        if self
            .min
            .as_ref()
            .is_none_or(|least| compare_keys(&sample, least).is_lt())
        {
            self.min = Some(sample.clone());
        }
        if self
            .max
            .as_ref()
            .is_none_or(|most| compare_keys(&sample, most).is_gt())
        {
            self.max = Some(sample);
        }
        self.count += 1;
        let delta = scale - self.scaled_mean;
        self.scaled_mean += delta / self.count as f64;
        self.squared_deviations += delta * (scale - self.scaled_mean);
        Ok(())
    }

    /// The statistics so far, as an `ono.measure/1` record whose median and percentiles are null.
    fn record(&self, schema: &Arc<ono_value::Schema>) -> Result<Value, ErrorValue> {
        let mean = match (&self.sum, self.count) {
            (Some(total), 1..) => total.div(&Value::Int(self.count)).ok(),
            _ => None,
        };
        let stddev = (self.count > 0)
            .then(|| Value::Float((self.squared_deviations / self.count as f64).sqrt()));
        let record = RecordValue::builder(Arc::clone(schema), provenance(schema))
            .set("count", Value::Int(self.count))?
            .set("skipped", Value::Int(self.skipped))?
            .set("sum", crate::schemas::or_null(self.sum.clone()))?
            .set("mean", crate::schemas::or_null(mean))?
            .set("median", Value::Null)?
            .set("min", crate::schemas::or_null(self.min.clone()))?
            .set("max", crate::schemas::or_null(self.max.clone()))?
            .set("stddev", crate::schemas::or_null(stddev))?
            .set("percentiles", Value::Null)?
            .build();
        Ok(record.into_value())
    }
}

/// `measure` in constant state (ADR-0953): one record at the end of a stream that ends, one after
/// every value of a stream declared unbounded.
fn running(key: Box<dyn KeyFn>, input: ValueStream) -> ValueStream {
    let boundedness = input.boundedness();
    input.stage(boundedness, move |mut input, sink| async move {
        let schema = match measure_schema() {
            Ok(schema) => schema,
            Err(error) => {
                let _ = sink.fail(error).await;
                return;
            }
        };
        let unbounded = !boundedness.is_bounded();
        let mut state = Running::default();
        while let Some(value) = input.next_value(&sink).await {
            match key.key(&value) {
                Ok(Value::Null) => {
                    state.skipped += 1;
                    sink.diagnostics().record_skipped_null();
                }
                Ok(sample) => match scale_of(&sample) {
                    None => {
                        let error = ErrorValue::new(
                            ErrorCode::TypeMismatch,
                            format!("`measure` needs numbers, but found {}", sample.type_name()),
                        );
                        if sink.fail(with_identity(error, &value)).await.is_err() {
                            return;
                        }
                        continue;
                    }
                    Some(scale) => {
                        if let Err(error) = state.fold(sample, scale) {
                            let _ = sink.fail(error).await;
                            return;
                        }
                    }
                },
                Err(error) => {
                    if sink.fail(with_identity(error, &value)).await.is_err() {
                        return;
                    }
                    continue;
                }
            }
            // A stream that does not end has no end to answer at, so the answer is the running
            // one: what is true of everything read so far, after every value.
            if unbounded {
                match state.record(&schema) {
                    Ok(record) => {
                        if sink.send(record).await.is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sink.fail(error).await;
                        return;
                    }
                }
            }
        }
        if unbounded {
            return;
        }
        // As `count`: a summary of a stream that could not be read is not a summary of nothing
        // (spec §35.3, ADR-0221).
        if state.count == 0 && state.skipped == 0 && input.saw_failure() {
            return;
        }
        match state.record(&schema) {
            Ok(record) => {
                let _ = sink.send(record).await;
            }
            Err(error) => {
                let _ = sink.fail(error).await;
            }
        }
    })
}

/// Builds the statistics record. Every field an empty stream cannot answer stays null, never a
/// fabricated zero (spec §35.3).
async fn summarise(
    schema: &Arc<ono_value::Schema>,
    mut samples: Vec<Value>,
    skipped: i128,
    percentiles: &[f64],
    sink: &StreamSink,
) -> Result<Value, ErrorValue> {
    samples.sort_by(compare_keys);
    let count = samples.len();

    let mut sum: Option<Value> = None;
    for sample in &samples {
        sum = Some(match sum {
            None => sample.clone(),
            Some(total) => match total.add(sample) {
                Ok(total) => total,
                Err(error) => {
                    let _ = sink.fail(error).await;
                    return Ok(Value::Null);
                }
            },
        });
    }

    let mean = match (&sum, count) {
        (Some(total), 1..) => total.div(&Value::Int(count as i128)).ok(),
        _ => None,
    };
    let median = median_of(&samples);
    let stddev = stddev_of(&samples);

    let mut ranks = MapValue::new();
    for percentile in percentiles {
        if let Some(value) = nearest_rank(&samples, *percentile) {
            ranks.insert(format!("p{}", trim(*percentile)).into(), value);
        }
    }

    let record = RecordValue::builder(Arc::clone(schema), provenance(schema))
        .set("count", Value::Int(count as i128))?
        .set("skipped", Value::Int(skipped))?
        .set("sum", crate::schemas::or_null(sum))?
        .set("mean", crate::schemas::or_null(mean))?
        .set("median", crate::schemas::or_null(median))?
        .set("min", crate::schemas::or_null(samples.first().cloned()))?
        .set("max", crate::schemas::or_null(samples.last().cloned()))?
        .set("stddev", crate::schemas::or_null(stddev))?
        .set(
            "percentiles",
            if percentiles.is_empty() {
                Value::Null
            } else {
                Value::Map(Arc::new(ranks))
            },
        )?
        .build();
    Ok(record.into_value())
}

/// The middle of a sorted sample. An even count takes the midpoint of the two middle values,
/// which keeps the answer between them rather than arbitrarily picking one.
fn median_of(sorted: &[Value]) -> Option<Value> {
    match sorted.len() {
        0 => None,
        length if length % 2 == 1 => sorted.get(length / 2).cloned(),
        length => {
            let lower = sorted.get(length / 2 - 1)?;
            let upper = sorted.get(length / 2)?;
            lower
                .add(upper)
                .and_then(|total| total.div(&Value::Int(2)))
                .ok()
                .or_else(|| Some(lower.clone()))
        }
    }
}

/// The magnitude of a value on its own scale: bytes, nanoseconds, percent points or the number
/// itself. `Value::as_float` covers only the plain numbers, and `measure memory` has to work.
fn scale_of(value: &Value) -> Option<f64> {
    match value {
        Value::Int(_) | Value::Float(_) | Value::Decimal(_) => value.as_float().ok(),
        Value::ByteSize(size) => Some(size.bytes() as f64),
        Value::Duration(span) => Some(span.nanoseconds() as f64),
        Value::Percent(percent) => Some(percent.value()),
        _ => None,
    }
}

/// The population standard deviation, as a plain number in the samples' own scale.
fn stddev_of(sorted: &[Value]) -> Option<Value> {
    let numbers: Vec<f64> = sorted.iter().filter_map(scale_of).collect();
    if numbers.len() != sorted.len() || numbers.is_empty() {
        return None;
    }
    let mean = numbers.iter().sum::<f64>() / numbers.len() as f64;
    let variance = numbers
        .iter()
        .map(|number| (number - mean).powi(2))
        .sum::<f64>()
        / numbers.len() as f64;
    Some(Value::Float(variance.sqrt()))
}

/// The nearest-rank percentile of a sorted sample.
fn nearest_rank(sorted: &[Value], percentile: f64) -> Option<Value> {
    if sorted.is_empty() {
        return None;
    }
    let fraction = (percentile / 100.0).clamp(0.0, 1.0);
    let rank = (fraction * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted.get(rank.min(sorted.len()) - 1).cloned()
}

/// `95.0` labels a percentile `p95`, not `p95.0`.
fn trim(percentile: f64) -> String {
    if (percentile.fract()).abs() < f64::EPSILON {
        format!("{}", percentile.trunc() as i64)
    } else {
        format!("{percentile}")
    }
}

/// Folds a stream that ends into a single value (spec §53).
///
/// Without an initial value the first value seeds the fold, and an empty stream is reported as an
/// error rather than answered with a zero the user did not ask for. A body that fails on one
/// value reports it and keeps the accumulator it had, so one unreadable row does not destroy the
/// whole answer (spec §16.5).
pub struct Reduce {
    body: Box<dyn Folder>,
    initial: Option<Value>,
    window: Option<Window>,
}

impl Reduce {
    /// Folds with `body`.
    #[must_use]
    pub fn new(body: impl Folder) -> Self {
        Self {
            body: Box::new(body),
            initial: None,
            window: None,
        }
    }

    /// Seeds the fold, which also gives an empty stream an answer.
    #[must_use]
    pub fn with_initial(mut self, initial: Value) -> Self {
        self.initial = Some(initial);
        self
    }

    /// Bounds an otherwise endless stream to its first `window` values (spec §11.1).
    #[must_use]
    pub const fn with_window(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }
}

impl Transform for Reduce {
    fn name(&self) -> &'static str {
        "reduce"
    }

    fn input_requirement(&self) -> InputRequirement {
        InputRequirement::Bounded(self.window)
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        input.stage(Boundedness::Bounded, move |mut input, sink| async move {
            let mut accumulator = self.initial.clone();
            let mut folded_nothing = true;
            while let Some(value) = input.next_value(&sink).await {
                folded_nothing = false;
                accumulator = Some(match accumulator {
                    None => value,
                    Some(current) => match self.body.fold(&current, &value) {
                        Ok(folded) => folded,
                        Err(error) => {
                            if sink.fail(with_identity(error, &value)).await.is_err() {
                                return;
                            }
                            current
                        }
                    },
                });
            }
            // A fold that never ran over a stream that failed has no answer, not the seed
            // (spec §35.3, ADR-0221).
            if folded_nothing && input.saw_failure() {
                return;
            }
            match accumulator {
                Some(result) => {
                    let _ = sink.send(result).await;
                }
                None => {
                    let _ = sink
                        .fail(
                            ErrorValue::new(
                                ErrorCode::TypeMismatch,
                                "`reduce` over an empty stream has no value to fold",
                            )
                            .with_help("give `reduce` an initial value with `--initial`"),
                        )
                        .await;
                }
            }
        })
    }
}

crate::transform::debug_as_name!(Count, Measure, Reduce);
