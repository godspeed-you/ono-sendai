# ADR-0652: A checkpoint holds a bounded class or none of it

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §3.6, §7.4, §9.1, §9.6, §42.1, §42.2, §42.4
- Decided by: agent (autonomous)

## Context

§42.2 lists what a default local checkpoint should hold — identity index summaries, services,
processes where visible, interfaces and routes, mounts and filesystems, containers, the relevant
relations and the coverage metadata — and ends with a prohibition: "huge unbounded sets such as
all files under `/` MUST NOT be checkpointed by default".

§42.4 constrains what a checkpoint means once written: "each checkpoint inherits the coverage
quality of its sources. A checkpoint is not globally authoritative." §3.6 says the same from the
other side: "a checkpoint is not automatically complete just because it serializes many objects".

§9.1 step 1 selects "the nearest **trusted** checkpoint at or before `T`", and never says what
trusted means.

## Decision

**Three rules, all of them about what a checkpoint may not imply.**

**1. A class is checkpointed whole or not at all.** `CheckpointPolicy::default_local()` names the
state classes of §42.2 and no others; the path tree — `File` and `Directory` — is absent, which is
§14.5's refusal stated from the checkpoint's side. A class the policy does not admit, and a class
larger than the policy's per-class ceiling, is excluded entirely rather than truncated. A
truncated set carries a completeness it does not have, and §9.6 forbids a result that implies its
rows are the whole list; there is no way to write "these 3 000 of the 12 000 processes" into a
`Checkpoint` and no reason to try.

**2. What was left out is stated as coverage, not dropped.** Every exclusion adds a
`TemporalCoverage` interval for that class's `<type>.existence` capability, at the capture
instant, with completeness `unavailable`. A reconstruction from the checkpoint then composes that
into a gap and reports the class as *unknown*, which is the difference between "we did not
capture your files" and "you had no files" (§7.4). The exclusion is also returned as a typed
`ExcludedClass` so the recorder can say what it skipped without parsing the coverage back.

An edge whose end was excluded goes with it: it points at a place no reader could enter.

**3. Trusted means the sources saw something.** `is_trusted` is false for a checkpoint whose every
declared coverage interval is `unavailable` or `permission_denied` — a record that nothing could
be seen, which is not state to stand on. It is true for a checkpoint that declares no coverage at
all: that makes no claim either way, and its weakness surfaces in the composition, where a
capability nothing declared composes to `unknown`. `nearest_trusted` walks back one instant past
an untrusted candidate and asks again, up to sixty-four times, so a run of empty captures does not
turn one refusal into an unbounded scan.

Selection is otherwise the ledger's, which partitions by scope: §42.1 wants "reconstructing one
service" not to "require deserializing an entire federated environment", so a checkpoint for
another host is never a candidate for this one.

## Consequences

- A default checkpoint has a size bounded by the number of processes, services, interfaces,
  mounts and containers on one host — sets a kernel already bounds — and never by the size of a
  filesystem.
- A recorder that wants file structure in a checkpoint states a policy that admits it. That is a
  deliberate decision with a size attached, which is what §42.2 asks for.
- Reconstruction from a partial checkpoint reports partial coverage, so §42.4 holds without the
  reconstruction having to know which checkpoint it read.
- Encoded in `crates/ono-temporal-reconstruct/tests/checkpoints.rs`.

## Alternatives considered

- **Truncate a large class and set a flag.** The flag is one field away from the row count, and
  every renderer that forgets it prints a partial list as a whole one.
- **Refuse to write the checkpoint at all when a class is over the bound.** Loses the services and
  processes that were fine, to punish the class that was not.
- **Treat any checkpoint as trusted.** Makes a capture taken while every provider was denied
  outrank an older one that actually saw the machine.
- **Treat a checkpoint with no declared coverage as untrusted.** Defensible, and it would make a
  hand-written checkpoint unusable rather than weak. Composition already reports it as `unknown`,
  which is the same information without the refusal.
