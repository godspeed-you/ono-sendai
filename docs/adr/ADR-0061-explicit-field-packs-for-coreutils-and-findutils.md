# ADR-0061: Explicit-field packs for coreutils and findutils

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.9 (tier B), §1.14, §1.38, §1.39, §1.69 step 7; ADR-0055, ADR-0059, ADR-0060
- Decided by: agent (autonomous)

## Context

`stat --printf`, `df --output` and `find -printf` are stable explicit field formats
(v0.3 §1.9 tier B), but three things about them the contract of ADR-0055/0060 could not yet
say: `find`'s action must come *after* the user's paths and tests; `df` prints a header
line; and a filename may contain the tab that separates fields and the newline that
separates records.

## Decision

1. **`plan.trailing_argv`** — words appended after everything the user typed. `find`'s
   `-printf …` action goes there; the user's paths and tests pass through in the order typed,
   which the matcher now guarantees for every pack (it used to gather flags before
   positionals).
2. **`decoder.header_lines`** — leading newline-separated records that are a header and are
   skipped, for `df`. A stream decoder skips them as they arrive.
3. **NUL-terminated records with the path last.** `stat --printf '…\0'` and
   `find -printf '…\0'` end a record with NUL, fields are tab-separated, and the path is the
   last column, which takes the rest of the record — tabs, newlines and all. The escapes are
   written as the tools read them (`\t`, `\0` as two characters in the contract) because an
   argv element cannot hold a NUL byte.
4. **`field.basename`** derives `name` from `path`, exactness `normalized`.
5. **Fractional epochs.** `%T@` writes seconds with a fraction; a timestamp in a declared
   unit of seconds keeps the fraction to the nanosecond and never rounds through a float.
6. **Bytes, never human units.** `df` is asked for `--block-size=1`; `-h`, `-H`, `-k`, `-B`
   and `--output` run raw. `stat -f`, `-c`, `-t` and every `find` action (`-exec`,
   `-delete`, `-print0`, `-ls`, …) are a different command and run raw.
7. **The contracts target GNU coreutils and findutils.** A machine whose `stat` is uutils or
   whose `find` is `bfs` gets `adapter.version_incompatible` from the probe (v0.3 §1.46) —
   honest, and the raw path — unless the tool happens to answer the GNU probe, in which case
   the fixture harness is what holds it to the format.

## Consequences

- `find . -type f -mtime +30 | where size > 100MiB` (spec v0.3 §1.38) composes, streams, and
  is byte-safe for hostile names; `du` remains for a later increment.
- Tests: the conformance harness over `docs/contracts/adapters/fixtures/{coreutils,findutils}/`,
  `ono-adapter/tests/negotiation.rs` (`should_append_trailing_argv_after_the_users_own_words`),
  `ono-cli/tests/adapters.rs`, acceptance case `079` (hostile names in the container).
