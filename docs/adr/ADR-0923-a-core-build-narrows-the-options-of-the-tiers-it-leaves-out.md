# ADR-0923: The core build narrows the options of the tiers it leaves out, not only their commands

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.2 §15.1, §50 (discoverability); v0.5 §4.5; v0.6 Appendix H; ADR-0910, ADR-0911
- Issues: #127
- Decided by: agent (autonomous)

## Context

ADR-0911 §5 says that in the core build, discoverability carries only what the build carries.
The core registry was narrowed by command only. Two tiers also own options of commands that stay:

- The temporal tier's `--at` appears on about eighteen core contracts (`get process`,
  `get file`, `get user`, …).
- The change tier's `--profile` appears on `get config`.

Both were rendered by `help`, both were offered by completion, and both were then refused at
invocation. An independent review found this.

## Decision

1. `CommandRegistry::retaining_options(keep)` narrows the options of every command. No command
   is removed.
2. `absent::tier_of_option` names the tier that owns an option:
   - `--at` on any command belongs to temporal;
   - `--profile` on `ono.config.get` belongs to change.
3. The core registry (`absent::registry`) applies both narrowings, by command and by option.
   `help`, completion, `get command` and binding therefore all see the command without the
   option.
4. `absent::claims` refuses a stage that writes such an option. The refusal is
   `resolve.not_in_build` naming `--at` or `--profile`, and it replaces the
   `type.unknown_field` that binding against the narrowed contract would give. The option is
   Ono's vocabulary and exists in the full build. It is not a typo.

This extends ADR-0911 §5. It does not change any rule of it.

## Consequences

- `help get process` in the core build lists no `--at`, and `get process --<TAB>` does not offer
  it. `get process --at -1h` still refuses by name, with status 126.
- Adding a tier-owned option to a core contract requires an entry in `tier_of_option`.
- Tests:
  - ono-command `registry` `should_drop_the_options_a_build_does_not_carry_when_narrowed`;
  - core_build `should_neither_show_nor_complete_an_option_of_a_compiled_out_tier`.

## Alternatives considered

**Marking the options unavailable in `help`.** This was rejected for the reason ADR-0911 gives
for commands: completion and binding would need a second notion of availability.
