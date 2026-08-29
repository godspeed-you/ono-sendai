//! The stream a native pipeline moves values along (spec §11.1, §11.2, §16.5, ADR-0013).

use std::fmt;
use std::future::Future;

use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};
use tokio::sync::mpsc;

use crate::{CancelToken, Diagnostics, InputRequirement, Transform, Window};

/// How much a channel between two stages buffers before the producer has to wait.
///
/// Large enough that a fast producer and a fast consumer never hand off one value at a time,
/// small enough that a stalled consumer stops an endless producer within a few dozen values.
pub const DEFAULT_CAPACITY: usize = 64;

/// How a pipeline's channels are sized and cancelled.
///
/// ```
/// use ono_pipeline::{CancelToken, PipelineConfig};
/// let config = PipelineConfig::new()
///     .with_capacity(8)
///     .with_cancel_token(CancelToken::new());
/// assert_eq!(config.capacity(), 8);
/// ```
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    capacity: usize,
    cancel: CancelToken,
    diagnostics: Diagnostics,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            capacity: DEFAULT_CAPACITY,
            cancel: CancelToken::new(),
            diagnostics: Diagnostics::new(),
        }
    }
}

impl PipelineConfig {
    /// The default configuration: [`DEFAULT_CAPACITY`] and a fresh cancellation scope.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets how many values a channel buffers. A capacity of zero would deadlock, so it is
    /// raised to one.
    #[must_use]
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity.max(1);
        self
    }

    /// Joins the pipeline to an existing cancellation scope, so one Ctrl-C stops all of it.
    #[must_use]
    pub fn with_cancel_token(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// Joins the pipeline to an existing set of diagnostics counters.
    #[must_use]
    pub fn with_diagnostics(mut self, diagnostics: Diagnostics) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    /// How many values a channel buffers.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// The cancellation scope.
    #[must_use]
    pub const fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// The diagnostics counters.
    #[must_use]
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }
}

/// Whether a stream is known to end.
///
/// Spec §11.1 divides transforms into those that can stay streaming and those that need input
/// which ends. A stream therefore has to know which kind it is, before anything runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundedness {
    /// The stream ends on its own: a finite enumeration, a file, a windowed live stream.
    Bounded,
    /// The stream may never end: a subscription, a follow, an endless generator.
    Unbounded,
}

impl Boundedness {
    /// Whether the stream is known to end.
    #[must_use]
    pub const fn is_bounded(self) -> bool {
        matches!(self, Boundedness::Bounded)
    }
}

/// One thing a consumer observes on a stream.
///
/// Values and partial failures are carried on separate channels and merged here, because spec
/// §16.5 forbids collapsing a per-item failure into the result and a consumer that only drained
/// values would either lose the failures or stall the producer.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// A value the pipeline produced.
    Value(Value),
    /// A failure that concerns one item, not the pipeline (spec §16.5).
    Failure(ErrorValue),
}

/// Reported when the far end of a stream is gone: the consumer dropped it, or it was cancelled.
///
/// A producer treats it the way an external program treats `SIGPIPE`: stop, quietly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkClosed;

impl fmt::Display for SinkClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the consumer of this stream is gone")
    }
}

impl std::error::Error for SinkClosed {}

/// The writing end of a stream: what a producer or a transform sends into.
///
/// Every send is bounded and cancellable. `send` waits when the channel is full, which is the
/// backpressure spec §11.2 requires — an infinite producer runs exactly as fast as its consumer
/// drains it, and no faster.
#[derive(Debug, Clone)]
pub struct StreamSink {
    values: mpsc::Sender<Value>,
    errors: mpsc::Sender<ErrorValue>,
    cancel: CancelToken,
    diagnostics: Diagnostics,
}

