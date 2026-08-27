//! A minimal HTTP/1.1 client over a Unix socket: one request, one response, one connection.
//!
//! The engine API is plain HTTP with JSON bodies, and this is the whole of what the provider
//! needs from HTTP: a request line, a few headers, a body that is either `Content-Length`-sized
//! or chunked. Anything the engine sends that does not fit is a protocol error naming what was
//! seen, never something guessed at.

use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// The engine's answer to one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
}

impl Response {
    /// The body as JSON, or `None` when it is empty or not JSON.
    pub(crate) fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.body).ok()
    }

    /// The `message` the engine attaches to an error body, or the status alone.
    pub(crate) fn message(&self) -> String {
        self.json()
            .and_then(|body| {
                body.get("message")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("the engine answered HTTP {}", self.status))
    }
}

/// Why a request produced no response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpError {
    /// The socket could not be reached, or the connection was lost.
    Unreachable(String),
    /// The engine answered with something that is not HTTP/1.1.
    Protocol(String),
    /// The engine did not answer within the budget.
    TimedOut(Duration),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Unreachable(why) => write!(f, "the runtime socket could not be used: {why}"),
            HttpError::Protocol(why) => write!(f, "the engine did not speak HTTP/1.1: {why}"),
            HttpError::TimedOut(budget) => {
                write!(f, "the engine did not answer within {}s", budget.as_secs())
            }
        }
    }
}

/// Sends one request and reads the whole response.
///
/// `path` is the request target including its query string. A JSON body, when given, is sent
/// as `application/json`. The connection is closed afterwards: the engine API is cheap to
/// reconnect to and a persistent connection would be state to get wrong.
pub(crate) async fn request(
    socket: &Path,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    budget: Duration,
) -> Result<Response, HttpError> {
    tokio::time::timeout(budget, exchange(socket, method, path, body))
        .await
        .unwrap_or(Err(HttpError::TimedOut(budget)))
}

async fn exchange(
    socket: &Path,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<Response, HttpError> {
    let mut stream = UnixStream::connect(socket)
        .await
        .map_err(|error| HttpError::Unreachable(error.to_string()))?;

    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    match body {
        Some(body) => head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )),
        None => head.push_str("Content-Length: 0\r\n\r\n"),
    }
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|error| HttpError::Unreachable(error.to_string()))?;
    if let Some(body) = body {
        stream
            .write_all(body)
            .await
            .map_err(|error| HttpError::Unreachable(error.to_string()))?;
    }
    stream
        .flush()
        .await
        .map_err(|error| HttpError::Unreachable(error.to_string()))?;

    let mut reader = Reader {
        stream,
        buffer: Vec::new(),
    };
    let head = reader.read_head().await?;
    let (status, headers) = parse_head(&head)?;
    let body = if headers
        .iter()
        .any(|(name, value)| name == "transfer-encoding" && value.contains("chunked"))
    {
        reader.read_chunked().await?
    } else if let Some(length) = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
    {
        reader.read_exact(length).await?
    } else if status == 204 || status == 304 || method == "HEAD" {
        Vec::new()
    } else {
        reader.read_to_end().await?
    };
    Ok(Response { status, body })
}

/// A response's status and its headers, names lower-cased.
fn parse_head(head: &[u8]) -> Result<(u16, Vec<(String, String)>), HttpError> {
    let text = std::str::from_utf8(head)
        .map_err(|_| HttpError::Protocol("the response head is not text".to_owned()))?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err(HttpError::Protocol(format!(
            "the status line was {status_line:?}"
        )));
    }
    let status = parts
        .next()
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| HttpError::Protocol(format!("the status line was {status_line:?}")))?;
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    Ok((status, headers))
}

/// Reads a response piecewise from the socket, keeping what has arrived and not yet been used.
struct Reader {
    stream: UnixStream,
    buffer: Vec<u8>,
}

/// More than this much head is not an engine API response.
const MAX_HEAD: usize = 64 * 1024;

impl Reader {
    /// Reads more bytes into the buffer; `false` at end of stream.
    async fn fill(&mut self) -> Result<bool, HttpError> {
        let mut chunk = [0u8; 16 * 1024];
        match self.stream.read(&mut chunk).await {
            Ok(0) => Ok(false),
            Ok(count) => {
                self.buffer.extend_from_slice(&chunk[..count]);
                Ok(true)
            }
            Err(error) => Err(HttpError::Unreachable(error.to_string())),
        }
    }

