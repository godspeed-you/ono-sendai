# ADR-0100: `get session` reads systemd-logind over D-Bus, and is unavailable where logind is

- Status: accepted
- Date: 2026-08-27
- Spec refs: §8.1, §9.1 Identity, §10.5, §23.3, §23.6, §35.3, §50; ADR-0012, ADR-0068
- Decided by: agent (autonomous)

## Context

Spec §9.1 lists `get session` — "Enumerate local/login/session objects", `Stream<Session>` —
and stops there. §8.1 has `session` among the system targets; §28 defines no Session schema;
`docs/contracts/commands/identity.yaml` has declared `ono.session.get` as `stable` with
`provider_capability: session.list` and a `--user ref<ono.user/1>` option since Phase D, with
`ono.session/1` parked in `docs/contracts/schemas/deferred.yaml`. Nothing answered the target:
`get session` was `E0102 no provider answers `session``.

Three sources exist for "who is logged in" on a Linux machine:

- `utmp`/`wtmp` — a binary record format, but one glibc is retiring (the 64-bit-time
  transition drops it), that `who`/`w` read and that only PAM modules keep current;
- `loginctl list-sessions` — text, which spec §50 forbids parsing;
- `org.freedesktop.login1` — systemd-logind's D-Bus API, typed, documented, stable, and the
  source `loginctl` itself reads.

## Decision

**`get session` is answered by a `session` provider in `crates/ono-provider-systemd`
(`systemd-logind`, `crates/ono-provider-systemd/src/logind.rs`) that reads
`org.freedesktop.login1.Manager.ListSessions` and, per session,
`org.freedesktop.DBus.Properties.GetAll` on `org.freedesktop.login1.Session`.** It runs no
program and parses no text (spec §23.3 says this in as many words for services; the same
reasoning holds for sessions). It shares the crate's system-bus plumbing (socket detection,
call budget, D-Bus error translation) with the service provider.

**`ono.session/1` is written** (`docs/contracts/schemas/session.v1.yaml`) and leaves
`deferred.yaml`. Identity is logind's session `id`. `user` is a `ref<ono.user/1>` carrying
`uid` and the login name logind recorded — the uid is what `ListSessions` returns, so a name
that no longer resolves still identifies the holder (spec §23.6). The other fields are the
Session properties a user asks about — `seat`, `tty`, `display`, `type`, `class`, `state`,
`remote`, `remote_host`, `service`, `leader`, `scope`, `since` — every one nullable, because
logind genuinely leaves them empty (an SSH login has no seat; a `background` session has no
tty). `type` and `state` are enums over the values logind documents; a value outside the list
is `null` for `type` and `unknown` for `state`, never copied through as a foreign word.

**`--user` filters by uid or by name**, on the `ListSessions` row, before the per-session
property read — so `get session --user root` costs one round trip on a machine where root has
no session.

**Where no login manager answers, the provider is `Availability::Unavailable` with the
reason, and `get session` is `provider.unavailable` (E0401).** This is the rule the service
provider already follows (ADR-0012, `crates/ono-provider-systemd/src/lib.rs`): an empty stream
would claim "nobody is logged in", which a machine without logind cannot know. A container
therefore answers E0401 with a sentence naming the missing bus socket; the acceptance case
asserts exactly that. The RED suite's two session tests accept this reading — they insist that
*a provider exists* (no E0102) and that the stream is well-formed where it has rows — and they
run where logind runs.

The provider is registered in `register_async` beside the service provider, because reaching it
is an `await`; it is declared in `docs/contracts/providers/systemd.yaml` as `systemd-logind`, and
`crates/ono-cli/tests/providers.rs` holds the declaration and the registry together.

## Consequences

- `get session`, `get session --user <name|uid>`, `get session | where state == "active"`,
  `select user.name` all work on a systemd machine; `enter session` and `trace session` are
  not declared and stay undeclared.
- A second provider in the systemd crate meant lifting `open_system_bus`, `budgeted`, `text`
  and `number` in `dbus.rs` to `pub(crate)`; no behaviour of the service provider changed.
- `crates/ono-provider-systemd/tests/session.rs` proves the record shape, the null discipline,
  the `--user` filter and the unavailable path over a recorded `LoginBus`;
  `crates/ono-cli/tests/identity_missing.rs` proves the command end to end.
- `docs/reference/schemas.md` is regenerated; `docs/contracts/schemas/deferred.yaml` no longer lists
  `ono.session/1`.

## Alternatives considered

- **Read `utmp`.** A format glibc is abandoning, updated only by cooperating PAM modules and
  login programs, with no notion of seat, class or scope. Rejected.
- **Answer `[]` where logind is absent.** Fabricates "nobody is logged in"; contradicts
  spec §35.3 and the service provider's precedent. Rejected.
- **Put the provider in `ono-provider-linux`.** It would have needed zbus and a second copy of
  the bus plumbing; logind is systemd's, and the crate that speaks to systemd is where it
  belongs. Rejected.
