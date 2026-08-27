//! Where the runtime's socket is, and whether anything answers on it.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// One place a runtime socket might be, and why it was looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    socket: PathBuf,
    /// `DOCKER_HOST`, `CONTAINER_HOST`, or `default`.
    origins: Vec<String>,
}

/// The sockets the provider tries, in order.
///
/// `DOCKER_HOST` and `CONTAINER_HOST` are the knobs Docker and Podman honour, so they are the
/// knobs the shell honours; a `unix://` URL names a socket. When either is set, only what it
/// names is tried — exactly as `docker` does not fall back to `/var/run/docker.sock` when
/// `DOCKER_HOST` points elsewhere — so a configured runtime that is down is reported as down,
/// never quietly replaced by another one. A URL of another scheme — `tcp://`, `ssh://` — is
/// noted so the refusal can say it was seen and not spoken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    candidates: Vec<Candidate>,
    unsupported: Vec<(String, String)>,
}

impl Endpoints {
    /// The sockets named by `environment`, followed by the well-known ones.
    #[must_use]
    pub fn from_environment<'a>(environment: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut docker_host = None;
        let mut container_host = None;
        let mut runtime_dir = None;
        for (name, value) in environment {
            match name {
                "DOCKER_HOST" => docker_host = Some(value.to_owned()),
                "CONTAINER_HOST" => container_host = Some(value.to_owned()),
                "XDG_RUNTIME_DIR" => runtime_dir = Some(value.to_owned()),
                _ => {}
            }
        }

        let mut endpoints = Self {
            candidates: Vec::new(),
            unsupported: Vec::new(),
        };
        for (origin, url) in [
            ("DOCKER_HOST", docker_host),
            ("CONTAINER_HOST", container_host),
        ] {
            let Some(url) = url.filter(|url| !url.is_empty()) else {
                continue;
            };
            match unix_socket(&url) {
                Some(socket) => endpoints.push(socket, origin),
                None => endpoints.unsupported.push((origin.to_owned(), url)),
            }
        }
        if endpoints.candidates.is_empty() && endpoints.unsupported.is_empty() {
            if let Some(runtime_dir) = runtime_dir.filter(|dir| !dir.is_empty()) {
                let runtime_dir = Path::new(&runtime_dir);
                endpoints.push(runtime_dir.join("docker.sock"), "default");
                endpoints.push(runtime_dir.join("podman/podman.sock"), "default");
            }
            endpoints.push(PathBuf::from("/var/run/docker.sock"), "default");
            endpoints.push(PathBuf::from("/run/podman/podman.sock"), "default");
        }
        endpoints
    }

    fn push(&mut self, socket: PathBuf, origin: &str) {
        if let Some(existing) = self
            .candidates
            .iter_mut()
            .find(|candidate| candidate.socket == socket)
        {
            existing.origins.push(origin.to_owned());
        } else {
            self.candidates.push(Candidate {
                socket,
                origins: vec![origin.to_owned()],
            });
        }
    }

    /// The first socket that accepts a connection, or the reason none did.
    ///
    /// The probe is a plain connect: it is what the engine's own clients do first, and it fails
    /// at once for a socket that is absent or that nobody listens on. The reason names every
    /// socket tried and where each came from, so a user can see that their `DOCKER_HOST` was
    /// honoured and simply had nothing behind it.
    ///
    /// # Errors
    ///
    /// The reason, as the sentence `provider.unavailable` carries.
    pub fn probe(&self) -> Result<PathBuf, String> {
        for candidate in &self.candidates {
            if UnixStream::connect(&candidate.socket).is_ok() {
                return Ok(candidate.socket.clone());
            }
        }
        Err(self.reason())
    }

    /// The sentence saying what was tried and where each socket came from.
    fn reason(&self) -> String {
        let tried: Vec<String> = self
            .candidates
            .iter()
            .map(|candidate| {
                let origins: Vec<&str> = candidate
                    .origins
                    .iter()
                    .filter(|origin| origin != &"default")
                    .map(String::as_str)
                    .collect();
                if origins.is_empty() {
                    candidate.socket.display().to_string()
                } else {
                    format!("{} ({})", candidate.socket.display(), origins.join(", "))
                }
            })
            .collect();
        let mut reason = format!("no container runtime answers on {}", tried.join(", "));
        for (origin, url) in &self.unsupported {
            reason.push_str(&format!(
                "; {origin}={url} names a transport this provider does not speak (only unix://)"
            ));
        }
        reason
    }
}

/// The socket path a `unix://` URL names, or `None` for any other scheme.
fn unix_socket(url: &str) -> Option<PathBuf> {
    let path = url
        .strip_prefix("unix://")
        .or_else(|| url.strip_prefix("unix:"))?;
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
mod tests {
    use super::*;

    #[test]
    fn should_try_only_the_configured_socket_when_one_is_named() {
        let endpoints = Endpoints::from_environment([("DOCKER_HOST", "unix:///tmp/x.sock")]);
        assert_eq!(endpoints.candidates.len(), 1);
        assert_eq!(endpoints.candidates[0].socket, PathBuf::from("/tmp/x.sock"));
        assert_eq!(endpoints.candidates[0].origins, ["DOCKER_HOST"]);
    }

    #[test]
    fn should_try_the_well_known_sockets_when_nothing_is_configured() {
        let endpoints = Endpoints::from_environment([("XDG_RUNTIME_DIR", "/run/user/1000")]);
        let sockets: Vec<PathBuf> = endpoints
            .candidates
            .iter()
            .map(|candidate| candidate.socket.clone())
            .collect();
        assert_eq!(
            sockets,
            [
                PathBuf::from("/run/user/1000/docker.sock"),
                PathBuf::from("/run/user/1000/podman/podman.sock"),
                PathBuf::from("/var/run/docker.sock"),
                PathBuf::from("/run/podman/podman.sock"),
            ]
        );
    }

    #[test]
    fn should_name_every_socket_tried_when_none_answers() {
        let directory = tempfile::tempdir().unwrap();
        let nowhere = directory.path().join("nowhere.sock");
        let url = format!("unix://{}", nowhere.display());
        let endpoints = Endpoints::from_environment([
            ("DOCKER_HOST", url.as_str()),
            ("CONTAINER_HOST", url.as_str()),
        ]);
        let reason = endpoints.probe().unwrap_err();
        assert!(reason.contains("nowhere.sock"), "{reason}");
        assert!(reason.contains("DOCKER_HOST, CONTAINER_HOST"), "{reason}");
    }

    #[test]
    fn should_report_a_transport_it_cannot_speak_rather_than_ignore_it() {
        let endpoints = Endpoints::from_environment([("DOCKER_HOST", "tcp://10.0.0.1:2375")]);
        assert!(
            endpoints.candidates.is_empty(),
            "no default is tried behind a configured host"
        );
        let reason = endpoints.reason();
        assert!(reason.contains("tcp://10.0.0.1:2375"), "{reason}");
        assert!(reason.contains("only unix://"), "{reason}");
    }
}