impl StreamSink {
    /// Sends one value downstream, waiting for room.
    ///
    /// # Errors
    ///
    /// Returns [`SinkClosed`] when the consumer has gone away or the pipeline was cancelled.
    pub async fn send(&self, value: Value) -> Result<(), SinkClosed> {
        tokio::select! {
            // Biased, so a cancelled pipeline stops rather than racing the channel for one more
            // value. Cancellation that only *usually* wins is cancellation a user cannot trust.
            biased;
            () = self.cancel.cancelled() => Err(SinkClosed),
            result = self.values.send(value) => result.map_err(|_| SinkClosed),
        }
    }

    /// Reports a failure that concerns one item, leaving the stream running (spec §16.5).
    ///
    /// # Errors
    ///
    /// Returns [`SinkClosed`] when the consumer has gone away or the pipeline was cancelled.
    pub async fn fail(&self, error: ErrorValue) -> Result<(), SinkClosed> {
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(SinkClosed),
            result = self.errors.send(error) => result.map_err(|_| SinkClosed),
        }
    }

    /// Whether the pipeline has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Waits until nothing will read another value: the consumer dropped the stream, or the
    /// pipeline was cancelled.
    ///
    /// A producer that waits on the outside world — a followed journal, a child that writes
    /// only when something happens — has no next `send` to learn from, so it selects on this
    /// instead and stops reading the moment `take` has what it wanted.
    pub async fn closed(&self) {
        tokio::select! {
            () = self.cancel.cancelled() => {}
            () = self.values.closed() => {}
        }
    }

    /// The pipeline's cancellation scope.
    #[must_use]
    pub const fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// The pipeline's diagnostics counters.
    #[must_use]
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }
}

/// Everything a bounded stream produced.
#[derive(Debug, Clone)]
pub struct Collected {
    values: Vec<Value>,
    errors: Vec<ErrorValue>,
    diagnostics: Diagnostics,
}

impl Collected {
    /// The values, in the order they arrived.
    #[must_use]
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// The per-item failures, in the order they arrived (spec §16.5).
    #[must_use]
    pub fn errors(&self) -> &[ErrorValue] {
        &self.errors
    }

    /// What the pipeline counted while it ran (ADR-0014).
    #[must_use]
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// The values, taken out of the collection.
    #[must_use]
    pub fn into_values(self) -> Vec<Value> {
        self.values
    }
}

/// An asynchronous sequence of values, its error channel and its cancellation scope.
///
/// This is the object a native pipeline stage hands to the next one. It is not a [`Value`]:
/// spec §25 asks that streams stay execution-layer objects, because a clonable value whose
/// second clone yields nothing would be a lie.
///
/// ```
/// use ono_pipeline::{Take, ValueStream, Where};
/// use ono_value::Value;
///
/// let runtime = tokio::runtime::Builder::new_current_thread()
///     .enable_all()
///     .build()
///     .unwrap();
/// runtime.block_on(async {
///     let collected = ValueStream::from_values((0..10).map(Value::Int))
///         .transform(Where::new(|value: &Value| {
///             Value::Bool(matches!(value, Value::Int(n) if n % 2 == 0))
///         }))
///         .unwrap()
///         .transform(Take::new(2))
///         .unwrap()
///         .collect()
///         .await;
///     assert_eq!(collected.values(), [Value::Int(0), Value::Int(2)]);
/// });
/// ```
#[derive(Debug)]
pub struct ValueStream {
    values: mpsc::Receiver<Value>,
    errors: mpsc::Receiver<ErrorValue>,
    values_done: bool,
    errors_done: bool,
    saw_failure: bool,
    capacity: usize,
    boundedness: Boundedness,
    cancel: CancelToken,
    diagnostics: Diagnostics,
}

