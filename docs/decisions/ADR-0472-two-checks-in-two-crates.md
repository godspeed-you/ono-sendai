# ADR-0472: Two checks in two crates, and the one that runs first is not the only one

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §10.2, §14.1, §20, §56.1, §56.2, §65.3, Appendix C; ADR-0470, ADR-0471
- Decided by: agent (autonomous)

## Context

§10.2: "negotiation filtering is not sufficient by itself. Every provider/adapter/action dispatch
path MUST also validate that the operation is permitted by the established peer authorization
context. A malicious peer that sends a validly encoded request for a capability omitted from
negotiation MUST receive a stable authorization refusal and the operation MUST NOT execute."

§65.3 names the failure mode this prevents, and §20 sets the bar for believing it has been
prevented: "a security control is accepted only when there is an automated **negative** test
proving the forbidden behavior is refused."

## Decision

**Every `RemoteService` method takes a `&PeerAuthorization` explicitly**, which is §10.3's
"request handlers MUST receive this context explicitly", and which means a new dispatch path
cannot be written without being handed the thing it has to ask.

**The check runs twice, in two crates, from the same context.** `ono_protocol::serve` asks before
it hands a request to the service; `ono-remote`'s `RegistryService` asks again before it touches
the registry. Neither is redundant in the way that matters: they are two different pieces of code
that would have to be wrong together for a request to reach a provider unchecked, and the second
is the one that protects a *future* service implementation whose author reads the trait rather
than the loop.

**The capability an action needs is resolved by the serving side, never read from the request.**
`ServerConfig::with_action_capability(target, operation, capability)` is filled by the CLI from
the `provider_capability` field the command contracts already declare, so a peer that names a
capability names nothing: §65.2 forbids a peer's own claim from granting it anything, and the
cheapest way to hold that is to have no field on the wire for it. An action this side cannot name
is denied — Appendix C's "an unknown capability ID is always denied", which also covers the
capability a later version introduces.

**`adapt` is denied to every policy-governed connection.** Adapting runs a program of the caller's
choosing on the agent's host. No entry in `docs/spec/capabilities.yaml` names that, so no grant can
name it, and §9.4's observe-only default does not include running things. Denying what cannot be
named is the conservative reading of Appendix C, and it is a real reduction: a v0.4.0 listening
agent would adapt for anyone who could reach the port. The stdio agent of §4.3 still adapts,
because there the carrier decided who may run the command at all. If a remote adapter capability is
wanted later it needs an id in `capabilities.yaml` first, which is where a decision like that
belongs.

**A refused dispatch is a failure on the stream, not a dropped request.** The stream opens, carries
one `remote.capability_denied` and ends. A client sees the same shape it sees for any other
refusal, and §53.2's "internal callers MUST match error codes/types, not human-readable messages"
has something stable to match.

## Consequences

Easy: the negative test §20 asks for is one connection that is listed and listed for nothing, and
four requests down four paths. `fixture.observed.sent() == 0` is what "the operation MUST NOT
execute" looks like as an assertion.

Hard: the `RemoteService` trait changed shape, which is a breaking change to a public trait with
two implementors. Both are in this repository, and the alternative — a context reachable through a
thread-local or an `Arc` the service captured at construction — would have made the thing a handler
must ask invisible in its own signature.

Also hard: the double check means a refusal is decided twice and the *first* one wins, so on the
paths that exist today the service-level check never fires. That is what a defence in depth is —
a guard on a path nobody has taken — and it cannot be driven end to end without removing the first
check, which is not a state the product is ever in. What is proved instead is the shape it
depends on: `PeerAuthorization::require_observe` and `require_action` are asked directly and
answer with the stable codes, and every `RemoteService` method takes the context in its signature,
so a new dispatch path cannot be written without being handed the thing it has to ask.

Encoded by: `crates/ono-protocol/tests/authorization.rs::should_refuse_a_request_for_a_capability_the_offer_omitted`,
`::should_refuse_it_on_every_dispatch_path_the_server_exposes`,
`::should_deny_an_action_whose_capability_id_is_unknown`, case `184`.

## Alternatives considered

**Check only in `ono-protocol`.** One place, no duplication. Rejected by §10.2's first sentence and
by §56.1, which gives `ono-remote` the provider composition: the crate that reaches the registry is
a dispatch path in the sense §10.2 means.

**Carry the required capability in `ActRequest`.** The client already knows it from its own command
contract, and it would save the map. Rejected: it is §65.2 with extra steps — the peer would be
supplying the fact that decides whether the peer is allowed.
