# ADR-0241: A follower waits on the file, not on a clock

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §7.1 (`tail`), §18.2, §34 (latency and cost are product properties);
  ADR-0083 §3, ADR-0235
- Decided by: agent (autonomous)

## Context

`tail file <path> --follow` re-read the file's metadata every 100 ms and compared it with what it
had (ADR-0083 §3). Every line therefore arrived up to a tick late, and a `tail` of a file nobody
is writing asked the kernel about it ten times a second for as long as it ran. ADR-0235 gave the
crate an inotify reader for `watch file`; the same kernel interface answers this question.

## Decision

**The follow loop waits on the file instead of on a clock.** `file_watch::changes` watches the
path's directory for `IN_MODIFY`, `IN_CREATE`, `IN_MOVED_TO`, `IN_MOVED_FROM` and `IN_DELETE`,
filtered to that name, and signals the loop. `IN_MODIFY` rather than `IN_CLOSE_WRITE`, because an
appender holds a log open for the whole run and there is no close to wait for.

**The loop itself is unchanged.** It still re-opens by name, still notices a rotation by inode,
still bounds what it reads in one pass. Only what it waits on moved: a signal, or a one-second
sweep, whichever comes first. The sweep is the fallback for a filesystem inotify cannot watch —
where `changes` answers `None`, the old 100 ms tick is used unchanged — and the bound on how long
a rotation could go unnoticed on a filesystem that is silent about it. A rotation on a filesystem
that is not silent is itself an event, so it wakes the loop at once.

## Consequences

- A line appears when it is written rather than at the next tick.
- An idle follow costs a tenth of what it did. Following a file nobody writes, for five seconds,
  with `strace -e trace=statx,newfstatat,openat`: **50 syscalls naming the file before, 5 after**
  — the one-second sweep instead of the ten-per-second tick.
- No test changed. `should_emit_existing_records_in_order_before_following_when_lines_is_given`,
  the `tail file` cases of `files_missing.rs` and `data_missing.rs`, and the journal followers all
  stand green and untouched: the observable contract is what it was, and this ADR is about what it
  costs to keep it.
- A filesystem without inotify support keeps the old behaviour exactly, because `changes`
  answering `None` selects the old interval.

## Alternatives considered

- **Watch the file itself rather than its directory.** An inotify watch on a file survives a
  rename, which is the wrong answer for a follower that re-opens by name: it would keep reading
  the rotated-away file and never see the new one. Watching the directory is how a rotation
  becomes visible.
- **Drop the sweep entirely.** Then a filesystem that reports nothing — a network mount, a
  filesystem without inotify — would follow forever and see nothing, silently. A bound that
  costs one syscall per second is the price of that never happening.
