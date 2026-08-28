# ADR-0185: Switching the spatial layer off is a refusal, not an absence

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §47, §40, §30.2, §9.4
- Decided by: agent (autonomous, S4d/S4e)

## Context

§47 ends with one sentence: "Disabling `spatial.enabled` MUST leave the typed shell and ordinary
commands functional." It does not say what the spatial commands then do, and there are two
readings that behave very differently for a script:

1. the verbs stop existing, so `look` falls through to `/usr/bin/look` and `near` is
   `resolve.command_not_found`;
2. the verbs exist and refuse.

Reading (1) is worse than it looks. `look` shadows util-linux `look` (ADR-0124), so a session
that switches the layer off would silently change what `look` *means* — a script written against
Ono would start feeding a word list. And a caller cannot tell "this build has no spatial layer"
from "this shell is misconfigured", which is exactly the distinction §40's error family exists
to make.

The setting also has to be readable at the moment a command runs. The rest of `spatial.*` is
snapshotted once, when the command table is built (`session::CONFIGURED`), so that an
already-drawn view cannot be redefined underneath itself. `spatial.enabled` is the one key whose
whole purpose is to be flipped.

## Decision

**A spatial verb refuses with `spatial.unsupported` (`Ono-Sendai-E1009`) while
`spatial.enabled` is false, and nothing else changes.**

- The refusal is raised where the shell binds a native stage
  (`crate::native::refuse_switched_off_spatial`), for every contract whose id begins with
  `ono.place.` — the thirteen commands of `docs/spec/commands/spatial.yaml`. It is raised for a
  foreground and a backgrounded pipeline alike, before any stage of the pipeline runs, so a
  refused `look` leaves nothing half-done.
- `spatial.enabled` is read from the session's live settings on every such bind, not from the
  snapshot: `set config spatial.enabled = false` takes effect on the very next statement.
- The **spatial side effects of ordinary commands stop with the verbs**: the v0.2
  `enter <target>` still pushes its context frame (§14.3) and no longer moves the place (§30.2),
  and `cd` no longer synchronises one (§30.3). A place nothing will answer for is not a place.
- §9.4's completion offers nothing from the neighbourhood, because an offered `enter compute`
  that refuses is worse than no offer.
- Everything else — the whole typed pipeline, `get`, `where`, `inspect`, the external `look` on
  `$PATH` — is untouched, which is what §47 requires.

## Consequences

- `try { look } catch e { $e.name }` reads `spatial.unsupported`, so a script can branch on the
  configuration instead of guessing from a missing command.
- The switch is honest in both directions: with the layer off nothing spatial happens, and with
  it on nothing has to be re-enabled command by command.
- A future spatial command must carry an `ono.place.*` contract id to be covered. That is
  already the naming rule of `docs/spec/commands/spatial.yaml`, and `spec-check` holds the file
  against the implementations.
- Exit test: `crates/ono-cli/tests/spatial_contracts_missing.rs::should_keep_the_typed_shell_working_when_the_spatial_layer_is_disabled`.

## Alternatives considered

- **Unregister the commands when the setting is false.** The table is built once per process and
  the setting can change inside a session, so the table would have to be rebuilt; and it would
  hand `look` back to util-linux, silently changing a word's meaning.
- **Refuse inside each command's `invoke_async`.** Thirteen copies of the same guard, and each
  one would have to reach the session's settings through a snapshot that is deliberately taken
  once. One guard at the bind point covers every spelling, including a stage in the middle of a
  pipeline.
