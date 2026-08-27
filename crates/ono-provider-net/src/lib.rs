//! Network providers that ask beyond the local kernel (spec §9.1, §23.2, §41.2).
//!
//! Two providers live here, and what they have in common is that neither answers from `/proc`
//! or from netlink: [`DnsProvider`] asks the C library's resolver, and [`PortProvider`] asks the
//! network itself. Nothing in this crate runs `dig`, `host`, `nslookup` or `nc`, or reads
//! anything they print — spec §50 forbids parsing another program's output, and the resolver
//! every other program uses is a function call away.
//!
//! # Why the C library, and why `unsafe`
//!
//! `getaddrinfo(3)` and `getnameinfo(3)` *are* the system resolver: they answer through NSS, so
//! `/etc/hosts`, DNS, mDNS and LDAP all take part exactly as they do for `ssh` or `curl`, and a
//! machine's `nsswitch.conf` is honoured without this crate knowing it exists. `std` wraps the
//! first but not the second, and folds the resolver's error codes into prose — and the code is
//! what tells "no such name" (`io.not_found`) apart from "no resolver reachable"
//! (`provider.unavailable`, retryable). So the two calls are made directly, in one module,
//! behind safe functions; no `unsafe` API and no raw pointer crosses this crate's boundary
//! (ADR-0087).
//!

//! # What a null means here
//!
//! A probe that timed out has `reachable: null`, not `false`: nothing answered, which is not the
//! same as something refusing. A probe the peer refused has `reachable: false` and the reason in
//! `error`. Unknown is null, never a fabricated answer (spec §10.5, §35.3).

#![deny(unsafe_op_in_unsafe_fn)]
#![allow(
    unsafe_code,
    reason = "ADR-0087: the system resolver is only reachable through getaddrinfo(3) and getnameinfo(3)"
)]

mod dns;
mod probe;
mod resolver;
mod schema;

pub use dns::DnsProvider;
pub use probe::PortProvider;
pub use resolver::{RecordType, lookup_error};

/// The id every record answered by the C library's resolver carries in its provenance.
pub const RESOLVER_PROVIDER: &str = "linux.resolver";

/// The id every probe result carries in its provenance: the shell itself made the attempt.
pub const PROBE_PROVIDER: &str = "ono.probe";
