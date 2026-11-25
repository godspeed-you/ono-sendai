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
//! # What does bound the frames no credit governs
//!
//! An agent also writes frames that no credit governs, because its peer asked for them and they
//! cost the peer nothing to ask: the answer to a probe (ADR-1095) and the refusal of a stream
//! beyond `max_streams`. A peer that asks for them and never reads would grow the queue without
//! limit. So they are counted: at most [`MAX_UNCREDITED_FRAMES`] may wait to be written at once,
//! and a sink asked for one more refuses it with [`Overrun`] — the session then ends with
//! a structured refusal (ADR-1115). An honest peer, which reads what it asked for, never comes near
//! the allowance.
//!
//! # Why the writer has deadlines
//!
//! A peer that stops reading leaves the writer blocked in `write_all`. While the session lives, a
//! write that makes no progress for [`WRITE_DEADLINE`] ends the connection: a link that cannot
//! deliver a frame in that long is not one anybody is reading. Once the session has ended — the
//! caller hung up, the agent revoked it, the serving loop returned — what is still queued gets
//! at most the handshake timeout to leave, and then the socket is dropped, so a peer that stops
//! reading cannot hold it past its registry slot or its revocation (v0.4.1 §12.5, §12.6).

//! # Why the inbound buffer cannot grow without limit
//!
//! A frame's header is decoded before its payload is waited for, and a header claiming more than
//! [`Limits::max_frame_payload`](crate::Limits::max_frame_payload) is refused there and then. So
//! the read buffer holds at most one header plus one maximal payload before a frame comes out of
//! it or the link fails.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::BytesMut;
use ono_value::ErrorValue;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::sync::mpsc;

use crate::error::unreachable;
use crate::{FRAME_HEADER_LEN, Frame, Limits, decode, encode};

/// How many frames no credit governs may wait to be written at once (ADR-1115).
pub const MAX_UNCREDITED_FRAMES: usize = 1024;

/// How long one write may make no progress while a session lives (ADR-1115).
pub const WRITE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// The far end is gone: the link failed, or the writer stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Gone;

/// Why a frame no credit governs was not queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// The far end is gone.
    Gone,
    /// [`MAX_UNCREDITED_FRAMES`] are already waiting: the peer is not reading what it asked for.
    Overrun,
}

/// What the writer task is asked to do next.
#[derive(Debug)]
enum Outbound {
    /// Put this frame on the wire; `true` when no credit governs it.
    Frame(Frame, bool),
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
    /// Frames no credit governs that are queued and not yet written.
    uncredited: Arc<AtomicUsize>,
    /// Set once the session has ended: the writer's last deadline starts.
    closing: Arc<tokio::sync::watch::Sender<bool>>,
}

impl FrameSink {
    /// Queues a frame. Never waits: see the module documentation.
    pub(crate) fn send(&self, frame: Frame) -> Result<(), Gone> {
        self.sender
            .send(Outbound::Frame(frame, false))
            .map_err(|_| Gone)
    }

    /// Queues a frame no credit governs — a probe's answer, a refusal of a stream — within the
    /// allowance of [`MAX_UNCREDITED_FRAMES`].
    pub(crate) fn send_uncredited(&self, frame: Frame) -> Result<(), Refused> {
        if self.uncredited.fetch_add(1, Ordering::SeqCst) >= MAX_UNCREDITED_FRAMES {
            self.uncredited.fetch_sub(1, Ordering::SeqCst);
            return Err(Refused::Overrun);
        }
        self.sender
            .send(Outbound::Frame(frame, true))
            .map_err(|_| Refused::Gone)
    }

    /// Flushes everything queued so far, then shuts the transport down — within the handshake
    /// timeout: a peer that does not read is not waited for longer.
    pub(crate) fn hangup(&self) {
        let _ = self.sender.send(Outbound::Hangup);
        self.closing.send_replace(true);
    }
}

