//! The byte pipes a link can run over when the network layer is somebody else's.
//!
//! Spec §21.5 requires authenticated encryption on a link, and `ono-protocol` deliberately does
//! not implement it: a [`Transport`] is "a byte stream that has already authenticated and
//! encrypted itself". These are the two transports Phase H needs that fit that shape without
//! bringing a cryptography stack of their own:
//!
//! - [`StdioTransport`] joins an existing reader and writer — an agent's stdin/stdout, a pair
//!   of test pipes — into one transport. It authenticates nobody by itself; whatever protects
//!   the two halves (the ssh session around the agent process, the process boundary in a test)
//!   is declared through [`StdioTransport::with_peer_key`].
//! - [`SubprocessTransport`] spawns a command and speaks through its stdin/stdout. With
//!   [`ssh_command`] as the command it is the SSH fallback of spec §37 Phase H: authentication,
//!   encryption and host verification are ssh's, and the agent at the far end is
//!   `ono --agent`. With any other command it is the same transport over a local child, which
//!   is how the suites prove it without a network.

use std::pin::Pin;
use std::process::ExitStatus;
use std::task::{Context, Poll};
use std::time::Duration;

use ono_core::ErrorCode;
use ono_protocol::{HostKey, Transport};
use ono_value::ErrorValue;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, watch};

/// A transport over a separate reader and writer, such as a process's stdin and stdout.
///
/// ```
/// use ono_protocol::Transport;
/// use ono_remote::StdioTransport;
/// let (reader, writer) = (tokio::io::empty(), tokio::io::sink());
/// assert!(StdioTransport::new(reader, writer).peer_key().is_none());
/// ```
#[derive(Debug)]
pub struct StdioTransport<R, W> {
    reader: R,
    writer: W,
    peer_key: Option<HostKey>,
}

impl<R, W> StdioTransport<R, W> {
    /// A transport reading from `reader` and writing to `writer`, authenticating nobody.
    #[must_use]
    pub const fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            peer_key: None,
        }
    }

    /// Declares the key an outer layer authenticated for this peer.
    #[must_use]
    pub fn with_peer_key(mut self, key: HostKey) -> Self {
        self.peer_key = Some(key);
        self
    }
}

impl<R, W> Transport for StdioTransport<R, W>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    fn peer_key(&self) -> Option<&HostKey> {
        self.peer_key.as_ref()
    }
}

impl<R: AsyncRead + Unpin, W: Unpin> AsyncRead for StdioTransport<R, W> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl<R: Unpin, W: AsyncWrite + Unpin> AsyncWrite for StdioTransport<R, W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.writer).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_shutdown(cx)
    }
}

/// A host to reach over ssh, spelled the way `link host` will collect it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    host: String,
    user: Option<String>,
    port: Option<u16>,
    config: Option<std::path::PathBuf>,
}

impl SshTarget {
    /// The host `host`, as ssh resolves it (a name, an address, or an `ssh_config` alias).
    #[must_use]
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            user: None,
            port: None,
            config: None,
        }
    }

    /// Reads `config` as the client configuration (`ssh -F`) rather than the files ssh finds
    /// on its own — so the file the shell lists hosts from (ADR-0103) is the file ssh resolves
    /// them with, whatever the account's home directory is.
    #[must_use]
    pub fn with_config(mut self, config: impl Into<std::path::PathBuf>) -> Self {
        self.config = Some(config.into());
        self
    }

    /// Logs in as `user` rather than as the local user.
    #[must_use]
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Connects to `port` rather than to ssh's default.
    #[must_use]
    pub const fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// The host name, as the trust and link layers should name it.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The user to log in as, where one was given.
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    /// The port to connect to, where one was given.
    #[must_use]
    pub const fn port(&self) -> Option<u16> {
        self.port
    }

    /// The client configuration file to read, where one was given.
    #[must_use]
    pub fn config(&self) -> Option<&std::path::Path> {
        self.config.as_deref()
    }
}

