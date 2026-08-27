//! Reading frames off a transport and writing them back, with the bounds of ADR-0015 T7.
//!
//! # Why the outbound queue is not itself bounded
//!
//! What bounds a link's memory is **credit**, not the queue: a producer may not put a data
//! message on the wire until its consumer has granted room for it (see [`crate::link`]), so at
//! most `max_streams × credit_window` data frames can be waiting to be written at any moment,
//! whatever the queue's capacity.
//!
//! Bounding the queue on top of that would buy nothing and cost something important: a full
//! queue would block a `cancel` behind the very data the cancel is meant to stop, and a stream
//! nobody can cancel is exactly the failure spec §18.5 forbids. Control frames must always be
//! able to leave.
//!
//! # Why the inbound buffer cannot grow without limit
//!
//! A frame's header is decoded before its payload is waited for, and a header claiming more than
//! [`Limits::max_frame_payload`](crate::Limits::max_frame_payload) is refused there and then. So
//! the read buffer holds at most one header plus one maximal payload before a frame comes out of
//! it or the link fails.

use bytes::BytesMut;
use ono_value::ErrorValue;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::sync::mpsc;

use crate::error::unreachable;
use crate::{FRAME_HEADER_LEN, Frame, Limits, decode, encode};

/// The far end is gone: the link failed, or the writer stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Gone;

/// What the writer task is asked to do next.
#[derive(Debug)]
enum Outbound {
    /// Put this frame on the wire.
    Frame(Frame),
    /// Flush what is queued ahead of this and shut the wire down.
    ///
    /// Hanging up is a queue entry rather than dropping the sink because the sink is shared:
    /// the reader task holds it to grant credit and send cancels, so it going out of scope
    /// cannot be the close signal — the owner of the link says goodbye explicitly.
    Hangup,
}

/// Where every frame this end sends is queued for the writer task.
#[derive(Debug, Clone)]
pub(crate) struct FrameSink {
    sender: mpsc::UnboundedSender<Outbound>,
}

impl FrameSink {
    /// Queues a frame. Never waits: see the module documentation.
    pub(crate) fn send(&self, frame: Frame) -> Result<(), Gone> {
        self.sender.send(Outbound::Frame(frame)).map_err(|_| Gone)
    }

    /// Flushes everything queued so far, then shuts the transport down.
    pub(crate) fn hangup(&self) {
        let _ = self.sender.send(Outbound::Hangup);
    }
}

/// Runs the writing half of a connection until the sink is dropped or the transport fails.
pub(crate) fn spawn_writer<W>(writer: W, limits: Limits) -> FrameSink
where
    W: AsyncWrite + Send + Unpin + 'static,
{
    let (sender, mut receiver) = mpsc::unbounded_channel::<Outbound>();
    tokio::spawn(async move {
        let mut writer = writer;
        let mut buffer = BytesMut::new();
        while let Some(outbound) = receiver.recv().await {
            let frame = match outbound {
                Outbound::Frame(frame) => frame,
                Outbound::Hangup => break,
            };
            buffer.clear();
            if encode(&frame, &limits, &mut buffer).is_err() {
                // A frame this end built that this end would refuse to read is a bug here, not a
                // fault of the peer; stopping is better than putting it on the wire.
                break;
            }
            if writer.write_all(&buffer).await.is_err() || writer.flush().await.is_err() {
                break;
            }
        }
        let _ = writer.shutdown().await;
    });
    FrameSink { sender }
}

/// The reading half of a connection.
#[derive(Debug)]
pub(crate) struct FrameReader<R> {
    reader: R,
    buffer: BytesMut,
    limits: Limits,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    /// A reader over `reader`, enforcing `limits`.
    pub(crate) fn new(reader: R, limits: Limits) -> Self {
        Self {
            reader,
            buffer: BytesMut::with_capacity(FRAME_HEADER_LEN * 4),
            limits,
        }
    }

    /// The next frame, or `None` when the peer closed the link cleanly.
    ///
    /// # Errors
    ///
    /// Returns `remote.protocol_mismatch` when the bytes are not frames this build can read, and
    /// `remote.unreachable` when the transport failed or ended in the middle of a frame.
    pub(crate) async fn next(&mut self) -> Result<Option<Frame>, ErrorValue> {
        loop {
            if let Some(frame) = decode(&mut self.buffer, &self.limits).map_err(ErrorValue::from)? {
                return Ok(Some(frame));
            }
            let read = self
                .reader
                .read_buf(&mut self.buffer)
                .await
                .map_err(unreachable)?;
            if read == 0 {
                return if self.buffer.is_empty() {
                    Ok(None)
                } else {
                    Err(unreachable(
                        "the peer closed the link in the middle of a frame",
                    ))
                };
            }
        }
    }
}
