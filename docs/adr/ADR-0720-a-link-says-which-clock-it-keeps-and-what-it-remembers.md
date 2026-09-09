# ADR-0720: A link says which clock it keeps and what it remembers

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §21.1, §24.1, §24.2, §24.3, §24.4, §24.5, §24.6, §25.5, §26.3, §26.4;
  v0.2 §21.2, §21.3; ADR-0015, ADR-0036
- Decided by: agent (autonomous)

## Context

v0.5 §24.1 says a linked host may offer current snapshots only, live events, persisted history, or
no temporal support at all, and that "negotiation MUST report the available capabilities". §24.5
turns the report into an obligation: standing on a remote place and asking about ten minutes ago,
"if the local machine has only local history and the remote has none, it MUST say so." A shell that
cannot tell the four cases apart has only one way to answer that question, and it is to show
current state and let the user assume.

Two things were missing to make the report possible.

The first is the claim itself. `ProviderDescriptor` carried a provider's id, targets, capabilities
and availability, and nothing about time. `ono_provider_api::TemporalCapabilities` had landed on the
local side, so the far side knew the answer and had no field to put it in.

The second is the clock. §24.2 requires a remote event to preserve four things — its `source_time`,
a source clock identity or host identity, the local `ingested_at`, and the clock uncertainty where
it is known — and forbids the local ledger from overwriting the first with the third.
`retag_provenance` already kept `observed` verbatim and rewrote only the link, which is the
prohibition already implemented; what had no home was the arrival instant and the identity of the
clock the far side read.

## Decision

### 1. A provider descriptor carries what its provider claims about time

`ProviderDescriptor` gains one optional field, `temporal`, holding the seven flags of
`TemporalCapabilities`. It is `#[serde(default, skip_serializing_if = "Option::is_none")]`, so a
peer built before v0.5 sends nothing, decodes as nothing, and is read as a provider that claims
nothing — which §21.5 already makes the honest reading of silence.

`Negotiated::temporal()` composes them into `RemoteTemporal`, which is §24.1's own four-case list
ordered from `None` to `PersistedHistory`, and answers the strongest thing any *available* provider
supports. `negotiate()` is untouched: it narrows and refuses nothing but a version mismatch, so an
absent temporal claim degrades the link rather than failing it, which is what §24.5 needs it to do.

`RemoteProvider` reads the claim back through `Provider::temporal`, and its `Provider::history`
refuses a peer that never claimed `historical_query` with `temporal.unsupported_source`, naming the
provider and the host. That refusal *is* §24.5's "it MUST say so", raised at the only place that
knows both what was asked and what the far side can answer.

### 2. A peer states its clock identity, as self-reported context

`PeerClock` carries a clock id, an optional boot id, the peer's own wall reading at the handshake,
and its stated uncertainty. It rides on `Hello` and `Accept` as an optional field and is exposed as
`PeerInfo::clock()`, documented the way `Identity` is: **self-reported context, never authority.**
Nothing is granted because of it, and a peer that says nothing leaves the clock unknown rather than
implying a zero offset (§35.3).

`ono-remote`'s `RemoteIngest` is what the rest of the tree uses it for. `RemoteIngest::times(...)`
produces the three instants of §3.3 for an arriving observation — the source's own time unchanged,
the far side's observation, and a *separate* local `ingested_at` — and `RemoteIngest::domain()`
produces the `ClockDomain` of §25.5 from the peer's host and boot. `clock_identity(host)` builds
this machine's own from `/proc/sys/kernel/random/boot_id`, and `AgentConfig::with_clock` announces
it; a kernel that publishes no boot id leaves the boot unstated, which
`ClockDomain::is_comparable_to` already treats as incomparable to everything.

**No second ordering model is introduced.** `ono_temporal_core::happens_before` answers
`Concurrent` for two events in different clock domains whatever their wall clocks say, and this
crate's whole contribution to ordering is to supply the domain so that the existing answer is
reachable. §24.6's per-host coverage is `RemoteIngest::scope()`, a `SpatialScope::remote_host` per
link, because a scope already nests and a nesting scope is what "per host/cluster/node rather than
one global label" means.

### 3. The transaction identity survives the crossing

`ObjectEvent::cause` — the token a source publishes for a job, a request or a connection — was not
on the wire. §26.4 permits a causal chain to cross a host boundary "only when the evidence chain
actually crosses the boundary", and that token is the evidence. It is now encoded and decoded with
the event, so the one thing that can honestly join two hosts arrives, and proximity, which §26.4
makes correlation at most, is not left as the only thing to join on.

## Consequences

`crates/ono-protocol/tests/handshake_temporal.rs` holds the negotiation report and the clock
identity; `crates/ono-remote/tests/temporal.rs` holds the three instants, the two-host skew fixture
of §52.4, the historical query and its refusal. Both suites run over an in-memory duplex with no
network and no clock, which is what makes an artificial skew deterministic rather than flaky.

`docs/contracts/schemas/link.v1.yaml` does **not** yet carry the clock columns. The record is built
in `crates/ono-cli/src/session_provider.rs`, which this package does not own, and a nullable field
added to the schema without the code that fills it would be a contract claiming something no
provider answers. The protocol side is complete and `PeerInfo::clock()` is what the columns would
read; adding them is one increment in the crate that owns the record.

## Alternatives considered

**Advertising temporal support as capability ids in `ProviderDescriptor.capabilities`** — no wire
change at all, which is why `INTERFACES.md` §5.5 suggested it. Rejected: those ids become
`ono_provider_api::Capability` values on the mounted provider and are shown by `get provider`, so
six pseudo-capabilities would appear beside the real ones, and `retained_history` has a duration to
carry that a bare id cannot. The optional typed field has the same compatibility story — absent
means what it always meant — without putting a claim about time into the list of things a provider
may be permitted to do.

**Deriving the clock domain from the link name alone.** Rejected: two links to the same host under
two names would be two domains, and one host rebooted between two sessions would be one. The boot
id is what §25.5 makes the separator, and a peer that will not state it is honestly incomparable.
