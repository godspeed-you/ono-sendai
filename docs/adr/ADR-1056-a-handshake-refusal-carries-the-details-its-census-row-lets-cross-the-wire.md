# ADR-1056: A handshake refusal carries the details its census row lets cross the wire

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.4.1 §9.1, §9.2, §10.4, §13.1, §53.2, §53.3, §54.1, §54.2, §59.1, §59.5; spec
  §21.2; ADR-0006, ADR-0439, ADR-0537, ADR-0938
- Issues: #182
- Decided by: agent (autonomous)

## Context

A link is refused at the handshake with one frame, `Reject { code, message }` (spec §21.2). The
refusal census `docs/contracts/hardening/refusals.yaml` declares, per refusal, the metadata keys
that explain it, and since ADR-0938 the shell prints them under the message. For a refusal made
at the far end of a link none of them arrived: the frame had nowhere to put them. E1202's registry
help promises the client `store_present` — "you are not on the list" against "there is no list" —
and the client could never tell the two apart (issue #182).

Three constraints shape the fix:

1. **Disclosure.** A handshake refusal is written to a peer the agent has authenticated by mutual
   TLS (§13.1) and has not authorized — or, for a ceiling or a deadline, has not asked about yet.
   §59.1 allows it "minimal rejection protocol data" and nothing about the machine. §53.3 allows
   limits, fingerprints and capability ids in error detail and forbids secrets.
2. **Compatibility.** Agents and clients of different versions meet. An older client must not
   read a new refusal as something else or as success; a new client must read an older agent's
   bare refusal; a hostile agent must not be able to inject arbitrary metadata into a client's
   error, nor make a refusal undecodable by garbling the new part.
3. **Extension.** The next workstream adds request/response pairs (probe, correlation). The rule
   this ADR states has to be the one way the link protocol grows.

## Decision

**The census decides what crosses the wire.** `refusals.yaml` gains a per-row field,
`over_the_wire`: the `explains` keys a handshake refusal carries to the refused peer. It is a
subset of `explains` (`cargo xtask spec-check` refuses a key that is not, so a key reaches a peer
only after it has been declared and argued in the census) and empty when omitted. Per refusal:

| refusal | crosses | stays home | why |
|---|---|---|---|
| `remote.unauthorized` (E1202) | `peer_fingerprint`, `store_present` | — | The fingerprint is the refused peer's own key (§53.3: public identity material). `store_present` adds nothing the sentence did not already say in words ("…where no client is authorized yet"); it lets a script branch without string matching (§53.2). It reveals whether *anyone* is authorized, never who. |
| `remote.unauthenticated` (E1201) | `denied_because` | — | A constant reason word. |
| `remote.authorization_store_invalid` | `denied_because` | `store_path`, `store_line`, the file-naming sentence | A place on the agent's filesystem is the operator's to read (diagnostic, audit), not a peer's. The listener now tells the peer a fixed sentence ("this agent cannot read its authorization store…") instead of the parser's message, which named the file and quoted the bad line. |
| `remote.connection_limit` (E1501) | `ceiling`, `limit` | — | A ceiling's name and figure are a non-secret limit (§53.3) the sentence already states. |
| `remote.handshake_timeout` (E1502) | `timeout_ms` | — | Likewise. |

`remote.capability_denied` is not a handshake refusal: it is a stream `Failure` on an authorized
connection and is not governed by this field. A code without a row carries nothing.

**The frame.** `Reject` gains an optional field, `details`, a JSON object of the crossing keys:
`{"code": …, "message": …, "details": {"peer_fingerprint": "sha256:…", "store_present": false}}`.
It is omitted when empty. Values have one of three shapes — text (at most 256 bytes), flag, or
non-negative count — fixed per key.

**Both ends apply the census.** `ono_protocol::refusal` holds `over_the_wire` as a table (the
contract test `crates/ono-protocol/tests/refusal_details.rs::should_carry_exactly_the_keys_the_contract_lets_cross_the_wire`
holds it to the YAML row by row). The agent builds the frame with `Reject::refusing`, which
copies only the row's keys in their declared shape, whatever else the error value carries. The
client builds its error with `Reject::to_error`, which keeps only the row's keys for the code that
arrived, each only in its declared shape and size; everything else is dropped, never believed.
`details` is read leniently: an array, a string or `null` where the object belongs is a block the
client cannot interpret, and the refusal is still the refusal its code names.

**Compatibility, decided and tested:**

- *Older client ↔ new agent.* An older client decodes `Reject` as `{code, message}`; serde ignores
  the unknown `details` field (no `deny_unknown_fields` anywhere in the handshake types), so it
  reads exactly the refusal it read before, without the details.
  `refusal_details.rs::should_send_a_refusal_an_older_client_still_reads` captures the agent's
  real frame and decodes it with the older shape.
- *New client ↔ older agent.* No `details` field: the default is empty, the error carries the code,
  the sentence and this side's remediation, and no detail is invented.
  `::should_read_a_refusal_from_an_agent_that_sends_no_details`.
- *Hostile agent.* Wrong types, oversized values, undeclared keys (`store_path`, a private key),
  five thousand keys, a non-object block: the refusal keeps its code and only declared,
  well-shaped keys survive (`::should_keep_a_refusal_a_refusal_whatever_its_details_look_like`).
  Nesting deeper than `Limits::max_value_depth` fails the whole frame's decoding before any
  conversion, as it does for every frame: the client then reports `remote.protocol_mismatch` —
  never success. An unknown code is reported as `remote.protocol_mismatch` and none of its details
  are kept (`::should_carry_nothing_for_a_code_the_client_does_not_know`). The fuzz target
  `remote-handshake` decodes every input as a `Reject` and asserts the same bounds; three seeds
  (`fuzz/corpus/remote-handshake/reject-*.json`) start it from a well-formed, a hostile and a
  non-object details block.
- `PROTOCOL_VERSION` stays 1 and the ALPN token stays `ono/2` (ADR-0439): nothing an older peer
  must understand was added.

The client also sets `retryable` from the code — true for E1501 and E1502, the two refusals about
the moment rather than about who is asking — instead of `false` for every refusal, matching what
the agent's own error value says.

**How the link protocol is extended — the one rule.** Three cases, in order of preference:

1. *An optional field an older peer may ignore.* Add it with `#[serde(default)]` (and
   `skip_serializing_if` when absent), read it leniently, and prove both directions as above. The
   change is allowed only if an older peer that ignores the field reaches a correct, if less
   detailed, outcome. `details` is this case.
2. *Something an older peer cannot ignore — a new request/response pair, a new frame kind.* The
   frame kind takes the next free number (`FrameKind` never reuses one). Its availability is
   announced as a capability id in the agent's offer (`ServerConfig::with_capabilities`, carried
   in `Accept.capabilities`, which the handshake already intersects with the client's `Hello`),
   e.g. `ono.link.probe/1`. A client sends the new frame only when `Negotiated::capabilities()`
   contains the id, and otherwise answers its caller locally with a structured refusal saying the
   peer does not offer it (`remote.protocol_mismatch` unless a more precise code is registered) —
   it never sends a frame the peer did not announce. An older agent announces nothing, so it is
   never sent one; an older client never sends one. A peer that nonetheless receives a frame kind
   it does not know refuses the link (`ProtocolError` → `remote.protocol_mismatch`), never
   ignores it into success.
3. *A change of meaning of an existing frame.* A new `PROTOCOL_VERSION`, negotiated in
   `Hello`/`Accept`; never a silent reinterpretation.

## Consequences

- Issue #182's exit test holds at three levels: the protocol
  (`refusal_details.rs::should_tell_an_unauthorized_client_that_the_agent_authorizes_nobody_yet`,
  `::should_tell_an_unauthorized_client_that_the_agent_lists_somebody_else`), the product
  (`crates/ono-cli/tests/authenticated_link.rs::should_tell_an_unauthorized_client_whether_the_agent_authorizes_anybody`
  — the E1202 refusal at the client shows `peer_fingerprint:` and `store_present:` under its
  message, and the value carries them), and the container
  (`415-a-refused-client-is-told-whether-the-agent-authorizes-anybody.case`).
- A refused peer no longer learns where the agent keeps its store
  (`crates/ono-remote/tests/limits.rs::should_not_tell_a_refused_peer_where_the_agent_keeps_its_store`).
- Adding a crossing key is a census edit plus a table row; the contract test fails until both
  agree, and spec-check fails if the key is not first an `explains` key.
- The message itself is still free text from the agent. Bounding it, and the `Failure` frames of
  an authorized connection, are outside this decision.

## Alternatives considered

- **Send the whole `ErrorValue` (as a stream `Failure` does).** Would carry `store_path`, help
  text describing the agent's configuration and anything a future refusal attaches — exactly the
  disclosure §59.1 forbids before authorization.
- **A free-form metadata map, filtered only on the agent.** Trusts the far end; a hostile agent
  could put any key, of any size, into a client's error and its rendering.
- **Gate `details` behind a negotiated capability or a protocol version bump.** Needless: an older
  peer ignores an unknown optional field, and E1502 is written precisely when no `Hello` — and so
  no announcement — ever arrived.
- **Supply `store_present` from the client's own knowledge.** The client has none; only the agent
  knows whether it has a store.

## Amendment (v0.6.6 remediation, review COR-R3)

`store_present` was set from "a file exists at the store's path", so a store emptied by `remove
client-key` — the header and nobody — answered `true` and "not on the list", exactly like a store
that lists somebody else, while the agent's own startup line said "no client is authorized yet".
The exit criterion of #182 is "an empty store" told apart from "a store that lists somebody else",
so the condition is now *the store authorizes somebody*: `store_present` is true when the store
names at least one client, and false (with the sentence "…where no client is authorized yet")
when it authorizes nobody, written or not. The field keeps its name, so an older client reads it
unchanged; the disclosure argument of the table above holds as before — it reveals whether
anyone is authorized, never who. Encoded by
`crates/ono-protocol/tests/refusal_details.rs::should_tell_an_emptied_store_apart_from_one_that_lists_somebody_else`.
