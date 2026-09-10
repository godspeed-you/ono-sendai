# ADR-0830: Apply confirms only when gated, a copy is verified by its bytes, and the session states its real authority

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §14, §19.4, §23.1, §24.5, §25.1, §40.1, §40.3, §41.3, §43.3, §55; spec v0.2 §11.6
- Decided by: agent (autonomous)

## Context

The final audit found six places where the contract, the view or the executor described a
different command than the one that ran: `apply`'s contract said `confirmation: always` while
§40.1 keeps an ungated plan one-step; a copy was verified by `exists`, which says nothing about
the bytes; `verify` exited 0 whatever its checks said; the shell handed the executor
`Authority::full()` and treated `conditional` privilege as elevated; a resumed recovery skipped the
§24.5 gate; and optional protection that did not come to be disappeared without a word.

## Decision

1. **`confirmation: gated`.** `ono-command`'s `Confirmation` gains `Gated`: `--confirm` is needed
   only where the run carries a gate, and the command enforces it because only it knows what its
   plan is gated on. `apply` declares it; `protect`, `resume plan` and `remove recovery` stay
   `always`.
2. **A copy is verified by the source's digest.** `actions.yaml` writes the check as
   `sha256 == {digest:source}`; `{digest:<argument>}` is the SHA-256 of that file when the plan is
   sealed. Where there is no single file to hash — a recursive copy — the check falls back to
   `exists`, which is what can still be answered. The compensation says what is true: removing the
   copy undoes it only where the destination did not exist.
3. **`verify` answers with its results.** §14 and §55 keep verification independent of exit
   status. `verify` returns every check's result and records the verdict as the plan's state — only
   over an earlier verdict, so checking a plan that never applied changes nothing about it. It does
   not replace its results with an error: that would take the evidence away from the script that
   asked.
4. **The session states what it holds.** `Authority::for_session(uid, CapEff)` reads
   `/proc/self/status`; elevation is `CAP_SYS_ADMIN` in the effective set, or uid 0 where the set
   cannot be read. An action is privileged up front only where its contract says `elevated`;
   `conditional` is decided by the provider when it acts.
5. **A resumed recovery passes the same gate.** `resume plan` on a recovery plan re-analyses newer
   state, applies §24.5's gate and routes every action through the owning provider, exactly as
   `apply` does (ADR-0822); `resume plan` declares `--accept-newer-state-loss` for it.
6. **A shortfall is said.** Optional protection that failed to be created or to validate is a
   `ProtectionShortfall`, printed as a note by `apply` and `protect`; an invalid optional asset is
   recorded as what it is and never counted as protection.

## Consequences

Tests: `crates/ono-command/tests/confirmation.rs`, `crates/ono-cli/tests/change_revalidate.rs`
(verify keeps its UNKNOWN results), `crates/ono-change-executor/tests/protection_and_claims.rs`,
`crates/ono-change-executor/tests/resume.rs`.

## Alternatives considered

- *`verify` fails its command on a failed check.* Rejected: `Outcome` carries values or a failure,
  not both, and §14 separates the verdict from the exit status.
- *Leave `apply` at `always` and demand `--confirm` everywhere.* Rejected: §40.1 — a gate on every
  plan is a gate nobody reads.
