//! The streaming execution engine of the Ono-Sendai shell (spec §11, §18.5, §34, ADR-0013).
//!
//! This crate is what makes an object pipeline *stream*. `get process | where cpu > 20 | take 10`
//! must show its first rows before the last process has been read, and
//! `get log --follow | where level == "error"` must not fill memory while the user reads slowly.
//! Both follow from one mechanism: every stage is a task, every channel between two stages is
//! bounded, and every await is selected against a shared cancellation scope.
//!
//! # What lives here
//!
//! - [`ValueStream`] — an asynchronous sequence of [`Value`](ono_value::Value) with an error
//!   channel beside it, a declared [`Boundedness`] and a [`CancelToken`].
//! - [`Transform`] — one stage. The built-ins are [`Where`], [`Select`], [`Take`], [`Skip`],
//!   [`Each`], [`First`], [`Last`], [`Sort`], [`Group`], [`Count`], [`Measure`], [`Reduce`],
//!   [`Join`] and [`Diff`].
//! - [`Diagnostics`] — what the pipeline counted while it ran, so a surprising row count has an
//!   explanation (ADR-0014).
//!
//! # Three properties, and the tests that hold them
//!
//! **Backpressure** (spec §11.2). Channels are bounded, so an endless producer runs exactly as
//! fast as its consumer drains it. A consumer that stops reading stops the producer; a consumer
//! that goes away ends it, the way `yes | head -1` ends.
//!
//! **Boundedness** (spec §11.1). A stream says whether it ends. `sort`, `group`, `count`,
//! `measure`, `reduce`, `join` and `diff` need input that ends, and applying one to a stream that
//! does not is [`ErrorCode::StreamUnboundedOperation`](ono_core::ErrorCode::StreamUnboundedOperation)
//! (Ono-Sendai-E0801) *before* anything runs — unless an explicit [`Window`] bounds it.
//!
//! **Cancellation** (spec §18.5). One token stops every stage at its next await, and the consumer
//! observes exactly one
//! [`ErrorCode::StreamCancelled`](ono_core::ErrorCode::StreamCancelled) (Ono-Sendai-E0802),
//! however long the pipeline is.
//!
//! # Parameters arrive resolved
//!
//! This crate does not depend on the parser and must not. [`Where`] holds a [`Predicate`] — a
//! function from a value to a value — not `cpu > 20`; [`Sort`] holds a [`KeyFn`], not an
//! expression. Turning source text into those is the evaluator's job (ADR-0005), which is what
//! lets the engine be tested without a language and keeps the layering of spec §5 intact.
//!
//! # Driving a pipeline
//!
//! ```
//! use ono_pipeline::{Sort, Take, ValueStream, Where};
//! use ono_value::Value;
//!
//! let runtime = tokio::runtime::Builder::new_current_thread()
//!     .enable_all()
//!     .build()
//!     .unwrap();
//! runtime.block_on(async {
//!     let collected = ValueStream::from_values((0..20).map(Value::Int))
//!         .transform(Where::new(|value: &Value| {
//!             Value::Bool(matches!(value, Value::Int(n) if n % 3 == 0))
//!         }))
//!         .unwrap()
//!         .transform(Sort::new(|value: &Value| Ok(value.clone())).descending())
//!         .unwrap()
//!         .transform(Take::new(3))
//!         .unwrap()
//!         .collect()
//!         .await;
//!
//!     assert_eq!(
//!         collected.values(),
//!         [Value::Int(18), Value::Int(15), Value::Int(12)]
//!     );
//!     assert!(collected.errors().is_empty());
//! });
//! ```

#![forbid(unsafe_code)]

mod cancel;
mod diagnostics;
mod function;
mod order;
mod schemas;
mod stream;
mod transform;
mod transforms;

pub use cancel::CancelToken;
pub use diagnostics::Diagnostics;
pub use function::{Folder, KeyFn, Mapper, Predicate};
pub use order::Direction;
pub use stream::{
    Boundedness, Collected, DEFAULT_CAPACITY, PipelineConfig, SinkClosed, StreamEvent, StreamSink,
    ValueStream,
};
pub use transform::{InputRequirement, Transform, Window};
pub use transforms::aggregate::{Count, Measure, Reduce};
pub use transforms::filter::Where;
pub use transforms::group::Group;
pub use transforms::map::Each;
pub use transforms::project::{PathSegment, Select, SelectField};
pub use transforms::relational::{Diff, Join, JoinKind};
pub use transforms::slice::{First, Last, Skip, Take};
pub use transforms::sort::Sort;
