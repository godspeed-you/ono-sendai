# ADR-1044: A link row does not offer a trust decision Ono never makes

- Status: accepted
- Date: 2026-11-24
- Spec refs: spec §10.4, §21.5; v0.4.1 §2.6, §7.3, §7.4, §8.4; ADR-0354, ADR-0438, ADR-0854
- Issues: #200
- Decided by: agent (autonomous)

## Context

ADR-0438 gave `ono.link/1` the field `transport_trust` with three values — `pinned`,
`newly_pinned`, `unauthenticated` — and recorded in its Consequences that `newly_pinned` is a
decision only `TrustPolicy::Required` (trust on first use) produces, and that the CLI never asks
for it: a `tcp` link uses `TrustPolicy::Pinned` (ADR-0354), and `ssh` and `local` use
`TrustPolicy::Unauthenticated` over a transport that reports no key. The value was kept "because
the protocol can produce it". Issue #200 asks for one of two endings: a production path that
produces it, or a schema that stops offering it.

A production path would be an operator-facing trust-on-first-use mode. No specification asks for
one: spec §21.5 asks for explicit host trust, v0.4.1 §8.4 for a first pin by a deliberate act, and
ADR-0354 made an unknown host key a refusal by default precisely so that the first machine to
answer for a name does not become that name. A stabilization release does not invent one.

## Decision

**`transport_trust` offers `pinned` and `unauthenticated`, and nothing else.** `newly_pinned` is
removed from `docs/contracts/schemas/link.v1.yaml`, from the generated conformance suite and from
the generated reference.

**The schema keeps its version, `ono.link/1`.** Spec §10.4 lists what breaks a schema — a removed
or renamed field, a changed meaning, a new required field — and narrowing the declared values of an
output-only enum to the values that can actually occur is none of them: every `ono.link/1` record
any released Ono produced is still valid, because no released Ono produced `newly_pinned`.
ADR-0854 is the precedent for an in-place enum change that leaves every valid record valid.

**The one place that maps a decision to the word now cannot produce the removed word.** The
session maps `TrustDecision::Pinned` to `pinned` and `Unauthenticated` to `unauthenticated`. The
library's `NewlyPinned` remains a decision `ono_protocol` can take for a library caller that asks
for `TrustPolicy::Required`, and the CLI asks for it nowhere; should a link ever carry it, the row
reports `transport_trust: null` — unknown — rather than claiming `pinned` for a key nobody pinned
before the handshake. A replacement identity is refused under every policy, as before.

## Consequences

`get link | where transport_trust == newly_pinned` matched nothing before and is now a comparison
against a value the schema does not declare. For MIGRATION and the release note: `ono.link/1`'s
`transport_trust` no longer lists `newly_pinned`, a value no released version ever produced;
scripts that matched on it matched nothing and can drop the clause.

Encoded by `crates/ono-cli/tests/authenticated_link.rs::should_offer_only_the_trust_decisions_a_link_can_carry`
(through `type get link`, the inspectable schema), beside the existing
`::should_show_the_proved_identity_and_the_reported_one_as_separate_fields` and
`::should_report_no_proved_key_over_a_transport_that_proves_nothing`.

## Alternatives considered

**An operator-facing `--trust-on-first-use` for `link host --transport tcp`.** It would make the
value real. It would also be a new trust mode with no specification behind it, in a release that
adds none, reversing ADR-0354's default for anyone who learns the flag.

**`ono.link/2` without the value.** Every command, provider declaration and test that names the
schema moves, for a value that never appeared in a record.

**Keep the value "for library callers".** The library's callers are this workspace's crates; the
schema is the product's contract, and a contract value no product path produces is the kind of
claim ADR-0438's own Consequences warned about.
