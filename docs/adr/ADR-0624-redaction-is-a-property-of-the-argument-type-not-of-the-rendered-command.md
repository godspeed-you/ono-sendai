# ADR-0624: Redaction is a property of the argument type, not of the rendered command

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §17.4, §17.5, §30.1, §30.3, §30.6; v0.2 §11.5; INTERFACES §5.2
- Decided by: agent (autonomous)

## Context

§17.5: "command recording MUST use semantic redaction. If a command includes a secret typed as
`Secret`, the persisted summary records `<secret:redacted>` rather than the raw value." §30.3 is
blunter: no secret values, ever, in the ledger.

The obvious implementation is a function from a rendered command line to a redacted one, driven by
patterns. It has a failure mode that matters: the raw text exists, in memory, in a variable, one
mistake away from being persisted — and every new call site is a new chance to persist the input
instead of the output.

There is also no `Secret` value type in this tree, and INTERFACES §5.2 confirms it: redaction is
`Value::map_text` plus a policy, with `ono_history::policy` as the capture-group precedent.

## Decision

**`RedactedCommandSummary` can only be built from typed parts.** Its one constructor is

```rust
RedactedCommandSummary::of(verb: &str, target: Option<&str>, arguments: &[Redactable])
```

and `Redactable` is `Plain`, `Option { name, value }` or `Secret { name }`. There is no
`from_str`, no `From<String>`, no way to hand it a rendered command line. The raw text of a secret
never enters the type, because `Redactable::secret(name, value)` takes the value and drops it,
keeping only the name.

Two safety nets sit under the explicit form, because a caller who forgets to mark an argument is
the case worth defending against:

- an `Option` whose **name** is secret-shaped is redacted whatever the caller said;
- a `Plain` argument of the form `name=value` whose name is secret-shaped is redacted the same way.

Secret-shaped means the name contains one of `password`, `passwd`, `passphrase`, `secret`,
`token`, `credential`, `apikey`, `api-key`, `api_key`, `key` or `auth`, case-insensitively and
ignoring leading dashes. `key` catches `--keyfile` and `--public-key`, which are not secrets. That
over-redaction is deliberate: §30.1's principle and §30.3's rule both point the same way, and a
hidden path costs a reader one lookup where a leaked token costs rather more.

The option **name** always survives, so `restart service nginx --password=<secret:redacted>` still
reads as the command it was. `is_redacted()` reports that something was hidden, so a renderer can
say so rather than leaving a reader to notice.

## Consequences

- There is no code path from a raw command line to a persisted summary. A caller who wants to
  persist one has to decompose it first, which is where the decision about each argument belongs.
- §17.5's other clause — "unknown arbitrary external command text is NOT persisted as an action
  body by default" — is satisfied by the same shape: an external command has no typed arguments to
  offer, so there is nothing for `of` to build from.
- A legitimate `--keyfile` argument shows its name and not its path. Reported as a known cost
  rather than a defect.
- The word list lives in one place, so widening it widens every call site at once.
- Encoded in `crates/ono-temporal-core/tests/actions.rs`, including the case that gives the test
  file its reason to exist: a secret-shaped argument never appears in a summary.

## Alternatives considered

- **Redacting a rendered string with a regular expression.** The precedent exists
  (`ono_history::policy`) and it is right for shell history, where the raw line is the artefact.
  Here the raw line is the thing that must not exist, so a function that takes one is the wrong
  shape.
- **Only redacting what the caller marks.** Correct when every caller is correct. The two safety
  nets cost a comparison per argument and cover the case where one is not.
- **A `Secret` value type in `ono-value`.** A larger change to a shipped crate, for a need one
  crate has, and INTERFACES §5.2 already records that the tree does not have one.
