//! What can be wrong with what a peer put on the wire, and the shell error it becomes.
//!
//! Everything in [`ProtocolError`] describes a peer that is not speaking this protocol
//! correctly, so all of it maps to `remote.protocol_mismatch` (E0602). The two remote conditions
//! that are *not* about the wire keep their own codes: a link that cannot be established or is
//! lost is `remote.unreachable` (E0601), and a peer whose key changed is
//! `remote.host_key_changed` (E0603), which ADR-0006 classifies as `safety` because it is a
//! trust decision rather than a transport failure.

use ono_core::ErrorCode;
use ono_value::ErrorValue;

use crate::FrameKind;

/// A peer said something this protocol cannot mean.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProtocolError {
    /// A frame claimed a payload larger than the agreed bound.
    #[error("a frame claims {claimed} bytes of payload, and the limit is {limit}")]
    FrameTooLarge {
        /// The length the header claimed.
        claimed: usize,
        /// The largest length that would have been accepted.
        limit: usize,
    },
    /// A frame carried a framing version this build does not speak.
    #[error("a frame carries framing version {found}, and this build speaks version 1")]
    UnsupportedFrameVersion {
        /// The version byte that arrived.
        found: u8,
    },
    /// A frame carried a message kind this protocol does not define.
    #[error("a frame carries message kind {kind}, which this protocol does not define")]
    UnknownFrameKind {
        /// The kind byte that arrived.
        kind: u8,
    },
    /// A frame set a flag bit this version does not define.
    #[error("a frame sets undefined frame flags {flags:#04x}")]
    UnknownFrameFlags {
        /// The bits that were set.
        flags: u8,
    },
    /// A payload was not the document its frame kind promises.
    #[error("a {kind} message does not hold what its kind promises: {detail}")]
    MalformedPayload {
        /// The kind the frame claimed.
        kind: FrameKind,
        /// What was wrong with it.
        detail: String,
    },
    /// A value nested deeper than the agreed bound.
    #[error("a value nests {depth} levels deep, and the limit is {limit}")]
    ValueTooDeep {
        /// How deep the value went before the decoder stopped.
        depth: usize,
        /// The deepest nesting that would have been accepted.
        limit: usize,
    },
    /// A peer sent more messages on a stream than it had been granted credit for.
    #[error("the peer sent more on stream {stream} than its credit window allows")]
    CreditExceeded {
        /// The stream the peer overran.
        stream: u32,
    },
    /// More streams were asked for than the link allows to be open at once.
    #[error("the link already has {limit} streams open, which is the limit")]
    TooManyStreams {
        /// The largest number of concurrent streams the link allows.
        limit: usize,
    },
}

impl From<ProtocolError> for ErrorValue {
    fn from(error: ProtocolError) -> Self {
        let help = match &error {
            ProtocolError::FrameTooLarge { .. }
            | ProtocolError::ValueTooDeep { .. }
            | ProtocolError::CreditExceeded { .. }
            | ProtocolError::TooManyStreams { .. } => {
                "the remote is exceeding a bound this link enforces; the link is not usable while \
                 it does"
            }
            ProtocolError::UnsupportedFrameVersion { .. }
            | ProtocolError::UnknownFrameKind { .. }
            | ProtocolError::UnknownFrameFlags { .. } => {
                "the remote speaks a different version of the Ono link protocol; upgrade the end \
                 that is behind"
            }
            ProtocolError::MalformedPayload { .. } => {
                "the bytes on this link are not Ono link frames; check that the transport reaches \
                 an ono agent and not something else"
            }
        };
        ErrorValue::new(ErrorCode::RemoteProtocolMismatch, error.to_string())
            .with_help(help)
            .with_retryable(false)
    }
}

/// The error a link reports when it cannot be established, or is lost while in use.
pub(crate) fn unreachable(detail: impl std::fmt::Display) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RemoteUnreachable,
        format!("the remote link is not available: {detail}"),
    )
    .with_retryable(true)
}

/// The error a peer reports when no protocol version is shared (spec §21.2).
pub(crate) fn version_mismatch(ours: &[u16], theirs: &[u16]) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RemoteProtocolMismatch,
        format!(
            "no shared link protocol version: this end speaks {}, the remote speaks {}",
            render_versions(ours),
            render_versions(theirs)
        ),
    )
    .with_help("upgrade whichever end is behind; the link protocol is versioned deliberately")
    .with_retryable(false)
}

fn render_versions(versions: &[u16]) -> String {
    if versions.is_empty() {
        return "nothing".to_owned();
    }
    versions
        .iter()
        .map(u16::to_string)
        .collect::<Vec<String>>()
        .join(", ")
}
