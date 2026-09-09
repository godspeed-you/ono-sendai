# ADR-0800: The v0.6 vocabulary is one crate, and §2's invariants are properties of its types

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §2, §3, §50, §50.1; ADR-0001, ADR-0005
- Decided by: agent (autonomous)

## Context

v0.6 §50 sketches a reference architecture of nine crates and gives one of them, `ono-change-core`,
"canonical plan/action/effect/risk types". §50.1 adds four dependency rules: core types do not call
providers, renderers do not mutate state, recovery providers do not own plan orchestration, and
`ono-cli` wires components without implementing plan semantics.

That leaves open where §2's eighteen invariants live. They can live in prose, in review, in a
linter, or in the shape of the types. The tranche is large enough — ten crates, several written in
parallel — that "in review" is not a real answer: an invariant that only holds because everybody
remembered it is an invariant with a half-life.

Three of them are the ones that decide the question, because each is a thing a reasonable
implementation does by accident:

- **§2.3 and §4.6.** A protection level that can be *assigned* will eventually be assigned
  optimistically, at the point in the code where the author knows what they meant.
- **§2.4 and §8.1.** A confidence lattice with both a `weakest_of` and a `strongest_of` will
  eventually have the second one called, because there is always a place where combining two
  statements feels like it should strengthen the result.
- **§2.17 and §12.3.** An execution model with a `command: String` field will eventually hold an
  interpolated one, whatever the comment above it says.

## Decision

The vocabulary is one crate, `ono-change-core`, and every invariant §2 states that *can* be a
property of a type is one.

Concretely, and each of these is the whole reason the type is shaped as it is:

| Invariant | How the type enforces it |
|---|---|
| §2.3, §4.6 — a level never overstates coverage | `ProtectionSummary::level()` computes §10.2's word from the per-domain matrix (Appendix A.5, A.7). There is no setter and no constructor that takes one. |
| §2.1 — planning is side-effect free | The crate performs no I/O and depends on nothing that does. A `RecoveryAsset` a plan mentions is `AssetState::Proposed`. |
| §4.4 — a sealed plan is immutable | `ChangePlan::seal` consumes the plan; every mutator is on the draft. `revise()` returns a new revision and leaves the original in the caller's hands. |
| §23.1 — a mutating plan carries verification | `seal` refuses otherwise, so an unverifiable plan never reaches a state `apply` accepts. |
| §2.4, §1.3 — unknown is never promoted | `EffectConfidence::weakest_of` and `ConsistencyClass::weakest_of` have no counterpart. |
| §2.17, §12.3, §43.6 — no interpolated shell strings | `Execution` has no variant holding a command line; `ToolRunner` takes a program and an argument vector. |
| §24.5, §62.8 — recovery is gated on its own destructiveness | `NewerStateImpact::unanalysed()` answers `requires_destructive_acceptance() == true`. Forgetting to look is not a way through. |
| §32.4, Appendix B.7 — pseudo-filesystems are never persistent | `FilesystemKind::is_persistent` answers `false` for each of them. |
| §4.1 — the lifecycle is a machine | `PlanState::after` is a total function returning `None` for an edge §4.1 does not draw. |

`ono-change-core` therefore depends on `ono-core`, `ono-value`, `jiff` and `sha2`, and on nothing
else. It was briefly given `ono-provider-api` and `ono-spatial-core`; neither was used, and both
were removed, because a vocabulary crate that can see a provider is one somebody will eventually
call one from.

The other nine crates of §50 are created as §50 names them, with one addition and one placement
worth recording. The addition is that persistence-domain resolution (Appendix B) lives in
`ono-change-protection` rather than in a crate of its own: it is the input to the coverage
algorithm and has no other consumer. The placement is that `ono-change-protection` sits in the
`surface` layer of `docs/contracts/hardening/module_architecture.yaml` rather than `capability`,
because it reads `/proc/self/mountinfo` through `ono-provider-linux` and a crate that touches the
machine belongs beside the crates that touch the machine.

## Consequences

What becomes easy: an agent or a reviewer working on one of the nine crates cannot violate the
listed invariants without editing `ono-change-core`, which is a visible, reviewable act rather than
an oversight. The 149 unit tests in the core are the invariants' proofs, and they run in under a
hundredth of a second because nothing in the crate touches the world.

What becomes hard: any of these invariants that turns out to need an exception needs a type change
rather than a flag. That is the intended cost. §60's decision table closes most of the questions
where an exception would be argued for.

What must be revisited: if a KUANG/11 provider ever needs to contribute a protection level directly
rather than a domain row, `ProtectionSummary::level()`'s lack of a setter becomes the constraint to
argue with, and §49.3 is the answer — a model or a plugin may not establish recovery coverage.

## Alternatives considered

**Invariants as a lint or an `xtask` check.** Rejected for the three listed above, kept for the
ones a type cannot carry: `xtask/src/change.rs` checks that the registries and the machine agree,
which is a different question and one no type can answer.

**A thinner core with the invariants in the executor.** Rejected because the executor is one of
six consumers, and an invariant enforced at one consumer is not enforced.
