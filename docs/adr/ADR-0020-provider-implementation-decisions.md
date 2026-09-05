# ADR-0020: What the providers do where the kernel and the contract do not line up

- Status: accepted
- Date: 2026-08-26
- Spec refs: §6, §10.5, §23, §28, §34, §35.3, §43, §50; ADR-0005, ADR-0014, ADR-0015, ADR-0016
- Decided by: agent (autonomous)

## Context

Implementing the Phase C providers turned up a set of places where the schema a contract declares
and what the operating system will actually tell you do not meet cleanly. Each was decided while
building; they are recorded here because a later reader will otherwise assume the difference is a
bug.

## Decision

### 1. CPU is a rate between two observations, and `null` before there are two

`/proc/<pid>/stat` reports ticks consumed since boot. A percentage needs a denominator, and there
are only two honest ones: the time since the process started, or the time since the last
observation.

The provider reports the second: `(Δticks / CLK_TCK) / Δseconds × 100`, the share of one logical
CPU, keyed by `(pid, started)`. The first observation of a process reports `null`.

A since-start average answers a different question from the one `where cpu > 20` asks — a process
that pinned a core for an hour last week and has been idle since would match, which is not what
anyone means. `null` for the first sample is the honest answer to "what is it doing *now*" when
nothing has been measured yet, and spec §35.3 forbids inventing one.

### 2. NSS resolves; `/etc/passwd` enumerates

`nix` exposes `getpwuid_r`/`getpwnam_r` but not `getpwent`, and the crate is
`#![forbid(unsafe_code)]`. So resolving a *named* user goes through NSS — `get user postgres`
finds an LDAP or SSSD account — while enumerating every user reads `/etc/passwd`, a
POSIX-specified colon-delimited database rather than a program's output, which keeps spec §6's
prohibition intact.

Every NSS call runs on a blocking thread under a 250 ms timeout with a positive-and-negative
cache, because spec §34 names slow NSS as a pathological case and one hanging lookup must not
stall an enumeration of five hundred processes. A lookup that times out leaves the numeric id and
a `null` name — never a fabricated one.

### 3. `device` means two different things, deliberately

In `ono.mount/1` and `ono.filesystem/1` it is the backing block device, so an anonymous device —
tmpfs, overlay — is `null`. In `ono.file/1` it is half the identity, so it is always reported as
`major:minor`: leaving it null would let two files on two different tmpfs mounts claim to be one
object as soon as their inode numbers agreed.

### 4. A file's birth time is `null`

It needs `statx`, which `nix` 0.31 does not expose, and reaching it another way would mean
re-resolving a path mid-traversal — exactly what ADR-0015 T14 forbids. Reporting `st_ctime` under
the name `created` would be a different value wearing the field's name, which spec §10.5 exists to
prevent.

### 5. No netlink crate, and `zbus` for D-Bus

`ono-provider-netlink` hand-rolls its message encoding on `nix`. Every candidate — `neli`,
`netlink-packet-route`, `rtnetlink` — hides the byte slice behind its own parser and error type,
which would have removed the seam that makes each decoder a pure function from bytes to records,
and therefore the seam that makes it fuzzable (spec §35.6, ADR-0015 T7). None of them covers
`sock_diag` well, so half the crate would have been hand-rolled regardless.

`ono-provider-systemd` uses `zbus` with `default-features = false, features = ["tokio"]`. It is
pure Rust, so the container needs no `libdbus`; it forces no `unsafe`; and dropping its default
reactor keeps it on the shell's own Tokio runtime rather than starting a second one (ADR-0005).

### 6. Socket-to-process joining is opt-in, and scans once

Finding which process holds a socket means matching its inode against every open descriptor on
the machine — six figures of syscalls on the host spec §34 describes. `--process` turns it on;
without it the field is `null` and the schema's own documentation names the option that fills it.
When it is on, `/proc` is scanned **once for the whole dump** rather than once per socket.

### 7. A refused unit operation is `provider.unsupported`

Spec §43 is closed and additive (ADR-0006) and has no code for "the service manager declined" — a
masked unit, a unit that cannot reload. `provider.unsupported` (E0402) carries it, with systemd's
own D-Bus error name kept verbatim in the message, rather than inventing a code.

### 8. A unit with no enablement reports `null`, not `false`

`static`, `indirect`, `generated`, `transient` and `linked` units have no enablement to report.
Only `enabled` and `enabled-runtime` are `true`; only `disabled`, `masked` and `bad` are `false`.
Reporting `false` for the rest would be an answer nobody gave (ADR-0014).

### 9. `env.set` is not a provider capability

Setting a variable changes the session's own scope, which the evaluator owns. A provider claiming
it would put the authority in the wrong place and make `set env` fail somewhere a user could not
reason about.

## Consequences

Easy: every field a provider reports is either what the system said or `null`, and the difference
between "unknown", "unreadable" and "absent" survives all the way to the renderer.

Hard: `cpu` being `null` on a first observation will surprise someone running `get process | where
cpu > 20` once. That is the correct behaviour and the schema's documentation says so; the
alternative is a number that means something other than what the reader thinks.

Encoded by: the fixture-backed tests in each provider crate, in particular the recorded-signal
fake that proves nothing is sent when an identity has moved, and the symlink-swap traversal test.
