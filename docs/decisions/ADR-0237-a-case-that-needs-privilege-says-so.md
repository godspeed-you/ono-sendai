# ADR-0237: A case that needs privilege says so

- Status: accepted
- Date: 2026-08-29
- Spec refs: `docs/ACCEPTANCE.md` §2 (a capability without a passing acceptance case is not
  delivered); AGENTS.md §10, §14 (the referee, and never weakening it); v0.2 §16.4 and §43
  (privilege is part of what a command answers)
- Decided by: agent (autonomous)

## Context

`scripts/acceptance.sh` runs every case as the unprivileged user `case`, with networking
disabled. That is deliberate and it is most of the harness's value: a shell that only works for
root is not a shell, and an unprivileged run is what proves the refusals are structured.

It also means two delivered capabilities had no acceptance case at all, because the thing they do
cannot be done without privilege:

- the nine network mutations of ADR-0088 (`add route`, `set interface`, …), whose only proof was
  the unprivileged refusal — the request layouts had never met a kernel;
- mount propagation peers (ADR-0236), which need a shared bind mount to exist.

"No case" is the worst of the three options. A capability nothing exercises is a capability nobody
has checked, and `docs/ACCEPTANCE.md` §2 says so.

## Decision

**A case may ask for the privilege it needs, in the case file, and nowhere else.** Three
directives, all optional and all absent from every existing case:

- `capability:` — a Linux capability, as `--cap-add` names it. Repeatable.
- `user:` — the container user, for the one thing a capability is useless without.
- `security:` — a `--security-opt` value. The host's AppArmor profile denies `mount(2)` even to
  `CAP_SYS_ADMIN`, so a case that mounts needs `apparmor=unconfined`; nothing else does.

The default is unchanged and stays the rule: unprivileged, `--network=none`, the `case` user. A
case that departs from it says so in its own text, so the departure is visible where a reader
meets the case rather than in a flag somewhere in the runner.

**A privileged case must still assert the unprivileged half.** The point is not to run the suite
as root; it is that a request layout meets a real kernel once. Everything a normal user can check
stays checked by a normal user, and the unprivileged refusals keep their own cases.

## Consequences

- Two capabilities gain the proof they were missing, on a live kernel: the network write paths
  under `CAP_NET_ADMIN` and the propagation peers under `CAP_SYS_ADMIN`.
- The harness is not weakened. Nothing was removed, no assertion loosened, and a case that does
  not name a capability gets none — the runner adds `--cap-add` only for what a case listed.
- Two cases in the suite now need a kernel that permits the operation. A container runtime that
  refuses `--cap-add` fails them loudly rather than skipping them, which is the right way round:
  a referee that quietly skips is not a referee (AGENTS.md §14).
- Encoded by `docker/acceptance/cases/122-mount-propagation-peers.case` and
  `123-privileged-network-writes.case`, and documented in `docker/README.md`.

## Alternatives considered

- **Run the whole suite privileged.** It would delete the harness's main proof: that an ordinary
  user can use this shell and is told honestly what they may not do.
- **A second suite, run separately.** Two runners to keep working, and a second one that is
  quietly not run is worse than no second one.
- **Leave both capabilities unproven and say so.** Honest, and it leaves nine mutations whose
  wire format has never been checked against a kernel — the failure mode a container harness
  exists to catch.
