# ADR-1095: A link is probed on demand, and a latency is a measurement with a time

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.4 §19.1, §34, §35.3; v0.4.1 §10.1, §10.2, §12.4; spec §21.2; ADR-0985, ADR-1056
- Issues: #308
- Decided by: agent (autonomous)

## Context

§19.1's link map shows a latency (`prod/web01 connected 12ms`). ADR-0985 gave `ono.link-place/1`
the handshake's `round_trip`: measured once, when the link was made, and aging from then on. A
latency measured *while the link is open* needs a request/response pair the agent answers without
touching a provider, and the link protocol had none. Issue #308 asks for a probe frame (versioned,
refused cleanly by an older peer), a bounded cadence or an on-demand probe when the map is drawn,
a field for the latest measured latency and its time, and a test over the local transport — and
it forbids presenting the historical round trip as a current latency, carrying a number forward,
and probe storms.

## Decision

**The frame.** `FrameKind::Probe` takes the next free number, 15. The client opens a fresh stream
id with an empty (`null`) payload; the agent answers with that stream's `End`, from its own
dispatch loop, at once: no task is spawned, no provider is asked, no credit is spent. The round
trip is the time between writing the probe and reading the `End`.

**ADR-1056's one rule, applied as written.** The agent announces `ono.link.probe/1` in its offer
(`ono_remote::AgentConfig` puts every id of `ono_protocol::FEATURES` into
`ServerConfig::with_capabilities`). Under an authorization policy (§10.1's intersection) a
protocol feature survives the filter for every authorized connection: it names a request this
agent understands, not a permission over the machine, and what a request may *read* is decided
when it arrives (§10.2). The probe reads nothing. The client sends a probe only when
`Negotiated::capabilities()` holds the id and otherwise answers its caller locally with
`remote.protocol_mismatch` naming the capability (`metadata.capability`); nothing crosses the
wire. An agent that receives a probe it did not announce fails the link as a protocol mismatch,
exactly as an older agent fails on a frame kind it does not know — never success.

**Bounds on the client.** `Link::probe(timeout)` (and `Prober::probe`, a handle that holds the link
only weakly, so a cached handle never keeps a link open) releases the probe's stream whatever
happens: answered, refused, or not answered within `timeout` — then it is cancelled and reported
as `remote.unreachable`, retryable, with `timeout_ms`. A late answer meets a stream that is gone
and is dropped (the existing crossing-with-cancel rule). So probes cannot accumulate streams or
tasks: a probe is one stream for at most its timeout, and each stream counts against
`max_streams`.

**When the shell probes — on demand, never in the background.** An idle shell with links sends
nothing it was not asked to. The shell probes when it draws something that shows a latency: the
local root's link map (`look` at the root), `map links`, and each poll of `watch host`
(ADR-1096). One pass probes every connected and followed agent link concurrently, each bounded by
`PROBE_TIMEOUT` (2 s), so a pass costs at most 2 s whatever the number of links. Passes are
serialized process-wide, and a measurement younger than `PROBE_REUSE` (5 s) is reused rather than
repeated, so drawing many maps — or a watch polling every second beside a map — sends at most one
probe per link per 5 s: no probe storm. A link made again after it went down is another link (the
handle compares by link identity), so nothing measured on the old connection is reused for the
new one.

**What the map says.** `ono.link-place/1` gains three fields beside `round_trip`, which keeps its
meaning (the handshake, measured once — never relabelled):

- `latency` — the latest probe's round trip, or null;
- `latency_measured_at` — when that probe was made (also set when it failed, so a reader knows
  when the link was last asked), or null when it never was;
- `latency_status` — `measured`, `timed_out` (asked, not answered within the bound),
  `not_offered` (an agent that does not announce the probe — an older one, or the agentless
  fallback, which has no agent to ask), `unavailable` (the link is not connected and followed,
  or the probe failed).

A failed or timed-out probe makes `latency` null; the previous number is never carried forward as
current. `look`'s text form prints the latency where measured, otherwise the handshake round trip
labelled `handshake`.

## Consequences

- Cost, measured by the suites below: one probe is two frames of twelve-byte header and a
  four-byte payload (`null`) out, twelve bytes back. Per link, at most one probe per 5 s while a
  view that shows latency is being drawn, and **zero** while nothing is drawn: nothing in the
  shell schedules a probe; `measure` runs only inside `look` at a root, `map links` and a host
  listing (ADR-1096). `crates/ono-cli/tests/link_latency.rs::
  should_reuse_one_probe_for_every_view_drawn_within_the_reuse_window` shows three views drawn
  back to back carrying one measurement with one `latency_measured_at`; the protocol suite
  shows that a probe nobody negotiated puts nothing on the wire.
- Tests: `crates/ono-protocol/tests/probe.rs` (answered without a provider; refused locally when
  not announced and when not negotiated — nothing on the wire; a probe that times out is released
  and the link is unharmed; a link that is gone; an agent that receives a probe it did not offer
  fails the link as a mismatch; a handle that does not keep the link open),
  `crates/ono-remote/tests/agent.rs::should_answer_a_probe_without_asking_any_provider`, and the
  shell suites named in the commit that wires the map.
- `PROTOCOL_VERSION` stays 1 and the ALPN token stays `ono/2`: nothing an older peer must
  understand changed meaning; the new kind is only ever sent to a peer that announced it.

## Alternatives considered

- **A background cadence (every N seconds per link).** Rejected: an idle shell would talk to every
  linked host for as long as it is open, an invisible cost §34 and §18.2 forbid, to refresh a
  number nobody is looking at.
- **Answering the probe with a dedicated `Pong` frame kind.** A second new kind for something the
  existing `End` already says; the stream machinery routes it, cancels it and bounds it for free.
- **Timing ordinary queries.** ADR-0985 already rejected it: the figure would depend on what was
  asked, not on the link.
- **Bumping `PROTOCOL_VERSION`.** ADR-1056 reserves that for a change of meaning; a new request
  behind an announced id is its case 2.
