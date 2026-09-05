# ADR-0469: A grant cannot spell a wildcard

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §9.5, §9.6, §52.3, §56.4, §59.3, Appendix C; ADR-0012, ADR-0453, ADR-0466
- Decided by: agent (autonomous)

## Context

§9.5: "actions MUST be authorized by exact capability ID […] wildcards MUST NOT be the storage
default. If the implementation offers an explicit convenience operation equivalent to 'all current
actions', it MUST expand to the exact capability IDs known at grant time and persist that expanded
list. Newly introduced future capabilities MUST therefore remain denied until explicitly
authorized." §9.6 adds that elevation and destructive risk "MUST require exact explicit grant even
if a future policy profile otherwise allows mutations", and that there MUST be no implicit `admin`
profile that grows by itself. Appendix C's last row: "for action authorization, an unknown
capability ID is always denied."

Every one of those sentences is a rule about a string. A validator would enforce them at the point
where a grant is written and nowhere else, and the interesting question is whether that is enough.

## Decision

**`ActionGrant` is a newtype whose only constructor is `FromStr`, and the grammar it accepts has
no room for a wildcard.** The grammar is the one `docs/contracts/capabilities.yaml` uses: at least two
dot-separated segments, each non-empty, each of ASCII lowercase letters, digits and dashes. Every
one of `*`, `process.*`, `*.signal`, `process.**`, `mutate`, `destructive`, `process.` and the
empty string is outside it. So a wildcard grant cannot be constructed, stored, rendered, parsed or
compared — not because it is validated away at one door, but because there is no value of the type
that means it.

This is deliberately the shape of `Budget` in ADR-0453, which made an unlimited budget
unrepresentable rather than merely refused. The reason is the same: a validator protects the paths
somebody remembered to route through it, and a type protects the paths nobody has written yet.

**`AuthorizationContext::allows_action` compares whole ids for equality and does nothing else** —
no prefix, no glob, no risk-class fallback. An id it does not hold is denied, including one this
build has never heard of, which is Appendix C's last row with no code of its own.

**Elevation and destructiveness require an exact grant, and read risk alone does not.** The offer
filter and the dispatch check both ask one question of a declared capability: is it an observation
that needs no elevation? If so, `observe` covers it. Otherwise the exact id must be granted. That
puts §9.6's two categories — `mutate`/`destructive` risk, and *any* capability marked as needing
elevation — on the exact-grant side together, which is what §9.6 asks for and what a rule keyed on
risk alone would have missed for an elevated read.

**There is no "grant all current actions" convenience operation in v0.4.1.** §9.5 permits one and
fixes what it would have to do; nothing in H2 needs it, and AGENTS.md §4 rules out building a
feature no test demands. If one is added later, §9.5's expansion rule is already enforceable,
because the only thing it could produce is a list of `ActionGrant`s.

## Consequences

Easy: "a capability introduced in a later version stays denied until someone authorizes it" needs
no code at all. It is what happens when the grant is a set of exact strings and the new capability
has a new string. The test that proves it does not invent a future — it configures the agent with
one more action mapping than the grant was written against.

Hard: an operator who wants a service account to manage everything types every capability. That is
§9.6's intent stated as ergonomics, and the diagnostic on a refused pattern says which id to name.

Also: §52.3 wants the gate to reject "unknown capability IDs in an authorization fixture". The
grammar check is not that check — it accepts `process.invented`, which is well-formed and names
nothing. Denying it at dispatch is Appendix C's rule and is implemented; failing the *gate* on it
needs a validator in `xtask`, which is outside this milestone's file scope. Recorded in
`docs/STATE.md`.

Encoded by: `crates/ono-cli/tests/authorized_clients.rs::should_refuse_a_wildcard_or_risk_class_in_an_action_grant`,
`::should_refuse_a_wildcard_from_the_command_that_writes_the_store`,
`crates/ono-protocol/tests/authorization.rs::should_deny_an_action_whose_capability_id_is_unknown`,
`::should_deny_a_capability_introduced_after_the_grant_was_written`,
`::should_leave_every_ungranted_action_refused_when_one_action_is_granted`, case `185`.

## Alternatives considered

**`Vec<String>` with a validator on the write path.** Less code. Rejected: the store's reader is a
second write path and a future `set client-key --from-file` would be a third, and each would have
to remember. A type remembers for them.

**A separate remote action taxonomy.** Rejected by §56.4 in one sentence: "provider capabilities
remain the canonical units used for authorization […] v0.4.1 SHOULD NOT introduce a duplicate
remote-only action taxonomy." The grant names a `docs/contracts/capabilities.yaml` id, and the agent
resolves which id an action needs from the same `provider_capability` field the command contracts
already declare.
