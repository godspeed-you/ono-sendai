# ADR-0971: Cgroups, namespaces and addresses are provider targets

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.2 §8.1, §8.3, §35.3, §50; v0.3 §1.33, §1.35; v0.4 §2.16, §7.3, §12, §16.3, §35.2, §42.4; ADR-0135, ADR-0193, ADR-0964
- Decided by: agent (autonomous)

## Context

Three collections of the spatial geography had nothing behind them. `compute/cgroups`,
`network/namespaces` and `network/addresses` answered `unsupported`, and with them an interface's
`addresses` exit (`interface.has_address`), because no provider served a cgroup, a namespace or an
address as an object (#206). v0.2 §8.1 lists `namespace` and `cgroup` among the system targets,
and the target registry carried both as `phase: planned, schema: null`. v0.4 §7.3 says the
network region MUST provide addresses and namespaces, and §16.3 wants cgroups navigable through
their hierarchy.

The bridge already composes a cgroup place and a pid-namespace place from a process record
(ADR-0135), and `ono.interface/1` carries an interface's addresses as a list. Two ways to fill the
collections follow from that: derive every member from the records other targets produce, or
give each kind a target of its own.

## Decision

**Each of the three is a provider target with its own records**, read from the kernel interface
that owns the fact, and the three collections take their members from those targets.

**`get cgroup`** — provider `linux.cgroups`, capability `cgroup.read`, schema `ono.cgroup/1`. It
walks the unified hierarchy: `/sys/fs/cgroup` where that directory holds `cgroup.controllers`,
`/sys/fs/cgroup/unified` on a hybrid host, and it is unavailable where neither exists. Every
directory is one record — `path` (`/` for the root, `/system.slice/nginx.service` below it, the
form `/proc/<pid>/cgroup` uses), `name`, `parent` (null for the root). A directory that vanished
between the listing and the descent is skipped; a directory the caller may not list is an error
in the stream beside the groups that were readable.

**`get namespace`** — provider `linux.namespaces`, capability `namespace.read`, schema
`ono.namespace/1`. For every process in `/proc` it reads the links `ns/{cgroup,ipc,mnt,net,pid,
time,user,uts}` and aggregates them by kind and inode: `id`, `type`, `processes`, and the lowest
pid in the namespace with its parent, user and command line. The `*_for_children` links name the
namespace a process's children will enter, which no process is in, so they are left out, as
`lsns` leaves them out. A link the caller may not read is skipped, so `processes` counts the
processes the caller can see into — the same answer `lsns` gives an unprivileged user. Where
links were refused and none could be read, the provider answers `permission_denied`: an empty
list there would be the false empty collection §42.4 forbids.

**`get address`** — a second target of the netlink route provider (`AddressProvider`, provider id
`linux.netlink`, capability `address.list`), schema `ono.interface-address/1`: one record per
`RTM_NEWADDR` message of the address dump, named through the link dump. `scope` is the kernel's
scope byte (`global`, `site`, `link`, `host`), `label` is `IFA_LABEL`, and `dynamic` is the
absence of `IFA_F_PERMANENT`.

**The geography.** `compute/cgroups` asks `cgroup`, `network/namespaces` asks `namespace`,
`network/addresses` asks `address`. `network/namespaces` lists every kind, the pid and mount
namespaces included: v0.4 §12 gives a process place a `namespace` exit, the place it leads to
needs one canonical collection, and one list grouped by `type` is what `lsns` shows. An interface
place asks `address` for its `addresses` exit; the relation `interface.has_address` was already
read from the `interface` field of an address record.

Identities line up with what was derived before: a served cgroup is keyed on `path` and a served
namespace on `id`, the keys the bridge composes from a process record, so the place a process's
exit leads to and the member of the collection are one place.

## Spec deviation

- Section: spec v0.2 §8.1
- Text: the target list — "socket", "connection", "interface", "route", "neighbor", "dns" among
  its entries — which names no `address`.
- Instead: `address` is a target, `ono.target.address`, serving `ono.interface-address/1`.
- Why: v0.3 §1.33 defines `InterfaceAddress` as a stream type of its own and v0.4 §7.3 makes
  addresses a mandatory collection of the network region. A collection needs members with an
  identity, and the identity `ono.interface-address/1` declares is index and address — the one
  the `ip address` adapter's records already carry. Members derived from an interface record's
  list would be keyed on the address alone, would lack `scope`, `label` and `dynamic`, and would
  never reconcile with an adapter's record of the same address (ADR-0193).

## Consequences

Easy: `get cgroup`, `get namespace` and `get address` are ordinary pipelines (`get namespace |
where type == "net"`); the three collections list members; an interface's `addresses` exit is a
list; `trace container` can reach namespaces and cgroups (#221).

Hard: `get namespace` reads eight links per process — on a host with thousands of processes that
is the cost of `lsns`, and the collection is `normal` cost, so an orientation sweep of `network`
pays it with the other collections (50 ms for 317 processes in a debug build). An unprivileged caller sees its own processes' namespaces only; the
provider contract says so. The Kubernetes provider's targets are `k8s-namespace` and friends, so
the native `namespace` target collides with nothing; `provider_qualified` stays on the registry
entry for the day another provider serves namespaces of its own.

A place the bridge composed is named by the text of its key, and the provider's record carries
the key in the type its schema declares — a path for a cgroup, an integer for a namespace. A look
at such a place asks the provider in the declared type, so the place a process led to stays the
live member of the collection and is never taken for a removed one.

Both collections state how many members there are when an orientation read bounds what it takes
(v0.4.1 §34.4, ADR-0576), so `compute` counts its cgroups and `network` its namespaces. The
netlink targets state no population, addresses included; an address collection larger than the
orientation bound counts what was read.

An address an adapter observed (`ip address`) and the address the provider serves are one place
with two sources. The collection-only-from-an-adapter behaviour ADR-0197 decided is still encoded
by acceptance case `091-spatial-unknown-web-service`; the test ADR-0197 named for addresses now
asserts the reconciliation.

Encoded by `crates/ono-provider-linux/tests/cgroup.rs`, `crates/ono-provider-linux/tests/namespace.rs`,
`crates/ono-provider-netlink/tests/address_decoding.rs`,
`crates/ono-cli/tests/spatial_contracts.rs::should_list_the_members_of_the_collections_that_used_to_answer_unsupported`,
`::should_lead_from_an_interface_to_the_addresses_it_carries`,
`::should_show_an_address_an_adapter_observed_as_the_place_the_native_provider_serves`,
`::should_keep_the_cgroup_and_the_namespace_a_process_led_to_alive_when_they_are_looked_at`
and acceptance case `132-cgroups-namespaces-addresses`.

## Alternatives considered

**Compose the collections in the bridge from process and interface records** — no new target, and
it is what the issue first suggested. Rejected: a cgroup with no process in it would be missing
from `compute/cgroups`, a namespace's record would carry no kind and no count, and an address
place would have an identity no served record shares (see the deviation above).

**Read namespaces through the `lsns` adapter** — the adapter exists and its records are
`ono.namespace/1`. Rejected for the collection's source: §2.16 wants the geography filled from
providers, the adapter needs util-linux and fails on kernels whose ioctls its `lsns` does not know
(it does in this build's sandbox), and `/proc/<pid>/ns` is the interface `lsns` reads itself.

**Serve addresses from the interface provider's existing target** with a flag — rejected: one
target would answer two schemas depending on an option, which breaks the provider conformance
contract of one schema per target.