/// Runs the writing half of a connection until the sink is dropped, the transport fails, or a
/// deadline runs out (see the module documentation).
pub(crate) fn spawn_writer<W>(writer: W, limits: Limits) -> FrameSink
where
    W: AsyncWrite + Send + Unpin + 'static,
{
    let (sender, mut receiver) = mpsc::unbounded_channel::<Outbound>();
    let uncredited = Arc::new(AtomicUsize::new(0));
    let (closing, mut closed) = tokio::sync::watch::channel(false);
    let counted = Arc::clone(&uncredited);
    tokio::spawn(async move {
        let mut writer = writer;
        let mut buffer = BytesMut::new();
        // Once the session has ended, everything left must be written by this moment.
        let mut close_by: Option<tokio::time::Instant> = None;
        let linger = limits.handshake_timeout();
        loop {
            let outbound = tokio::select! {
                biased;
                outbound = receiver.recv() => outbound,
                () = until(close_by) => break,
                changed = closed.changed(), if close_by.is_none() => {
                    if changed.is_err() || *closed.borrow() {
                        close_by = Some(tokio::time::Instant::now() + linger);
                    }
                    continue;
                }
            };
            let Some(outbound) = outbound else { break };
            let (frame, free) = match outbound {
                Outbound::Frame(frame, free) => (frame, free),
                Outbound::Hangup => break,
            };
            if free {
                counted.fetch_sub(1, Ordering::SeqCst);
            }
            buffer.clear();
            if encode(&frame, &limits, &mut buffer).is_err() {
                // A frame this end built that this end would refuse to read is a bug here, not a
                // fault of the peer; stopping is better than putting it on the wire.
                break;
            }
            let write = async {
                writer.write_all(&buffer).await?;
                writer.flush().await
            };
            tokio::pin!(write);
            let deadline = tokio::time::Instant::now() + WRITE_DEADLINE;
            let written = loop {
                let by = close_by.map_or(deadline, |at| at.min(deadline));
                tokio::select! {
                    written = &mut write => break written.is_ok(),
                    () = tokio::time::sleep_until(by) => break false,
                    changed = closed.changed(), if close_by.is_none() => {
                        if changed.is_err() || *closed.borrow() {
                            close_by = Some(tokio::time::Instant::now() + linger);
                        }
                    }
                }
            };
            if !written {
                break;
            }
        }
        if close_by.is_none_or(|at| tokio::time::Instant::now() < at) {
            let _ = tokio::time::timeout(linger, writer.shutdown()).await;
        }
    });
    FrameSink {
        sender,
        uncredited,
        closing: Arc::new(closing),
    }
}

/// Resolves at `at`, or never.
async fn until(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Writes one refusal and lets the peer go, all within `limits`' handshake timeout.
///
/// A refusal is the last thing a connection carries before a link exists, so it is written here
/// rather than through a writer task: a writer task blocked on a peer that never reads would hold
/// the socket for as long as the peer liked (§12.6). Inside the one bound, three things happen:
/// the frame is written and flushed, the sending direction is shut down, and whatever the peer
/// still sends is read and discarded until it hangs up — a socket closed with unread input is
/// reset, and a reset can discard the refusal before the peer reads it. A peer that neither
/// reads nor hangs up is let go when the bound runs out, and costs the agent exactly that long
/// whatever it does (ADR-1057).
pub(crate) async fn refuse_within<R, W>(
    reader: &mut FrameReader<R>,
    mut writer: W,
    frame: &Frame,
    limits: &Limits,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = BytesMut::new();
    if encode(frame, limits, &mut buffer).is_err() {
        // A refusal this end built and would not itself read is a defect here; nothing is sent.
        return;
    }
    let _ = tokio::time::timeout(limits.handshake_timeout(), async {
        if writer.write_all(&buffer).await.is_ok() && writer.flush().await.is_ok() {
            let _ = writer.shutdown().await;
        }
        while matches!(reader.next().await, Ok(Some(_))) {}
    })
    .await;
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
