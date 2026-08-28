# ADR-0144: An option whose value is optional

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.1, §6.2, §24.3, §47; v0.2 §7, §15.2; ADR-0009
- Decided by: agent (autonomous)

## Context

§6.1 writes `look --changes [duration]` and §6.2 writes `near --changed [duration]`. The square
brackets are the spec's own: the duration is optional, and §47's `spatial.look.change_window = "5m"`
is what the option means without one.

The binder had two kinds of option. A `bool` is satisfied by its presence and consumes no word;
anything else declares a type, which is a promise of a value, and a missing one is a usage error.
Neither spelling is `[duration]`: declaring `changes` as `bool` makes `look --changes 10s` bind the
`10s` as a selector `look` does not have, and declaring it as `duration` makes bare `look --changes`
a usage error the spec does not sanction.

## Decision

A parameter in `docs/spec/commands/*.yaml` may declare `optional_value: true`. Such an option takes
the next word **when that word is a value of its declared type**, and otherwise means "present,
without a value" — which reaches the implementation as `Value::Bool(true)`, and which the
implementation reads as the configured default.

The word is tried rather than assumed, so `near --changed 10s` reads the window and
`near --changed socket` reads the relation and leaves the window to `spatial.look.change_window`.
That is what makes the value genuinely optional rather than merely last.

Every other option keeps the old rule, because the marker defaults to false: a declared type is
still a promise of a value, and a missing one is still a usage error.

## Consequences

- `help` and completion describe the option from the same declaration, so the optional value is
  documented wherever the option is.
- Two options use it today, both from §6: `look --changes` and `near --changed`. A third would be a
  reason to look again at whether it is the right general shape.
- A defect found on the way and fixed in its own commit: a bare flag followed by another option was
  silently dropped, because the flag waiting for a possible `true`/`false` word was overwritten by
  the next option rather than decided. `get dir --all --recursive` set only `--recursive`. Test:
  `ono-command::binding::should_bind_both_flags_when_one_bare_flag_follows_another`.

## Alternatives considered

- **`--changes=10s` only** — rejected: the spec writes `look --changes 10s`, and a spelling the
  documented example cannot use is not the spelling.
- **A second option (`--changes --window 10s`)** — rejected: §6 fixes the surface, and inventing a
  companion option is a different command from the one specified.
- **A `Spec deviation` recording that the duration is required** — rejected: the feature is twenty
  lines of the binder, and a deviation is for what cannot be built, not for what is inconvenient.
