# ADR-0896: A test socket lives in a short, private directory of its own

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §38.1
- Issues: #143
- Related: ADR-0890, ADR-0895
- Decided by: agent (autonomous)

## Context

Since ADR-0890, `ono_testkit::scratch()` lives in cargo's target directory:
`<checkout>/target/tmp/ono-test-<pid>-<n>/`. A Unix socket's path is limited to 108 bytes,
including the terminating NUL. In a deep checkout, such as a CI runner's
`/home/runner/work/<repo>/<repo>/target/tmp/…`, a socket bound under `scratch()` comes close to
that limit or goes past it, and `bind` then fails with an error that has nothing to do with the
test.

A review named four sites. Two of them, `ono-provider-container/tests/events.rs` and
`ono-provider-linux/tests/file.rs`, bind in a `tempfile::tempdir()` in `/tmp` and never used
`scratch()`, so they are unaffected. The other two bind under `scratch()`:
`ono-cli/tests/containers_packages.rs` (the fake Docker socket) and
`ono-cli/tests/spatial_orientation_bound.rs` (the `dbus-daemon` bus).

## Decision

**A test that binds a Unix socket makes the socket's directory with
`ono_testkit::socket_scratch()`.** That directory is `ono-s-<pid>-<n>` in `$XDG_RUNTIME_DIR` (the
per-user directory meant for sockets), or in `/tmp` when that variable is not set. It is created
with mode `0700` and removed when the value is dropped. It is a separate helper from
`scratch()`. It holds sockets and their small companions, never files a recovery provider would
be asked to protect, because it is usually a tmpfs.

## Consequences

A socket path is about 40 bytes plus its name, wherever the checkout is. Encoded by
`crates/ono-testkit/tests/harness.rs::should_give_a_socket_a_directory_whose_path_fits_the_unix_socket_limit`.

`crates/ono-recovery-files/tests/scope.rs` binds `run/nginx.sock` inside its fixture tree on
purpose: the socket has to be in the scope it tests. It stays under `scratch()`. Its path is
about 80 bytes on this host and about 85 on a GitHub runner, inside the limit but with less
margin.

## Alternatives considered

**Put all of `scratch()` back in `/tmp`.** That is the defect #143 fixed.

**Shorter scratch names.** They gain a dozen bytes, and the checkout's depth still decides the
rest.
