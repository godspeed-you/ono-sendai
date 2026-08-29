# ADR-0223: `to bytes --field` writes one field, and `to bytes` adds nothing

- Status: accepted
- Date: 2026-08-29
- Spec refs: §12.1, §12.2, §12.3, §29.1
- Decided by: agent (autonomous, `close-data`)

## Context

`to text --field <name>` is the bridge spec §29.1 describes: an object pipeline feeds an ordinary
Unix tool by naming the one field that is the line. Its byte-shaped twin did not exist. `to bytes`
took no `--field`, so

```text
adapt curl https://example.com | to bytes --field body > page.html
```

— the natural way to put an adapted program's payload in a file — answered
`type.mismatch a record has no raw byte form`, and the refusal's help named `to json`, `to yaml`,
`to csv` and `to text`: four codecs that would all wrap or re-encode the bytes the user wanted
verbatim.

Two smaller things were wrong with the same use. The refusal never mentioned `--field`, so it
pointed away from the answer even once the option existed. And the writer that puts serialised
output on stdout appends a newline where the value has none — right for a `to json` document,
which is line-oriented, and wrong for `to bytes`, whose entire purpose is that the bytes arrive
unchanged.

## Decision

**`--field` applies to `bytes` as it does to `text`.** `to bytes --field body` writes the named
field of every value, byte for byte, in order, with nothing between them and nothing added. The
path may be dotted, exactly as `to text`'s may. The contract's `field` option documents both.

**`to bytes` output is never padded.** The shell appends a trailing newline to a *string* the
serialisers produced; a `Value::Bytes` is written unchanged. `to json`, `to yaml`, `to csv` and
`to text` still end with a newline.

**The refusal names `--field` first**: a record has no byte form, and the field that does is the
answer more often than a re-encoding is.

## Consequences

- `adapt curl url | to bytes --field body > page.html` produces the file the program sent, and
  `cmp` agrees with a direct download.
- A field that is a number, a timestamp or a record is still refused — `--field` names *which*
  bytes, it does not invent an encoding for something that has none (spec §12.3).
- `ono_value::to_bytes_of(values, field)` is the codec's own entry point, beside
  `to_text(values, field)`; `to_bytes(&Value)` is unchanged.

## Alternatives considered

- **Write the only field of a one-field record, as `to text` does.** Deferred rather than
  rejected: `to text`'s convenience exists because a line is unambiguous, and nothing yet asks
  for the byte form of `select body | to bytes`. `--field` says which field in one word.
- **Keep the trailing newline and let the user strip it.** Rejected: a byte the shell added is a
  byte the file did not have, and there is no way to ask for it back.
