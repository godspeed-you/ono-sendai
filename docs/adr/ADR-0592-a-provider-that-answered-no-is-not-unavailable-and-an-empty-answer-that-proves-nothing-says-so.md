# ADR-0592: A provider that answered no is not unavailable, and an empty answer that proves nothing says so

- Status: accepted
- Date: 2026-09-07
- Spec refs: §16.1, §43; `docs/contracts/errors.yaml`; `docs/architecture/external-system-provider.md`
  §8.4, §18.1, §18.3, §19.1, §19.4, §20.1, §20.2, §21.6, §46.4 of the Kubernetes provider
  specification (by reference); ADR-0006, ADR-0587
- Decided by: agent (autonomous)

## Context

The provider family of the error registry had three codes: `provider.unavailable` ("the backing
system is absent or not running"), `provider.unsupported` ("the provider does not implement this
capability") and `provider.schema_violation`. ADR-0587 added `contribution.refused` for a package
declining under a rule of its own, and named the codes it had been forced into as "each asserting
something else".

The generic provider contract asks a provider to distinguish, at minimum: configuration,
authentication, authorization, scope, not-found, conflict, rate-limited, timeout, transport,
remote-service, incompatibility, partial-result, internal and cancelled failures (§19.1); to keep
"authentication expired", "authorization denied", "remote API unavailable" and "resource absent"
apart (§8.4); to treat "denied" as data rather than as absence (§18.1); and to let a user tell an
incomplete or inconclusive result from a complete one (§19.3, §21.6). Several of those have a home
already — `Ono-Sendai-E0301` for absence, the `conflict`, `timeout` and `cancelled` kinds, the
capability family for the host's own boundary. Four did not, and the Kubernetes provider was
spelling them with codes whose summaries are false of the situation:

- an API server that answers `401` has answered; it is not *unavailable*;
- an API server that answers `403` has answered, and the provider is perfectly able; it is neither
  unavailable nor *unsupported*;
- an API server that answers `429` with `Retry-After` is not unavailable either, and a script
  that matches `provider.unavailable` cannot tell a throttle from an outage;
- a search that finds nothing where nothing can be concluded from that — Kubernetes Events
  retained for an hour, a log with no lines, a verification that ran out of window — was being
  reported as `provider.unavailable` because the alternative was an empty result, which the
  specification forbids in as many words.

## Decision

**Four codes join the provider family. Each is generic — nothing in its name or summary belongs to
one external system — and each states the truth the existing codes could not.**

| Code | Name | Kind | Means |
|---|---|---|---|
| `Ono-Sendai-E0404` | `provider.inconclusive` | `provider` | The answer is empty or incomplete, and that establishes nothing about the system. |
| `Ono-Sendai-E0405` | `provider.authentication_failed` | `permission` | The external system did not accept the credential the provider presented. |
| `Ono-Sendai-E0406` | `provider.authorization_denied` | `permission` | The external system refused the operation to the identity the provider presented. |
| `Ono-Sendai-E0407` | `provider.rate_limited` | `timeout` | The external system asked the provider to slow down. |

The kinds are chosen for what a script branches on (ADR-0006): a credential refusal and an
authorization refusal are both `permission`, because both are answered by a change of identity or
of grant and neither by a retry; a throttle is `timeout`, because waiting is the one thing that
resolves it; an inconclusive answer is `provider`, because it is a statement about what the
provider could establish and nothing else.

The difference the four preserve, which the registry now has a word for at every step:

```text
unsupported     the provider cannot do it                 provider.unsupported
unavailable     the system did not answer                 provider.unavailable
denied          the system answered, and said no           provider.authorization_denied
                the system answered, and said who are you  provider.authentication_failed
safety-refused  the provider declined under its own rule   contribution.refused
inconclusive    the answer is empty and proves nothing     provider.inconclusive
```

## Consequences

The taxonomy is closed and additive (ADR-0006): nothing was renumbered or re-pointed, and
`E0404`–`E0407` are the next free numbers of the family. `cargo run -p xtask -- spec-check`
compares `docs/contracts/errors.yaml` against `ono_core::ErrorCode` in both directions, so the
four are registered and implemented in one change.

A provider that was spelling these four truths with a false code migrates to the true one, and
its tests move with it. The Kubernetes provider is the first: its `401` after a failed credential
refresh, its `403` on a named object or a mutation, its `429`, and the refusals of `k8s-event`,
`k8s-log` and an inconclusive verification are the call sites. A script that matched
`provider.unavailable` to mean "the cluster is down" was matching a lie before and matches the
truth after; one that matched it to mean "anything went wrong" should match the `provider` kind.

What this does not add: a code for "scope error", "configuration error" or "schema/version
incompatibility". The first two are `resolve.*` and `contribution.refused` in practice, and the
third is `adapter.version_incompatible` for adapters and a coverage gap for a provider, where it
is data rather than an error. A code nothing raises is refused by `spec-check`, and none of these
had a call site asking for it.

## Alternatives considered

**Reuse `io.permission_denied` for the two refusals.** Rejected: its help text sends the user to
the operating system, and its summary says so. An external system's refusal is not the kernel's.

**One `provider.denied` for both authentication and authorization.** Rejected: §8.4 of the
generic contract lists them as two of the four things a provider MUST distinguish, and the fixes
have nothing in common — one is a credential, the other a role.

**Make `provider.inconclusive` a `stream` kind, since it ends a stream.** Rejected: what it
carries is a statement about evidence, not about transport, and a script that treats every
`stream` error as a broken pipe would retry a question that was answered.

**Let providers keep answering empty streams and put the truth in provenance.** Rejected: that
is the anti-pattern the Kubernetes provider's ADR-0025 argues against, and the generic contract's
§19.3 requires the user to be able to *tell*. An empty table downstream of a pipe tells nothing.
