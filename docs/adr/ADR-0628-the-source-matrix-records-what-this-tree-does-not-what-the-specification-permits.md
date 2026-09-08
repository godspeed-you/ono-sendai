# ADR-0628: The source matrix records what this tree does, not what the specification permits

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §7.1, §7.4, §8, §21.1–§21.8, §22.1–§22.8, §23.1, §24.1, §36.4, §37.5
- Decided by: agent (autonomous)

## Context

v0.5 §21.1 lets every provider advertise seven temporal capabilities, and §22 describes what the
Linux sources can do. Read as a permission list, §22 sounds generous: systemd *"SHOULD contribute
live unit state transitions"*, container runtimes *"SHOULD expose runtime-native lifecycle
events"*, netlink *"SHOULD provide live evidence"*.

The survey of this tree says something narrower. **Only netlink's link, address and route tables
and the filesystem's inotify watches implement `subscribe`.** systemd, procfs, containers, mounts
and sockets are polled by the runtime, however promptly. A capability matrix written from §22's
verbs would claim `live_events` for systemd and be wrong today, and every coverage record that
provider wrote would inherit the error: coverage says what a source could have seen, so an
overstated capability produces an overstated absence, and §7.4's negative claims rest on exactly
that.

§21.5 makes the point itself about the strongest capability: *"Providers MUST NOT advertise
`exhaustive_events` merely because events usually arrive."*

## Decision

### 1. `sources.yaml` records the implementation, not the aspiration

Each row states the capabilities the source may honestly claim **as this tree implements it**, and
its `doc` says what the upgrade would be. `linux.systemd-dbus` has `live_events: false` with the
note that it becomes true when the provider subscribes to `PropertiesChanged`; `container-engine`
has `live_events: false` with the note that a runtime event stream changes that flag and not the
identity model.

### 2. Two prohibitions of §21 and §22 are gate checks, not paragraphs

- `linux.procfs` must declare `historical_query: false`. §22.1: *"The reference provider MUST NOT
  claim native historical process coverage."* Process history on a default installation comes from
  the recorder comparing snapshots, with provenance `snapshot_diff` and coverage stating the
  polling interval.
- A source claiming `exhaustive_events: true` must have `sampled: false` and complete coverage.
  §21.5 makes the claim one about sequence continuity strong enough to support absence claims, and
  a polled source cannot keep it.

`claim_rules:` in the file states both, plus §21.8's *"a temporal provider failure MUST produce
coverage loss"*, so a future row cannot drift past them without a reviewer seeing the rule beside
the row.

### 3. Exactly one source claims `exhaustive_events`, and it is the shell

`ono.session` claims it, for one capability: actions taken through this shell. The shell is the
sole authority for that fact, it records every action it authorizes, in sequence, with no
sampling. So absence proves something here — narrowly, and only about this session — which is what
§21.5's contract means and what §17.1's *"the shell itself possesses unusually strong knowledge of
operator intent and execution"* is describing.

Every other source, including netlink, declares `exhaustive_events: false`. A netlink socket can
drop messages under buffer pressure and the kernel says so; a dropped message is a coverage gap
with reason `source_disconnected`, not a hole in a stream still described as continuous.

### 4. The matrix is joined to the provider contracts, in both directions

`docs/contracts/providers/*.yaml` carries a `temporal:` block per provider. `sources.yaml` names
the provider that fronts each source, and the gate compares the two on the six boolean
capabilities. §36.4's last rule — *"a provider advertises temporal capability absent from its
contract metadata"* — is that comparison.

Two facts about the join are decisions rather than mechanics:

- One provider id may appear several times, once per target group. `linux.netlink` fronts three,
  and only two of them push. The source-level claim is therefore the **disjunction** across the
  rows: the source can do a thing if any of its groups can, which is what a user asking "does
  netlink give me live events" means.
- `retained_history` is excluded from the comparison. It is a duration rather than a claim, and a
  source's effective retention is its configuration's rather than its contract's.

### 5. The evidence source `ono.session` is not the provider `ono.session`

They share a word and nothing else: the provider serves login-session *objects*, and the evidence
source is the shell. The row's `provider` is null, with the collision named in the file, because a
join on the name would have made the matrix claim that the object provider is exhaustive about
actions it knows nothing about.

## Consequences

- The matrix understates several sources relative to §22's verbs, and each understatement carries
  the sentence that would change it. That is the intended direction of error: §7.4's absence
  claims and §8's coverage composition both read these flags, and an overstated flag becomes a
  false claim about the world rather than a missing feature.
- Agent J's provider work and this matrix are held against each other on every gate run, so
  turning on a subscription is one edit in each file and neither can land alone.
- `adapter:*`, `remote:*` and `kuang:*` rows state a floor rather than a claim: what a given
  adapter, link or package may advertise is its own contract's, bounded by §37.4 for a package and
  by negotiation for a link (§24.5 lets a remote with no history degrade to no history rather than
  failing).

## Alternatives considered

**Write the matrix from §22 and let the providers catch up.** Rejected: it inverts the direction
of truth. A contract that describes an unimplemented capability is the failure §36.4 exists to
catch, and here it would silently corrupt coverage rather than merely misinform.

**Claim `live_events` for any source the runtime polls promptly.** Rejected on §21.3 and §8: the
capability is about the source pushing, and the sampling interval is what coverage has to state.

**Merge `sources.yaml` into the provider contracts.** Rejected: three of the nine evidence source
classes (`adapter:*`, `remote:*`, `kuang:*`) have no provider contract to live in, and `ono.session`
and `ono.recorder` are not providers at all.
