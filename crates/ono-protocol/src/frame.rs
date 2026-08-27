//! The frame a link puts on the wire (spec §21.2, §49: "bounded protocol frames and streams").
//!
//! # The shape
//!
//! ```text
//! 0        1        2        3        4                8                12
//! +--------+--------+--------+--------+----------------+----------------+
//! | version| kind   | flags  | rsvd   | stream id (u32)| length (u32)   |  payload…
//! +--------+--------+--------+--------+----------------+----------------+
//! ```
//!
//! Twelve fixed bytes, big-endian, then exactly `length` bytes of payload. Nothing about the
//! frame needs the payload to be understood, which is what lets a receiver route a frame for a
//! stream it is not decoding and refuse a frame it must not read.
//!
//! # Why every field is checked before anything is allocated
//!
//! `length` is a claim made by another machine. ADR-0015 T7 names "schema/protocol bombs causing
//! memory exhaustion" as a release-blocking threat, and a decoder that reserves `length` bytes
//! before comparing it against a bound is exactly that bomb's fuse. So the order here is fixed:
//! version, kind, flags, then `length` against [`Limits::max_frame_payload`], and only then is
//! anything taken out of the buffer.
//!
//! Undefined flag bits are refused rather than ignored. A flag a later version defines may change
//! how the payload must be read, and a receiver that ignored it would decode the payload wrongly
//! instead of saying it cannot.
//!
//! [`Limits::max_frame_payload`]: crate::Limits::max_frame_payload

use std::fmt;

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::{Limits, ProtocolError};

/// The number of bytes before a frame's payload.
pub const FRAME_HEADER_LEN: usize = 12;

/// The framing version this build writes and accepts.
///
/// This is the version of the *envelope*, not of the protocol spoken inside it: the message set
/// is negotiated in the handshake (spec §21.2), while the envelope has to be readable before any
/// negotiation can happen at all.
pub const FRAME_VERSION: u8 = 1;

/// What a frame carries.
///
/// The numbers are part of the wire format and never change meaning: a kind is added by taking
/// the next free number, exactly as an error code is (ADR-0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FrameKind {
    /// The opening offer of the handshake (spec §21.2).
    Hello,
    /// The answer that establishes a link.
    Accept,
    /// The answer that refuses one.
    Reject,
    /// Open a stream carrying the objects a query matches.
    StartQuery,
    /// Open a stream carrying the changes a query matches.
    StartSubscribe,
    /// Perform an action and answer with one outcome.
    Act,
    /// Stop a stream. The producer learns of it at its next send.
    Cancel,
    /// Grant the peer permission to send this many more messages on a stream.
    Credit,
    /// One value produced by a stream.
    Value,
    /// One object event produced by a subscription.
    Event,
    /// A failure concerning one item, leaving the stream running (spec §16.5).
    Failure,
    /// The outcome of an action.
    Outcome,
    /// The stream produced everything it is going to.
    End,
    /// Ask the agent to adapt an external invocation for a demand, or to say what it would do
    /// (spec v0.3 §1.54).
    StartAdapt,
}

impl FrameKind {
    /// Every kind, in wire order.
    pub const ALL: &'static [FrameKind] = &[
        FrameKind::Hello,
        FrameKind::Accept,
        FrameKind::Reject,
        FrameKind::StartQuery,
        FrameKind::StartSubscribe,
        FrameKind::Act,
        FrameKind::Cancel,
        FrameKind::Credit,
        FrameKind::Value,
        FrameKind::Event,
        FrameKind::Failure,
        FrameKind::Outcome,
        FrameKind::End,
        FrameKind::StartAdapt,
    ];

    /// The byte that stands for this kind on the wire.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            FrameKind::Hello => 1,
            FrameKind::Accept => 2,
            FrameKind::Reject => 3,
            FrameKind::StartQuery => 4,
            FrameKind::StartSubscribe => 5,
            FrameKind::Act => 6,
            FrameKind::Cancel => 7,
            FrameKind::Credit => 8,
            FrameKind::Value => 9,
            FrameKind::Event => 10,
            FrameKind::Failure => 11,
            FrameKind::Outcome => 12,
            FrameKind::End => 13,
            FrameKind::StartAdapt => 14,
        }
    }

    /// The kind a wire byte stands for, or `None` if this protocol defines none.
    #[must_use]
    pub fn from_u8(byte: u8) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_u8() == byte)
    }

    /// The kind's name, as diagnostics and `explain` spell it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            FrameKind::Hello => "hello",
            FrameKind::Accept => "accept",
            FrameKind::Reject => "reject",
            FrameKind::StartQuery => "start-query",
            FrameKind::StartSubscribe => "start-subscribe",
            FrameKind::Act => "act",
            FrameKind::Cancel => "cancel",
            FrameKind::Credit => "credit",
            FrameKind::Value => "value",
            FrameKind::Event => "event",
            FrameKind::Failure => "failure",
            FrameKind::Outcome => "outcome",
            FrameKind::End => "end",
            FrameKind::StartAdapt => "start-adapt",
        }
    }

    /// Whether a message of this kind is spent against a stream's credit window.
    ///
    /// Only the messages a producer emits are: control messages must always be deliverable, or a
    /// stalled stream could not be cancelled.
    #[must_use]
    pub const fn spends_credit(self) -> bool {
        matches!(
            self,
            FrameKind::Value | FrameKind::Event | FrameKind::Failure | FrameKind::Outcome
        )
    }
}