/// The one place the real ssh invocation is spelled.
///
/// Everything else — the tests above all — goes through [`SubprocessTransport::spawn`] with a
/// command of its own choosing, so nothing but this function has an opinion about ssh:
///
/// - `-o BatchMode=yes`: a refusal is never a prompt (ADR-0015 standing rule 4). A host whose
///   key ssh does not trust fails visibly instead of asking a question a script would
///   eventually answer for the user.
/// - `-T`: the wire is a byte pipe carrying frames, not a terminal session.
/// - `--` before the host, so a host name can never be parsed as an option.
/// - `ono --agent` at the far end: the agent loop of spec §21.4, reading the same pipe.
#[must_use]
pub fn ssh_command(target: &SshTarget) -> Command {
    let mut command = Command::new("ssh");
    command.arg("-o").arg("BatchMode=yes").arg("-T");
    if let Some(config) = &target.config {
        command.arg("-F").arg(config);
    }
    if let Some(port) = target.port {
        command.arg("-p").arg(port.to_string());
    }
    if let Some(user) = &target.user {
        command.arg("-l").arg(user);
    }
    command.arg("--").arg(&target.host);
    command.arg("ono").arg("--agent");
    command
}

/// A transport speaking to a child process over its stdin and stdout.
///
/// The child's stderr is left alone, so ssh's own diagnostics — an unreachable host, a refused
/// key — reach the user's terminal the way they always have (spec §12.5 keeps stderr a byte
/// stream).
///
/// The transport reports no peer key of its own: when the command is ssh, host authentication
/// already happened in ssh's `known_hosts` before a single frame crossed, and claiming it here
/// again would assert something this process did not verify. A caller that *does* verify the
/// peer through another channel declares it with [`with_peer_key`](Self::with_peer_key).
#[derive(Debug)]
pub struct SubprocessTransport {
    /// `None` once the transport was shut down: a pipe signals end-of-input only when its file
    /// descriptor closes, so shutting down must actually let go of the handle — otherwise a
    /// child agent would wait forever for an EOF that a mere flush never delivers.
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
    peer_key: Option<HostKey>,
    child: ChildProcess,
}

impl SubprocessTransport {
    /// Spawns `command` with piped stdin/stdout and speaks through them.
    ///
    /// Closing the child's stdin is the hang-up signal, exactly as it is for the agent loop,
    /// and the child is reaped in the background whenever it exits. The child is *not* killed
    /// when the transport goes away — a transport is a byte pipe and has no opinion about the
    /// process's lifetime — but whoever started it can end it deliberately through
    /// [`child`](Self::child), and must, so that nothing it started outlives it.
    ///
    /// # Errors
    ///
    /// Returns `remote.unreachable` (E0601) when the command cannot be started.
    pub fn spawn(mut command: Command) -> Result<Self, ErrorValue> {
        let program = command
            .as_std()
            .get_program()
            .to_string_lossy()
            .into_owned();
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            ErrorValue::new(
                ErrorCode::RemoteUnreachable,
                format!("cannot start `{program}`: {error}"),
            )
        })?;
        let stdin = take_pipe(child.stdin.take(), &program)?;
        let stdout = take_pipe(child.stdout.take(), &program)?;
        let (sender, exit) = watch::channel(None);
        // One order at most is ever sent, and it is sent once: "end yourself".
        let (orders, order) = mpsc::channel(1);
        let pid = child.id();
        tokio::spawn(reap(child, sender, order));
        Ok(Self {
            stdin: Some(stdin),
            stdout,
            peer_key: None,
            child: ChildProcess { pid, exit, orders },
        })
    }

    /// A handle on the child, kept by whoever is responsible for its lifetime.
    ///
    /// The handle outlives the transport, which is what lets a caller hand the transport to a
    /// link and still end the process when the link is torn down.
    #[must_use]
    pub fn child(&self) -> ChildProcess {
        self.child.clone()
    }

    /// Declares the key an outer layer authenticated for this peer.
    #[must_use]
    pub fn with_peer_key(mut self, key: HostKey) -> Self {
        self.peer_key = Some(key);
        self
    }

    /// Resolves when the child has exited, with its status where the system reported one.
    ///
    /// The handle outlives the transport, so a caller can hand the transport to a link and
    /// still observe the child's end.
    pub fn exited(&self) -> impl std::future::Future<Output = Option<ExitStatus>> + Send + 'static {
        self.child.exited()
    }
}

