# ADR-1043: An action grant is named once per `--allow`

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.4.1 §9.5, §9.7; spec §10.4; ADR-0468
- Issues: #194
- Decided by: agent (autonomous)

## Context

v0.4.1 §9.7 writes the command as

```text
set client-key <fingerprint> --allow <capability>...
```

`docs/contracts/commands/remote.yaml` declared `allow` a single `string` with the doc "comma-
separated and quoted where there is more than one", because when ADR-0468 was written the binder
kept one value per option. A bare comma ends a word in the shell, so the spelling that worked was
`--allow "process.signal,service.manage"`, and `--allow a --allow b` — the reading of `<capability>...`
a person reaches for — did not grant both. The binder has carried repeatable options since
(`ParameterSpec::is_repeatable`, used by `help`, `explain` and `group --member`), so ADR-0468's
§Alternatives named the fix: declare the option repeatable.

## Decision

**`allow` is a repeatable option, one capability id per occurrence:**
`set client-key <fingerprint> --allow process.signal --allow service.manage` replaces the
allowlist with exactly those two. Every occurrence is parsed as an `ActionGrant`, so a pattern,
`*` or a risk class in any of them refuses the whole command and changes nothing.

**The comma-separated spelling keeps working through the 0.6 line.** A capability id never
contains a comma, so splitting each occurrence on commas is unambiguous, and a patch release
must not break a script that followed the old documentation. The two spellings mix
(`--allow "a,b" --allow c` grants three). Help and the contract name the repeated form as the
spelling; the comma form is described as accepted for compatibility. Whether it is ever removed
is a later minor release's decision, to be made with a notice first; nothing here schedules it.

`--allow ""` still clears the allowlist, as before: an empty occurrence names no capability.

## Consequences

The contract declares `allow` with `repeatable: true` and carries a two-id example, so
`docs/reference/commands.md` (regenerated) marks it *(repeatable)*; `help set client-key` and the
completion candidate's doc say "one per `--allow`", and completion already offers an option again
after it was given. The stored file and `ono.client-key/1` are unchanged.

For MIGRATION: nothing to do. `--allow "a,b"` keeps its meaning; `--allow a --allow b` now grants
both, where before it did not.

Encoded by `crates/ono-cli/tests/client_keys.rs::should_grant_every_capability_named_by_a_repeated_allow`,
`::should_keep_granting_a_comma_separated_allow_for_compatibility`,
`::should_refuse_a_pattern_in_any_one_of_several_allows_and_change_nothing`; acceptance case
`401-repeated-allow-grants-each-capability`.

## Alternatives considered

**Retire the comma form now, with an error that points at the repeated form.** Cleaner contract,
and a breaking change in a patch release to a command whose only documented multi-grant spelling
was the comma.

**Declare `allow` as `list<string>`.** Also accumulates, but a list-typed option would also take a
list *value* (`--allow ["a", "b"]`), a third spelling nobody asked for.