impl ValueStream {
    /// Runs `producer` as a task and streams what it sends.
    ///
    /// The producer declares its own boundedness: only it knows whether it enumerates a finite
    /// set or subscribes to an endless one, and every later stage depends on the answer.
    pub fn spawn<F, Fut>(config: PipelineConfig, boundedness: Boundedness, producer: F) -> Self
    where
        F: FnOnce(StreamSink) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (stream, sink) = Self::channel(&config, boundedness);
        tokio::spawn(async move { producer(sink).await });
        stream
    }

    /// A bounded stream over values that are already in hand.
    #[must_use]
    pub fn from_values<I: IntoIterator<Item = Value>>(values: I) -> Self {
        Self::from_values_with(PipelineConfig::new(), values)
    }

    /// A bounded stream over values that are already in hand, joined to an existing pipeline.
    #[must_use]
    pub fn from_values_with<I: IntoIterator<Item = Value>>(
        config: PipelineConfig,
        values: I,
    ) -> Self {
        let values: Vec<Value> = values.into_iter().collect();
        Self::spawn(config, Boundedness::Bounded, move |sink| async move {
            for value in values {
                if sink.send(value).await.is_err() {
                    return;
                }
            }
        })
    }

    /// Whether the stream is known to end (spec §11.1).
    #[must_use]
    pub const fn boundedness(&self) -> Boundedness {
        self.boundedness
    }

    /// The pipeline's cancellation scope, which a consumer may trip itself.
    #[must_use]
    pub const fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// What the pipeline has counted so far (ADR-0014).
    #[must_use]
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// How many values each channel in this pipeline buffers.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// The next value or failure, or `None` when the stream has ended.
    ///
    /// A cancelled pipeline yields one [`ErrorCode::StreamCancelled`] failure before it ends, so
    /// a consumer never has to guess why the values stopped (spec §18.5).
    pub async fn recv(&mut self) -> Option<StreamEvent> {
        if let Some(event) = self.recv_raw().await {
            self.saw_failure |= matches!(event, StreamEvent::Failure(_));
            return Some(event);
        }
        if self.cancel.claim_report() {
            self.saw_failure = true;
            return Some(StreamEvent::Failure(cancelled_error()));
        }
        None
    }

    /// The next value, forwarding any failure that arrives first to `sink`.
    ///
    /// This is how a transform reads its input: forwarding rather than buffering is what keeps
    /// a partial failure from being swallowed by a later stage, and what keeps a stream of
    /// nothing-but-failures from growing without bound.
    pub async fn next_value(&mut self, sink: &StreamSink) -> Option<Value> {
        loop {
            match self.recv_raw().await? {
                StreamEvent::Value(value) => return Some(value),
                StreamEvent::Failure(error) => {
                    self.saw_failure = true;
                    sink.fail(error).await.ok()?;
                }
            }
        }
    }

    /// Whether a failure has passed through this stream.
    ///
    /// An aggregate asks after it has read to the end: a summary of a stream that could not be
    /// read is not a summary of nothing, and spec §35.3 forbids answering `0` for what was never
    /// known (ADR-0221).
    #[must_use]
    pub const fn saw_failure(&self) -> bool {
        self.saw_failure
    }

    /// Drains the stream into everything it produced.
    ///
    /// Only ever call this on a stream that ends: an unbounded stream never finishes collecting.
    /// [`transform`](Self::transform) refuses the blocking transforms for the same reason.
    pub async fn collect(mut self) -> Collected {
        let mut values = Vec::new();
        let mut errors = Vec::new();
        while let Some(event) = self.recv().await {
            match event {
                StreamEvent::Value(value) => values.push(value),
                StreamEvent::Failure(error) => errors.push(error),
            }
        }
        Collected {
            values,
            errors,
            diagnostics: self.diagnostics,
        }
    }

    /// Applies a transform, checking first that the stream can support it.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::StreamUnboundedOperation`] when a blocking transform is applied to a
    /// stream that never ends and no window was given (spec §11.1). Nothing runs and nothing is
    /// consumed: the error arrives before the first value would have.
    pub fn transform<T: Transform>(self, transform: T) -> Result<Self, ErrorValue> {
        let input = match transform.input_requirement() {
            InputRequirement::Streaming => self,
            InputRequirement::Bounded(_) if self.boundedness.is_bounded() => self,
            InputRequirement::Bounded(Some(window)) => self.windowed(window),
            InputRequirement::Bounded(None) => return Err(unbounded_error(transform.name())),
        };
        Ok(Box::new(transform).apply(input))
    }

    /// Runs `body` as a stage of this pipeline, streaming what it sends.
    ///
    /// The stage inherits the pipeline's capacity, cancellation scope and diagnostics, so a
    /// composed pipeline stays one pipeline.
    pub fn stage<F, Fut>(self, boundedness: Boundedness, body: F) -> Self
    where
        F: FnOnce(Self, StreamSink) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let config = PipelineConfig::new()
            .with_capacity(self.capacity)
            .with_cancel_token(self.cancel.clone())
            .with_diagnostics(self.diagnostics.clone());
        let (stream, sink) = Self::channel(&config, boundedness);
        tokio::spawn(async move { body(self, sink).await });
        stream
    }

    /// Truncates the stream to the first `window.limit()` values, which bounds it (spec §11.1).
    fn windowed(self, window: Window) -> Self {
        let limit = window.limit();
        self.stage(Boundedness::Bounded, move |input, sink| async move {
            forward_at_most(input, &sink, limit).await;
        })
    }

    /// The next event without the cancellation report, for stages rather than consumers.
    async fn recv_raw(&mut self) -> Option<StreamEvent> {
        loop {
            match (self.values_done, self.errors_done) {
                (true, true) => return None,
                (false, true) => match self.values.recv().await {
                    Some(value) => return Some(StreamEvent::Value(value)),
                    None => self.values_done = true,
                },
                (true, false) => match self.errors.recv().await {
                    Some(error) => return Some(StreamEvent::Failure(error)),
                    None => self.errors_done = true,
                },
                (false, false) => {
                    tokio::select! {
                        value = self.values.recv() => match value {
                            Some(value) => return Some(StreamEvent::Value(value)),
                            None => self.values_done = true,
                        },
                        error = self.errors.recv() => match error {
                            Some(error) => return Some(StreamEvent::Failure(error)),
                            None => self.errors_done = true,
                        },
                    }
                }
            }
        }
    }

    /// The pair of channels one stage writes and the next reads.
    fn channel(config: &PipelineConfig, boundedness: Boundedness) -> (Self, StreamSink) {
        let (value_tx, value_rx) = mpsc::channel(config.capacity());
        let (error_tx, error_rx) = mpsc::channel(config.capacity());
        let sink = StreamSink {
            values: value_tx,
            errors: error_tx,
            cancel: config.cancel_token().clone(),
            diagnostics: config.diagnostics().clone(),
        };
        let stream = Self {
            values: value_rx,
            errors: error_rx,
            values_done: false,
            errors_done: false,
            saw_failure: false,
            capacity: config.capacity(),
            boundedness,
            cancel: config.cancel_token().clone(),
            diagnostics: config.diagnostics().clone(),
        };
        (stream, sink)
    }
}

