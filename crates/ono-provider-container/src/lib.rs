//! The Ono `container` and `image` provider: the engine API, over the runtime's socket.
//!
//! Spec §23 and §31.57 fix the design: a provider speaks the daemon's API where one exists and
//! never parses the human output of a tool. Docker publishes its Engine API as HTTP over a Unix
//! socket, and Podman serves the same API on its own socket, so one client answers for both.
//! Every field of `ono.container/1` and `ono.image/1` is a field of the JSON the engine returned
//! from `GET /containers/json`, `GET /containers/{id}/json` and `GET /images/json` — and from
//! nowhere else (spec §50, AGENTS.md §6).
//!
//! # Finding the runtime
//!
//! The shell is pointed at a runtime the way `docker` and `podman` themselves are: `DOCKER_HOST`
//! and `CONTAINER_HOST` name a `unix://` socket. Where neither is set, the well-known sockets of
//! both engines are tried — the rootless ones under `XDG_RUNTIME_DIR` first, then the system
//! ones. The first socket that accepts a connection answers (ADR-0112).
//!
//! # Being honest about not being there
//!
//! Most machines have no container runtime, and a machine with one may have it stopped. Where no
//! socket answers, the provider reports
//! [`Availability::Unavailable`](ono_provider_api::Availability::Unavailable) naming every socket
//! it tried, and refuses to answer queries. It does *not* return an empty stream, because an
//! empty stream says "there are no containers", which is a different and false claim
//! (spec §10.5, §35.3).
//!
//! # The client
//!
//! The `http` module is a minimal HTTP/1.1 client over a Unix socket — one request per connection,
//! `Content-Length` and chunked bodies — because the engine API needs nothing more and a full
//! HTTP stack would be a dependency the shell's cold start pays for on every machine.

#![forbid(unsafe_code)]

mod endpoint;
mod http;
mod provider;
mod record;

pub use endpoint::Endpoints;
pub use provider::{ContainerProvider, PROVIDER_ID};
pub use record::{container_schema, image_schema};
