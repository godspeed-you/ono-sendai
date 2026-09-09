# ADR-0786: `out_of_retention` is a fact about what a store held, not about what its policy allows

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §12.3, §34, §35.3, §55.5, §10.4
- Decided by: agent (autonomous)

## Context

§34 gives two refusals for an instant nothing can answer about, and words them differently:

| Code | Name | §34's meaning |
|---|---|---|
| E1102 | `temporal.not_recorded` | "No evidence source covers the requested time/scope." |
| E1103 | `temporal.out_of_retention` | "Requested history is **known to have expired**." |

`crates/ono-cli/src/temporal/coordinate.rs::unreachable` chose between them by comparing the
requested instant against the policy horizon alone — `now - temporal.retention.max_age`. Anything
below it was reported as expired.

That is wrong for the commonest shell there is: one whose recorder started a few minutes ago. Its
`max_age` is 24h and its retained record begins minutes ago, so `at -3d` fell below the horizon and
answered

```text
Ono-Sendai-E1303 temporal.out_of_retention 2026-09-06T… is older than the retained history,
which starts at 2026-09-09T…
```

Nothing expired. Three days ago was never inside that store's window, and saying it "is known to
have expired" is the fabricated certainty §35.3 exists against — the shell asserting a fact about
data it never had. §12.3's own worked example is precisely this shell asking `at -3d`, and the
answer it prints there is `temporal.not_recorded` with a list of how far each source reaches.

Found by `docker/acceptance/cases/238-timeline-gaps.case`, whose `t6am` asked a fresh home for
`at -3d` and expected §12.3's refusal.

## Decision

**An instant is `temporal.out_of_retention` only where the store demonstrably swept it: the
retained record must begin at or before the policy horizon.**

```text
out_of_retention  ⟺  at < now - max_age  ∧  earliest ≤ now - max_age
```

The second conjunct is the whole decision. It says the store has been keeping history for longer
than it keeps history, which is the only circumstance in which the policy has discarded anything.
Until then an unreachable instant is `temporal.not_recorded`, and §12.3's availability list — what
the session ledger reaches, what the recorder reaches — is what the reader gets instead of a
boundary that would be a guess.

`changes` shares the helper, so both commands give the same reason for the same instant.

## Consequences

Easy: the refusal a reader sees now matches what the shell can actually prove, and the two codes
stop being interchangeable. A user whose recorder is young is told what *is* reachable rather than
being told their data expired.

Hard: `temporal.out_of_retention` is now unreachable in a container that cannot age a store past
its own retention window, which is why its proof lives where a store's expiry can be constructed —
`crates/ono-temporal-ledger/tests/retention.rs` and `docker/acceptance/cases/244-retention-boundary.case`,
the two the §4.11.4 box names. Case 238 proves the other side: a young store never claims expiry.

Also hard: a store that was stopped for a week and restarted has an `earliest` from before the
horizon and will report expiry for an instant inside the stop — which is true (the policy did sweep
that stretch) even though the deeper reason is that nothing was recording. §44.1's gap marking is
what tells the reader which it was.

## Alternatives considered

**Record the store's creation instant and compare against it.** Strictly more precise: an instant
before creation was never recorded, one after it and before `earliest` expired. It needs a new
persisted fact and a migration, and `earliest ≤ horizon` already separates the two cases that
matter without one. Revisit if a case appears where the difference is visible.

**Keep the policy-horizon rule and soften the message.** "May have expired" is not one of §34's two
answers, and a refusal that hedges is one a pipeline cannot branch on.
