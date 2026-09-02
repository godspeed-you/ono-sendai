//! Every bound this crate and the agent above it enforce, in one place (ADR-0015 T7, spec §49;
//! v0.4.1 §12.4, §52.2).
//!
//! A decoder reads bytes another machine chose. Every length in those bytes is a *claim*, and a
//! decoder that acts on a claim before checking it is how "schema/protocol bombs causing memory
//! exhaustion" happens. Collecting the bounds here means a reviewer can see all of them at once
//! and a test can tighten all of them at once.
//!
//! v0.4.1 §12.4 widens the subject from the wire to the listener and fixes where the numbers
//! live:
//!
//! > Their defaults MUST be centralized in one `Limits` contract […] No code path may construct
//! > an effectively unlimited `Limits` instance for a network listener in production.
//!
//! So the four connection ceilings of §12.1–§12.3 are fields of the same [`Limits`] the frame
//! decoder already reads, and every setter **clamps** into the range
//! `docs/spec/hardening/limits.yaml` declares. There is no constructor, no default and no builder
//! call that answers an unlimited value, which is the guarantee that needs no reviewer
//! (ADR-0501, following ADR-0453).

use std::time::Duration;

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

/// Concurrent connections one listening agent holds (v0.4.1 §12.1, Appendix A).
///
/// `limits.remote_connections` in `docs/spec/hardening/limits.yaml`, which is where the number
/// is declared for the configuration layer; this is the same number for the code that enforces
/// it, and `crates/ono-remote/tests/limits.rs` fails if the two ever differ.
pub const MAX_CONNECTIONS: u32 = 32;

/// Connections that may be mid-negotiation at once (v0.4.1 §12.2, Appendix A).
pub const MAX_PENDING_HANDSHAKES: u32 = 16;

/// Concurrent connections one authenticated fingerprint may hold (v0.4.1 §12.3, Appendix A).
pub const MAX_CONNECTIONS_PER_CLIENT: u32 = 4;

/// How long TLS and Ono negotiation together may take (v0.4.1 §12.2, Appendix A).
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(10_000);

/// The largest value `limits.remote_*` connection counts accept, from the registry's own range.
///
/// A ceiling on the ceiling is what makes "no unlimited limits" true rather than encouraged: a
/// caller may raise a bound and cannot remove it.
const CONNECTION_COUNT_CEILING: u32 = 65_536;

/// The shortest handshake budget a listener will honour.
///
/// A timeout too short to complete a handshake is a listener that accepts nobody, which is a
/// denial of service spelled as a configuration value (ADR-0456).
const HANDSHAKE_TIMEOUT_FLOOR: Duration = Duration::from_millis(100);

/// The longest handshake budget a listener will honour.
const HANDSHAKE_TIMEOUT_CEILING: Duration = Duration::from_millis(600_000);

/// The bounds one end of a link enforces on the other, and the ceilings a listening agent
/// enforces on everyone (v0.4.1 §12).
///
/// ```
/// use ono_protocol::Limits;
/// let limits = Limits::default().with_max_frame_payload(4096);
/// assert_eq!(limits.max_frame_payload(), 4096);
/// assert_eq!(limits.max_connections(), 32);
/// ```
///
/// Every setter clamps rather than accepts: asking for more than the contract permits gives the
/// contract's maximum, and asking for none gives its minimum. §12.4 forbids an effectively
/// unlimited instance, and a type that cannot hold one needs no review to stay that way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    max_frame_payload: usize,
    max_value_depth: usize,
    max_streams: usize,
    max_credit: u32,
    max_connections: u32,
    max_pending_handshakes: u32,
    max_connections_per_client: u32,
    handshake_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_frame_payload: MAX_FRAME_PAYLOAD,
            max_value_depth: MAX_VALUE_DEPTH,
            max_streams: MAX_STREAMS,
            max_credit: MAX_CREDIT,
            max_connections: MAX_CONNECTIONS,
            max_pending_handshakes: MAX_PENDING_HANDSHAKES,
            max_connections_per_client: MAX_CONNECTIONS_PER_CLIENT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
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
    /// Clamped to [`MAX_FRAME_PAYLOAD`]: a bound of zero would refuse every frame, and a bound
    /// above the declared one would let a peer reserve memory the protocol never needs.
    #[must_use]
    pub fn with_max_frame_payload(mut self, bytes: usize) -> Self {
        self.max_frame_payload = bytes.clamp(1, MAX_FRAME_PAYLOAD);
        self
    }

    /// Sets how deeply a decoded value may nest, clamped to [`MAX_VALUE_DEPTH`].
    #[must_use]
    pub fn with_max_value_depth(mut self, depth: usize) -> Self {
        self.max_value_depth = depth.clamp(1, MAX_VALUE_DEPTH);
        self
    }

    /// Sets how many streams a link may have open at once, clamped to [`MAX_STREAMS`].
    #[must_use]
    pub fn with_max_streams(mut self, streams: usize) -> Self {
        self.max_streams = streams.clamp(1, MAX_STREAMS);
        self
    }

    /// Sets the largest credit window a peer may be granted, clamped to [`MAX_CREDIT`].
    #[must_use]
    pub fn with_max_credit(mut self, credit: u32) -> Self {
        self.max_credit = credit.clamp(1, MAX_CREDIT);
        self
    }

    /// Sets how many connections one listening agent holds at once (§12.1).
    #[must_use]
    pub fn with_max_connections(mut self, connections: u32) -> Self {
        self.max_connections = connections.clamp(1, CONNECTION_COUNT_CEILING);
        self
    }

    /// Sets how many connections may be mid-negotiation at once (§12.2).
    #[must_use]
    pub fn with_max_pending_handshakes(mut self, handshakes: u32) -> Self {
        self.max_pending_handshakes = handshakes.clamp(1, CONNECTION_COUNT_CEILING);
        self
    }

    /// Sets how many connections one authenticated fingerprint may hold (§12.3).
    #[must_use]
    pub fn with_max_connections_per_client(mut self, connections: u32) -> Self {
        self.max_connections_per_client = connections.clamp(1, CONNECTION_COUNT_CEILING);
        self
    }

    /// Sets how long TLS and Ono negotiation together may take (§12.2).
    #[must_use]
    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout.clamp(HANDSHAKE_TIMEOUT_FLOOR, HANDSHAKE_TIMEOUT_CEILING);
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

    /// How many connections one listening agent holds at once (§12.1).
    #[must_use]
    pub const fn max_connections(&self) -> u32 {
        self.max_connections
    }

    /// How many connections may be mid-negotiation at once (§12.2).
    #[must_use]
    pub const fn max_pending_handshakes(&self) -> u32 {
        self.max_pending_handshakes
    }

    /// How many connections one authenticated fingerprint may hold (§12.3).
    #[must_use]
    pub const fn max_connections_per_client(&self) -> u32 {
        self.max_connections_per_client
    }

    /// How long TLS and Ono negotiation together may take (§12.2).
    #[must_use]
    pub const fn handshake_timeout(&self) -> Duration {
        self.handshake_timeout
    }
}
