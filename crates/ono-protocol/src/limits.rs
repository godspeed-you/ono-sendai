//! Every bound this crate enforces, in one place (ADR-0015 T7, spec §49).
//!
//! A decoder reads bytes another machine chose. Every length in those bytes is a *claim*, and a
//! decoder that acts on a claim before checking it is how "schema/protocol bombs causing memory
//! exhaustion" happens. Collecting the bounds here means a reviewer can see all of them at once
//! and a test can tighten all of them at once.

/// The largest payload a single frame may carry, in bytes.
///
/// One mebibyte is far more than any message this protocol defines needs, and small enough that
/// a peer cannot make the shell reserve a meaningful fraction of its memory per frame.
pub const MAX_FRAME_PAYLOAD: usize = 1 << 20;

/// The deepest a decoded value may nest.
///
/// Records nest through their fields, lists through their items. Sixty-four levels is deeper
/// than any real object graph and shallow enough that the recursive walk that builds the value
/// cannot exhaust the stack.
pub const MAX_VALUE_DEPTH: usize = 64;

/// The most streams one link may have open at once.
pub const MAX_STREAMS: usize = 256;

/// The largest credit window a peer may be granted, in messages.
pub const MAX_CREDIT: u32 = 1024;

/// The credit window a link uses when nothing else is asked for.
///
/// Large enough that a fast producer and a fast consumer are never handing over one value at a
/// time across a latency, small enough that a stalled consumer stops the producer within a few
/// dozen values — the same trade-off, and the same size, as a local pipeline channel.
pub const DEFAULT_CREDIT: u32 = 32;

/// The bounds one end of a link enforces on the other.
///
/// ```
/// use ono_protocol::Limits;
/// let limits = Limits::default().with_max_frame_payload(4096);
/// assert_eq!(limits.max_frame_payload(), 4096);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    max_frame_payload: usize,
    max_value_depth: usize,
    max_streams: usize,
    max_credit: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_frame_payload: MAX_FRAME_PAYLOAD,
            max_value_depth: MAX_VALUE_DEPTH,
            max_streams: MAX_STREAMS,
            max_credit: MAX_CREDIT,
        }
    }
}

impl Limits {
    /// The default bounds.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the largest payload a frame may carry.
    ///
    /// A frame length is carried in four bytes, so the bound is clamped to what fits; a bound of
    /// zero would refuse every frame, so it is raised to one byte.
    #[must_use]
    pub fn with_max_frame_payload(mut self, bytes: usize) -> Self {
        self.max_frame_payload = bytes.clamp(1, u32::MAX as usize);
        self
    }

    /// Sets how deeply a decoded value may nest.
    #[must_use]
    pub fn with_max_value_depth(mut self, depth: usize) -> Self {
        self.max_value_depth = depth.max(1);
        self
    }

    /// Sets how many streams a link may have open at once.
    #[must_use]
    pub fn with_max_streams(mut self, streams: usize) -> Self {
        self.max_streams = streams.max(1);
        self
    }

    /// Sets the largest credit window a peer may be granted.
    #[must_use]
    pub fn with_max_credit(mut self, credit: u32) -> Self {
        self.max_credit = credit.max(1);
        self
    }

    /// The largest payload a frame may carry.
    #[must_use]
    pub const fn max_frame_payload(&self) -> usize {
        self.max_frame_payload
    }

    /// How deeply a decoded value may nest.
    #[must_use]
    pub const fn max_value_depth(&self) -> usize {
        self.max_value_depth
    }

    /// How many streams a link may have open at once.
    #[must_use]
    pub const fn max_streams(&self) -> usize {
        self.max_streams
    }

    /// The largest credit window a peer may be granted.
    #[must_use]
    pub const fn max_credit(&self) -> u32 {
        self.max_credit
    }
}
