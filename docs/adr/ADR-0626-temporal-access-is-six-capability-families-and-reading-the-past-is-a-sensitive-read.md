# ADR-0626: Temporal access is six capability families, and reading the past is a sensitive read

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §30.1, §30.7, §37.2–§37.5, §10.3; v0.2 §31.16, §31.5;
  `docs/specs/kuang11/kuang11-plugin-installation-permissions-spec.md` (K11P) §6.4, §6.6, §7;
  ADR-0022, ADR-0594, ADR-0600
- Decided by: agent (autonomous)

## Context

v0.5 §30.7 names six capabilities and says only that temporal access is capability-controlled:

```text
temporal.read.current  temporal.read.history  temporal.read.evidence
temporal.contribute.events  temporal.contribute.causality  temporal.recorder.manage
```

It fixes one property of them — *"A plugin with current object read permission does not
automatically receive historical access"* — and leaves risk, elevation, scope shape, consent
class, permission kind and risk floor to be decided. Those six decisions are what the broker and
the install prompt actually run on: `Capability` carries risk, elevation and scope keys;
`permission.rs` maps every family onto a consent class (K11P §7), a minimum risk (K11P §6.6) and a
host-owned permission kind (K11P §6.4), and a package may raise the risk it declares but never
lower it.

The families arrive into a model with 30 of them, three declarations that `spec-check` compares
(`docs/contracts/capabilities.yaml`, `docs/contracts/kuang/capabilities.v1.yaml`, and the
`capabilities! { … }` macro) and a fourth the permission layer compares
(`docs/contracts/kuang/permissions.v1.yaml`).

## Decision

### 1. Six families, in registry order after `provider.mutate`

| family | risk | elevation | scope | consent class | risk floor | kind |
|---|---|---|---|---|---|---|
| `temporal.read.current` | read | none | — | observation | observe | host-observe |
| `temporal.read.history` | read | none | `window` | observation | **sensitive-read** | host-observe |
| `temporal.read.evidence` | read | none | — | observation | observe | host-observe |
| `temporal.contribute.events` | mutate | none | `kinds` | explicit | mutate | host-change |
| `temporal.contribute.causality` | mutate | none | `rules` | explicit | mutate | host-change |
| `temporal.recorder.manage` | mutate | none | — | explicit | mutate | host-change |

### 2. Reading the past has a `sensitive-read` floor

`temporal.read.history` sits beside `filesystem.read` and `secret.use` rather than beside
`object.read`, and the reason is §30.1: *"Temporal retention increases privacy risk because
harmless current-state facts become a behavioral history when persisted."* A package that may see
that a process exists is asking one thing; a package that may see every process this person ran
last night is asking another. K11P §9.1 excludes anything at or above `mutate` from a recommended
profile and admits `sensitive-read`, so the floor keeps the family out of the automatic path
without making it unofferable.

The read side is split three ways because the three answer different questions. Knowing the
session is at 12:07 (`current`) is not knowing what the machine was doing at 12:07 (`history`),
and neither is reading the raw material Ono derived that from (`evidence`, which carries source
statements and journal cursors an event summary does not).

### 3. `window` is the scope of a historical read

`ScopeKind::Window` already exists — `history.read` uses it for the session's semantic history —
and it is the right shape here for the same reason: *how far back* is the one dimension a temporal
read is naturally bounded on, and the broker can check it before the read rather than filter after
it. A read outside the window is `capability.denied`, not an empty answer.

### 4. Contribution is explicit, and scoped by what may be said

`temporal.contribute.causality` is `explicit` + `host-change` because it changes what Ono asserts
about the world rather than what it reports about itself. Its scope is `rules` (an id list): the
namespaced rule ids the package may register, which is also where §31.5's reservation of `ono.*`
is enforced. `temporal.contribute.events` is scoped by `kinds` (a name list) so a prompt can say
*"may add provider events"* rather than *"may add events"*, which is the difference between a
decision an operator can weigh and one they cannot.

Neither is `extension-local`, although `relation.write` scoped to a package's own contributions
is. A contributed relation is an edge attributed to the package and filterable by it; a
contributed event enters the ledger the whole temporal interface answers from, and a contributed
causal link is an assertion about cause. Two host rules bound them whatever is granted (§37.3,
§37.4): the host stamps the source identity, and contributed causal strength may not exceed
`asserted` unless the host contract explicitly trusts the package as authoritative for a domain.

### 5. `temporal.recorder.manage` is mutating and unscoped

There is one recorder and the decision is binary, so there is nothing to scope and a decorative
scope is forbidden (§31.16). It is `mutate` rather than `read` because starting the recorder
changes what the machine retains about its user — §10's intent requires that to be impossible to
confuse with something that happened quietly. It does not confer reading what the recorder
collects; that is `temporal.read.history`.

## Consequences

- `Capability::ALL` is 36 families. The test that asserts the count is updated with the reason,
  which is what makes the count a check rather than a number.
- Four exhaustive matches in `permission.rs` gained arms; `minimum_risk` gained one explicit arm
  (`temporal.read.history`) and derives the other five from their consent class.
- A package that reads current objects gains no historical access by doing so, which is §30.7's
  own sentence, now enforced by the family split rather than by review.
- `HOST_API` will move from `11.2` to `11.3` when the host calls behind these capabilities land.
  A package declaring `>=11.1` still loads, because the minor is additive.

## Alternatives considered

**One `temporal.read` family.** Rejected: it makes §30.7's guarantee unenforceable, because a
package needing the prompt marker would receive the ledger.

**`temporal.read.history` at `observe`.** Rejected on §30.1. A behavioural history is at least as
sensitive as a file, and `filesystem.read` is `sensitive-read`.

**`temporal.contribute.events` as `extension-local`, by analogy with `relation.write`.** Rejected:
the analogy breaks on what the contribution enters. A relation is an attributed edge; an event is
a claim about what happened, and every later reconstruction, timeline and explanation reads it.

**Scoping contribution by spatial scope instead of by kind.** Rejected: §37.3 already forbids a
package asserting an object exists outside the objects it can resolve through permitted providers,
so a spatial scope would restate an invariant the host enforces anyway, while the kinds it may
emit is the thing an operator can actually judge.
