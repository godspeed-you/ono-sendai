# ADR-0088: The network write paths — rtnetlink and sock_diag mutations, the unresolved target, and always-required confirmation

- Status: accepted
- Date: 2026-08-27
- Spec refs: §9.1, §11.5, §11.6, §16.5, §17.1, §17.4, §23.2, §27.3, §28.4, §28.5, §43, §50,
  §52; ADR-0006, ADR-0012, ADR-0015 T13, ADR-0068
- Decided by: agent (autonomous)

## Context

`docs/spec/commands/network.yaml` declared nine write paths — `add`/`remove`/`set route`,
`set`/`start`/`stop`/`add`/`remove interface`, `stop socket` — as `phase: planned`: ADR-0012
read §37's Phase C criterion as inspection only and left network reconfiguration for a later
product decision. That decision is now taken by the RED suite
`crates/ono-cli/tests/network_missing.rs`, which states the contract these commands must meet:
`explain` reports `privilege elevated` and the capability's risk; an unprivileged run yields
one `failed` ActionResult row with `io.permission_denied` (E0302), exit 1, and the system
untouched — never `E0101` "not built", and never a not-found for a prefix or interface that
happens not to exist, because rtnetlink refuses a caller without `CAP_NET_ADMIN` *before it
looks at the payload*; `stop socket` in a script without `--confirm` fails with
`safety.confirmation_required` (E0701) and closes nothing.

Three seams stood in the way:

1. `ProviderMutation` resolved every selector first and wrote an `io.not_found` row when
   nothing answered (ADR-0068 §2). `add route 10.99.0.0/24` names a route that does not exist
   yet; `remove route 10.99.0.0/24` unprivileged must say "permission", which only the kernel
   can say.
2. Nothing distinguished a command whose *single* action is destructive from one whose bulk
   form is. `--confirm` was only the bulk guard of spec §11.6.
3. An ActionResult's `target` was the identity alone — `ono.socket/1[620332]` — which tells a
   person nothing about which socket was closed.

## Decision

### 1. The write paths are delivered over the families the read paths already use

`ono-provider-netlink` gains `act` on its three providers, in `src/act.rs`:

- **routes** — `RTM_NEWROUTE` with `NLM_F_CREATE|NLM_F_EXCL` for `add`, with
  `NLM_F_CREATE|NLM_F_REPLACE` for `set`, `RTM_DELROUTE` for `remove`; the `rtmsg` and its
  attributes (`RTA_DST`, `RTA_GATEWAY`, `RTA_OIF`, `RTA_PRIORITY`, `RTA_TABLE`) are built from
  the route's identity when it came through the pipeline (`route.v1.yaml`: table, family,
  destination, gateway, interface) and from the selector and options when the user named it;
- **interfaces** — `RTM_NEWLINK` with `IFLA_MTU` and the `IFF_UP` flag/change pair for `set`,
  `start` and `stop`; `RTM_NEWLINK` with `IFLA_IFNAME` and `IFLA_LINKINFO{IFLA_INFO_KIND}` for
  `add --kind`; `RTM_NEWADDR`/`RTM_DELADDR` for `add`/`remove --address`; `RTM_DELLINK` for a
  bare `remove`;
- **sockets** — `SOCK_DESTROY` over `NETLINK_SOCK_DIAG`, with the `inet_diag_req_v2` rebuilt
  from a fresh dump keyed by the socket's inode, so the request carries the addresses and ports
  the kernel matches on. A Unix socket is refused (`provider.unsupported`): sock_diag destroys
  inet sockets only.

Every request is sent with `NLM_F_ACK` and the kernel's `NLMSG_ERROR` is the outcome: errno 0
is `success`, `changed: true`; anything else is a `failed` row carrying the kernel's errno as
the structured error — `EPERM`/`EACCES` → `io.permission_denied`, `ENOENT`/`ESRCH`/`ENODEV` →
`io.not_found`, `EEXIST` → `io.already_exists`, the rest `provider.unavailable`. Nothing is
checked ahead of the kernel and nothing is simulated: the refusal the tests want is the
refusal the kernel gives. `--dry-run` answers `skipped` with what would have been sent. The
capabilities are advertised as `route.set`, `interface.set` (mutate, elevation required) and
`socket.close` (destructive, elevation required), which is what binds the commands (ADR-0068
§3) and what `explain` reports.

