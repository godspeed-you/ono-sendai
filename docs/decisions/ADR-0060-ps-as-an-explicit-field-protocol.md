# ADR-0060: `ps` as an explicit field protocol

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.9 (tier B), §1.13, §1.34, §1.69 step 6; v0.2 §28.1; ADR-0055, ADR-0058, ADR-0059
- Decided by: agent (autonomous)

## Context

`ps` has no machine format. Spec v0.3 §1.34 says the adapter "SHOULD NOT parse the visual
`ps aux` table if an explicit field invocation can reproduce its semantic data", and §1.9
lists `ps -eo pid=,ppid=,user=,comm=` as the tier B example: the adapter owns the columns and
the empty headers make the output a protocol. Three things needed deciding: how columns that
`ps` pads with spaces are split, how a row becomes a canonical `Process` whose identity is
`(pid, started)`, and which user spellings select which invocation.

## Decision

1. **`field_separator: whitespace`.** A `lines` decoder may declare whitespace as its
   separator: runs of spaces or tabs separate the first `columns.len() - 1` fields, and the
   last column takes the rest of the line verbatim. The one field that can hold spaces —
   `args` — is therefore always declared last, and a row with fewer fields than columns is
   `adapter.decode_failed`, never padded.

2. **Two new derivations**, written down in `docs/spec/adapters/schema.yaml`:
   `first: true` on a string takes its first character (`Ss` → `S`), and
   `infer: program-name` takes the basename of the first word of a command line, brackets
   stripped for a kernel thread (`[kthreadd]` → `kthreadd`); `infer: started-from-elapsed`
   subtracts elapsed seconds from the moment the decoder ran, to the second. All three are
   what their exactness says: the first two `normalized`, the last two `inferred`
   (spec v0.3 §1.8), because `ps` reports neither the kernel's `comm` nor an absolute start
   time in a form worth parsing across locales.

3. **Streaming per line.** A `lines` decoder whose record separator is a newline (or NUL)
   yields a record per complete line from `feed`, exactly as `jsonl` does (ADR-0059), so
   `ps` — and `find -printf`, `du`, `lsof -F` after it — flows while the child runs.

4. **Invocations preserve `ps`'s own selection semantics.** `ps aux`/`ax`/`-e`/`-A` become
   `ps -e -o …` (every process); a bare `ps` or one with `-u`, `-p`, `-C`, `-t`, `-g`
   becomes `ps -o …` with those selectors passed through (the caller's terminal, a user, a
   pid list), because a `ps` that silently widened its selection would be the kind of
   adapter §1.14 forbids. `-o`, `-L`, `-T` and `-O` change what a row is and run raw.
   `COLUMNS=10000` in the plan's environment keeps `ps` from truncating `args`.

5. **What is not reported stays null and is listed as a limit**: `executable`, `cwd`, `group`,
   `service`, `container`; `get process` answers them from procfs.

## Consequences

- `ps aux | where cpu > 20 | sort memory desc` — the sentence spec v0.3 §1.71 opens with —
  works, and `ps aux | grep x` stays bytes.
- Tests: the conformance harness over `docs/spec/adapters/fixtures/procps/ps/`,
  `ono-adapter/tests/decode.rs` (the derivations), `ono-cli/tests/adapters.rs`
  (the real `ps`), acceptance case `078`.

## Alternatives considered

- Parsing `lstart` for `started` — rejected: locale- and width-dependent, exactly the
  brittle parser §1.9 warns about; elapsed seconds are exact and the subtraction is stated.
- Taking `comm` for `name` — rejected: `comm` can contain spaces (`Web Content`), and two
  greedy columns cannot be split.
