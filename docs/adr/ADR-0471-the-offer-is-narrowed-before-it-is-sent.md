# ADR-0471: The offer is narrowed before it is sent, and the inventory is itself information

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §9.6, §10.1, §21.2, §59.1, §65.3, Appendix C; ADR-0015, ADR-0470
- Decided by: agent (autonomous)

## Context

§10.1: "the protocol handshake MUST NOT advertise the full server capability set and then rely
only on later dispatch checks. The `Offer` used to negotiate a direct link MUST first be
intersected with the authenticated client's authorization. This means unauthorized capabilities
are absent from the accepted link contract."

The handshake of ADR-0015 built one `Offer` from `ServerConfig` and sent it to whoever asked.
§59.1 adds the reason this matters beyond tidiness: an unlisted client must learn no "process
list, schema list or capability inventory". An inventory is information about a machine, and a
client that may only observe should not be able to read the shape of what it may not do.

## Decision

**`ServerConfig::offer_for(&PeerAuthorization)` replaces `ServerConfig::offer()`.** There is no
way to build an unfiltered offer for a policed connection, because the only function that builds
one takes the policy.

The filter asks one question of each declared capability, and it is the same question the dispatch
check asks (ADR-0472), through the same function:

- an observation (`read` or `observe` risk) that needs no elevation is covered by `observe`;
- everything else — `mutate`, `destructive`, and *any* capability marked as needing elevation —
  requires the exact granted id (§9.6).

**A provider that declared capabilities and kept none is withheld whole.** Leaving it in the offer
with an empty capability list would still advertise its id and its targets, which tells a client
what this machine runs. A provider that declared no capabilities at all is kept, because it
withholds nothing either way — the unavailable-provider descriptors of §21.2 stay visible, and
§35.3's "a capability that is missing must be visibly missing" is unaffected for the capabilities
a client may actually use.

**The agent-wide capability list is narrowed to what survived.** Those are bare ids with no risk to
judge, so a policed connection keeps only the ones a surviving provider still declares. An id no
surviving provider names is dropped: fail conservative, as Appendix C's last row asks.

**An unlisted client never reaches the filter at all.** `serve` resolves the policy before it
negotiates, and refuses with a `Reject` carrying `remote.unauthorized` (E1202). §59.1's "before
provider negotiation" is the order of two statements, and the refusal is the only thing that
crosses the wire.

## Consequences

Easy: `link host` against an agent that authorized you to observe negotiates a contract with no
action capability in it, so the local risk display (spec §17.1) has nothing to show and `get
command` on the far side offers nothing that would be refused. The offer stops being a catalogue
of what someone else may do.

Hard: two clients of the same agent now negotiate different contracts, so a bug report that quotes
"the providers this host offers" is only true for the client that asked. The `connection_id` in the
audit trail is what ties an offer to the policy that shaped it.

Also hard: the filter is not the enforcement and must never be mistaken for it. §65.3 names
"hiding a capability in `Accept` but still executing a forged request for it" as a failure mode,
and ADR-0472 is the other half. The two are written next to each other so neither can be read as
sufficient.

Encoded by: `crates/ono-protocol/tests/authorization.rs::should_offer_only_the_capabilities_the_clients_policy_allows`,
`::should_leave_an_ungranted_action_capability_out_of_the_offer_the_provider_advertises`,
`::should_disclose_no_process_schema_or_capability_inventory_to_an_unlisted_client`, case `183`.

## Alternatives considered

**Offer everything and refuse at dispatch.** One code path, and §10.2's check would catch every
forged request anyway. Rejected by §10.1 in its first sentence, and by §59.1: the refusal would
come after the inventory had already been read.

**Keep a filtered provider with an empty capability list.** Preserves the shape of the offer.
Rejected: the id and the targets are the disclosure, not the capability list.