The nine commands move from `phase: planned` to `phase: C` and from `stability: planned` to
`experimental`. The `?` cells of §52 keep `validation_required: true` and their overlap notes
(`start interface` vs `set interface --up true`): delivering both spellings is how their
usefulness gets validated; withdrawing one is a later decision.

### 2. An unresolved selector is the provider's question, not the shell's verdict

`ProviderMutation` still resolves a selector through the provider first, because a resolved
identity is what keeps a signal from reaching a recycled pid (ADR-0015 T13). When nothing
resolves, it no longer writes the `io.not_found` row itself. It builds the object as the user
named it — `ObjectId(ono.<target>/1, [<value>])`, with the selector carried on the `Action` as
an argument under the selector's name — and asks the provider to act on that:

- a provider that acts answers for itself: the kernel says `EPERM` to an unprivileged
  `remove route 10.99.0.0/24` and `ESRCH` to a privileged one; the process provider confirms
  the pid and answers `io.not_found`; systemd loads the unit and answers `io.not_found`;
- a provider that cannot attempt it at all (`Err` from `act`) gets the ADR-0068 row —
  `io.not_found`, "no `<target>` answers to `<name> <value>`", with the provider's error as
  the cause.

ADR-0068 §2's second bullet is narrowed accordingly: *a target that does not exist is a failed
row* still holds; *who says so* is now the provider wherever it can. The row's target, code and
exit status are unchanged for every existing case.

### 3. `confirmation: always`

`docs/spec/commands/*.yaml` gains an optional command-level field `confirmation`, `bulk` by
default. `bulk` is the guard of spec §11.6 that every mutating command with a `confirm` option
already has. `always` marks the option as required for every run: `ProviderMutation` refuses
with `safety.confirmation_required` (E0701) before the first action when it is absent, whether
or not a terminal is attached — no prompt exists in this build, and spec §17.4 forbids a script
from waiting for one. `ono.socket.stop` declares it. `remove file`'s `confirm` stays `bulk`: a
single deletion the user spelled out is not the case §17.4 protects against.

### 4. The ActionResult `target` carries the label a person knows the object by

`Action::labelled` and `ActionOutcome::label` carry a short human label beside the identity:
for a piped record it is `ono_graph::label_of` (now public; the spec §22.4 form — `tcp/:443`,
`process/921 nginx`, `nginx.service`), for a resolved selector the `ObjectRef`'s label. The
row's `target` renders as `<identity> <label>` when a label is known and differs from the
identity — `ono.socket/1[620332] tcp/127.0.0.1:45801` — and as the bare identity otherwise.
The identity is what `inspect` resolves; the label is what the row is read by.

## Consequences

- The ten mutation tests of `crates/ono-cli/tests/network_missing.rs` are green unprivileged,
  and the case `docker/acceptance/cases/039-network-dns-port-mutations.case` proves the refused
  route add and the unconfirmed `stop socket` in the container.
- Privileged runs are not exercised by the suite (the tests stand down as root, on purpose);
  the request layouts follow `rtnetlink(7)`, `sock_diag(7)` and iproute2's own messages, and a
  root run of `add route … --dry-run` shows what would be sent. A privileged conformance case
  is a later increment, and is noted in `docs/STATE.md`.
- `stop process 4000000` now reports the process provider's own `io.not_found` message rather
  than the shell's; code, row shape and exit status are the same.
- `ono-provider-netlink` stays `#![forbid(unsafe_code)]`; the write path is the same
  `nix`-wrapped socket as the read path, with one new `request()` beside `dump()`.

## Alternatives considered

- **Keep resolving first and special-case `add`.** Rejected: `remove route` and `set route`
  need the kernel's permission answer too, and a verb-keyed exception would have hidden a
  general truth — existence is the provider's to judge.
- **Check `CAP_NET_ADMIN` in the shell and refuse before sending.** Rejected: it duplicates the
  kernel's policy, gets it wrong under user namespaces and file capabilities, and is exactly the
  "fake the refusal" the tests forbid.
- **Derive "always confirm" from the capability's `destructive` risk.** Rejected: `file.remove`
  is destructive too and its `--confirm` is the bulk guard; the contract has to say which it is.
- **Use `ss -K` / `ip route` through the adapter layer.** Rejected by spec §50 and AGENTS.md
  §6: the kernel interface exists and answers structurally.