/// Forwards at most `limit` values, then stops and lets the upstream channel close.
///
/// Dropping the input is what makes `get log --follow | take 3` terminate: the producer's next
/// send fails, exactly as `yes | head -1` receives `SIGPIPE`.
pub(crate) async fn forward_at_most(mut input: ValueStream, sink: &StreamSink, limit: usize) {
    let mut sent = 0;
    while sent < limit {
        let Some(value) = input.next_value(sink).await else {
            return;
        };
        if sink.send(value).await.is_err() {
            return;
        }
        sent += 1;
    }
}

/// The error a consumer observes when its pipeline was cancelled (spec §18.5, §43).
pub(crate) fn cancelled_error() -> ErrorValue {
    ErrorValue::new(ErrorCode::StreamCancelled, "the pipeline was cancelled")
        .with_retryable(true)
        .with_help(
            "the values produced before the cancellation are complete; the rest were not read",
        )
}

/// The error a blocking transform reports over a stream that never ends (spec §11.1, §43).
pub(crate) fn unbounded_error(transform: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::StreamUnboundedOperation,
        format!("`{transform}` needs input that ends, and this stream may not end"),
    )
    .with_retryable(false)
    .with_help(format!(
        "bound the stream first, for example `| take 100 | {transform} …`, or give `{transform}` \
         an explicit window"
    ))
}
