//! The remote-link machinery of spec §21, below the CLI (spec §37 Phase H).
//!
//! `ono-protocol` speaks the wire: frames, handshake, credit, trust. This crate is what stands
//! on either side of that wire and makes spec §21's promise real — that a linked machine is
//! reached through the same commands, the same records and the same registry as the local one:
//!
//! - **The agent** ([`AgentConfig`], [`serve_registry`], [`agent_main`]): the remote end of
//!   spec §21.4, answering the protocol out of a real
//!   [`ProviderRegistry`](ono_provider_api::ProviderRegistry) — provider negotiation, snapshot
//!   queries as multiplexed value streams, subscriptions, actions, clean shutdown.
//!   [`agent_main`] is the loop `ono --agent` runs over stdin/stdout.
//! - **The client** ([`RemoteLink`], [`RemoteProvider`]): the local end, mounting each
//!   negotiated remote target as an ordinary
//!   [`Provider`](ono_provider_api::Provider) whose records arrive re-tagged with the host
//!   they came from (spec §25.2), so `get process` against a linked machine works unchanged.
//! - **The transports** ([`StdioTransport`], [`SubprocessTransport`], [`ssh_command`]): the
//!   byte pipes of spec §21.4's picture. The SSH fallback is a subprocess transport whose
//!   command happens to be `ssh <host> ono --agent`; only [`ssh_command`] knows that spelling,
//!   so every test drives the identical transport over a local child instead.
//!
//! Security stays where `ono-protocol` put it: connecting applies the pinned-key trust model,
//! a changed host key is `remote.host_key_changed` (E0603) with no way past it, and a transport
//! that authenticated nobody is accepted only under a policy that says so by name
//! (spec §21.5, ADR-0015 T5/T6).

#![forbid(unsafe_code)]

mod agent;
mod agentless;
mod client;
mod retag;
mod transport;

pub use agent::{AgentConfig, agent_main, serve_registry};
pub use agentless::{
    AGENTLESS_PROVIDER, AgentlessLink, AgentlessProvider, FarSide, LocalFarSide, SshFarSide,
};
pub use client::{RemoteLink, RemoteProvider};
pub use retag::retag_value;
pub use transport::{ChildProcess, SshTarget, StdioTransport, SubprocessTransport, ssh_command};
