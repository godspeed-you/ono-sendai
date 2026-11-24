# ADR-1097: A peer's link table is a read, and the second hop is a report

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.4 §2.17, §19.3, §19.4, §34, §35.4; v0.4.1 §9.4, §10.1, §10.2, §14.1, §59.1;
  spec §21.2; ADR-0985, ADR-1056, ADR-1095
- Issues: #310
- Decided by: agent (autonomous)

## Context

§19.3's federated map draws `LOCAL/workstation +---- prod/web01 ----- prod/db01`: a link between
two hosts that are both remote. `map links` drew one hop, because nothing in the link protocol
carried a peer's link table — and nothing on the far side *held* links: an `ono --agent` reached
over stdio serves one caller and keeps nothing, and a listening agent made no links of its own.
Issue #310 asks for a protocol request for a peer's link table (bounded, authorized like any read,
refused by an older peer), a second hop drawn with the far side's confidence and provenance, a
depth bound, and a chained test. The workstream brief adds: the table is the peer's information
and is served only under the capability a read of `link` requires; a refusal is "not permitted to
see", never "no links"; unknown remote topology is never drawn as verified; cycles, duplicate
observations, unavailable peers and partial answers are handled; traversal depth, fan-out,
response size and time are bounded; a hostile table is refused safely.

## Decision

### Who holds links on the far side

A **listening agent** holds the links its operator declares: `ono --agent --listen <addr> --link
<name>=<host:port>` (repeatable, at most 64). Each is made over the authenticated transport with
the agent's own identity and trust store — the next hop must be pinned on this host and must
authorize this agent's key — and is made again when it goes down (1 s, doubling to 30 s; a
`held link … connected to` / `went down` line on stderr). `--link` without `--listen` is a usage
error: an agent serving one caller over stdio holds no links of its own. Every `ono --agent` keeps
a link table — the stdio agent's is empty — so the request is announced by every agent of this
build, and "no links" from an agent is true.

### The request — `ono.link.links/1`

`FrameKind::ListLinks` (16) asks; `FrameKind::LinkTable` (17) answers with one table, then the
stream's `end`. ADR-1056's one rule, as for the probe: announced in the offer, sent only when
negotiated, refused locally otherwise, a protocol mismatch when it arrives unannounced.

**Authorization is `get link`'s.** The serving loop asks `require_observe("get link")` — the
check every read of a target is served under, for the target whose contract declares `link.list`
— and the service asks again on its side of the crate boundary (§10.2; both are dispatch paths in
`security_boundaries.yaml`). A denial is audited as `authorization.denied` with
`requested_capability=link.list`, and reaches the client as the agent's structured
`remote.capability_denied`. No parallel authorization exists: a client the operator lets observe
the machine may read its link table; one it does not, may not.

The feature id is announced to **every** authorized connection rather than filtered out of a
non-observing one's offer. §10.1 withholds *provider capabilities* because the inventory is
information; a protocol feature reveals only that this agent understands a request, which the
`Accept`'s agent version already says. Withholding it would make the client report `not_offered`
— a false statement about an agent that does offer it — where the truth is `not_permitted`.

**Bounds.** At most 64 entries (`LinkTable::of` cuts and sets `truncated`); every text field at most
256 bytes. The client refuses a larger table or a longer field at decode, before anything of it
is believed, and — as for any malformed frame — the link fails as a protocol mismatch. The
`remote-protocol` fuzz target asserts the bounds on every table the decoder accepts. The request
is bounded by the caller's timeout and its stream released on every path.

**What an entry is.** The far side's own observation of its own link: name, address, transport,
`connected`/`disconnected` and since when, the fingerprint of the key the next hop proved on that
link, the next hop's boot id, the handshake's round trip, and a latency probed when the table is
asked for (ADR-1095's pass, reused for 5 s). Nothing in an entry is relayed from further away.

### The second hop on the map

`map links --depth 2` asks each connected, followed agent link for its table, side by side, each
within 5 s, at most 32 peers per map (the rest are `not_asked`). Depth 1 — the default, §19.3's
"SHOULD NOT automatically expand" — asks nobody. **Depth 3 is refused** (`type.mismatch`, with the
limit): an agent answers about the links it holds and never asks its own peers on a caller's
behalf. Relaying would let a client read the topology of a host that has never authorized it,
under the relaying agent's identity — the confused deputy the per-connection authorization exists
to prevent — so the depth bound is two by construction, not by configuration.

**Honesty (§19.4).** A second-hop edge is `host.linked_to` with confidence **`inferred`**: this
session observed nothing of it, and a one-sided report "MUST carry the correct confidence". The
far side's own claim travels beside it as `reported_confidence` (`exact` for a link it holds
connected, `user_declared` for one that is down — its declaration and nothing more), with
`reported_by`, `verified: false`, the state, latency and key it reported, and a provenance of
provider `ono.link.links` on the reporting link, observed at the far side's time. The text map
draws it dashed, as every inferred edge.

