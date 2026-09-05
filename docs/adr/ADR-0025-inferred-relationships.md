# ADR-0025: A hostname from reverse DNS is an inferred relationship

- Status: accepted
- Date: 2026-08-26
- Spec refs: §22.1, §22.2, §31.25, §49
- Decided by: agent (autonomous)

## Context

Spec §22.2 requires that exact relationships and inferred ones never be confused, and states the
reason: "The UI must not visually imply certainty that the provider does not possess." It then
classifies four examples, and one of them is a hostname obtained by reverse DNS, which it places
on the *exact* side:

> socket -> remote hostname from reverse DNS: derived but exact mapping at query time

Implementing `trace socket` forced a decision about whether to follow that classification.

## Decision

An edge from a socket to a hostname obtained by reverse DNS is **`Confidence::Inferred`**, and it
carries the address it was derived from and the resolver that answered.

## Spec deviation

- Section: spec §22.2
- Text: "socket -> remote hostname from reverse DNS: derived but exact mapping at query time"
- Instead: the edge is inferred, not exact, and carries its evidence.
- Why: every other exact relationship in §22.2 is an observation of *this machine's own state* —
  a descriptor in `/proc/<pid>/fd`, a socket inode, a cgroup membership. The kernel is the
  authority on all of them and cannot be wrong about them in the sense that matters.

  A PTR record is not an observation. It is a claim, made by whoever controls the reverse zone
  for that address, delivered by a resolver Ono did not choose, and it can be stale, absent,
  arbitrary, or chosen by the operator of the machine on the other end of the connection. Spec
  §49 already treats remote-supplied data as hostile in every other place it appears; a
  relationship graph is not the one place it stops being so.

  The sentence's own reasoning is what decides it. "Exact mapping at query time" is true and is
  about the *lookup* — the resolver really did say that. But the edge a user reads does not say
  "this resolver returned this name"; it says *this socket is connected to that host*, and that
  is precisely the certainty the provider does not possess. Drawing it the same way as a
  descriptor read from `/proc` would visually imply it.

  The cost of the deviation is one marker on one kind of edge. The cost of following the spec here
  would be a graph in which an attacker-controlled string is indistinguishable from a kernel fact.

## Consequences

Easy: `trace connection` shows a hostname, marked `+~~` in the ASCII drawing and carrying its
evidence, so a reader can see both the name and that it is a name someone else chose. The
evidence requirement matches what spec §31.25 demands of a plugin's findings, so a KUANG/11
contributor and a core provider are held to the same standard.

Hard: a user who wanted the hostname treated as fact has to say so. There is no way to promote an
inferred edge — `Edge::exact` and `Edge::inferred` are the only constructors and confidence is
never written afterwards — which is deliberate, because a promotion mechanism is a mechanism for
losing the distinction.

Encoded by: `crates/ono-graph/tests/` — the inferred edge carries its evidence, nothing promotes
it, and an exact edge beside an inferred one over the same pair stays two edges rather than
merging.

## Alternatives considered

- **Following §22.2 and marking it exact** — rejected above.
- **Omitting the relationship entirely** — rejected: the hostname is genuinely useful, and
  refusing to show it would push people back to `dig` and to reading it with no marker at all.
- **A third confidence level between exact and inferred** — rejected as speculative generality
  (AGENTS.md §4). Two levels are what spec §22.2 defines and what a reader can act on; a third
  would need a rule for what to do with it that nothing yet requires.
