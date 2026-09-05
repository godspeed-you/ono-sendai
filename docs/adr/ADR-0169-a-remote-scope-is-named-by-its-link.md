# ADR-0169: A remote scope is named by the link, never by what the far side calls itself

- Status: accepted
- Date: 2026-08-28
- Spec refs: §3.2, §10.1, §10.2, §14.5, §19.2, §35.3, §43.7, §2.17
- Decided by: agent (autonomous, phase S8)

## Context

Every object observed across a link belongs to a `RemoteHostScope` (§3.2), and that scope is part
of the object's identity (§10.1: "a remote object's identity belongs to its host's scope"). The
scope needs a name and a boot identity. Two candidates existed: what the far side reports about
itself (its hostname, its `boot_id`), and what this session calls the link.

The `local` transport of ADR-0037 makes the choice sharp. It spawns this very binary as
`ono --agent` over a pipe pair, so the "remote" host is this same kernel: its hostname and its
`/proc/sys/kernel/random/boot_id` are byte-for-byte the local ones. A scope named from the far
side's self-report would be equal to the local scope, `pid 1` on both sides would reduce to one
`SpatialId`, and §43.7's "no accidental local/remote identity merge" would fail on the one fixture
the offline test suite can actually build.

The general case is no better. §14.5 says identity "MUST NOT be inferred from IP coincidence alone
when ambiguity exists"; two links to two container hosts that both answer `localhost` are exactly
that ambiguity, and a self-reported name is a claim by the machine being identified.

## Decision

A remote scope is `SpatialScope::remote_host(<link name>, BootIdentity::unknown_boot(<link name>))`.

- The **name** is the link's, as `link host` recorded it. It is what this session can vouch for,
  it is what the user typed, and it is what `jump`, the prompt, the place path and the trail all
  already spell.
- The **boot identity** is unknown. No `ono.host/1` carries a boot id, and §35.3 and §2.17 forbid
  inventing one: `BootIdentity::unknown_boot` never compares equal to a known one, so a remote
  lifetime identity is honest about how far it can be trusted.

The far side's own hostname is not discarded — it is what `get host` reports and what a place view
shows as its provider data. It is simply not identity.

## Consequences

- Remote `pid 1` and local `pid 1` are two places even when they are literally the same kernel
  process, because the boot component differs (`testbox/?` against `<hostname>/<boot_id>`).
- Two links to the same machine under two names are two scopes. That is a false *distinction*
  rather than a false merge, and §2.17 prefers it: the shell says what it knows, and it knows two
  links.
- Renaming a link renames its scope, so ids of places behind it change. Pins record a selector as
  well as an id (§20.4, ADR-0153), which is what a rename leaves usable.
- Encoded by `spatial_remote_missing::should_keep_a_remote_process_place_distinct_from_the_local_one_with_the_same_pid`.

## Alternatives considered

- **Ask the far side for its hostname and boot id.** Truthful only where the far side is truthful,
  and it fails outright on the only fixture available offline. Rejected.
- **Hash the transport endpoint.** Opaque, unstable across a reconnect, and unrelated to anything
  the user typed.
