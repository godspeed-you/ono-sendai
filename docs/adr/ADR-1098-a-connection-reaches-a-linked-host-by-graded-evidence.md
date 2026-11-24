# ADR-1098: A connection reaches a linked host by graded evidence, and ambiguity is said

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.4 §2.17, §14.5, §19.4, §34, §35.4, §43.3; v0.4.1 §10.2; ADR-0985, ADR-1097
- Issues: #309
- Decided by: agent (autonomous)

## Context

§14.5: "If a connection's remote endpoint is confidently mapped to a host available through an
Ono remote link, the map MAY expose" an edge to it; "It MUST NOT infer remote identity from IP
coincidence alone when ambiguity exists." §19.4: "Cross-host edges MUST be based on explicit
remote evidence or strong multi-sided correlation. One-sided observations MAY be displayed but
MUST carry the correct confidence." Nothing mapped a remote endpoint to a linked host (#309).
ADR-0985's `same_host_as` recognises two links to one machine by boot id — context, never
identity.

## Decision

**Where.** `map links --connections` draws, beside the links, every established TCP connection of
this host that the evidence maps to a linked host: the connection as a place at depth 1 and an
edge `connection.reaches_host` (Connection → Host, a new relation in `relations.yaml`, cost class
`remote`, confidence `exact_or_provider_declared`) to the linked host's root. It is opt-in
because it reads the far sides' socket and address tables (§34): an ordinary `map links` costs
nothing more than before.

**Evidence, read like any read.** For each connected, followed agent link the far side's `socket`
(at most 4096) and `address` (at most 1024) tables are read with ordinary queries, side by side,
each within 5 s, so the far agent authorizes them as it authorizes `get socket` and `get address`
(§10.2). A refusal is a `not_permitted` entry, a failure or timeout `unavailable`; nothing is
inferred from a table this session may not read.

**Graded, and the grade is the confidence:**

| basis | evidence | confidence |
|---|---|---|
| `two_sided` | the far side's own socket table holds the mirror of this connection — its local end is this connection's remote end and its remote end this connection's local end | `strong` |
| `address` | no mirrored socket; the remote address is one of the far side's interface addresses (an address in a private range is said so: `private_address`) | `inferred` |
| none | a loopback, unspecified or link-local remote address names no host and is never evidence; anything else unmatched | no edge |

The strongest basis found decides. **Several links on it are `ambiguous`** — no edge is drawn,
nothing is picked, and the map's new `correlations` field names the connection and every link it
could not tell apart. Two links whose far sides share a boot id are still two candidates
(ADR-0985: context, not identity): one machine reached under two names is ambiguous about which
name, and says so.

**What the edge says.** `basis`, `link`, `local`, `remote`, `far_socket` (the mirrored pair, for
`two_sided`), `private_address`, `crosses_link`, and a provenance of provider
`ono.link.correlate` on the link the far evidence came from. Following it is crossing a link, and
§35.4 still governs: the map names the far root, `jump` is what stands on it.

**The exit on a connection place.** The relation gives a connection place a `hosts` exit. With no
link held it is answered — there is no linked host to reach; with links held it is `unknown`,
"available on request", because answering it costs remote reads a `look` must not spend (§32.1).

## Consequences

- Tests: the grading is pure logic with its own contract and is unit-tested in
  `crates/ono-cli/src/spatial/correlate.rs` (two-sided, address only, private range, addresses
  every host has, ambiguous, two-sided over a shared address, an unreadable far side);
  `crates/ono-cli/tests/cross_host_correlation.rs` runs real agents on loopback with homes and
  identities of their own and real TCP connections the test opens to listeners it owns — one
  correlated two-sided and `strong` with its evidence, one that two linked agents both mirror
  reported `ambiguous` with no edge, a far side that refuses its socket table reported
  `not_permitted`, and a plain `map links` that reads no far table. Acceptance case
  `458-a-connection-to-a-linked-host-is-correlated-on-two-sides.case` correlates the shell's own
  link connection the same way, in the container.
- On one machine every peer shares the kernel's socket table, so every peer holds every mirror:
  the integration suite links one peer where it proves a correlation and two where it proves
  ambiguity. The address basis cannot be proved over loopback (every loopback address names no
  host, by design); it is proved by the unit contract.
- Cost: with `--connections`, two queries per connected agent link per map, bounded as above.

## Alternatives considered

- **Correlate inside every `look` of a connection.** §32.1 forbids a default view from spending
  remote reads; the exit says where the answer is.
- **Pick the link with the best address match.** §14.5 forbids it in so many words.
- **Merge by boot id.** Every local peer shares one; and ADR-0985 keeps the scopes two.
