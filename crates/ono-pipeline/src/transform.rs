//! What a pipeline stage is, and what it needs from its input (spec §11.1).

use crate::ValueStream;

/// A bound placed on an otherwise endless stream so a blocking transform can run over it.
///
/// Spec §11.1: "For unbounded streams, blocking transforms MUST either require a window or
/// reject the operation with a structured error." A count window is the only kind Ono defines
/// today, because it is the only one whose meaning is unambiguous without a clock the shell
/// controls; a time window belongs with the live subscriptions of spec §17, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    limit: usize,
}

impl Window {
    /// A window over the first `limit` values of the stream.
    #[must_use]
    pub const fn count(limit: usize) -> Self {
        Self { limit }
    }

    /// How many values the window admits.
    #[must_use]
    pub const fn limit(self) -> usize {
        self.limit
    }
}

/// What a transform needs from the stream it is applied to (spec §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRequirement {
    /// Runs on any input: it yields as it reads and never has to see the end.
    Streaming,
    /// Needs the input to end. Over an unbounded stream it runs only with an explicit window,
    /// and is rejected with `stream.unbounded_operation` (Ono-Sendai-E0801) without one.
    Bounded(Option<Window>),
}

/// One stage of a native pipeline: a function from a stream of values to a stream of values.
///
/// Parameters are already resolved by the time a transform is built. `where` holds a predicate,
/// not an expression; `sort` holds a key function, not an AST. Turning `where cpu > 20` into a
/// predicate is the evaluator's job, which is what keeps this crate — and its tests — free of a
/// dependency on the parser (ADR-0005).
pub trait Transform: Send + 'static {
    /// The transform's name, as the language spells it. Used in the errors it raises.
    fn name(&self) -> &'static str;

    /// What the transform needs from its input. Streaming unless stated otherwise.
    fn input_requirement(&self) -> InputRequirement {
        InputRequirement::Streaming
    }

    /// Runs the transform over `input`.
    ///
    /// Implementations build their output with [`ValueStream::stage`], which inherits the
    /// pipeline's capacity, cancellation scope and diagnostics. By the time this is called the
    /// boundedness check has already passed and any window has already been applied.
    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream;
}

/// Gives a transform a `Debug` that names it.
///
/// A transform holds resolved functions — closures, trait objects — which have no `Debug` worth
/// printing, and derivation is therefore impossible. What a reader of a failed assertion wants
/// is the transform's name, which is exactly what the trait already provides.
macro_rules! debug_as_name {
    ($($transform:ty),+ $(,)?) => {
        $(
            impl ::std::fmt::Debug for $transform {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    f.write_str($crate::Transform::name(self))
                }
            }
        )+
    };
}

pub(crate) use debug_as_name;
