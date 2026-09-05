# ADR-0054: `raw` and `adapt` are stage keywords

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.17, §1.18, §1.53; v0.2 §6.5; ADR-0011, ADR-0027
- Decided by: agent (autonomous)

## Context

Spec v0.3 §1.17 requires "at least one concise raw path" and offers `raw ss -tunap` and
`exec:ss -tunap` as candidate forms; §1.18 wants a way to force adaptation and offers
`adapt ss -tunap` and `ono:ss -tunap`. ADR-0011 already gave `exec:` and `ono:` meanings —
*where a name resolves*, external-only and native-only — and ADR-0027 noted that both of
v0.3's namespace spellings are therefore taken and the two semantics need keywords.

## Decision

1. **`raw <program> [args…]`** is a stage keyword. The stage runs `<program>` resolved on
   `PATH` only (ADR-0011 step 5), with the arguments exactly as typed: no argv rewrite, no
   decoder, no Ono renderer, stdout and stderr as ordinary external streams, the program's own
   exit status. It is external wherever it stands — `get process | raw wc -l` feeds the
   rendered bytes to `/usr/bin/wc` even though structure arrives, where `wc` alone might have
   been a transform. `raw` with nothing after it is `resolve.command_not_found` with help;
   `raw get process` is the same error, because `get` is not a program.

2. **`adapt <program> [args…]`** is the forced-adaptation keyword of §1.18 (spelled here,
   implemented with ADAPT-002/004): the stage must be adapted, and fails with
   `adapter.required_for_structured_pipeline` or the specific `adapter.*` error rather than
   downgrade to text.

3. `exec:` and `ono:` keep their ADR-0011 meanings. Resolution and output semantics are
   orthogonal, and they get orthogonal spellings: `exec:` says *the program, not a native
   command of the same name* and leaves adaptation to the demand; `raw` says *bytes, whatever
   the demand* and implies the program.

4. Both keywords win over a program of the same name, as `explain` does; `exec:raw` runs a
   program called `raw`.

5. `explain` shows the bypass: the plan's stage carries an `adaptation` row reading
   `` bypassed (`raw`, spec v0.3 §1.17) `` and the demand row reads
   `` bytes (`raw` bypasses adaptation) ``. `help raw` documents the keyword alongside the
   shell's other keywords.

## Consequences

- A script can state its intent in either direction and be read by someone who knows only
  Unix: `raw` is a word, not a sigil.
- Completion after `raw` and `adapt` must offer programs; that lands with the completion
  increment of the tranche (v0.3 §1.59).
- Tests: `ono-cli/tests/external.rs` (the `raw` cases), `ono-cli/tests/builtins.rs` (`explain`
  and `help`), acceptance case `072`.

## Alternatives considered

- Redefining `exec:` as raw — rejected: it would silently change ADR-0011's meaning for every
  existing script, and there would be no way to say "the program, adapted".
- A `--raw` option — rejected: options belong to the program after `raw`; the shell must not
  eat one.
