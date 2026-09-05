# ADR-0062: Builtin decoders for git porcelain v2 and lsof fields

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.9 (tier B), §1.40, §1.42, §1.69 steps 8–9; ADR-0055, ADR-0061
- Decided by: agent (autonomous)

## Context

`git status --porcelain=v2 -z` and `lsof -F` are stable, documented protocols — tier B — but
not column protocols: a porcelain entry's shape depends on its first character and a rename
carries its origin in the *next* NUL field; an `lsof -F` record is a process line followed by
any number of file lines, each a tagged field on a line of its own. A field map cannot say
that. ADR-0055 reserved `decoder.kind: builtin` for exactly this.

## Decision

1. **A builtin decoder parses structure only.** `git-status-v2` and `lsof-fields-v1` turn
   bytes into decoded records with tool-named fields (`index`, `worktree`, `type`, `name`,
   …); the contract's `fields` map still decides which canonical field each becomes and how it
   is coerced, so the YAML stays the single description of the adapter and the Rust knows
   nothing about schemas. A builtin id must exist in the binary, which `spec-check` enforces.
2. **`git-status-v2`**: headers (`#`) are not records; `1`/`2`/`u` entries are split on the
   documented field count with the path last; a `2` entry consumes the following NUL field as
   `original_path`; `state` is summarised from the two status letters — the index letter when
   set, the working-tree letter otherwise; renamed/copied by the score; `unmerged` for `u`;
   `untracked`/`ignored` for `?`/`!`, whose letters are kept as `index` and `worktree`. A
   submodule entry says so. Anything else is `adapter.decode_failed`.
3. **`lsof-fields-v1`**: `p` opens a process (its `c` and `u` follow), `f` opens a file that
   inherits the process, `t` and `n` complete it; `path` is the name only for a filesystem
   kind (`REG`, `DIR`, `CHR`, `BLK`, `FIFO`, `LINK`) that starts with `/`. A file before any
   process is not the protocol. lsof's warnings stay on stderr and what it could not see is
   absent (v0.3 §1.40).
4. **New canonical schemas**: `ono.git-status-entry/1`, `ono.commit/1` (author and committer
   flattened to `author_email` and friends), `ono.open-file/1` (process and user as
   references by name and id).
5. **`git log` needs no code**: an explicit `--format` with NUL fields and the record-separator
   byte `\x1e` between commits is a `lines` protocol; the contract's escapes now include
   `\xHH`.

## Consequences

- `git status | where state != "modified"` and `git log | where author_email == …` compose
  (spec v0.3 §1.42); `git diff`, `git add -p` and the human formats stay git.
- Tests: the conformance harness over `docs/contracts/adapters/fixtures/{git,lsof}/` (every
  porcelain entry kind, a root commit, a file before any process),
  `ono-cli/tests/adapters.rs` (a real repository, the shell's own open files), acceptance
  case `080`.

## Alternatives considered

- `git status --porcelain` (v1) — rejected: v1 is the format git documents as *not* stable
  across versions in its details; v2 is the machine surface.
- Nested `author {name, email}` records on `ono.commit/1` — rejected for now: a record type
  for four strings, and `where author_email == …` reads as well.