    /// The head, up to and excluding the blank line that ends it.
    async fn read_head(&mut self) -> Result<Vec<u8>, HttpError> {
        loop {
            if let Some(end) = find(&self.buffer, b"\r\n\r\n") {
                let head = self.buffer[..end].to_vec();
                self.buffer.drain(..end + 4);
                return Ok(head);
            }
            if self.buffer.len() > MAX_HEAD {
                return Err(HttpError::Protocol(
                    "the response head did not end within 64 KiB".to_owned(),
                ));
            }
            if !self.fill().await? {
                return Err(HttpError::Protocol(
                    "the connection closed before a response head arrived".to_owned(),
                ));
            }
        }
    }

    async fn read_exact(&mut self, length: usize) -> Result<Vec<u8>, HttpError> {
        while self.buffer.len() < length {
            if !self.fill().await? {
                return Err(HttpError::Protocol(format!(
                    "the body ended after {} of {length} bytes",
                    self.buffer.len()
                )));
            }
        }
        let body = self.buffer[..length].to_vec();
        self.buffer.drain(..length);
        Ok(body)
    }

    async fn read_to_end(&mut self) -> Result<Vec<u8>, HttpError> {
        while self.fill().await? {}
        Ok(std::mem::take(&mut self.buffer))
    }

    /// One line, without its CRLF.
    async fn read_line(&mut self) -> Result<String, HttpError> {
        loop {
            if let Some(end) = find(&self.buffer, b"\r\n") {
                let line = String::from_utf8_lossy(&self.buffer[..end]).into_owned();
                self.buffer.drain(..end + 2);
                return Ok(line);
            }
            if !self.fill().await? {
                return Err(HttpError::Protocol(
                    "the connection closed inside a chunked body".to_owned(),
                ));
            }
        }
    }

    /// A `Transfer-Encoding: chunked` body, reassembled.
    async fn read_chunked(&mut self) -> Result<Vec<u8>, HttpError> {
        let mut body = Vec::new();
        loop {
            let line = self.read_line().await?;
            let size_text = line.split(';').next().unwrap_or_default().trim();
            let size = usize::from_str_radix(size_text, 16)
                .map_err(|_| HttpError::Protocol(format!("the chunk size was {size_text:?}")))?;
            if size == 0 {
                // Trailers, if any, up to the blank line.
                while !self.read_line().await?.is_empty() {}
                return Ok(body);
            }
            let chunk = self.read_exact(size).await?;
            body.extend_from_slice(&chunk);
            let separator = self.read_line().await?;
            if !separator.is_empty() {
                return Err(HttpError::Protocol(
                    "a chunk was not followed by CRLF".to_owned(),
                ));
            }
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Serves `response` verbatim to the first connection on a fresh socket.
    fn serve_once(response: &'static [u8]) -> std::path::PathBuf {
        let directory = tempfile::tempdir().unwrap().keep();
        let socket = directory.join("engine.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = vec![0u8; 4096];
            let _ = stream.read(&mut request);
            stream.write_all(response).unwrap();
        });
        socket
    }

    #[tokio::test]
    async fn should_reassemble_a_chunked_body() {
        let socket = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n[{\"\r\n6\r\nId\":1}\r\n1\r\n]\r\n0\r\n\r\n",
        );
        let response = request(
            &socket,
            "GET",
            "/containers/json",
            None,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"[{\"Id\":1}]");
    }

    #[tokio::test]
    async fn should_read_a_content_length_body_and_the_engine_message() {
        let socket = serve_once(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 34\r\n\r\n{\"message\":\"No such container: x\"}",
        );
        let response = request(
            &socket,
            "GET",
            "/containers/x/json",
            None,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(response.status, 404);
        assert_eq!(response.message(), "No such container: x");
    }

    #[tokio::test]
    async fn should_refuse_a_response_that_is_not_http() {
        let socket = serve_once(b"garbage\r\n\r\n");
        let error = request(&socket, "GET", "/_ping", None, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(error, HttpError::Protocol(_)), "{error:?}");
    }

    #[tokio::test]
    async fn should_report_a_socket_nothing_listens_on() {
        let directory = tempfile::tempdir().unwrap();
        let error = request(
            &directory.path().join("none.sock"),
            "GET",
            "/_ping",
            None,
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, HttpError::Unreachable(_)), "{error:?}");
    }
}
