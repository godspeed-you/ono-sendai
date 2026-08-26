//! The framing contract of spec §21.2 and the bounds ADR-0015 T7 makes release-blocking.
//!
//! Every assertion here is about bytes on the wire and the error a decoder raises — never about
//! how the decoder is written.

use bytes::BytesMut;
use ono_protocol::{FRAME_HEADER_LEN, Frame, FrameKind, Limits, ProtocolError, decode, encode};

fn limits() -> Limits {
    Limits::default()
}

#[test]
fn should_round_trip_a_frame_when_encoded_and_decoded() {
    let frame = Frame::new(FrameKind::Value, 7, b"payload".to_vec());
    let mut buffer = BytesMut::new();
    encode(&frame, &limits(), &mut buffer).expect("a small frame encodes");

    let decoded = decode(&mut buffer, &limits())
        .expect("a complete frame decodes")
        .expect("the buffer held one whole frame");

    assert_eq!(decoded.kind(), FrameKind::Value);
    assert_eq!(decoded.stream(), 7);
    assert_eq!(decoded.payload().as_ref(), b"payload");
    assert!(buffer.is_empty(), "the decoder consumed exactly one frame");
}

#[test]
fn should_write_a_fixed_header_before_the_payload_when_encoding() {
    let frame = Frame::new(FrameKind::End, 0x0102_0304, Vec::new());
    let mut buffer = BytesMut::new();
    encode(&frame, &limits(), &mut buffer).expect("an empty frame encodes");

    assert_eq!(
        buffer.len(),
        FRAME_HEADER_LEN,
        "an empty payload costs exactly the header"
    );
    assert_eq!(
        buffer.as_ref(),
        &[
            1,
            FrameKind::End.as_u8(),
            0,
            0,
            0x01,
            0x02,
            0x03,
            0x04,
            0,
            0,
            0,
            0
        ],
        "the header is version, kind, flags, reserved, stream id and length, big-endian"
    );
}

#[test]
fn should_report_incomplete_when_the_buffer_holds_less_than_a_whole_frame() {
    let frame = Frame::new(FrameKind::Value, 1, b"twelve bytes".to_vec());
    let mut whole = BytesMut::new();
    encode(&frame, &limits(), &mut whole).expect("the frame encodes");

    for prefix in 0..whole.len() {
        let mut partial = BytesMut::from(&whole[..prefix]);
        assert_eq!(
            decode(&mut partial, &limits()),
            Ok(None),
            "a {prefix}-byte prefix is incomplete, not malformed"
        );
        assert_eq!(
            partial.len(),
            prefix,
            "an incomplete frame consumes nothing, so the next read can complete it"
        );
    }
}

#[test]
fn should_decode_every_frame_when_several_arrive_in_one_read() {
    let mut buffer = BytesMut::new();
    for stream in 1..=3u32 {
        let frame = Frame::new(FrameKind::Credit, stream, vec![stream as u8]);
        encode(&frame, &limits(), &mut buffer).expect("the frame encodes");
    }

    let mut streams = Vec::new();
    while let Some(frame) = decode(&mut buffer, &limits()).expect("each frame decodes") {
        streams.push(frame.stream());
    }
    assert_eq!(streams, [1, 2, 3]);
    assert!(buffer.is_empty());
}

#[test]
fn should_refuse_a_frame_claiming_more_than_the_limit_before_allocating() {
    let limits = Limits::default().with_max_frame_payload(1024);
    let mut buffer = BytesMut::new();
    buffer.extend_from_slice(&[1, FrameKind::Value.as_u8(), 0, 0]);
    buffer.extend_from_slice(&1u32.to_be_bytes());
    buffer.extend_from_slice(&u32::MAX.to_be_bytes());

    let outcome = decode(&mut buffer, &limits);

    assert_eq!(
        outcome,
        Err(ProtocolError::FrameTooLarge {
            claimed: u32::MAX as usize,
            limit: 1024,
        }),
        "a length field is a claim, not an instruction to allocate"
    );
    assert!(
        buffer.capacity() < 1024 * 16,
        "refusing an oversized frame must not have grown the buffer, it held {} bytes",
        buffer.capacity()
    );
}

#[test]
fn should_accept_a_payload_of_exactly_the_limit_and_refuse_one_byte_more() {
    let limits = Limits::default().with_max_frame_payload(64);

    let at_limit = Frame::new(FrameKind::Value, 1, vec![7u8; 64]);
    let mut buffer = BytesMut::new();
    encode(&at_limit, &limits, &mut buffer).expect("a payload at the limit encodes");
    let decoded = decode(&mut buffer, &limits)
        .expect("a payload at the limit decodes")
        .expect("the frame was complete");
    assert_eq!(decoded.payload().len(), 64);

    let over_limit = Frame::new(FrameKind::Value, 1, vec![7u8; 65]);
    assert_eq!(
        encode(&over_limit, &limits, &mut BytesMut::new()),
        Err(ProtocolError::FrameTooLarge {
            claimed: 65,
            limit: 64,
        }),
        "a local sender must not put on the wire what a peer must refuse"
    );
}

#[test]
fn should_refuse_a_frame_whose_version_is_not_the_one_we_speak() {
    let mut buffer = BytesMut::new();
    buffer.extend_from_slice(&[9, FrameKind::Value.as_u8(), 0, 0]);
    buffer.extend_from_slice(&1u32.to_be_bytes());
    buffer.extend_from_slice(&0u32.to_be_bytes());

    assert_eq!(
        decode(&mut buffer, &limits()),
        Err(ProtocolError::UnsupportedFrameVersion { found: 9 })
    );
}

#[test]
fn should_refuse_a_frame_whose_kind_is_not_in_the_protocol() {
    let mut buffer = BytesMut::new();
    buffer.extend_from_slice(&[1, 0xEE, 0, 0]);
    buffer.extend_from_slice(&1u32.to_be_bytes());
    buffer.extend_from_slice(&0u32.to_be_bytes());

    assert_eq!(
        decode(&mut buffer, &limits()),
        Err(ProtocolError::UnknownFrameKind { kind: 0xEE })
    );
}

#[test]
fn should_refuse_a_frame_whose_reserved_bits_are_set() {
    let mut buffer = BytesMut::new();
    buffer.extend_from_slice(&[1, FrameKind::Value.as_u8(), 0x80, 0]);
    buffer.extend_from_slice(&1u32.to_be_bytes());
    buffer.extend_from_slice(&0u32.to_be_bytes());

    assert!(
        matches!(
            decode(&mut buffer, &limits()),
            Err(ProtocolError::UnknownFrameFlags { .. })
        ),
        "a flag this version does not define may mean a payload this version cannot read"
    );
}

#[test]
fn should_report_every_framing_failure_as_a_structured_shell_error() {
    let failures = [
        ProtocolError::FrameTooLarge {
            claimed: 1 << 30,
            limit: 1024,
        },
        ProtocolError::UnsupportedFrameVersion { found: 9 },
        ProtocolError::UnknownFrameKind { kind: 0xEE },
    ];
    for failure in failures {
        let error = ono_value::ErrorValue::from(failure.clone());
        assert_eq!(
            error.code(),
            ono_core::ErrorCode::RemoteProtocolMismatch,
            "a peer that puts {failure} on the wire is not speaking this protocol"
        );
        assert!(
            !error.message().is_empty(),
            "an error a user sees must say what happened"
        );
    }
}
