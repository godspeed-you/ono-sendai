# ADR-0462: A limit must be settable before it can be proven

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §55.1, §57 (phase H5), §60.4, §60.5, §60.6, Appendix A; AGENTS.md §7, §9, §11
- Decided by: agent (autonomous)

## Context

§57 sequences H5's deliverables — estimator, budget type, materialization helper, history byte
limits, "configuration defaults and generated docs" — and the milestone listed the issues in the
same order, with `limits.*` configuration (#74, P2) after the four P1 items it configures.

That order is right for the *primitives* and wrong for their *proofs*. §60.4's scenario is 100 001
values and §60.5's is more than 128 MiB, and the acceptance criterion of every H5 issue is a
user-visible refusal. A test that reached Appendix A's defaults with real data would allocate
128 MiB inside `cargo test` to prove a byte ceiling — which is a test nobody runs, and therefore a
proof of nothing (ADR-0428's subject in a different costume). The only other way to reach a
ceiling is to lower it, and lowering it is what #74 is.

## Decision

**The configuration keys landed before the CLI-level proofs of #67, #70, #71, #72 and #120, and
the increments are still one issue each.**

The order that was implemented:

| # | Issue | What it delivered | Why here |
| --- | --- | --- | --- |
| 1 | #65 | the estimator | nothing depends on |
| 2 | #66 | `Budget`, the three error codes, `ErrorKind::Resource` | needs the estimator |
| 3 | #67 | `materialize`, and the budget inside every blocking transform | needs `Budget` |
| 4 | #68 | the §54.1 refusal shape, Appendix E as a contract | needs the classification |
| 5 | **#74** | `limits.*`, ranges, the registry | **moved forward: everything below is proven through it** |
| 6 | #70 | the per-command capture ceiling | needs `limits.command_capture_bytes` |
| 7 | #72 | `ResultHistory` | needs `limits.history_*` |
| 8 | #71 | cancellation stops capture growth | needs #70's capture |
| 9 | #73 | the error contract, pinned | needs codes to pin |
| 10 | #69 | `explain` | needs #68's classification and #74's limits |
| 11 | #120 | `inspect limits` | needs #74's catalogue |

Two things this does **not** change. The error codes of #73 land with #66, because #66's
`Budget::charge` cannot compile without them and AGENTS.md §7 forbids production code without a
failing test for it — #73's own increment is the contract tests that pin the codes and the
detail discipline, which is what its exit criterion asks for. And the estimator stays first,
because §21.2's figure is what every other item spends.

Every proof below step 5 narrows a ceiling through `limits.*` and asserts the refusal a user
sees. That the *defaults* are Appendix A's is asserted separately —
`meta_config.rs::should_accept_every_documented_limits_key_and_reject_an_unknown_one` against the
catalogue, `resource_limits.rs::should_answer_the_same_figures_inspect_limits_shows_from_the_contract_registry`
against `docs/spec/hardening/limits.yaml`, and case `192` against the running binary — so nothing
is proven only under a lowered ceiling.

## Consequences

Easy: no test allocates a hundred megabytes, and every refusal in H5 is proven at the boundary a
user meets it.

Hard: #74 is P2 and now blocks five P1 issues' proofs, so a decision to drop it would strand them.
It should not be droppable, and §55.2's binding sentence — a security-sensitive limit must not
silently become unlimited — is a P1 property wearing a P2 issue number.

Also hard: a lowered ceiling proves the mechanism, not the number. A regression that changed
`limits.materialize_bytes`'s default from 128 MiB to 128 KiB would keep every ceiling test green,
and would be caught by the three default assertions named above. Those three are the reason the
lowered-ceiling proofs are honest, and they are worth more than they look.

## Alternatives considered

**Implementing #74 last, as the milestone listed it.** The five proofs above it would have had to
reach Appendix A's defaults with real data — 100 001 values is tolerable and 128 MiB is not — or
be written against internal APIs rather than against the shell, which AGENTS.md §11 rules out for
an outcome test.

**Exposing a test-only environment variable to lower the ceilings.** It would have kept the issue
order and added a second configuration path that only tests use, which is the "new
security-sensitive environment variable" §55.4 asks to avoid and a surface no user benefits from.
The declarative keys were owed anyway.

**Proving the ceilings only in `ono-pipeline`'s unit tests, where a `Budget` can be built with any
figure.** They are there and they are green, and they cannot answer #67's actual question, which
is whether *every* global collection in the shell goes through the helper. That question is only
answerable from outside the process.
