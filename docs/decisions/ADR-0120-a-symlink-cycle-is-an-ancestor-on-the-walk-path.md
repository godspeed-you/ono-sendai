# ADR-0120: A symlink cycle is a directory that is its own ancestor on the walk path

- Status: accepted
- Date: 2026-08-27
- Spec refs: §8, §35.4
- Decided by: agent (autonomous)
- Supersedes one sentence of ADR-0083 §3 (the cycle guard); the rest of ADR-0083 stands.

## Context

ADR-0083 §3 made `find file --follow-symlinks` keep "a set of visited `(device, inode)` so a
cycle is walked once". A set of everything visited cuts more than cycles: when two names reach
the same directory — `sub` and `link -> sub` — whichever readdir yields first is listed and the
other is skipped. readdir order differs between filesystems, so
`should_descend_through_a_symlinked_directory_when_follow_symlinks_is_set` was green on ext4
locally and red on the CI runner (run 33091663417 at 75362dc), where `link` came out after `sub`
and `link/c.md` was never listed.

## Decision

Following symlinks lists a directory by **every** name it is reached by. A **cycle** is a
directory that is one of its own ancestors on the current walk path, and only that is cut: the
link that closes the cycle is still listed as the entry it is, and nothing is listed beneath it.
The walk carries, per queued directory, the `(device, inode)` chain of the directories above it
on its own path — never a global visited set.

This is what "off by default, because it can cycle" in `file.yaml` means, and it is
deterministic however the filesystem orders its entries.

## Consequences

- `crates/ono-provider-linux/src/file.rs` `walk_root`: the frontier entry carries its ancestor
  chain; the `visited` set is gone.
- A directory reachable through N names is listed N times, each under its own path, which is
  the truthful answer to "what lies under this root, following links".
- Encoded by `crates/ono-cli/tests/options_and_selectors_missing.rs`:
  `should_list_a_directory_under_every_symlink_that_reaches_it_when_follow_symlinks_is_set`
  (deterministic: three names to one directory, all three expected) and
  `should_cut_a_symlink_cycle_when_follow_symlinks_is_set` (`sub/back -> ..` terminates).

## Alternatives considered

- **Sort readdir output and keep the visited set** — deterministic, but still lists the
  directory under one name only, and which one depends on the names, not on the tree.
- **Canonical-path visited set** — the same defect with a different key.
