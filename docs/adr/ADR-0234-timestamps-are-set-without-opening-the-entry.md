# ADR-0234: Timestamps are set without opening the entry

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §10.5 (a refusal is reported, never silently absorbed), §35.3;
  `docs/contracts/commands/file.yaml` (`--preserve`); ADR-0082 §6
- Decided by: agent (autonomous)

## Context

`copy file src dst --recursive --preserve` left `dst` and `dst/inner` at the current time while
`src` kept its 2020 mtime. `file.yaml` promises that `--preserve` keeps "the mode, the timestamps
and — where permitted — the ownership of every copied entry", so a preserved copy of an archive
was an archive that all happened today.

`preserve_attributes` set the times through a writable handle:

```rust
std::fs::File::options().write(true).open(path).and_then(|file| file.set_times(times))
    // A directory or a read-only file cannot be opened for writing; the timestamps are
    // then the one attribute "where permitted" does not reach.
    .or(Ok(()))
```

The comment states the limit honestly and the limit is not a real one. A directory can never be
opened for writing, and a mode-`0444` file only by its owner — which is precisely the population of
a copied tree: every directory in it, and every file the source protected. The failure was also
silent: the `.or(Ok(()))` turned "this entry kept no timestamps" into success.

## Decision

**Timestamps are set with `utimensat(2)`, not through a writable handle.** It takes a path rather
than an open file, so a directory, a read-only file and a symlink are all reachable, and
`AT_SYMLINK_NOFOLLOW` keeps a link's own times from being written to its target.

The failure is no longer swallowed: `utimensat` failing is an `io::Error` like any other, and
`copy file --preserve` reports it against the entry it happened at. "Where permitted" in
`file.yaml` continues to describe ownership alone — only root may give a file away — and no longer
quietly covers the timestamps as well.

## Consequences

- A copied tree is the tree that was copied: `copy file archive copied --recursive --preserve`
  leaves every directory, every file and every read-only file at the source's time.
- `move file` across a filesystem boundary copies with `preserve = true`, so a move that had to
  fall back to copy-and-remove now keeps the directory times a same-filesystem `rename(2)` never
  touched. The two spellings of the same move finally agree.
- Symlinks still keep their early return in `copy_entry` and gain nothing here: `set_permissions`
  follows a link, so preserving a symlink's attributes needs its own decision about mode and
  ownership. That is ADR-0082's ground, not this one's.
- Encoded by `should_preserve_the_timestamps_of_a_copied_tree_when_preserve_is_given` and
  acceptance case `121-copy-preserves-a-tree`.

## Alternatives considered

- **Open the directory read-only and `futimens` it.** Works, and needs a second code path for the
  read-only file, which cannot be opened for writing either. `utimensat` covers both with one.
- **Set the times before writing the children.** It does not help: creating an entry inside a
  directory updates that directory's mtime afterwards, which is why the attributes are applied on
  the way out of the recursion in the first place.
