//! What a handshake refusal may carry to the peer it refuses (ADR-1056; v0.4.1 §53.3, §54.1,
//! §59.1).
//!
//! A `Reject` is written before a link exists, to a peer the agent has authenticated and has not
//! authorized — or, for a ceiling or a deadline, has not yet asked about at all. §54.2 wants the
//! refusal's explanation in the ordinary structured error at the client; §59.1 wants the peer to
//! learn nothing "beyond minimal rejection protocol data". The two meet in
//! `docs/contracts/hardening/refusals.yaml`, whose `over_the_wire` field names, per refusal, the
//! explaining keys that are rejection data and nothing more. This module is that field as code,
//! applied twice: by the agent before the frame is written, and by the client to what it reads,
//! so neither a careless agent nor a hostile one can put anything else into a client's error.
//! `tests/refusal_details.rs` holds the table to the contract.

use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};
use serde_json::{Map, Value as Json};

/// The longest text detail either side accepts, in bytes.
///
/// The text details are a fingerprint (`sha256:` and 64 hex digits), a ceiling's name and a
/// reason word. A value longer than this is not one of those, and a client is better told
/// nothing than told a truncated fingerprint.
pub(crate) const MAX_DETAIL_TEXT: usize = 256;

/// The shape a detail must have to be believed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Text,
    Flag,
    Count,
}

/// The keys a handshake refusal with `code` carries to the peer, in the census's order.
///
/// `refusals.yaml` `over_the_wire`, row by row; a code without a row carries nothing.
fn over_the_wire(code: ErrorCode) -> &'static [(&'static str, Shape)] {
    match code {
        ErrorCode::RemoteUnauthenticated | ErrorCode::RemoteAuthorizationStoreInvalid => {
            &[("denied_because", Shape::Text)]
        }
        // The fingerprint is the refused peer's own key; whether a store exists is what the
        // sentence already says in words (ADR-1056).
        ErrorCode::RemoteUnauthorized => &[
            ("peer_fingerprint", Shape::Text),
            ("store_present", Shape::Flag),
        ],
        ErrorCode::RemoteConnectionLimit => &[("ceiling", Shape::Text), ("limit", Shape::Count)],
        ErrorCode::RemoteHandshakeTimeout => &[("timeout_ms", Shape::Count)],
        _ => &[],
    }
}

/// The details of `refusal` that may cross the wire, as the `Reject` frame carries them.
pub(crate) fn details_for_the_wire(refusal: &ErrorValue) -> Map<String, Json> {
    over_the_wire(refusal.code())
        .iter()
        .filter_map(|(key, shape)| {
            let value = refusal.metadata().get(key)?;
            Some(((*key).to_owned(), to_wire(value, *shape)?))
        })
        .collect()
}

/// The structured error a client reports for a refusal with `code`, `message` and `details`.
///
/// Every detail the contract does not name for `code`, or that does not have the shape the
/// contract's key has, is dropped rather than believed: what arrives here came from a peer that
/// just refused to serve us, and it may have been written by anyone.
/// The longest refusal sentence a client keeps from the wire, in bytes (review SEC-5): `details`
/// is bounded by the census, and the sentence is bounded here rather than by the frame limit.
pub const MAX_REFUSAL_MESSAGE: usize = 1024;

/// `message` cut to [`MAX_REFUSAL_MESSAGE`] bytes at a character boundary, marked when cut.
fn bounded(message: &str) -> std::borrow::Cow<'_, str> {
    if message.len() <= MAX_REFUSAL_MESSAGE {
        return std::borrow::Cow::Borrowed(message);
    }
    let mut end = MAX_REFUSAL_MESSAGE;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    std::borrow::Cow::Owned(format!("{}…", &message[..end]))
}

pub(crate) fn refusal_from_the_wire(
    code: Option<ErrorCode>,
    message: &str,
    details: &Map<String, Json>,
) -> ErrorValue {
    let message = &*bounded(message);
    let Some(code) = code else {
        // A code this build cannot name is the mismatch it is; nothing governs its details.
        return ErrorValue::new(ErrorCode::RemoteProtocolMismatch, message).with_retryable(false);
    };
    // A ceiling and a deadline are the two handshake refusals that are about the moment rather
    // than about who is asking (§12.1–§12.3), so they are the two worth trying again.
    let retryable = matches!(
        code,
        ErrorCode::RemoteConnectionLimit | ErrorCode::RemoteHandshakeTimeout
    );
    let mut refused = ErrorValue::new(code, message).with_retryable(retryable);
    for (key, shape) in over_the_wire(code) {
        if let Some(value) = details.get(*key).and_then(|json| from_wire(json, *shape)) {
            refused = refused.with_metadata(key, value);
        }
    }
    refused
}

fn to_wire(value: &Value, shape: Shape) -> Option<Json> {
    match (shape, value) {
        (Shape::Text, Value::String(text)) if text.len() <= MAX_DETAIL_TEXT => {
            Some(Json::String(text.to_string()))
        }
        (Shape::Flag, Value::Bool(flag)) => Some(Json::Bool(*flag)),
        (Shape::Count, Value::Int(count)) => u64::try_from(*count).ok().map(Json::from),
        _ => None,
    }
}

fn from_wire(json: &Json, shape: Shape) -> Option<Value> {
    match (shape, json) {
        (Shape::Text, Json::String(text)) if text.len() <= MAX_DETAIL_TEXT => {
            Some(Value::string(text))
        }
        (Shape::Flag, Json::Bool(flag)) => Some(Value::Bool(*flag)),
        (Shape::Count, Json::Number(number)) => number.as_u64().map(|n| Value::Int(n.into())),
        _ => None,
    }
}
