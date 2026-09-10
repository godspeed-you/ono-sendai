# ADR-0844: A plan inside a link is refused, because this build freezes targets only here

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §7.1, §29.1, §29.2, §29.3, §29.4, §55.10 cases 44 and 45, §66.9; ADR-0838
- Decided by: agent (autonomous)

## Context

§29 makes a remote change a first-class plan: truth per host (§29.1), protection per host
(§29.2), a link failure that leaves remote state unknown (§29.3) and recovery planned per host
(§29.4). The libraries hold those rules — `ono_change_protection::hosts::compose` (ADR-0838),
`ono_change_recovery::plan_hosts`, the executor's `RemoteDisconnect` failure point — but the shell
has no path that freezes a target on a linked host, runs an action over a link or verifies there.

Nothing in the change path carries a host on a frozen target or runs an action over a link, so a
plan made inside `enter link` could not be trusted to act where the operator is working: its
targets would carry no host, and `apply` would run them on this machine. A plan made there would
be the misleading success the tranche exists to prevent, even though none was observed doing
harm: the first probe of this path entered no link at all (`add link` fills the v0.4 registry;
the session enters links made by `link host`), and the second, through `link host … --transport
local`, already met the refusal below.

## Decision

`plan` refuses inside an entered link with `change.action_not_plannable`, naming the link and
saying why: this build freezes targets only on the machine the shell runs on (§7.1, §29.1), and a
plan over a link is not orchestrated. Nothing is sealed. Leaving the link plans on this machine,
as before.

Orchestrating a change over a link — freezing through the link's providers, carrying the host on
every target, executing and verifying there, composing protection per host and planning recovery
per host through the shell — is not in this build. The boxes of `docs/ACCEPTANCE.md` §4.12 that
only it can prove stay open, and `docs/releases/v0.6.0.md` lists them as recorded exclusions under
§66.9, pointing here.

## Consequences

No plan can be sealed against the wrong host. Cases 307 and 308 keep their declared skips and
prove the local half; `crates/ono-cli/tests/change_remote.rs` proves the refusal. The library
pieces keep their own tests, ready for the shell path that uses them.

## Alternatives considered

Planning inside the link as the local machine with a warning. Rejected: §2.4 forbids an
unestablished fact being read as an established one, and a warning beside a sealed wrong-host plan
is the misleading success the tranche exists to prevent.
