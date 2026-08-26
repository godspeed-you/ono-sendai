# ADR-0004: Implementation runs on a disposable feature branch

- Status: accepted
- Date: 2026-08-26
- Spec refs: none — process decision
- Decided by: user instruction, mechanism chosen by the agent

## Context

The implementation is carried out by an autonomous fleet over a long, unattended run. Such a run
can go wrong in ways that are only visible much later: an early architectural decision that
poisons everything built on top of it, a misreading of the spec that propagates, or simply a
result the user does not want. The user needs to be able to throw the whole attempt away and
start again from a known-good state, without archaeology and without losing the specification,
the instructions or the verification harness.

That is only true if the starting state and the attempt live in different places.

## Decision

- `main` holds the specification, `AGENTS.md`, the harness and the README. **No agent writes to
  it.**
- All implementation happens on **`implementation`**, branched from `main`. Parallel agents may
  use `implementation/<crate>` sub-branches, merged back into `implementation`.
- Agents never merge `implementation` into `main`, and never delete or recreate the branch.
  Promoting and discarding are the user's actions.
- `scripts/gate.sh` **refuses to run on `main`**, with `ONO_ALLOW_MAIN=1` as the escape hatch for
  the user working on the harness itself. Since the gate runs on every increment, an agent that
  forgets the policy is stopped at its first attempt to verify work rather than after fifty
  commits.
- CI sets `ONO_ALLOW_MAIN=1`: it verifies whatever branch it is handed, and the policy is
  enforced where commits are actually made.

## Consequences

- Discarding a run is `git branch -D implementation` plus deleting the remote branch. `main` is
  untouched by construction, so "start over" costs nothing and needs no cleanup.
- Several attempts can coexist for comparison if the user wants them, by branching
  `implementation-2` from `main`.
- `main` and `implementation` diverge for the whole run. That is intended: `main` is not a
  development branch here, it is the baseline.
- The user must merge deliberately at the end. Nothing lands in `main` by accident, which is the
  point.

## Alternatives considered

- **Work directly on `main`** — rejected: it is the requirement being solved. A bad run would be
  entangled with the baseline and could only be undone by rewriting history.
- **A branch per phase** — rejected: phases build on each other, so the branches would be a
  chain, and discarding phase C would mean discarding D through J anyway. One disposable branch
  expresses that honestly.
- **Document the rule without a guard** — rejected for the same reason as ADR-0003: in a long
  autonomous run, an unenforced rule is a rule that will eventually be broken silently.
