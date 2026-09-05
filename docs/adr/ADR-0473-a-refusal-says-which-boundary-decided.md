# ADR-0473: A refusal says which boundary decided, in a code and in a field

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §10.4, §53.1, §53.2, §53.3, §54.1, §54.2, §59.9; ADR-0006, ADR-0125,
  ADR-0466, ADR-0472
- Decided by: agent (autonomous)

## Context

§10.4 names a stable error family — `remote.unauthenticated`, `remote.unauthorized`,
`remote.capability_denied` — and says the exact numeric codes "MUST be allocated through the
existing error registry and checked by contract tests". §53.2 says why the codes matter: "internal
callers MUST match error codes/types, not human-readable messages". §54.1 asks that a refusal tell
the user which boundary made the decision.

## Decision

**Four codes, registered in `docs/contracts/errors.yaml` and `crates/ono-core/src/error.rs` in the same
increment, all of kind `safety`.**

| code | name | when |
|---|---|---|
| `Ono-Sendai-E1201` | `remote.unauthenticated` | the transport proved no key, so there is no identity to authorize (§2.2) |
| `Ono-Sendai-E1202` | `remote.unauthorized` | authenticated, and not in the store (§9.4, §59.1) |
| `Ono-Sendai-E1203` | `remote.capability_denied` | listed, and not for this (§10.2) |
| `Ono-Sendai-E1204` | `remote.authorization_store_invalid` | the store would not load, so nothing is authorized (§9.2, §59.5) |

`safety` rather than `provider`, following ADR-0006: these are policy decisions, and a script that
branches on the kind must not confuse "the operator has not authorized you" with "the host is
unreachable". The fourth is not in §10.4's list and is needed by §9.2 — a store that will not parse
is a different condition from a client that is not in it (ADR-0466), and folding it into E1202
would send an operator to `add client-key` when the answer is a typo on line 4.

**The denial carries the boundary in structured metadata, not in prose.** `denied_because` is
`observe_not_allowed`, `action_not_granted` or `capability_unknown`; `requested_capability` names
the id; `peer_fingerprint` names the client; `connection_id` ties it to the audit trail. §10.4 asks
for exactly the first distinction — "whether the request was denied because observe access is off
or because the action capability is absent" — because the two are fixed by different commands.

**A fingerprint is shown in full.** §53.3: "a fingerprint is public identity material and MAY be
shown in full", and a truncated one is one an impersonator can search a collision against. Nothing
else about the peer appears: no key material, no payload, no environment.

**No refusal asks a question.** §59.9 requires every trust failure to be non-interactive and
deterministic in scripts, and §54.2 forbids making a user turn on debug logging to learn why. So
the refusal is a value with `retryable: false` and a help line that names the command an operator
would run on the agent's host — and there is no "continue anyway" anywhere, which is ADR-0015's
standing rule 4 applied to the second half of the trust model.

**The refusal an unlisted client receives discloses nothing.** It names the fingerprint to add and
whether a store exists, and no provider, schema, target or capability. §59.1 requires that, and
the test greps the whole rendered error for the inventory.

## Consequences

Easy: `try { link host … } catch e { $e | select code name kind }` is how a script tells an
unauthorized client from an unreachable host, and the answer is stable across versions because
ADR-0125 makes the taxonomy closed and additive.

Hard: four codes is one more than §53.1 lists, and the extra one is load-bearing rather than
decorative. It is registered with the reason in its `help`, so a reader of `docs/reference/errors.md`
finds the distinction rather than having to reconstruct it.

Encoded by: `crates/ono-protocol/tests/authorization.rs::should_declare_the_three_remote_refusal_codes_with_their_details`,
`::should_answer_the_same_stable_code_for_the_same_refusal_every_time`,
`::should_refuse_without_prompting_when_no_terminal_is_attached`,
`crates/ono-cli/tests/authenticated_link.rs::should_report_an_authenticated_but_unauthorized_link_as_exactly_that`,
cases `182`, `184`, `186`, `187`.

## Alternatives considered

**Reuse `safety.policy_denied` (E0702).** It exists and it is the right kind. Rejected by §10.4,
which asks for a family a caller can distinguish, and by experience: E0702 already carries the
unpinned-host refusal, so a script could not tell "this host is not pinned here" from "you are not
authorized there".

**Put the three refusals in `crates/ono-value/tests/errors.rs`, beside the resource family.** That
is where `docs/ACCEPTANCE.md` §4.8.3 named them. Moved to `crates/ono-protocol/tests/authorization.rs`,
where the codes are produced and where the behaviour that produces them is proved in the same file;
the box text is corrected to match.
