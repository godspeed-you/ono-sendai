# ADR-0988: A field a tool matches on by text is read only in the texts it matches

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.3 §1.8, §1.20; v0.4.1 §36.2; v0.2 §35.3; ADR-0980
- Decided by: agent (autonomous)

## Context

ADR-0980 hands `journalctl --priority` the comparisons a `where` makes on `priority`, and states
one bound of the equivalence: an entry that carries no `PRIORITY`, "or one outside the canonical
digits", violates `ono.journal-event/1` and is reported as `Ono-Sendai-E0908` when it is read,
while journald's index never returns it. The second half was not true. The adapter read
`PRIORITY` like every integer field: `text.trim().parse::<i128>()`, so `03`, ` 3` and `+2` were
read as 3 and 2, `-1` and `8` as numbers outside the syslog range — and none was reported.
journald keeps an entry under `--priority=N` only when its stored field is exactly one of the
digits `0` … `N`. So `get journal | where priority <= 3` kept an entry stored as `PRIORITY=03`
without the handover and lost it, silently, with it. The release review of v0.6.4 found it; the
STATE entry "a journal `PRIORITY` outside 0–7 is accepted" is the same defect seen from the
schema's side.

## Decision

An adapter field mapping may close the set of texts the tool's value may be:

```yaml
priority: {from: PRIORITY, accept: ["0", "1", "2", "3", "4", "5", "6", "7"]}
```

`accept` is compared with the value as the tool wrote it, before any other step of the mapping —
before a translation, a unit, or reading it as a number. Any other value is a schema violation
of the field, reported as `Ono-Sendai-E0908` like every other; an absent value is still absent
and handled as before. The journal's `PRIORITY` declares the eight syslog digits, which makes
ADR-0980's stated bound true: an entry the handover lets journald withhold is exactly an entry
that reading would have reported.

The rule for a future declaration is the one ADR-0980 already wrote: a provider that hands its
source a comparison on a field states the bound of the equivalence, and where the source matches
on the field's text, the adapter accepts only the texts the source matches.

## Consequences

- A journal entry whose `PRIORITY` is not one of `0` … `7` written as one digit is reported as
  `Ono-Sendai-E0908` where v0.6.3 read it as a number; with a priority ceiling handed over it is
  left out, as ADR-0980 says. `docs/MIGRATION.md` §14 states it.
- `accept` is part of the adapter pack format (`docs/contracts/adapters/schema.yaml`), so a pack
  author can close any field a tool matches on by text.
- Proven by `predicate_pushdown.rs::should_report_an_entry_whose_priority_is_not_one_canonical_digit_rather_than_read_it_as_one`
  (`03`, `+2`, `-1`, ` 3`, `8`), which failed first.

## Alternatives considered

- **Read every integer strictly in every adapter** (no sign, no padding, no surrounding space).
  Rejected: other tools pad their numbers and nothing matches on their text, so the change would
  turn working decodes into refusals for no gain.
- **Withdraw the journal's pushdown declaration.** Rejected: the equivalence holds once the
  decode is strict, and the handover is the measured gain of #281.
- **Translate non-canonical texts to the number they spell.** Rejected: journald does not, so
  the two paths would still differ.
