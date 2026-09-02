# ADR-0470: A connection decides its policy once, and cannot widen it afterwards

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.2, §9.2, §10.3, §12.5, §14.2, §65.2; ADR-0437, ADR-0438, ADR-0466
- Decided by: agent (autonomous)

## Context

§10.3: "the server-side connection state MUST carry an immutable authorization context created
immediately after TLS peer verification […] request handlers MUST receive this context explicitly
or through a connection service object. They MUST NOT re-read a mutable authorization file on each
individual request." The reasons are a performance one and a TOCTOU one, and the second is the
serious one: a handler that re-reads the file can be handed a different answer half way through a
request that has already been authorized.

## Decision

**`AuthorizationContext` is built once, from the authenticated fingerprint, and has no way to
change.** All fields are private, there is no `&mut self` method, no setter, and the only
constructor takes an `&AuthorizedClient` the store just looked up. It carries §10.3's six fields:
`peer_fingerprint`, `client_label`, `observe_allowed`, `allowed_action_capabilities`,
`connection_id`, `connected_at`.

**It is built from the fingerprint and from nothing the peer said.** There is no constructor
parameter for a user, a uid, an elevation flag or a source address, so §65.2 — "using
`Hello.identity.user`, UID, elevation or source IP to grant capabilities is forbidden" — holds
because there is no argument to pass. The `Hello` has not even been read when the store is
consulted; §2.2's order is the order of the code.

**`PeerAuthorization` is the two-variant answer every dispatch path consults**, and it exists so
that `AuthorizationContext` never has to be able to say "everything":

- `CarriedByTransport` — the stdio agent of §4.3, reached through `ssh <host> ono --agent`, where
  OpenSSH already decided who may run the command and `peer_key` is truthfully `None`;
- `Policy(Arc<AuthorizationContext>)` — a listening agent's own decision.

A single type with an `allow_everything: bool` would have been a wildcard in the place ADR-0469
worked to keep one out.

**The store is read once per accepted connection, not once per process and not once per request.**
Per process would mean an operator's revocation reached nobody until a restart; per request is the
TOCTOU §10.3 rules out. Per connection is what makes §10.3's "changes to authorization affect new
connections" literally true.

**Live revocation is deferred, and §12.5 asked for this paragraph.** Revoking a client refuses its
*next* connection; a session already running is not torn down. Two reasons. First, a running
connection may be mid-action, and killing it half way through a mutation leaves the far side in a
state nobody chose, which is a worse failure than a few more seconds of a grant somebody has just
withdrawn. Second, tearing down a live connection needs a registry of live connections and a
cancellation path into each, which is phase H3's connection semaphore (§12.1, §12.3) and does not
exist yet. When H3 builds that registry, live revocation becomes a small addition on top of it,
and this ADR should be superseded rather than extended.

## Consequences

Easy: a request handler cannot widen its own policy, because it holds a value with no way to. The
proof is an outcome test — the store is widened while a connection is up, the live link still
cannot act, and the next connection can.

Hard: the deferral above is a real gap between the implementation and what an operator might
expect from the word "revoke". `remove client-key` says "its next connection is refused" in the
message it answers with, so the expectation is set where the action is taken rather than in a
document nobody reads.

Also: reading the store per accept costs a `stat` and a small parse per connection. At H3's
connection limits that is noise, and the alternative is one of the two things §10.3 forbids.

Encoded by: `crates/ono-protocol/tests/authorization.rs::should_build_the_authorization_context_from_the_authenticated_fingerprint_alone`,
`::should_keep_the_authorization_context_immutable_for_the_life_of_the_connection`,
`crates/ono-cli/tests/authenticated_link.rs::should_refuse_the_next_connection_from_a_revoked_client_key`.

## Alternatives considered

**A `Session` object handed to handlers, holding the context and the registry.** §10.3 permits it
("or through a connection service object"). Rejected: the service already exists and is the
agent's, and threading a fifth argument through four trait methods is less machinery than a new
object that would exist only to carry one field.

**Tear down live connections on revocation now.** §12.5 says "MAY". Rejected as above, and
recorded here rather than in a comment so the next reader finds the reasoning beside the decision.
