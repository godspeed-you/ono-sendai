# ADR-0191: One `enter`, one refusal

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.3 (`enter`), §40 (the spatial error model), §30.2, §2.3, §20.3;
  v0.2 §14.3 (the context stack), §16, §43; AGENTS.md §5.2 (the later specification wins where
  they overlap)
- Decided by: agent (autonomous), settling a conflict two RED tests could not settle between them

## Context

`enter` is spelled two ways in this shell:

- `enter <target> <identity>` — v0.2's context stack: `enter user root`, `enter process 1842`,
  `enter service nginx.service`. Its refusal is `resolve.target_not_found`, `Ono-Sendai-E0102`.
- `enter <selector>` — v0.4 §6.3's spatial navigation: `enter compute`, `enter /mnt/backup`,
  `enter 1842`. Its refusal is `spatial.not_found`, `Ono-Sendai-E1001` (§40).

ADR-0142 already decided that **both spellings move the current place** (§30.2 applies to each),
so the two are one action reached by two grammars. Their refusals nevertheless diverged, and two
tests now disagree about which one a missing object gets:

- `identity_missing::should_refuse_to_enter_a_user_that_does_not_exist` (written for v0.2) pins
  `Ono-Sendai-E0102` for `enter user nobody-such-user-ono`;
- `spatial_identity_missing::should_distinguish_a_tombstone_from_a_place_that_never_existed`
  requires `spatial.not_found` for `enter process 4000000`, so that "it is gone" and "there is no
  such thing" are told apart (§40, §10.3).

The same command cannot honestly answer two different codes for the same condition depending on
whether the user typed a target word — and a reader who has to know which grammar they used in
order to know which error family to catch has been handed the implementation's history as an API.

## Decision

1. **A failed `enter` is `spatial.not_found` (`Ono-Sendai-E1001`), whichever grammar was used.**
   v0.4 §40 governs the refusal of a navigation that cannot resolve its place, and after ADR-0142
   every `enter` is such a navigation. Under AGENTS.md §5.2 the later specification wins where the
   two overlap, so v0.2 §14.3's `resolve.target_not_found` is superseded **for `enter` only**.
2. **`resolve.target_not_found` keeps every other job it has** — an unknown verb/target pair, a
   `set`/`remove` of a target nothing serves, a selector that names nothing for a command that is
   not navigation. This ADR narrows one command's refusal, not the family.
3. **A place visited this session and since gone stays `spatial.destination_gone`** (§10.3, §20.3,
   ADR-0179). The two conditions §40 separates are separated by *what the shell knows about the
   place*, never by the grammar the user typed.
4. **The exit status of a script is unchanged**: v0.2's rule (a script's status is its last
   statement's, ADR-0008) is not superseded by anything in v0.4, and the shell keeps reading a
   script after a failed statement unless told otherwise. A test that wants to see the refusal's
   status must read the status of *that statement*.

## Consequences

- `identity_missing::should_refuse_to_enter_a_user_that_does_not_exist` is updated in the commit
  carrying this ADR: it still requires a structured refusal that names the account and fails, and
  now names `Ono-Sendai-E1001` / `spatial.not_found`. Nothing it proves is lost; the taxonomy it
  reads is the one v0.4 fixes.
- `spatial_identity_missing::should_distinguish_a_tombstone_from_a_place_that_never_existed` is
  corrected on the status half: it reads the status of the refused `enter` rather than of a script
  that deliberately continues past it. Both of its substantive claims — `spatial.not_found` for
  what never existed, and never that code for a tombstone — stay verbatim, and the continuation is
  what `spatial_storage_missing::should_refuse_a_path_that_does_not_exist_with_a_structured_error`
  independently requires.
- `docs/contracts/errors.yaml` gains nothing: both codes exist. What changes is which one `enter` emits.
- A script that catches `resolve.target_not_found` around an `enter` must catch
  `spatial.not_found` instead. That is a visible change for v0.2 users, and it is the kind §5.2
  anticipates: v0.4 makes `enter` a spatial verb, and its failures belong to the spatial family.

## Alternatives considered

- **Keep both, split by grammar** — `enter user x` → E0102, `enter x` → E1001. Rejected: the
  condition is identical, so the code would report which syntax was used rather than what
  happened; and the tombstone case already crosses the line, because `enter process <dead pid>`
  answers `spatial.destination_gone` under either grammar.
- **Make every `enter` answer `resolve.target_not_found`** and drop `spatial.not_found` from §40's
  fourteen. Rejected: §40 is normative, and the spatial family exists precisely so a script can
  tell "no such place" from "no such command".
- **Let a failed `enter` abort the script**, which would make the disputed status assertion true as
  written. Rejected: it contradicts v0.2 §16.4 and the existing test that pins continuation, and it
  would make navigation the only statement in the language that ends a script.
