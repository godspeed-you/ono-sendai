# ADR-0723: The host owns what a package says about the past

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.1, §7.2, §15.1, §15.5, §15.8, §30.7, §37.1, §37.3, §37.4, §37.5;
  v0.2 §31.5, §31.16, §31.19, §31.26, §31.37, §31.80; ADR-0600, ADR-0722
- Decided by: agent (autonomous)

## Context

v0.5 §37.1 draws the boundary: "KUANG/11 may extend history and causality, but Ono core retains
authority over identity, evidence classes, causal labels, capability policy and rendering truth."
§37.3 turns it into a list the host validates — schema, source identity, timestamps, scope
visibility, referenced spatial identities, and event size and rate limits — with one sentence
standing out from the rest:

> A plugin cannot assert an object exists outside objects it can resolve through permitted
> providers.

§37.4 adds the other half: a third-party causal rule is namespaced, identifies its source, and
"by default, plugin causal strength MUST NOT exceed `asserted` unless the host contract explicitly
trusts that package/source as authoritative for a domain."

## Decision

`ono_kuang_supervisor::temporal` is where all of it lives, and every rule is a refusal rather than
a correction.

### 1. Attribution is set, not checked

A contributed event's evidence source is `kuang:<package-id>/<source-id>`, written by the host over
whatever the package supplied. This is `restamp_provenance`'s rule (§31.80) applied to evidence: a
package that writes `linux.procfs` into its own event has written a field the host replaces, and
there is nothing to argue about because there is nothing a package can say that changes it.

### 2. Visibility is a set of schemas, resolved once at load

`VisibleSchemas` is built from the grant on `object.read` — unscoped, a `schemas` list, or nothing
— plus every schema the package contributes itself. An event whose subject is of a schema outside
that set is `capability.scope_violation`, in the same words a path outside a granted `paths` scope
is refused with.

The reasoning is worth keeping: a package able to manufacture an object's existence could
manufacture anything reconstructed from it. Reconstruction (§9.1) walks events; an invented
`object.appeared` is an invented object in every historical map afterwards.

### 3. A ceiling that lowers and never raises

`CONTRIBUTED_STRENGTH_CEILING` is `asserted`, and the constant is held against
`docs/contracts/temporal/causality.yaml` → `contributed_rules.strength_ceiling` by a test that reads
the registry rather than a second copy — the pattern the remote suites already use for
`limits.yaml`. The cap is applied through `EvidenceStrength::weakest_of`, which is the only
operation §7.2 permits: there is no branch in which a claim gets stronger, including the trust
exception, so a package declaring `correlated` keeps `correlated` and a package declaring
`authoritative` carries `asserted`.

§37.4's exception is `AuthoritativeDomains`: a set of `(package, domain)` pairs, empty by default,
with no wildcard. `ceiling_for` answers `Authoritative` for a pair the host contract names and the
constant for everything else. It hangs on `LoadConfig`, so the shell can fill it when there is an
operator spelling for it; until then every package is capped, which is the safe direction and the
one §37.4 makes the default.

### 4. Ceilings the host owns

`ContributionLimits` is §37.3's last item made concrete: 256 events per call, 64 KiB per event, and
10 000 events a minute per instance. Every one is the host's; a package declares none of them.
Exceeding them is the shipped resource family — `resource.item_limit` and `resource.byte_limit` —
rather than a new taxonomy, because a ceiling met is a ceiling met (§31.79: there is no second
error model for extensions).

A timestamp more than two seconds ahead of the host clock is refused. Not zero, because a machine a
few milliseconds ahead is not lying; not generous, because an event stamped in the future reorders
a timeline it was never part of.

### 5. A call is refused whole

The first refusal ends the call and nothing is stored. A partially accepted contribution leaves the
package unable to say which half landed and the ledger holding half a story.

## Consequences

`crates/ono-kuang-supervisor/tests/temporal.rs` is the outcome suite: fifteen cases over the scope
rule, the vocabulary, the timestamps, the three ceilings, the namespace, the class and the
strength. `crates/ono-kuang-sdk/tests/conformance.rs` proves the same through a live instance,
including that the host's stamp — not the package's — is what reaches the ledger. The decoder is in
the `plugin-protocol` fuzz target, whose invariants are that an accepted event names a schema the
package can resolve and carries the host's attribution, so a crash is not the only thing a finding
can be.

Every refusal is audited as loudly as a success (§31.37), through the same trail a capability
denial uses.

## Alternatives considered

**Refusing an event whose `source` the package filled in.** Rejected: it makes a package that
copied a field from its own external system fail for a reason it cannot understand, and the field
is overwritten anyway. `relations.contribute` documents `provider` as host-set for the same reason.

**Trusting a package by publisher rather than by domain.** Rejected: §37.4 says "authoritative for
a domain", and a package the operator trusts about network flows has said nothing about storage.
The pair is the smallest thing that can be trusted, so it is what is trusted.
