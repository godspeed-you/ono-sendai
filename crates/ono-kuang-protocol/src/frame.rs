//! Length-declared frames over the plugin's stdin/stdout (spec §31.11).
//!
//! A frame is a four-byte big-endian length followed by the JSON payload of one [`Envelope`].
//! Every length is checked against [`FrameLimits`] before anything is allocated, because a
//! length field from isolated code is a claim, not an instruction (protocol invariant
//! `bounded-frames`, ADR-0015 T7). A frame that overstates its size, or whose payload is not a
//! valid envelope, is a protocol violation — the sender is misbehaving, and the connection does
//! not try to resynchronise with a peer it can no longer trust to frame correctly.

use std::io::{Read, Write};

use crate::Envelope;

/// The frame bounds both sides enforce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLimits {
    /// The largest frame either side may send, in bytes.
    pub max_frame: u32,
}

impl Default for FrameLimits {
    fn default() -> Self {
        Self {
            max_frame: 1024 * 1024,
        }
    }
}

/// Why a frame could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The underlying stream failed or closed.
    #[error("the peer's stream failed: {0}")]
    Io(#[from] std::io::Error),
    /// The peer declared a frame larger than the negotiated ceiling.
    #[error("the peer declared a {declared}-byte frame; the ceiling is {ceiling}")]
    TooLarge {
        /// The declared length.
        declared: u32,
        /// The negotiated ceiling.
        ceiling: u32,
    },
    /// The payload is not a valid envelope.
    #[error("the payload is not a valid protocol envelope: {0}")]
    Malformed(#[from] serde_json::Error),
}

/// Encodes one envelope as a frame.
///
/// # Errors
///
/// Returns [`FrameError::TooLarge`] when the encoded payload exceeds the ceiling — the sender's
/// own ceiling applies to the sender too, so a host cannot push a frame it would refuse to read.
pub fn encode_frame(envelope: &Envelope, limits: FrameLimits) -> Result<Vec<u8>, FrameError> {
    let payload = serde_json::to_vec(envelope)?;
    let declared = u32::try_from(payload.len()).map_err(|_| FrameError::TooLarge {
        declared: u32::MAX,
        ceiling: limits.max_frame,
    })?;
    if declared > limits.max_frame {
        return Err(FrameError::TooLarge {
            declared,
            ceiling: limits.max_frame,
        });
    }
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&declared.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decodes a payload the caller has already length-checked and read.
///
/// # Errors
///
/// Returns [`FrameError::Malformed`] when the bytes are not a valid envelope.
pub fn decode_payload(payload: &[u8]) -> Result<Envelope, FrameError> {
    Ok(serde_json::from_slice(payload)?)
}

/// Writes one envelope as a frame to a blocking stream and flushes it.
///
/// # Errors
///
/// Propagates encoding and I/O failures.
pub fn write_frame(
    writer: &mut impl Write,
    envelope: &Envelope,
    limits: FrameLimits,
) -> Result<(), FrameError> {
    let frame = encode_frame(envelope, limits)?;
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

/// Reads one envelope from a blocking stream, or `None` on a clean end of stream.
///
/// # Errors
///
/// Returns [`FrameError::TooLarge`] before allocating anything for an oversized declaration,
/// and [`FrameError::Malformed`] for a payload that is not an envelope.
pub fn read_frame(
    reader: &mut impl Read,
    limits: FrameLimits,
) -> Result<Option<Envelope>, FrameError> {
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let declared = u32::from_be_bytes(header);
    if declared > limits.max_frame {
        return Err(FrameError::TooLarge {
            declared,
            ceiling: limits.max_frame,
        });
    }
    let mut payload = vec![0u8; declared as usize];
    reader.read_exact(&mut payload)?;
    Ok(Some(decode_payload(&payload)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::method;

    #[test]
    fn should_round_trip_an_envelope_when_framed_and_read_back() {
        let envelope = Envelope::Request {
            seq: 1,
            method: method::HEALTH_PROBE.to_owned(),
            params: serde_json::json!({}),
        };
        let frame = encode_frame(&envelope, FrameLimits::default()).expect("encodes");
        let mut cursor = std::io::Cursor::new(frame);
        let back = read_frame(&mut cursor, FrameLimits::default())
            .expect("reads")
            .expect("one frame");
        assert_eq!(back, envelope);
    }

    #[test]
    fn should_refuse_an_oversized_declaration_before_allocating_when_reading() {
        // A 4 GiB length claim must fail on the claim, not on the allocation (ADR-0015 T7).
        let mut bytes = Vec::from(u32::MAX.to_be_bytes());
        bytes.extend_from_slice(b"{}");
        let mut cursor = std::io::Cursor::new(bytes);
        let error = read_frame(&mut cursor, FrameLimits::default()).unwrap_err();
        assert!(matches!(error, FrameError::TooLarge { .. }));
    }

    #[test]
    fn should_report_a_clean_end_of_stream_as_none_when_reading() {
        let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
        let frame = read_frame(&mut cursor, FrameLimits::default()).expect("clean end");
        assert!(frame.is_none());
    }

    #[test]
    fn should_refuse_garbage_bytes_as_malformed_when_decoding() {
        let mut bytes = Vec::from(7u32.to_be_bytes());
        bytes.extend_from_slice(b"not-json...."[..7].as_ref());
        let mut cursor = std::io::Cursor::new(bytes);
        let error = read_frame(&mut cursor, FrameLimits::default()).unwrap_err();
        assert!(matches!(error, FrameError::Malformed(_)));
    }
}
