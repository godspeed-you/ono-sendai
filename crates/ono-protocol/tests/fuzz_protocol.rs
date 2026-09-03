//! Fuzz-style suites for every decoder in the crate (spec §35.6, ADR-0015 T7).
//!
//! Each decoder here reads bytes another machine chose. The contract they are held to is the
//! same one three times over: **no panic, no unbounded allocation, a structured error every
//! time.** The seeds are fixed, so a failure is reproducible from the message it prints
//! (AGENTS.md §11).

mod common;

use bytes::BytesMut;
use ono_protocol::{
    FRAME_HEADER_LEN, Frame, FrameKind, Limits, Message, decode, decode_message, encode,
    encode_message,
};
use ono_testkit::Rng;

/// A tight bound, so that a decoder that grew its buffer to the claimed length would be caught.
fn limits() -> Limits {
    Limits::default()
        .with_max_frame_payload(4096)
        .with_max_value_depth(16)
}

/// Every frame kind, for driving the message decoder with payloads it never asked for.
fn kinds() -> Vec<FrameKind> {
    FrameKind::ALL.to_vec()
}

#[test]
fn should_never_panic_when_the_frame_decoder_is_fed_arbitrary_bytes() {
    let mut rng = Rng::seeded(0x0FF1_CE0F_F1CE);
    for round in 0..2_000 {
        let length = rng.below(96);
        let mut buffer = BytesMut::with_capacity(length);
        for _ in 0..length {
            buffer.extend_from_slice(&[(rng.next_u64() & 0xFF) as u8]);
        }
        let before = buffer.capacity();
        // The loop drains whatever happens to be decodable, exactly as a connection would; a
        // refusal and an incomplete frame both end it, and neither may panic.
        while let Ok(Some(_)) = decode(&mut buffer, &limits()) {}
        assert!(
            buffer.capacity() <= before.max(FRAME_HEADER_LEN) + 4096,
            "round {round}: the decoder grew its buffer from {before} to {}",
            buffer.capacity()
        );
    }
}

#[test]
fn should_refuse_every_impossible_length_a_header_can_claim() {
    let mut rng = Rng::seeded(0xDEAD_BEEF_CAFE);
    for round in 0..1_000 {
        let claimed = match round % 4 {
            0 => u32::MAX,
            1 => u32::MAX - (rng.below(1024) as u32),
            2 => 4097 + (rng.below(1 << 20) as u32),
            _ => (rng.next_u64() as u32) | 0x8000_0000,
        };
        let mut buffer = BytesMut::new();
        buffer.extend_from_slice(&[1, FrameKind::Value.as_u8(), 0, 0]);
        buffer.extend_from_slice(&1u32.to_be_bytes());
        buffer.extend_from_slice(&claimed.to_be_bytes());

        let outcome = decode(&mut buffer, &limits());

        assert!(
            outcome.is_err(),
            "round {round}: a frame claiming {claimed} bytes must be refused, not believed"
        );
        assert!(
            buffer.capacity() < 64 * 1024,
            "round {round}: refusing {claimed} bytes allocated {} of buffer",
            buffer.capacity()
        );
    }
}

#[test]
fn should_treat_every_truncation_of_a_valid_frame_as_incomplete() {
    let mut rng = Rng::seeded(0x5EED_0001);
    for round in 0..500 {
        let payload_len = rng.below(400);
        let payload: Vec<u8> = (0..payload_len)
            .map(|_| (rng.next_u64() & 0xFF) as u8)
            .collect();
        let kind = *rng.pick(&kinds()).expect("the kind list is not empty");
        let frame = Frame::new(kind, rng.next_u64() as u32, payload);
        let mut whole = BytesMut::new();
        encode(&frame, &limits(), &mut whole).expect("a bounded frame encodes");

        let cut = rng.below(whole.len());
        let mut truncated = BytesMut::from(&whole[..cut]);

        assert_eq!(
            decode(&mut truncated, &limits()),
            Ok(None),
            "round {round}: {cut} of {} bytes is an incomplete frame, not a broken one",
            whole.len()
        );
        assert_eq!(
            truncated.len(),
            cut,
            "round {round}: an incomplete frame must stay in the buffer for the next read"
        );
    }
}

#[test]
fn should_answer_every_random_payload_with_a_structured_error_or_a_message() {
    let mut rng = Rng::seeded(0xB0A7_1234);
    let schemas = common::schemas();
    for round in 0..2_000 {
        let kind = *rng.pick(&kinds()).expect("the kind list is not empty");
        let length = rng.below(64);
        let payload: Vec<u8> = (0..length).map(|_| (rng.next_u64() & 0xFF) as u8).collect();

        // The contract is that this returns; a panic here is the failure the suite exists to
        // catch, and the fixed seed makes it reproducible.
        let outcome = decode_message(kind, &payload, &schemas, &limits());

        if let Err(error) = outcome {
            let structured = ono_value::ErrorValue::from(error);
            assert!(
                !structured.message().is_empty(),
                "round {round}: every refusal must say what was wrong"
            );
        }
    }
}

