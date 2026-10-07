# ADR-0933: `history.result_cache` is a retired name for the history ceiling

- Status: accepted
- Date: 2026-10-07
- Spec refs: spec §20.2, §30; v0.4.1 §4.5, §24.1, §55.1; v0.6.1 §32; ADR-0010, ADR-0094, ADR-0456
- Decided by: agent (autonomous)

## Context

Two settings named one ceiling. `history.result_cache` came from spec §30's example
(`set config history.result_cache = 64MiB`); `limits.history_bytes_total` is v0.4.1 §55.1's name
for the same figure and the one the result history enforces. ADR-0456 §4 kept the old key declared,
read by nothing, "so an existing file parses", and left its retirement to "a release that may break
configuration" (issue #175). The declared-but-dead key was worse than a duplicate: a user who set
it saw `set config` succeed and nothing change.

v0.6.3 is a patch release, and v0.6.1 §32's compatibility rule holds for it: a configuration that
parsed yesterday must not stop the shell starting. The existing unknown-key policy (ADR-0094) would
not stop it either — an undeclared key in a file is `type.unknown_field`, reported and recorded,
and the shell starts — but it would print an error on every start and fail every script that sets
the key, for a name the specification itself uses.

## Decision

**`history.result_cache` leaves the catalogue and becomes a retired name for
`limits.history_bytes_total`.**

1. `ono_cli::settings::RETIRED` lists retired names with their replacement. `Settings::canonical`
   maps a retired name to its replacement wherever a key is accepted: `set config` in a file or at
   the prompt, the `ONO_*` variable the old key had by ADR-0010's mechanical mapping
   (`ONO_HISTORY_RESULT_CACHE`), and the selector of `get config`.
2. The value goes to the replacement key with its type and range check, at the layer it was given
   at. A retired variable is read before the catalogue's, so `ONO_LIMITS_HISTORY_BYTES_TOTAL` wins
   when both are set.
3. The first time a session meets the name the shell says so once on standard error, naming the
   replacement: `` `history.result_cache` is a retired name for `limits.history_bytes_total`, which
   holds its value now; write `limits.history_bytes_total` instead (ADR-0933) ``. It is a notice,
   not a problem: the assignment succeeded, so `get config --problems` does not list it.
4. `set config`'s action result names the key that now holds the value, and `get config` lists one
   key for the ceiling.
5. The contract changes with it: `docs/contracts/commands/meta.yaml`'s example becomes
   `set config limits.history_bytes_total = 64MiB` and the `key` selector documents the retired
   name; `docs/MIGRATION.md` §11 tells an upgrading user.

## Consequences

- One key names the ceiling. A configuration naming the old key starts the shell, applies its
  figure — which it never did before, because nothing read the old key — and prints one notice.
  That is a behaviour change for anyone who set the old key expecting nothing, and MIGRATION.md
  says so. A figure outside `limits.history_bytes_total`'s range (the old key had none) is now
  refused with the range error and the earlier layer's value stays, as for any setting.
- Retiring another key later is one row in `RETIRED`.
- `crates/ono-cli/tests/meta_config.rs::should_store_a_bytesize_setting_as_a_bytesize` used the old
  key as its example of a byte-size setting; it uses `limits.history_bytes_total` now, with the
  contract example it mirrors.

Encoded by `crates/ono-cli/tests/meta_config.rs`:
`should_set_the_history_ceiling_when_the_retired_key_is_set_at_the_prompt`,
`should_start_and_apply_a_config_file_naming_the_retired_key_with_one_notice`,
`should_read_the_retired_environment_variable_as_the_history_ceiling`,
`should_list_one_key_for_the_history_ceiling`; acceptance case `371-retired-history-key`.
Case `041-config-and-resolve` still sets and reads the old name and passes unchanged, which is the
compatibility claim made executable.

## Spec deviation

- Section: spec §30
- Text: "set config history.result_cache = 64MiB" (in the "Example conceptual config")
- Instead: the line still works, as a retired name of `limits.history_bytes_total`, and earns a
  notice; `history.result_cache` is not a setting of its own.
- Why: v0.4.1 §55.1 gives every hardening limit one dotted key and §52.2 one home, and
  `limits.history_bytes_total` is that key for this ceiling. Two declared keys for one figure is the
  second copy §52.2 forbids, and the one nothing read was a setting that silently did nothing.

## Alternatives considered

- **Remove the key and let the unknown-key policy refuse it.** The shell would still start, but
  every start would print an error and every script setting it would fail, in a patch release, for
  the name the specification's own example uses.
- **Keep both keys and make the old one write the new one.** Still two rows in `get config` for one
  figure; the issue's exit test is that one key names it.
