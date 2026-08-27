# ADR-0082: File mutations act by path — the file family's mutation road

- Status: accepted
- Date: 2026-08-27
- Spec refs: §9.1, §11.5, §11.6, §12.1, §16.5, §17.3, §17.4, §28.2, §30, §43; ADR-0006,
  ADR-0010, ADR-0068, ADR-0081
- Decided by: agent (autonomous)

## Context

ADR-0068 built one mutation road for every family: a selector is resolved through the provider
into `ObjectId`s, each becomes an `Action`, each answer is an `ono.action-result/1` row. Files
do not fit it in four places:

1. `ono.file/1`'s identity is `[device, inode]` (spec §28.2: "path is a reference, not always
   identity"), but every filesystem call the provider can make takes a *path*. An `Action`
   carrying only the identity cannot be acted on.
2. `write file ./out.txt` names a file that does not exist yet; resolving it through the
   provider yields nothing, and ADR-0068 §2 would answer with a failed `io.not_found` row for
   the file the user asked to create.
3. `write file`'s input is `bytes | string`, not a stream of objects: the pipeline carries the
   *content*, and the target is the selector alone.
4. `copy file a b` and `move file a b` have two selectors; the road forwarded only options, so
   the destination never reached the provider.

Spec §11.6 makes the bulk threshold configurable; the road had a constant of 10 and no way to
read `safety.confirm.bulk_threshold` (spec §30).

## Decision

### 1. A `path` selector names the target directly

When the selector that supplies a mutation's targets is declared `type: path`, the shell does
**not** resolve it through the provider. Each path (one, or every element of the list a glob
resolved to — ADR-0081) becomes an `ObjectId` of the target's schema whose single value is the
path, and the provider acts on that path. "It is not there" is then the outcome of the act —
`unlink` answering `ENOENT` is the failed `io.not_found` row of ADR-0068 §2 — and a file that
does not exist yet is exactly what `write file --create` (default `true`) creates. The row's
`target` reads `ono.file/1[./a.txt]`, which is the name a person acted on; spec §11.5's
`ValueRef` is a reference, and a path is one.

### 2. Every bound selector travels in the `Action`

The selectors a contract declares besides the one that supplied the targets — `destination`
for `copy` and `move` — are carried as action arguments under their own names, exactly as
options are. A provider reads `action.argument("destination")`.

### 3. Content input becomes the `content` argument

A mutating command whose declared input admits bytes or text and no stream of objects
(`write file`: `bytes | string`) consumes the pipeline as content: strings and bytes are
concatenated, in order, byte for byte (spec §12.1 — what `echo` produced is what lands on
disk, newline included), into one `bytes` value carried as the `content` argument. Any other
value on the pipeline is a `type.mismatch` naming `to json` as the way to choose a
representation (spec §12.3). The targets are the selector's, as in §1.

### 4. An object from the pipeline carries its provenance source

For objects that arrive as records, the `Action` also carries the record's provenance
`source` (`Action::with_source`), the string the provider itself wrote when it observed the
object — for `linux.fs`, the path the record was reached by. A path-addressed provider re-finds
the object through it; a provider whose identity is complete (a pid and its start time) ignores
it. `Action::source()` is also the answer when the identity holds a path (§1), so a provider
has one accessor for "the path I act on". The remote protocol carries the source in the act
request so a mutation across a link acts on the same file (ADR-0036).

### 5. The bulk threshold is read from configuration

`ProviderMutation` reads `safety.confirm.bulk_threshold` from the invocation's scope, where the
session's `config.*` bindings already arrive (`set config safety.confirm.bulk_threshold = 1`,
ADR-0010's invocation layer), and falls back to 10 when it is unset. A selection above the
threshold on a command that declares `--confirm` mutates nothing and fails with
`safety.confirmation_required` (E0701) naming the count — before the first action, so a refused
bulk never half-ran (spec §11.6, §17.4). `--confirm` confirms non-interactively.

### 6. What the `linux.fs` provider does with each verb

| Verb | Behaviour | Failure rows |
|---|---|---|
| `write` | create (default) or `--append`; an existing file without `--overwrite`/`--append` is `io.already_exists` (E0303) and is left as it was | E0303, E0301 (`--create false` on a missing file), E0302 |
| `copy` | `fs::copy`; `--recursive` copies a directory tree (symlinks as symlinks); `--preserve` keeps mode, timestamps and — where permitted — ownership; an existing destination without `--overwrite` is E0303 | E0303, E0301, E0302, `--recursive` missing on a directory |
| `move` | `rename`, falling back to copy-then-remove across filesystems (`EXDEV`); `--overwrite` as for copy | as copy |
| `remove` | `unlink`; a directory needs `--recursive` (`remove_dir_all`); `remove dir` without `--recursive` refuses a non-empty directory | E0301, E0302, `io.*` for a non-empty directory |
| `set` | `--mode` as four octal digits; `--owner`/`--group` by name or id through NSS; `changed: false` when the requested state already holds; `--recursive` applies to a tree | E0301, E0302, `type.mismatch` for a mode that is not octal |
| `open` | spawns `--with <handler> <path>`, or `xdg-open <path>` when no handler is named and one is on `PATH`; the handler's non-zero exit is a failed row | `provider.unavailable` when nothing can open the file |

`--dry-run` (spec §11.6) answers `skipped` with what would have happened, for every verb.

## Consequences

- `remove file *.txt` is two rows naming `a.txt` and `b.txt`; `remove file nope.txt` is one
  failed E0301 row and exit status 1; `… | where … | remove file` acts on exactly the records
  selected. A refused bulk deletes nothing.
- `Action` gains an optional `source`; `ActRequest` in `ono-protocol` carries it as an optional
  `"source"` key, absent when unset, so older frames still decode.
- The generic road stays generic: nothing in `ono-command` knows about files. The one
  type-driven rule is "a `path` selector is acted on, not resolved", which the storage family
  (`mount filesystem <path>`) can reuse.
- Tests: `crates/ono-cli/tests/files_missing.rs` (write, copy, move, remove, set, open) and
  `docker/acceptance/cases/037-files-read-write-remove.case`.

## Alternatives considered

- **Give `ono.file/1` the path as identity.** Spec §28.2 says otherwise, and hard links would
  become two objects. Rejected.
- **Resolve the path through the provider and hand `act` the `ObjectRef`'s label.** The label
  is the file *name*, not its path, and `write` still has nothing to resolve. Rejected.
- **A dedicated `write file` implementation in `ono-command` doing the I/O itself.** It would
  bypass the provider and therefore the capability check and the link. Rejected.
