# ADR-1066: `why <target> <selector>` resolves its target before it asks

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.5 §14.1, §16.2, §16.3, §16.8, §34; v0.4 §27.1, §40
- Issues: #148
- Decided by: agent (autonomous)

## Context

§16.2's first form names a target — `why service nginx`, `why process 2741` — and the
explanation is about that target (§16.3: "the most recent causally explainable notable state
transition relevant to the service"). The command built the request from the session's
*current place* and only borrowed the last word as a label, so `why process 1`, `why process 9`
and `why link testbox` all answered with the same subject — the place the session stood in,
`ono:stable:241accd8…` at the root. A target that did not exist was "explained" as the current
place too.

## Decision

- The words of the first form are a v0.4 selector, `<target>/<selector>` — the spelling the
  trail, the map and `enter` print (`process/1`, `service/nginx.service`); a single word is a
  selector as it stands. It is resolved by `spatial::commands::resolved_place`, the one
  resolution `enter` uses: the current host's index first, then the provider that owns the key,
  and at a historical coordinate the reconstructed world (§14.1) without asking any provider.
- The request is built from the identity that resolved, and labelled with that place's concise
  path, so the explanation's `subject` is the target's spatial identity.
- A selector that does not resolve is refused with that resolution's own §40 refusal —
  `spatial.not_found`, or `spatial.ambiguous_selector` with the candidates — and nothing is
  explained.
- The target is resolved before the temporal state is taken, so `why` holds the spatial and the
  temporal state one after the other rather than one inside the other.

## Consequences

- `why process 1` and `why process 9` now answer about two different subjects; `why process
  999999999` is `spatial.not_found`.
- Two tests and one conformance example asked `why service nginx` on a host that need not run
  nginx and passed only because the target was never looked at. They now ask about a target the
  test owns or that every Linux host has (`process/1`); `docs/contracts/commands/temporal.yaml`'s
  example list is unchanged, and the conformance register says the example needs nginx.
- Tests: `crates/ono-cli/tests/why.rs` —
  `should_explain_the_named_target_when_two_targets_are_asked_about`,
  `should_refuse_a_target_that_does_not_resolve_rather_than_explain_the_current_place`; acceptance
  case 426.

## Alternatives considered

- **Resolve the last word alone, as a name.** `why process 1` would then look for anything called
  `1`; the target word is what makes the selector unambiguous (§27.2), and it is what §16.2
  writes.
- **Explain the current place when the target does not resolve.** That is the defect: an
  explanation of something other than what was asked.
