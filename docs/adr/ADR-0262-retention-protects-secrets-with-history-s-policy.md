# ADR-0262: The retention protects secrets with history's own policy

- Status: accepted
- Date: 2026-08-29
- Spec refs: §17.5, §20.2, §34; ADR-0033
- Decided by: agent (autonomous, `close-data`)

## Context

Spec §20.2 says it in one sentence: "Retention policy must protect secrets and potentially large
values." The large half was done — a retained result is bounded by result count and by values per
result, and says when it was truncated. The secret half was not.

Spec §17.5 requires that secrets not reach history through renderer output, and `ono-history`
implements it: a `Policy` with a default pattern list that replaces the *value* of
`--password=…`, `--token …`, `SECRET_KEY=…` and their spellings while leaving the surrounding
text readable, pinned by `ono-history/tests/history.rs`. Nothing applied it to the structured
results ADR-0033 retains for `@`, `@-1` and `@N`, so

```text
$ ono -c 'from json; @-1 | to json'   # a row holding `psql --password=hunter2`
```

replayed the secret in full. The shell would redact the command that read a token and keep the
token.

## Decision

**The retention applies history's policy to what it keeps.** `Session::retain_result` rewrites
every text leaf of every retained value through `ono_history::Policy::redact` — the same
patterns, the same replacement, one policy — so what `@-1` replays has no secret in it either.

`Value::map_text` is the walk: it reaches a string inside a record inside a list, keeps schema,
provenance and field positions, and returns the value unchanged where nothing matched. It is a
general utility with one caller today; the alternative was for the session to rebuild a record
from parts it cannot see.

Redaction happens **when the result is retained**, not when it is read. What is kept is already
clean, so every later reader — `@-1`, `@N`, `enter @-1`, a future on-disk retention — is covered
by the one decision rather than by remembering to ask.

## Consequences

- `@-1` replays `psql --password=<redacted>`: the command stays readable and only the value is
  gone, exactly as in history.
- Ordinary text is untouched. A redaction that fires on ordinary text teaches people to turn it
  off, which is the reasoning the default pattern list was chosen by.
- The cost is one pass over a retained result, bounded by the 10 000 values retention already
  keeps, and only over its text leaves. Spec §34's budgets are per-interaction and this is well
  inside them.
- Where a secret is *rendered* it is still shown: the redaction protects what is kept, not what
  the user asked to see. Spec §17.5's semantic secret type, with a redacted default rendering and
  an explicit reveal, is a separate and larger thing that this does not claim to be.

## Alternatives considered

- **Redact when `@-1` is read.** Rejected: it leaves the secret in memory to be found by the next
  reader that forgets, and an on-disk retention would keep it.
- **A second pattern list for values.** Rejected: two lists drift, and the one that mattered
  would be the one nobody configured.