/// The child a [`SubprocessTransport`] started, seen from outside the transport.
///
/// A process that was started to serve a link is a resource of that link: the side that spawned
/// it is the side that must see it end, and [`end`](Self::end) is where that happens.
#[derive(Debug, Clone)]
pub struct ChildProcess {
    pid: Option<u32>,
    exit: watch::Receiver<Option<ExitStatus>>,
    orders: mpsc::Sender<Duration>,
}

impl ChildProcess {
    /// The process id, while the system still has one for it.
    #[must_use]
    pub const fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Resolves when the child has exited, with its status where the system reported one.
    pub fn exited(&self) -> impl std::future::Future<Output = Option<ExitStatus>> + Send + 'static {
        let mut receiver = self.exit.clone();
        async move {
            loop {
                if let Some(status) = *receiver.borrow() {
                    return Some(status);
                }
                if receiver.changed().await.is_err() {
                    return *receiver.borrow();
                }
            }
        }
    }

    /// Ends the child and waits for it, giving it `grace` at each step.
    ///
    /// A child that has already noticed the hang-up — the ordinary case, because the link shuts
    /// its stdin down first — is never signalled: this returns as soon as it exits. One that has
    /// not is asked with `SIGTERM` after `grace`, and made to with `SIGKILL` after another
    /// `grace`, so the wait is bounded whatever the far end does.
    pub async fn end(&self, grace: Duration) -> Option<ExitStatus> {
        // A closed channel means the reaper has already finished; then the status is published.
        let _ = self.orders.send(grace).await;
        self.exited().await
    }
}

/// Waits for the child so it is reaped, and publishes how it ended.
///
/// The task owns the [`Child`], so it is also the only place that may signal it: a pid is only
/// safely a pid until it is waited for, and here it cannot be waited for behind the signal's
/// back.
async fn reap(
    mut child: Child,
    sender: watch::Sender<Option<ExitStatus>>,
    mut orders: mpsc::Receiver<Duration>,
) {
    let status = tokio::select! {
        status = child.wait() => status.ok(),
        order = orders.recv() => match order {
            Some(grace) => end_child(&mut child, grace).await,
            // Nobody is left to ask for an end; the child's own is the only one that comes.
            None => child.wait().await.ok(),
        },
    };
    let _ = sender.send(status);
}

/// Waits `grace` for the child to go on its own, then `SIGTERM`, then `SIGKILL`.
async fn end_child(child: &mut Child, grace: Duration) -> Option<ExitStatus> {
    if let Ok(status) = tokio::time::timeout(grace, child.wait()).await {
        return status.ok();
    }
    if let Some(pid) = child.id() {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid.cast_signed()),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
    if let Ok(status) = tokio::time::timeout(grace, child.wait()).await {
        return status.ok();
    }
    let _ = child.start_kill();
    child.wait().await.ok()
}

fn take_pipe<P>(pipe: Option<P>, program: &str) -> Result<P, ErrorValue> {
    pipe.ok_or_else(|| {
        // Unreachable when the pipes were requested above; stated rather than panicked on.
        ErrorValue::new(
            ErrorCode::RemoteUnreachable,
            format!("`{program}` started without the requested pipes"),
        )
    })
}

impl Transport for SubprocessTransport {
    fn peer_key(&self) -> Option<&HostKey> {
        self.peer_key.as_ref()
    }
}

impl AsyncRead for SubprocessTransport {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stdout).poll_read(cx, buf)
    }
}

impl AsyncWrite for SubprocessTransport {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.stdin.as_mut() {
            Some(stdin) => Pin::new(stdin).poll_write(cx, buf),
            None => Poll::Ready(Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.stdin.as_mut() {
            Some(stdin) => Pin::new(stdin).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        match Pin::new(stdin).poll_shutdown(cx) {
            Poll::Ready(result) => {
                // Closing the descriptor is the hang-up signal a pipe understands; see the
                // field's documentation.
                self.stdin = None;
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