impl fmt::Display for FrameKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One framed message, addressed to one stream.
///
/// Stream `0` is the connection itself: the handshake and anything that concerns the link rather
/// than one query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    kind: FrameKind,
    stream: u32,
    payload: Bytes,
}

impl Frame {
    /// A frame of `kind` for `stream`, carrying `payload`.
    #[must_use]
    pub fn new(kind: FrameKind, stream: u32, payload: impl Into<Bytes>) -> Self {
        Self {
            kind,
            stream,
            payload: payload.into(),
        }
    }

    /// What the frame carries.
    #[must_use]
    pub const fn kind(&self) -> FrameKind {
        self.kind
    }

    /// The stream it belongs to; `0` is the connection itself.
    #[must_use]
    pub const fn stream(&self) -> u32 {
        self.stream
    }

    /// The payload bytes.
    #[must_use]
    pub const fn payload(&self) -> &Bytes {
        &self.payload
    }

    /// The payload, taken out of the frame.
    #[must_use]
    pub fn into_payload(self) -> Bytes {
        self.payload
    }
}

/// Appends `frame` to `out`.
///
/// # Errors
///
/// Returns [`ProtocolError::FrameTooLarge`] when the payload exceeds `limits`. A sender refuses
/// what a receiver would have to refuse, so an oversized message is reported where it can still
/// be attributed to the code that built it.
///
/// ```
/// use bytes::BytesMut;
/// use ono_protocol::{Frame, FrameKind, Limits, encode};
///
/// let mut out = BytesMut::new();
/// encode(&Frame::new(FrameKind::End, 3, Vec::new()), &Limits::default(), &mut out)?;
/// assert_eq!(out.len(), ono_protocol::FRAME_HEADER_LEN);
/// # Ok::<(), ono_protocol::ProtocolError>(())
/// ```
pub fn encode(frame: &Frame, limits: &Limits, out: &mut BytesMut) -> Result<(), ProtocolError> {
    let length = u32::try_from(frame.payload.len()).map_err(|_| ProtocolError::FrameTooLarge {
        claimed: frame.payload.len(),
        limit: limits.max_frame_payload(),
    })?;
    if frame.payload.len() > limits.max_frame_payload() {
        return Err(ProtocolError::FrameTooLarge {
            claimed: frame.payload.len(),
            limit: limits.max_frame_payload(),
        });
    }
    out.reserve(FRAME_HEADER_LEN + frame.payload.len());
    out.put_u8(FRAME_VERSION);
    out.put_u8(frame.kind.as_u8());
    out.put_u8(0);
    out.put_u8(0);
    out.put_u32(frame.stream);
    out.put_u32(length);
    out.put_slice(&frame.payload);
    Ok(())
}

/// Takes the next whole frame out of `buffer`.
///
/// Returns `Ok(None)` when the buffer does not yet hold a whole frame, leaving it untouched so
/// the next read can complete it.
///
/// # Errors
///
/// Returns a [`ProtocolError`] when the header is not one this build can read: an unknown framing
/// version, an unknown kind, an undefined flag, or a length beyond `limits`. **No allocation of
/// the claimed size happens before that check**, which is what makes a length field safe to read
/// from a machine you do not control (ADR-0015 T7).
pub fn decode(buffer: &mut BytesMut, limits: &Limits) -> Result<Option<Frame>, ProtocolError> {
    let Some(header) = buffer.get(..FRAME_HEADER_LEN) else {
        return Ok(None);
    };
    let version = header[0];
    if version != FRAME_VERSION {
        return Err(ProtocolError::UnsupportedFrameVersion { found: version });
    }
    let Some(kind) = FrameKind::from_u8(header[1]) else {
        return Err(ProtocolError::UnknownFrameKind { kind: header[1] });
    };
    let flags = header[2] | header[3];
    if flags != 0 {
        return Err(ProtocolError::UnknownFrameFlags { flags });
    }
    let stream = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    let claimed = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
    if claimed > limits.max_frame_payload() {
        return Err(ProtocolError::FrameTooLarge {
            claimed,
            limit: limits.max_frame_payload(),
        });
    }
    if buffer.len() < FRAME_HEADER_LEN + claimed {
        return Ok(None);
    }
    buffer.advance(FRAME_HEADER_LEN);
    let payload = buffer.split_to(claimed).freeze();
    Ok(Some(Frame {
        kind,
        stream,
        payload,
    }))
}
