# ADR-0518: A family names a case, or names the phase that owes it

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §40.1 (the real binary), §40.2 (network default), §40.3 (the fourteen
  families), §40.4 (timeouts); `docs/ACCEPTANCE.md` §3, §4.8, §4.8.13; ADR-0401
- Issues: #91
- Decided by: agent (autonomous)

## Context

§40.3 names fourteen acceptance families and `docs/ACCEPTANCE.md` §4.8.13 holds one box per
family, written before the work as §4.8 requires. Read against the tree, the checklist was ahead
of it in two different ways.

**Eight boxes named a case that does not exist under the name they used.** `182` is
`182-unknown-client-is-refused` and the box said 182-remote-unknown-client-refused — written here
without backticks, because it is a name recorded absent rather than a case anybody can run
(ADR-0401). The same held for `183`, `184`, `185`, `186`, `187`, `195` and `196`. Every one of those cases exists and passes.
The names were written from the specification's own vocabulary before the delivering increments
chose theirs, and nothing resolved them, because ADR-0401 deliberately leaves an unticked box's
references unresolved — a checklist written ahead of the work names cases that are by definition
absent, and resolving them would report a plan as a defect.

That exemption is right and it has a cost: a name can be wrong for as long as its box is unticked,
and eight of them were.

**Two cases were genuinely missing.** `180`, the mutual-TLS family, which H1 closed without
writing; and `199`, the release-provenance family, which H11 has not reached.

## Decision

**Every family resolves: to a case file, or to the phase that owes it.**
`xtask/tests/hardening_evidence.rs::should_find_a_case_for_every_one_of_the_fourteen_acceptance_families`
reads §4.8.13 and holds it to four things, none of which reads a tick:

* there are exactly fourteen boxes, and each opens with §40.3's family in §40.3's order. The
  fourteen phrases are typed into the test from the specification rather than read from the
  document being checked, because a test that read them would agree with whatever the document
  said;
* every box names a case;
* **a case that exists is named in backticks, and a case that does not is named in prose.** That
  is §4.8's own convention and ADR-0401's rule, used in both directions rather than one: a
  backticked name that resolves to nothing fails, *and* so does a plain name whose file is there.
  The second half is what would have caught the eight wrong names — a box that got its name right
  would have to backtick it;
* a family whose case is still owed says which phase owes it, and at most one may be.

Ticking stays what it was: `scripts/acceptance.sh` proving the case green. This test proves the
checklist points at something, which is the other half and the half that rots silently.

`should_find_a_finite_timeout_on_every_v041_case` covers §40.4 from both ends — the harness gives
every case a finite default and reports an expiry as a failure rather than a slow pass, and every
case numbered 170–200 states a positive budget of its own, because a case that runs a Profile M
benchmark inside a container is not a case the thirty-second default was chosen for.

`180-remote-mutual-authentication` is written here. It asserts what only the product can show:
both ends present the same fingerprint on every invocation and the client's key file is `0600`; a
peer that opens the port and speaks no TLS receives a TLS record and never an Ono protocol frame
(§13.1); an established link reports `authenticated`, `authorized` and `transport_trust: pinned`
as three separate answers (§7.3, §19.1), and its `transport_fingerprint` is the host key the
operator pinned; and a different machine answering for the same address is refused with
`Ono-Sendai-E0603` (#18, ADR-0274).

## Consequences

Easy: eight pointers that were wrong are right, and cannot go wrong again while their boxes are
ticked — and the one that is still owed says so in the document rather than in somebody's memory.

Hard: the fourteen family phrases are typed into the test, so renaming a box in §4.8.13 means
editing the test too. That is the point rather than the cost: §40.3's list is the specification's,
the specification is immutable, and a checklist that could rename a family without anything
objecting is a checklist that could lose one.

Also hard, and stated: **ten of the fourteen boxes are ticked, and three of the four that are not
are red for reasons outside this increment.** `189`, `190` and `191` — KUANG confinement,
materialization limits, result-history truncation — fail in the container today and are being
repaired in the main checkout; the fourteenth is H11's. Their pointers all resolve, which is what
this increment owes; their green is what their own repairs owe.

Encoded by: `xtask/tests/hardening_evidence.rs::should_find_a_case_for_every_one_of_the_fourteen_acceptance_families`,
`::should_find_a_finite_timeout_on_every_v041_case`, and case `180-remote-mutual-authentication`
green in `scripts/acceptance.sh`.

## Alternatives considered

**A machine-readable family registry under `docs/contracts/hardening/`.** §52.1 names seven registries
and this is not one of them, and the checklist already holds one box per family with the issues,
the sections and the scenarios each carries. A second list would be a second thing to keep in
step with §40.3, and §52.2 is about not having two.

**Resolve every backticked case reference in §4.8, ticked or not.** It would have caught the eight
names immediately and it would also report every case a checklist written before the work has
promised — which is most of them. ADR-0401 settled that trade; this rule works inside it by
asking what the *spelling* claims rather than by resolving everything.

**Write case `199` now with the assertions H11 will need.** It would fail, because the checksums,
the signature and the provenance it asserts do not exist yet. A case that cannot pass is not a
case recorded absent; it is a red gate with a plan inside it.
