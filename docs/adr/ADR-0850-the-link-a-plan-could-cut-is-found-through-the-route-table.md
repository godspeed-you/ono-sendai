# ADR-0850: The link a plan could cut is found through the route table this host routes by

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §34.2, §19.1, §29.1, Appendix I.3, §55.10 case 46; v0.2 §50; ADR-0848
- Decided by: agent (autonomous)

## Context

§34.2: "If a plan may remove the path used by the active remote Ono link, this MUST be a CRITICAL
risk landmark." `ono-change-impact` has the rule (`risk.remote.link-loss`) and takes the link as an
input, `ActiveLink`, with the route and the interface it depends on. Nothing in the shell built
one, so the rule never fired outside its unit tests.

The spec names the path and leaves open how a shell learns it. A link knows the host it dialled and
the transport; it does not know which interface its packets leave through. Asking `ip route get`
would parse human-readable output, which v0.2 §50 forbids.

## Decision

`plan` builds an `ActiveLink` for each connected link whose transport reaches another machine over
the network (`tcp`, `ssh`), and hands the assessment the one whose path the plan touches.

- The far address is the link's host with any `user@` removed, resolved by the system resolver
  (`getaddrinfo`, which honours `/etc/hosts` with no network).
- The egress interface is chosen from the netlink provider's `ono.route/1` records, every table,
  by longest-prefix match on `destination` (a null destination is the default route, prefix 0),
  ties broken by the lower `metric`. This is the kernel's rule for a host without policy routing.
- The interface is recorded by the identity the plan froze it as: the `ono.interface/1` target
  whose label is that interface's name. `ActiveLink` compares identities exactly, and a frozen
  identity carries a generation (`interface:lo#generation=unknown`) no name spelled by hand has.
- A `local` link has no network path, so it has no `ActiveLink`. A plan made inside a link freezes
  the linked host's objects (ADR-0848), and this host's routes say nothing about them, so it has
  none either.

## Consequences

`plan stop interface lo` in a session holding a loopback `tcp` link is CRITICAL, with the finding
naming the link; the same plan with no link, or with a `local` link, is not. Case
`308-remote-disconnect` proves it in the container, whose loopback is the only network it has.

Policy routing (`ip rule`) is not modelled: on a host that selects tables by source or mark, the
interface found here may differ from the one the kernel uses. The finding then fails to fire; it
never fires falsely for an interface the link does not use, except where two tables disagree.

## Alternatives considered

- Parse `ip route get <address>` — v0.2 §50 forbids parsing unstable human output.
- Send `RTM_GETROUTE` with a destination — exact, including policy routing, but it needs a new
  netlink request and decoder in the provider for one caller; the dump is already decoded.
- Treat every interface change as link loss while a link is open — CRITICAL for `lo` changes
  while the link runs over `eth0` is the false alarm §19 exists to avoid.