**Identity: cycles and duplicates.** A far host is identified by the key it proved to the
reporter, never by a name or an address:

- a far link whose key is this shell's own link identity is a cycle and leads back to the centre
  (`back_to_this_host`), never to a second place for this host;
- a far link whose key is the key one of this session's own links proved joins that link's place
  (`same_peer_as`) — one host reached two ways is drawn once;
- two peers reporting the same key draw one far place with two edges;
- a far link with no proved key stays one place per reporter (§2.17 prefers the false
  distinction to a false merge). Boot ids are reported, never used to merge (ADR-0985).

A far host this session holds no link to is named through the peer that reported it
(`web01/db01`), the only route it knows; learning the name stands on nothing (§35.4).

**What each link answered** is the map's new `federation` field (null when nobody was asked):
`listed`, `partial` (the far side left links out), `not_permitted`, `not_offered`, `unavailable`
(link down, request failed or timed out) or `not_asked` (not connected and followed, agentless,
or beyond the fan-out bound), with `count`, `code` and `reason`. The text map prints a `far links`
section — "not permitted to see its links" is printed as that.

## Consequences

- Cost, per `map links`, with N links this session holds and M links a peer holds:
  depth 1 — no table request; at most one probe per connected agent link (ADR-1095, reused 5 s).
  depth 2 — additionally one table request per connected agent link, at most 32 (two frames out
  of 16 bytes each, one table back of at most 64 entries, bounded by the 1 MiB frame); each peer
  probes its M held links at most once per 5 s while it is asked. An idle shell and an idle agent
  send nothing; a held link that is down costs one connection attempt per back-off interval.
  Worst-case time of a map: the 2 s probe pass, then the 5 s table requests, side by side.
- Tests: `crates/ono-protocol/tests/link_table.rs` (served; cut and marked; refused locally by an
  older agent and nothing sent; refused and recorded without observe, and nothing of the table
  in the audit trail; oversized table, long field and malformed payloads refused; an unanswered
  request released; an unannounced request fails the link), `crates/ono-remote/tests/agent.rs::
  should_serve_the_agents_link_table_only_where_it_keeps_one`, and
  `crates/ono-cli/tests/federated_map.rs` over real agents on `127.0.0.N` with homes of their own:
  A→B→C chain, depth 1 asks nobody, depth 3 refused with its reason, not permitted (and the audit
  line), a cycle A↔B, one host reached two ways, one far host reported by two peers, a far link
  that goes down and comes back (B makes it again), a frozen peer bounded beside a live one.
  Acceptance case `457-map-links-draws-a-second-hop-as-a-report.case`.
- `ono.spatial-map/1` gains `federation`; `map links` gains `--depth`; `ono --agent` gains
  `--link`.

## Alternatives considered

- **Relay the request (depth > 2).** The confused deputy above; and a far host's authorization is
  between it and its direct peers.
- **Serve the table as `ono.link/1` records through `get link` on the far side.** `link` is a
  session target that never crosses a link (§14.4, ADR-0269) — the far side's shell sessions are
  not the agent — and a new request is what ADR-1056's rule is for.
- **Publish the far host's shell sessions' links through a runtime file the agent reads.** A
  second cross-process registry with liveness of its own to track, for links the operator never
  declared the host should be seen holding.
- **Draw the second hop at the far side's confidence (`exact`).** It would draw a report as
  verified topology; the far side's claim is kept, beside this session's.
- **Merge far hosts by boot id or address.** Every local peer shares a kernel and `127.0.0.1`; a
  key a peer proved is identity, the rest is context.
