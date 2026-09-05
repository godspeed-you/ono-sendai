# ADR-0083: Content verbs — `read file`, `tail file`, `open file` and the `find file` options

- Status: accepted
- Date: 2026-08-27
- Spec refs: §7.1, §9.1, §12.1, §12.2, §18.2, §18.3, §27.1, §35.3; ADR-0012, ADR-0024,
  ADR-0068, ADR-0081, ADR-0082
- Decided by: agent (autonomous)

## Context

`docs/contracts/commands/file.yaml` declares two commands whose answer is not objects but what an
object *holds*: `read file` (`output: bytes | string`) and `tail file` (`output:
stream<string>`). The provider API had one question a producer could ask — `snapshot(&Query)`,
"which objects" — and the command table bound only `get`/`find` to it, so both commands were
declared and unreachable (E0101). `open file` was `phase: planned` although §7.1 lists `file`
among `open`'s targets, and `find file` accepted `--name`, `--depth`, `--kind` and
`--follow-symlinks` without honouring them.

## Decision

### 1. The `Query` carries the verb it is asked in

`Query::for_verb` / `Query::verb()`; `get` by default. A contract's query is built with the
contract's verb, the remote protocol carries it as `"verb"` (absent or `get` for older frames),
and a provider that answers more than "which objects" dispatches on it. Nothing else changes:
producers still ignore it, and the capability the contract names — `file.read`, `file.watch` —
is what decides whether a command reaches a provider at all (ADR-0068 §3).

### 2. `read` and `tail` bind to one `ContentCommand`

Bound exactly like a mutation: when a provider for the target advertises the contract's
capability. Named by selector, the command runs the contract's query. Fed by the pipeline, each
record's value of the contract's first selector (`path` for a file) becomes one query for its
content, in arrival order, and everything the provider streams is forwarded as it is.

### 3. What `linux.fs` answers

- **`read file`**: the whole content as **one value**. Without `--encoding` it is `bytes`, which
  `to json` writes as hex without loss (spec §12.1, §12.2); `--encoding utf-8` decodes, and
  invalid UTF-8 is a `type.mismatch` failure naming the offset, never a lossy replacement. Any
  other encoding is `provider.unsupported`: this build carries no transcoding table, and a name
  it cannot honour is refused rather than approximated. Chunking large files into several
  `bytes` values is deferred (`docs/STATE.md`).
- **`tail file`**: the last `--lines N` existing lines (default 10) as one `string` per line
  without its terminator, then — `--follow` being `true` by default — every line appended
  afterwards, as it is written, for as long as anything listens. The follow **polls** the file's
  size every 100 ms; polling is the honest mechanism until an inotify road exists, and spec
  §18.2 asks only that it be explicit, which this ADR is. A file replaced under the tail (log
  rotation) is reopened by name on the next poll. `--follow false` makes the stream bounded.
- **`open file`**: `phase: C`, delivered through the mutation road (ADR-0082): `--with
  <handler>` runs that program with the path; without it, `xdg-open` from `PATH`, or a
  `provider.unavailable` failed row when nothing on this host can open files. A handler's
  non-zero exit is the failed row.
- **`find file`** honours its options: `--name` is a glob over the entry's name (`globset`,
  literal separators, no descent control); `--kind` restricts the emitted entries to one
  `ono.file/1` kind and still descends through directories; `--depth N` bounds the descent so
  that the root's direct entries are depth 1 and nothing below depth `N` is listed;
  `--follow-symlinks` descends through symlinked directories, opening them by path instead of
  with `RESOLVE_NO_SYMLINKS`, and keeps a set of visited `(device, inode)` so a cycle is walked
  once. The T14 property of ADR-0015 (a component swapped for a symlink cannot redirect the
  walk) holds only without `--follow-symlinks`, which is what "off by default, because it can
  cycle" in the contract means.

## Consequences

- `read file ./data.bin | to json` prints `["00ff41"]`; `get file *.md | read file --encoding
  utf-8` streams each file's text. `tail file app.log --lines 0 | take 1` returns the next
  appended line and ends because `take` closes the stream (spec §18.3).
- A KUANG/11 or remote provider that advertises `file.read` gets the same queries.
- Tests: `crates/ono-cli/tests/files_missing.rs` (read, tail, open) and the four `find file`
  tests of `crates/ono-cli/tests/options_and_selectors_missing.rs`;
  `docker/acceptance/cases/037-files-read-write-remove.case`.

## Alternatives considered

- **A `read` method on the provider trait.** Every provider and the remote protocol would
  grow a method that only one target answers. Rejected in favour of the verb on the query.
- **`read file` implemented in `ono-command` with `std::fs`.** It would bypass the provider,
  the capability check and any link. Rejected.
- **inotify for `tail`.** Right eventually; a second mechanism for the one test that needs it
  is not the smallest increment. Polling first, explicitly.
