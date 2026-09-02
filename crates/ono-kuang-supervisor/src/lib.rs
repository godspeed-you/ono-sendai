//! The KUANG/11 supervisor: the host side of the extension boundary (spec §31.11).
//!
//! The supervisor is the component that makes "I installed a plugin" different from "I executed
//! a stranger's code" (spec §31.8). It spawns a package's runtime artifact as a subprocess,
//! drives the handshake against the manifest it validated *before* spawning anything — manifest
//! before code, spec §31.89 rule 1 — negotiates the contract of spec §31.63, and then brokers
//! every host call the instance makes:
//!
//! - a call without a granted capability is refused with the structured denial of
//!   `docs/spec/kuang/errors.v1.yaml`, and the denial is **audited** as loudly as a success
//!   (spec §31.37);
//! - a call outside a granted scope is `capability.scope_violation`, carrying the attempted
//!   value and the granted scope in its metadata;
//! - values cross as typed values, restamped with the package's provenance by the host — a
//!   plugin cannot forge where its data came from (spec §31.80);
//! - flow is pull-based with explicit credit; an emission beyond credit is a protocol
//!   violation, not a queue (spec §31.15, ADR-0022 §8);
//! - a malformed or oversized frame quarantines the instance: the shell keeps running and the
//!   package becomes inert with the reason recorded (spec §31.34, ADR-0041).
//!
//! What this crate deliberately does not do: registry wiring. [`LoadedPlugin::commands`] and
//! [`LoadedPlugin::targets`] expose contract-shaped contribution tables; entering them into the
//! real `CommandRegistry`/`TargetRegistry` with origin `plugin(...)` (spec §31.64) is the
//! shell's integration step.

mod adapters;
mod negotiate;
mod policy;
mod sandbox;
mod state;
mod supervisor;
mod trail;

pub use adapters::{AdapterPackageError, declared_executables, validate_package};
pub use negotiate::{HostLimits, negotiate};
pub use policy::{DenialSource, Evaluation, Grant, Policy, ScopeUse, denial_error};
pub use sandbox::{
    Confinement, Sandbox, allocated_bytes, apply, cpu_nanoseconds, native_process, nice_of,
    working_directory,
};
pub use state::StateStore;
pub use supervisor::{
    LoadConfig, LoadedPlugin, RegisteredCommand, RegisteredTarget, RunningInvocation, StreamEvent,
    Supervisor, host_platform,
};
pub use trail::{AuditTrail, HostClock};
