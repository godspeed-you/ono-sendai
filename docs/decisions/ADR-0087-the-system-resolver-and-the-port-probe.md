# ADR-0087: `resolve dns` and `test port` — the system resolver and the port probe

- Status: accepted
- Date: 2026-08-27
- Spec refs: §9.1 (the network table), §10.5, §16.5, §23.2, §34, §35.3, §41.2, §43, §50;
  ADR-0012, ADR-0028, ADR-0068
- Decided by: agent (autonomous)

## Context

`docs/spec/commands/network.yaml` declares `ono.dns.resolve` (`resolve dns <query>`, output
`stream<ono.dns-record/1>`) and `ono.port.test` (`test port <host> <port>`, output
`ono.probe-result/1`) as stable Phase C commands, and `docs/spec/schemas/deferred.yaml` carried
both schemas as promises. Nothing implemented either: the verbs `resolve` and `test` were not
bound to any implementation, no provider served the targets `dns` and `port`, and the shell
answered `E0101 … declared but this build implements nothing for it`.

Spec §9.1 says what the two commands do — "perform explicit DNS resolution", "probe reachability
with timing and error detail" — and no more. Three things had to be decided: what a DnsRecord
and a ProbeResult are, where the answers come from, and how a lookup that fails is reported.

## Decision

### 1. The C library's resolver is the resolver

`resolve dns` asks `getaddrinfo(3)` for a name and `getnameinfo(3)` for an address. Those two
calls *are* the system resolver: they answer through NSS, so `/etc/hosts`, DNS, mDNS and LDAP
all take part exactly as they do for `ssh` or `curl`, and `nsswitch.conf` is honoured without
the shell knowing it exists. Nothing runs `dig`, `host` or `nslookup`, and nothing reads what
they print (spec §50, AGENTS.md §6).

A query that parses as an address performs a reverse lookup (`network.yaml`, selector `query`)
and answers with one `PTR` record naming the host; any other query answers with the `A` and
`AAAA` records the resolver returns, without duplicates. `--type` keeps one record type (`A`,
`AAAA`, `PTR`; anything else is `type.mismatch`). `--server` is refused with
`provider.unsupported`: the system resolver answers from `resolv.conf`, and asking a particular
nameserver needs a DNS client this build does not include. Refusing it is more honest than
silently asking the wrong server.

`ono.dns-record/1` (`docs/spec/schemas/dns-record.v1.yaml`) has the three fields the resolver
can actually fill — `name`, `type`, `address` — and no TTL, because `getaddrinfo` does not
expose one and a field that is always null would be a promise nobody keeps (spec §10.5).

### 2. The resolver's error code decides the taxonomy code

`std` wraps `getaddrinfo` but not `getnameinfo`, and it folds the resolver's `EAI_*` codes into
prose. The code is what tells the two failures spec §43 needs kept apart:

- `EAI_NONAME`, `EAI_NODATA`, `EAI_SERVICE` — the resolver knows no such name → `io.not_found`
  (E0301);
- everything else (`EAI_AGAIN`, `EAI_FAIL`, `EAI_SYSTEM`, …) — the resolver could not be asked
  → `provider.unavailable` (E0401), `retryable: true`, because the name may still exist.

So the two calls are made directly through `libc`, in one module of a new crate
`ono-provider-net`, behind safe functions. The crate allows `unsafe_code` with that reason;
every `unsafe` block carries a `SAFETY:` comment, and no raw pointer or `unsafe` API crosses the
crate boundary. `ono-provider-linux` and `ono-provider-netlink` keep `#![forbid(unsafe_code)]`
— the resolver is the one thing on this side of the kernel that cannot be reached safely, and
it gets its own crate rather than loosening two that do not need it. Spec §24.2 does not list
the crate; ADR-0001 creates crates when a phase needs them.

A lookup runs on a blocking thread and is bounded to 15 s (`resolv.conf`'s own
`timeout × attempts × servers` can exceed what spec §34 lets an interactive shell wait);
past the bound the stream fails with `provider.unavailable`, retryable.

### 3. A probe's finding is data; only a probe that could not be made is an error

`test port <host> <port>` connects with `--timeout` (default 5 s) over `--protocol` (`tcp` by
default; `udp` sends an empty datagram and listens for the ICMP refusal) and answers one
`ono.probe-result/1`:

- `host`, `port`, `protocol` — what was probed, as given;
- `reachable` — `true` when the connection completed, `false` when the peer or the network
  refused it, **`null` when nothing answered before the timeout** (spec §10.5: silence is not a
  refusal);
- `duration` — how long the attempt took, success or not;
- `error` — the operating system's reason, as text, when `reachable` is not `true`; null
  otherwise.

A refused or silent port is the probe *working* — the answer the user asked for — so the run
succeeds and the row says why. Only a probe that could not be attempted at all (a host name the
resolver cannot map, a socket the kernel would not open) fails the stream with a structured
error. `error` is a string, not an `ono.error/1`: a refused connection is the peer's answer, not
a failure of the shell, and a taxonomy code for it would be invented.

### 4. `resolve` and `test` bind like `get`

`impls/mod.rs` binds the verbs `resolve` and `test` to the generic `ProviderProducer` exactly as
it binds `get` and `find`: the contract's selectors become the provider query, the provider's
stream is the stage's output. A new target of either verb is therefore a registry entry and a
provider, and no new command code.

## Consequences

- `resolve dns localhost` works offline on every machine (`/etc/hosts`); `resolve dns
  127.0.0.1` names `localhost`; `resolve dns definitely.invalid.` fails with E0301 where a
  resolver is reachable and E0401 where none is, within the bound, never with a hang.
- `test port 127.0.0.1 <listening>` → `reachable: true`; a closed loopback port →
  `reachable: false`, `error: "connection refused …"`, exit 0.
- Tests: `crates/ono-cli/tests/network_missing.rs` (`resolve dns` ×4, `test port` ×3),
  `docker/acceptance/cases/039-network-dns-port-mutations.case`.
- `docs/spec/providers/linux-resolver.yaml` declares the two providers (`linux.resolver`,
  `linux.probe`); `crates/ono-cli/tests/providers.rs` keeps the declaration and the registry in
  step.
- A future `--server` needs a DNS client (a UDP/TCP query builder and parser); it is a separate
  increment, and the refusal names that.

## Alternatives considered

- **Parse `/etc/hosts` and `resolv.conf` and speak DNS directly.** Rejected: it would answer
  differently from every other program on the machine (no mDNS, no LDAP, no `nsswitch.conf`
  ordering), and re-implement what the C library already does correctly.
- **`std::net::ToSocketAddrs` for the forward lookup and `/etc/hosts` for the reverse.**
  Rejected: the reverse half bypasses NSS, and `std` hides the error code that separates
  "no such name" from "no resolver".
- **Loosen `forbid(unsafe_code)` in `ono-provider-linux`.** Rejected in favour of a crate whose
  reason for existing is the one unsafe seam.
- **`error` on a probe result as `ono.error/1`.** Rejected: a refusal is the peer's answer, and
  every taxonomy code would misdescribe it.