#[test]
fn should_answer_a_payload_of_plausible_json_with_a_structured_error_or_a_message() {
    let alphabet = [
        "{",
        "}",
        "[",
        "]",
        ",",
        ":",
        "\"",
        "$record",
        "$bytesize",
        "$error",
        "$timestamp",
        "schema",
        "fields",
        "provenance",
        "ono.test.remote/1",
        "null",
        "true",
        "0",
        "-1e999",
        "\\u0000",
    ];
    let mut rng = Rng::seeded(0x1234_5678_9ABC);
    let schemas = common::schemas();
    for round in 0..2_000 {
        let text = rng.assemble(&alphabet, 40);
        let kind = *rng.pick(&kinds()).expect("the kind list is not empty");

        let outcome = decode_message(kind, text.as_bytes(), &schemas, &limits());

        if let Err(error) = outcome {
            let structured = ono_value::ErrorValue::from(error);
            assert_eq!(
                structured.code().kind(),
                ono_core::ErrorKind::Provider,
                "round {round}: a wire failure is reported as one, for {text:?}"
            );
        }
    }
}

#[test]
fn should_refuse_a_deeply_nested_value_rather_than_recursing_into_it() {
    let schemas = common::schemas();
    for depth in [17usize, 64, 1_000, 100_000] {
        let mut payload = String::with_capacity(depth * 2);
        for _ in 0..depth {
            payload.push('[');
        }
        for _ in 0..depth {
            payload.push(']');
        }

        let outcome = decode_message(FrameKind::Value, payload.as_bytes(), &schemas, &limits());

        assert!(
            outcome.is_err(),
            "a value {depth} deep must be refused: the limit is {}",
            limits().max_value_depth()
        );
    }
}

#[test]
fn should_refuse_a_deeply_nested_object_rather_than_recursing_into_it() {
    let schemas = common::schemas();
    for depth in [17usize, 64, 1_000] {
        let mut payload = String::new();
        for index in 0..depth {
            payload.push_str(&format!("{{\"k{index}\":"));
        }
        payload.push_str("null");
        for _ in 0..depth {
            payload.push('}');
        }

        let outcome = decode_message(FrameKind::Value, payload.as_bytes(), &schemas, &limits());

        assert!(outcome.is_err(), "an object {depth} deep must be refused");
    }
}

#[test]
fn should_survive_a_connection_fed_arbitrary_bytes_instead_of_frames() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a test runtime starts");

    runtime.block_on(async {
        let mut rng = Rng::seeded(0xFEED_FACE);
        for round in 0..32 {
            let (near, far) = tokio::io::duplex(4096);
            let noise: Vec<u8> = (0..512).map(|_| (rng.next_u64() & 0xFF) as u8).collect();
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt as _;
                let mut far = far;
                let _ = far.write_all(&noise).await;
                let _ = far.shutdown().await;
            });

            let outcome = common::within(ono_protocol::Link::connect(
                ono_protocol::UnauthenticatedTransport::new(near),
                common::client_config("noisy"),
            ))
            .await;

            let error = outcome
                .err()
                .unwrap_or_else(|| panic!("round {round}: random bytes are not a handshake"));
            assert_eq!(
                error.code().kind(),
                ono_core::ErrorKind::Provider,
                "round {round}: a link that cannot be established fails, it does not hang"
            );
        }
    });
}

#[test]
fn should_round_trip_every_frame_it_encodes_however_random_the_payload() {
    let mut rng = Rng::seeded(0xACE0_ACE1);
    for round in 0..1_000 {
        let payload: Vec<u8> = (0..rng.below(2048))
            .map(|_| (rng.next_u64() & 0xFF) as u8)
            .collect();
        let kind = *rng.pick(&kinds()).expect("the kind list is not empty");
        let stream = rng.next_u64() as u32;
        let frame = Frame::new(kind, stream, payload.clone());

        let mut buffer = BytesMut::new();
        encode(&frame, &limits(), &mut buffer).expect("a bounded frame encodes");
        let decoded = decode(&mut buffer, &limits())
            .unwrap_or_else(|error| panic!("round {round}: {error}"))
            .unwrap_or_else(|| panic!("round {round}: a whole frame decoded as incomplete"));

        assert_eq!(decoded.kind(), kind);
        assert_eq!(decoded.stream(), stream);
        assert_eq!(decoded.payload().as_ref(), payload.as_slice());
    }
}

#[test]
fn should_keep_an_encoded_message_within_the_frame_limit_or_refuse_it() {
    let mut rng = Rng::seeded(0x0A0B_0C0D);
    for round in 0..500 {
        let text: String = (0..rng.below(8192)).map(|_| '\u{1f600}').collect();
        let message = Message::Value(ono_value::Value::String(text.as_str().into()));

        match encode_message(&message, &limits()) {
            Ok(payload) => assert!(
                payload.len() <= limits().max_frame_payload(),
                "round {round}: an accepted message must fit in a frame"
            ),
            Err(error) => assert!(
                matches!(error, ono_protocol::ProtocolError::FrameTooLarge { .. }),
                "round {round}: the only reason to refuse is size, got {error}"
            ),
        }
    }
}
