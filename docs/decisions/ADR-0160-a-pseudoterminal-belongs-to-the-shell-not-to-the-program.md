# ADR-0160: A pseudoterminal belongs to the shell, not to the program it starts

- Status: accepted
- Date: 2026-08-28
- Spec refs: §18.1, §29, §29.3
- Decided by: agent (autonomous)

## Context

Interactive `ono` processes were outliving the process that started them and accumulating. A
census on 2026-08-28 found 160 of them, up to 16 hours old, each still holding its D-Bus system
bus connections; together they held 214 connections and pushed UID 1000 past
`org.freedesktop.DBus.Error.LimitsExceeded`, which made `get session` fail in tests that have
nothing to do with sessions. Reaping them by hand made the gate green; they came back with the
next run.

The survivors were not spinning and not stuck on a lock. Every one of them looked like this:

```
$ ps -o pid,ppid,tty,etimes,stat,args -p 1193672
1193672 5821 pts/29 150 Ssl+ target/debug/ono          # reparented to systemd --user
$ cat /proc/1193672/wchan
ep_poll                                               # blocked reading its own input
$ ls -l /proc/1193672/fd
0 -> /dev/pts/29     1 -> /dev/pts/29     2 -> /dev/pts/29
3 -> /dev/ptmx       4 -> /dev/ptmx       7 -> /dev/pts/29    9 -> /dev/ptmx
$ cat /proc/1193672/fdinfo/4
tty-index: 29
```

Descriptor 4 is the **master** side of `/dev/pts/29` — the very terminal the shell reads its
input from. The shell held the far end of its own input. When the process that started it closed
its copy of the master and exited, the master's reference count did not reach zero, because the
shell itself still held a reference; no end of file was ever generated, and the shell waited for
a byte that no one could send. Descriptors 3 and 9 were the masters of two *other* shells started
concurrently from the same test binary, and descriptor 7 was the original slave.

The cause is `PtySession::start` (`crates/ono-process/src/pty.rs`). It allocates the terminal with
`nix::pty::openpty`, which is glibc's `openpty(3)`: it opens `/dev/ptmx` and the slave with
`O_RDWR | O_NOCTTY` and **no** `O_CLOEXEC`. Everything else the child is handed goes through
`plan::redup`, which duplicates close-on-exec, so the pipes, files and slave duplicates were
already correct — only the pair that comes straight from `openpty` was not, and an ordinary
descriptor survives `exec`. Every program `ono` starts under a terminal therefore inherited that
terminal's master, and `ono` starting `ono` — which is exactly what the PTY test harnesses in
`crates/ono-cli/tests/{view,watch_live,session_lifetime}.rs` and
`crates/ono-process/tests/terminal_control.rs` do — turned that into a shell that can never end.

Spec §18.1 requires job control, terminal process groups and PTYs "well enough to run normal
interactive Unix software", and §29.3 requires the shell to get out of the way of a program that
owns the terminal. Neither states what a descriptor table may contain, so the rule is decided
here.

## Decision

**A pseudoterminal the shell allocates is the shell's, on both sides. Neither descriptor may
cross `exec`.** `PtySession::start` marks the master and the slave `FD_CLOEXEC` immediately after
`openpty`, before anything is spawned. The child still gets the terminal — as three duplicates on
0, 1 and 2, made by `plan::prepare_pty`, which `dup2` installs and which are therefore exempt from
close-on-exec by construction. It gets nothing else.

The behavioural rule this enforces, and which the tests state:

1. An interactive shell whose input has reached end of file **exits**. This already held; what
   was missing is that end of file could occur at all.
2. Nothing under a shell's control points at `/dev/ptmx`. A process that holds the master of its
   own controlling terminal has made its own input unclosable, and that is a defect wherever it
   appears — not a condition to be recovered from by a timeout or a reaper.

`SIGHUP` keeps its default disposition: a shell whose terminal is destroyed under it dies of the
signal and reports 128+1, as every other Unix shell does. That is the terminal *going away*, and
the shell has nothing left to say; it is not the same event as end of file on a terminal that is
still there, which is rule 1 and exits with the last command's status.

## Consequences

- Interactive `ono` processes started under a pseudoterminal now exit within milliseconds of the
  far end closing, so the test suites stop accumulating shells, and the D-Bus connections,
  memory and session registrations they hold are released with them.
- The fix is one `fcntl` per session and changes nothing a program can observe about its own
  terminal: it still sees a TTY on all three streams, its own session, its own controlling
  terminal and the right window size.
- A future caller that genuinely needs to pass a master to a child must duplicate it
  deliberately into the `FdPlan`, which is the mechanism that already exists for that and which
  records the intent.
- No reaper, no watchdog and no timeout was added. A shell that is running because it has work
  to do is never killed by this change.
- Encoded by `crates/ono-cli/tests/session_lifetime.rs`:
  `should_exit_when_the_terminal_it_was_given_goes_away` and
  `should_not_hold_the_terminal_that_drives_it`.

## Alternatives considered

- **Have the shell notice an orphaned session and exit** (poll `getppid`, or check whether the
  session is orphaned) — treats the symptom. The shell was not wrong to be waiting; it was wrong
  to be holding the descriptor that made waiting eternal, and a shell legitimately outlives its
  starting process when it is a login shell.
- **A timeout on the prompt read** — would kill a healthy idle shell, which is the normal state
  of a login shell.
- **A reaper in the test harness** — leaves the defect in the product and only hides it in tests;
  the same leak reaches a real user through `ono` running `ono`, or any program that re-execs.
- **`close_range` after `fork`, closing everything above 2** — heavier, changes the contract for
  every spawn path, and would silently paper over descriptor leaks instead of preventing them at
  the point they are created.
