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

## Amendment (v0.6.6 remediation, reviews TST-1, TST-12, TST-13, ARC-2, ARC-5, COR-3, PRF-8)

The review found the decision's evidence unsound in two places and its proof unable to fail.

**A loopback mirror is evidence only from this machine.** Every host has `127.0.0.1`, a
link-local and an unspecified address: a far side that is another machine and reports a socket
`127.0.0.31:5432 → 127.0.0.1:41000` reports its own loopback socket, and the table above counted
it as `two_sided` because only the *address* basis excluded such addresses. Now a mirror whose
ends include an address that names no host counts only where the far side is proven to be this
same machine by the identity the repository already uses for that — its `kernel` target answers
this kernel's boot id (ADR-0985, the boot `same_host_as` compares) — and then "this connection
reaches that host" is simply true. Otherwise it is no evidence. Non-loopback mirrors are
`two_sided` and `strong` as before. IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`, what a
dual-stack listener reports) are read as the IPv4 address they map, on both sides.

**A partial table cannot exclude.** A far table cut at its bound, or that failed after its first
record, used to be graded as if complete, so a unique verdict could be drawn `strong` where the
whole table would have been ambiguous. Now each table is read to one more than its bound, so a
table of exactly the bound is not called cut; a cut or part-way-failed far table is reported
`partial` (link entry `status: partial`), and a verdict that is unique only because another
link's table is partial is demoted to `ambiguous`, its `partial` field naming those links — no
edge, nothing picked. This host's own table, cut or failed part way, is reported as a `this host`
entry with `status: partial`: a connection it did not show is not drawn, those it showed are
judged on their own evidence. The far and local reads ask `connection --protocol tcp` — TCP
sockets that have a peer — rather than every socket, so the bound is not spent on listeners, UDP
and unix sockets.

**Gathered before the session is taken, and bounded.** The far reads and the local read happen
before the spatial session is locked; only placing the correlated connections takes it, so a
`look` is not held behind a far side that answers nothing. At most `MAX_PEERS_ASKED` (32) links
are read, as the second hop asks no more; the others are `not_asked` with the bound as the
reason. The correlation and ADR-1097's second hop are asked side by side, so `map links --depth 2
--connections` costs the probe pass (2 s) and then the longer of the two (5 s each): 7 s worst
case, where it was 12 s in sequence.

**Proof that can fail.** `crates/ono-cli/tests/cross_host_correlation.rs` now links, beside real
agents, far agents whose connection tables and boot the test writes (`tests/support/far_side.rs`:
a real TLS listening agent with its own key and authorization, serving fixture `connection`,
`address` and `kernel` targets). With a connection the test owns, it proves: no edge when the
far side does not hold the mirror (red under "fill the far sockets from this host's snapshot");
no `two_sided` for the same service with another client (red under "any socket whose local end
is our remote end"); `strong` with its evidence when it holds the mirror; no edge for a loopback
mirror from another machine; and no strong edge, `ambiguous` with `partial`, while another
link's table is cut at its bound. (A table that fails part way is detected only where the failure
arrives after a record: a far stream's failures travel beside its values, not behind them, so a
failure that overtakes the first record makes the whole table `unavailable` — the conservative
reading, and why the suite proves partiality by the bound.) A table whose read runs out of time after records arrived is likewise partial — what arrived is
evidence, what did not cannot be excluded — and only a table of which nothing arrived is
`unavailable`. (Found when the bound-cut fixture's 4,097 rows took longer than 5 s on a loaded
host and the cut table was read as unavailable, leaving the other link's verdict unique.) The grading of each basis — confidence, `far_socket`,
`private_address` — is unit-tested on `edge`.

**What still has no real two-host proof.** Unprivileged user namespaces are not available on the
run's host, so no second network stack exists for a test: no test correlates a connection
between two real machines, and the non-loopback `two_sided` and the `address` basis are proved
by the unit contract only. Acceptance case 458 runs agents in one container, which share its
boot: it proves what that honestly can — the shell's own link connection is correlated with an
agent proven to be this machine, ambiguity between two such agents is said, and nothing is read
unasked.

**The exit's request path (review COR-1).** The decision left a connection's `hosts` exit
"available on request" with links held, and `follow host` (or `hosts`, or the relation's id),
with `--resolve` or without, answered `spatial.unsupported` with the generic help "`follow
<relation> --resolve` pays for it" — which paid for nothing, since no path in `follow` reads the
far tables. v0.4.1 §34.3: "If a relationship is described as 'available on request', there MUST
actually be a request path." The request path is `map links --connections`, and `follow` does not
cross a link (§35.4: the map names the far root, `jump` stands on it). So the exit's detail now
says "available on request: `map links --connections` reads it across the links", and `follow`
along `connection.reaches_host` from a connection, by any of its words and with or without
`--resolve`, refuses with that sentence and help naming `map links --connections` and `jump`,
never `--resolve`. Encoded by
`crates/ono-cli/tests/cross_host_correlation.rs::should_name_the_request_path_that_answers_whether_a_connection_reaches_a_linked_host`.
Traversing the edge from `follow` — correlating the one connection on demand — is not built: it
would cross a link from a command §35.4 keeps local.
